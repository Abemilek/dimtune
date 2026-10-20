use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{self, TextureHandle};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

use crate::config::{Config, default_music_folder};
use crate::library::{ScanResult, scan, track_name};
use crate::metadata::{Metadata, read_metadata};
use crate::style::apply_style;
use crate::util::Rng;

pub(crate) struct App {
    pub(crate) config: Config,
    // Audio (the sink must stay alive while something is playing)
    sink: Option<MixerDeviceSink>,
    pub(crate) player: Option<Player>,
    // Playlist
    pub(crate) folder: Option<PathBuf>,
    pub(crate) tracks: Vec<PathBuf>,
    order: Vec<usize>,
    pos: usize,
    pub(crate) current: Option<usize>,
    rng: Rng,
    // Current track information
    pub(crate) generation: u64,
    pub(crate) metadata: Option<Metadata>,
    pub(crate) cover: Option<TextureHandle>,
    // UI
    pub(crate) choosing_folder: bool,
    pub(crate) loading: bool,
    pub(crate) folder_rx: Receiver<PathBuf>,
    folder_tx: Sender<PathBuf>,
    pub(crate) scan_rx: Receiver<ScanResult>,
    scan_tx: Sender<ScanResult>,
    pub(crate) scan_generation: u64,
    pub(crate) meta_rx: Receiver<Metadata>,
    meta_tx: Sender<Metadata>,
    pub(crate) drag_position: Option<f32>,
    pub(crate) scroll_to_current: bool,
    pub(crate) consecutive_failures: usize,
    pub(crate) skip_next: bool,
    pub(crate) status: String,
}

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_style(&cc.egui_ctx);
        let config = Config::load();
        let (folder_tx, folder_rx) = mpsc::channel();
        let (scan_tx, scan_rx) = mpsc::channel();
        let (meta_tx, meta_rx) = mpsc::channel();
        let mut app = App {
            sink: None,
            player: None,
            folder: None,
            tracks: Vec::new(),
            order: Vec::new(),
            pos: 0,
            current: None,
            rng: Rng::new(),
            generation: 0,
            metadata: None,
            cover: None,
            choosing_folder: false,
            loading: false,
            folder_rx,
            folder_tx,
            scan_rx,
            scan_tx,
            scan_generation: 0,
            meta_rx,
            meta_tx,
            drag_position: None,
            scroll_to_current: false,
            consecutive_failures: 0,
            skip_next: false,
            status: String::new(),
            config,
        };
        let initial = app
            .config
            .folder
            .clone()
            .filter(|p| p.is_dir())
            .unwrap_or_else(default_music_folder);
        app.load_folder(initial, &cc.egui_ctx);
        app
    }

    pub(crate) fn load_folder(&mut self, dir: PathBuf, ctx: &egui::Context) {
        self.stop();
        self.scan_generation = self.scan_generation.wrapping_add(1);
        let generation = self.scan_generation;
        self.tracks.clear();
        self.order.clear();
        self.pos = 0;
        self.scroll_to_current = false;
        self.status.clear();
        self.loading = true;
        self.folder = Some(dir.clone());
        self.config.folder = Some(dir.clone());
        self.config.save();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("dimtune".into()));
        let tx = self.scan_tx.clone();
        let context = ctx.clone();
        std::thread::spawn(move || {
            let mut tracks = Vec::new();
            scan(&dir, &mut tracks, 0);
            tracks.sort_by_cached_key(|p| p.to_string_lossy().to_lowercase());
            let _ = tx.send(ScanResult {
                generation,
                folder: dir,
                tracks,
            });
            context.request_repaint();
        });
    }

    pub(crate) fn rebuild_order(&mut self, first: Option<usize>) {
        let mut order: Vec<usize> = (0..self.tracks.len()).collect();
        if self.config.shuffle {
            self.rng.shuffle(&mut order);
            if let Some(index) = first
                && let Some(position) = order.iter().position(|&x| x == index)
            {
                order.swap(0, position);
            }
        }
        self.order = order;
        self.pos = match first {
            Some(index) => self.order.iter().position(|&x| x == index).unwrap_or(0),
            None => 0,
        };
    }

    fn stop(&mut self) {
        self.player = None;
        self.current = None;
        self.metadata = None;
        self.cover = None;
        self.drag_position = None;
        self.scroll_to_current = false;
        self.consecutive_failures = 0;
        self.skip_next = false;
        self.generation = self.generation.wrapping_add(1);
    }

    fn ensure_output(&mut self) -> bool {
        if self.sink.is_none() {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    sink.log_on_drop(false);
                    self.sink = Some(sink);
                }
                Err(error) => {
                    self.status = format!("Could not open audio output: {error}");
                    return false;
                }
            }
        }
        true
    }

    pub(crate) fn play_index(&mut self, index: usize, ctx: &egui::Context) {
        let Some(path) = self.tracks.get(index).cloned() else {
            return;
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{} — dimtune",
            track_name(&path)
        )));
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.player = None;
        self.current = Some(index);
        self.metadata = None;
        self.cover = None;
        self.drag_position = None;
        self.skip_next = false;
        self.status.clear();
        if !self.ensure_output() {
            return;
        }
        let decoder = File::open(&path)
            .map_err(|e| e.to_string())
            .and_then(|file| Decoder::try_from(file).map_err(|e| e.to_string()));
        let decoder = match decoder {
            Ok(decoder) => decoder,
            Err(error) => {
                self.status = format!("Could not play {}: {error}", track_name(&path));
                self.consecutive_failures += 1;
                self.skip_next = true;
                ctx.request_repaint();
                return;
            }
        };
        self.consecutive_failures = 0;
        let Some(sink) = self.sink.as_ref() else {
            return;
        };
        let player = Player::connect_new(sink.mixer());
        player.set_volume(self.config.volume);
        player.append(decoder);
        // The previous player (if any) stops when dropped.
        self.player = Some(player);
        self.scroll_to_current = true;
        let (tx, context) = (self.meta_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(read_metadata(generation, &path));
            context.request_repaint();
        });
    }

    fn play_position(&mut self, pos: usize, ctx: &egui::Context) {
        if self.order.is_empty() {
            return;
        }
        self.pos = pos % self.order.len();
        let index = self.order[self.pos];
        self.play_index(index, ctx);
    }

    pub(crate) fn next_track(&mut self, ctx: &egui::Context) {
        if self.order.is_empty() {
            return;
        }
        let mut pos = self.pos + 1;
        if pos >= self.order.len() {
            pos = 0;
            if self.config.shuffle {
                let current = self.current;
                self.rebuild_order(None);
                // Avoid repeating the same song right after a new round starts.
                if self.order.len() > 1 && current == Some(self.order[0]) {
                    self.order.swap(0, 1);
                }
            }
        }
        self.play_position(pos, ctx);
    }

    pub(crate) fn previous_track(&mut self, ctx: &egui::Context) {
        if self.order.is_empty() {
            return;
        }
        // If the track has already played for a few seconds, restart it.
        if let Some(player) = &self.player
            && player.get_pos() > Duration::from_secs(4)
        {
            let _ = player.try_seek(Duration::ZERO);
            return;
        }
        let pos = if self.pos == 0 {
            self.order.len() - 1
        } else {
            self.pos - 1
        };
        self.play_position(pos, ctx);
    }

    pub(crate) fn toggle_pause(&mut self, ctx: &egui::Context) {
        match &self.player {
            Some(player) => {
                if player.is_paused() {
                    player.play();
                } else {
                    player.pause();
                }
            }
            None => {
                if !self.order.is_empty() {
                    self.consecutive_failures = 0;
                    self.skip_next = false;
                    if let Some(index) = self.current {
                        self.play_index(index, ctx);
                    } else {
                        self.play_position(self.pos, ctx);
                    }
                }
            }
        }
    }

    pub(crate) fn choose_folder(&mut self, ctx: &egui::Context) {
        if self.choosing_folder {
            return;
        }
        self.choosing_folder = true;
        let tx = self.folder_tx.clone();
        let context = ctx.clone();
        let start = self.folder.clone().unwrap_or_else(default_music_folder);
        std::thread::spawn(move || {
            let result = rfd::FileDialog::new()
                .set_title("Choose music folder")
                .set_directory(start)
                .pick_folder();
            // Always respond (even if cancelled) so the button is re-enabled.
            let _ = tx.send(result.unwrap_or_default());
            context.request_repaint();
        });
    }
}
