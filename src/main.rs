//! dimtune — an ultra-minimal music player.
//! Fixed dark theme, no animations, no network, no telemetry.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32, ColorImage, Key, RichText, TextureHandle, TextureOptions};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

const EXTENSIONS: [&str; 5] = ["mp3", "flac", "ogg", "wav", "m4a"];
const ROW_HEIGHT: f32 = 22.0;
const COVER_SIZE: f32 = 148.0;

// ───────────────────────────── utilities ─────────────────────────────

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn config_base_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
}

fn config_path() -> PathBuf {
    config_base_dir().join("dimtune").join("config")
}

/// The user's XDG music folder, for example ~/Music or its localized name.
/// If there is no valid folder, fall back to ~/Music even if it does not exist:
/// scanning the whole home directory by default would be too expensive and surprising.
fn default_music_folder() -> PathBuf {
    let home_dir = home();
    let dirs = config_base_dir().join("user-dirs.dirs");
    if let Ok(text) = std::fs::read_to_string(dirs) {
        for line in text.lines() {
            if let Some(value) = line.trim().strip_prefix("XDG_MUSIC_DIR=") {
                let value = value
                    .trim()
                    .trim_matches('"')
                    .replace("$HOME", &home_dir.to_string_lossy());
                let path = PathBuf::from(value);
                if path.is_dir() && path != home_dir {
                    return path;
                }
            }
        }
    }
    home_dir.join("Music")
}

struct Config {
    folder: Option<PathBuf>,
    shuffle: bool,
    volume: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            folder: None,
            shuffle: false,
            volume: 0.8,
        }
    }
}

impl Config {
    fn load() -> Self {
        std::fs::read_to_string(config_path())
            .map(|text| Self::from_text(&text))
            .unwrap_or_default()
    }

    fn from_text(text: &str) -> Self {
        let mut config = Self::default();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "folder" if !value.trim().is_empty() => {
                        config.folder = Some(PathBuf::from(value.trim()))
                    }
                    "shuffle" => config.shuffle = value.trim() == "1",
                    "volume" => {
                        if let Ok(number) = value.trim().parse::<f32>()
                            && number.is_finite()
                        {
                            config.volume = number.clamp(0.0, 1.0);
                        }
                    }
                    _ => {}
                }
            }
        }
        config
    }

    fn save(&self) {
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let folder = self
            .folder
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = format!(
            "folder={}\nshuffle={}\nvolume={:.2}\n",
            folder,
            if self.shuffle { 1 } else { 0 },
            self.volume
        );
        // Writing to a temporary file and renaming avoids leaving a partial file.
        let temp = path.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::write(&temp, text).is_ok() {
            if std::fs::rename(&temp, &path).is_err() {
                let _ = std::fs::remove_file(&temp);
            }
        } else {
            let _ = std::fs::remove_file(&temp);
        }
    }
}

fn scan(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 12 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            scan(&path, out, depth + 1);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        {
            out.push(path);
        }
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, secs) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// Minimal pseudo-random generator (xorshift64), no dependencies.
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        Rng(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn shuffle(&mut self, values: &mut [usize]) {
        for i in (1..values.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            values.swap(i, j);
        }
    }
}

// ─────────────────────── metadata and cover (thread) ───────────────────────

struct Metadata {
    generation: u64,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration: Option<Duration>,
    cover: Option<ColorImage>,
}

struct ScanResult {
    generation: u64,
    folder: PathBuf,
    tracks: Vec<PathBuf>,
}

fn decode_cover(bytes: &[u8]) -> Option<ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = image.thumbnail(320, 320).to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
}

fn folder_cover(file: &Path) -> Option<ColorImage> {
    let dir = file.parent()?;
    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    for name in ["cover", "folder", "front", "album"] {
        for extension in ["jpg", "jpeg", "png"] {
            for path in &entries {
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                    continue;
                };
                if !stem.eq_ignore_ascii_case(name) || !ext.eq_ignore_ascii_case(extension) {
                    continue;
                }
                if let Some(image) = std::fs::read(path)
                    .ok()
                    .and_then(|bytes| decode_cover(&bytes))
                {
                    return Some(image);
                }
            }
        }
    }
    None
}

fn read_metadata(generation: u64, path: &Path) -> Metadata {
    let mut metadata = Metadata {
        generation,
        title: None,
        artist: None,
        album: None,
        duration: None,
        cover: None,
    };
    if let Ok(tagged_file) = lofty::read_from_path(path) {
        let duration = tagged_file.properties().duration();
        if !duration.is_zero() {
            metadata.duration = Some(duration);
        }
        if let Some(tag) = tagged_file
            .primary_tag()
            .or_else(|| tagged_file.first_tag())
        {
            metadata.title = tag
                .title()
                .map(|s| s.into_owned())
                .filter(|s| !s.trim().is_empty());
            metadata.artist = tag
                .artist()
                .map(|s| s.into_owned())
                .filter(|s| !s.trim().is_empty());
            metadata.album = tag
                .album()
                .map(|s| s.into_owned())
                .filter(|s| !s.trim().is_empty());
            metadata.cover = tag
                .pictures()
                .iter()
                .find_map(|picture| decode_cover(picture.data()));
        }
    }
    if metadata.cover.is_none() {
        metadata.cover = folder_cover(path);
    }
    metadata
}

// ───────────────────────────────── app ─────────────────────────────────

struct App {
    config: Config,
    // Audio (the sink must stay alive while something is playing)
    sink: Option<MixerDeviceSink>,
    player: Option<Player>,
    // Playlist
    folder: Option<PathBuf>,
    tracks: Vec<PathBuf>,
    order: Vec<usize>,
    pos: usize,
    current: Option<usize>,
    rng: Rng,
    // Current track information
    generation: u64,
    metadata: Option<Metadata>,
    cover: Option<TextureHandle>,
    // UI
    choosing_folder: bool,
    loading: bool,
    folder_rx: Receiver<PathBuf>,
    folder_tx: Sender<PathBuf>,
    scan_rx: Receiver<ScanResult>,
    scan_tx: Sender<ScanResult>,
    scan_generation: u64,
    meta_rx: Receiver<Metadata>,
    meta_tx: Sender<Metadata>,
    drag_position: Option<f32>,
    scroll_to_current: bool,
    consecutive_failures: usize,
    skip_next: bool,
    status: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
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

    fn load_folder(&mut self, dir: PathBuf, ctx: &egui::Context) {
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

    fn rebuild_order(&mut self, first: Option<usize>) {
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

    fn play_index(&mut self, index: usize, ctx: &egui::Context) {
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

    fn next_track(&mut self, ctx: &egui::Context) {
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

    fn previous_track(&mut self, ctx: &egui::Context) {
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

    fn toggle_pause(&mut self, ctx: &egui::Context) {
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

    fn choose_folder(&mut self, ctx: &egui::Context) {
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

fn track_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ─────────────────────────────── style ───────────────────────────────

fn apply_style(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut visuals = egui::Visuals::dark();
    let background = Color32::from_rgb(0x12, 0x12, 0x13);
    let panel = Color32::from_rgb(0x18, 0x18, 0x1a);
    let text = Color32::from_rgb(0xc8, 0xc8, 0xc8);
    visuals.panel_fill = background;
    visuals.window_fill = background;
    visuals.extreme_bg_color = Color32::from_rgb(0x0d, 0x0d, 0x0e);
    visuals.override_text_color = Some(text);
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.popup_shadow = egui::Shadow::NONE;
    visuals.selection.bg_fill = Color32::from_rgb(0x2b, 0x3a, 0x4a);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(0xdd, 0xdd, 0xdd));
    visuals.widgets.noninteractive.bg_fill = panel;
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0_f32, Color32::from_rgb(0x26, 0x26, 0x29));
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x22, 0x22, 0x25);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x22, 0x22, 0x25);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x2e, 0x2e, 0x32);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2e, 0x2e, 0x32);
    visuals.widgets.active.bg_fill = Color32::from_rgb(0x38, 0x38, 0x3d);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x38, 0x38, 0x3d);
    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style.animation_time = 0.0; // no animations
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.interaction.tooltip_delay = 0.6;
        style.scroll_animation = egui::style::ScrollAnimation::none();
        for font in style.text_styles.values_mut() {
            font.size = (font.size * 1.08).round();
        }
    });
}

// ──────────────────────────────── ui ────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // — thread events —
        while let Ok(path) = self.folder_rx.try_recv() {
            self.choosing_folder = false;
            if path.is_dir() {
                self.load_folder(path, ctx);
            }
        }
        while let Ok(result) = self.scan_rx.try_recv() {
            if result.generation == self.scan_generation {
                self.tracks = result.tracks;
                self.folder = Some(result.folder.clone());
                self.config.folder = Some(result.folder);
                self.rebuild_order(None);
                self.loading = false;
                self.scroll_to_current = false;
                self.status = if self.tracks.is_empty() {
                    "No music found in this folder".into()
                } else {
                    String::new()
                };
            }
        }
        // A decode failure is skipped on the next frame to avoid chaining
        // recursive calls or freezing the UI.
        if self.skip_next {
            self.skip_next = false;
            if self.consecutive_failures < self.tracks.len().max(1) {
                self.next_track(ctx);
            }
        }
        while let Ok(metadata) = self.meta_rx.try_recv() {
            if metadata.generation == self.generation {
                self.cover = metadata
                    .cover
                    .as_ref()
                    .map(|image| ctx.load_texture("cover", image.clone(), TextureOptions::LINEAR));
                self.metadata = Some(metadata);
            }
        }

        // — end of track → next automatically —
        let finished = self.player.as_ref().is_some_and(|player| player.empty());
        if finished {
            self.next_track(ctx);
        }

        // — shortcuts —
        if !ctx.wants_keyboard_input() {
            let (space, right, left) = ctx.input(|input| {
                (
                    input.key_pressed(Key::Space),
                    input.key_pressed(Key::ArrowRight),
                    input.key_pressed(Key::ArrowLeft),
                )
            });
            if space {
                self.toggle_pause(ctx);
            }
            if right {
                self.consecutive_failures = 0;
                self.skip_next = false;
                self.next_track(ctx);
            }
            if left {
                self.consecutive_failures = 0;
                self.skip_next = false;
                self.previous_track(ctx);
            }
        }

        let paused = self
            .player
            .as_ref()
            .is_some_and(|player| player.is_paused());
        let playing = self.player.is_some() && !paused;

        // — top bar —
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.choosing_folder, egui::Button::new("Folder…"))
                    .clicked()
                {
                    self.choose_folder(ctx);
                }
                let path = self
                    .folder
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ui.add(egui::Label::new(RichText::new(path).weak()).truncate());
            });
            ui.add_space(2.0);
        });

        // — controls —
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.add_space(6.0);
            // progress
            let duration = self.metadata.as_ref().and_then(|m| m.duration);
            let real_position = self
                .player
                .as_ref()
                .map(|player| player.get_pos())
                .unwrap_or_default();
            let position_seconds = self.drag_position.unwrap_or(real_position.as_secs_f32());
            let displayed_position = duration
                .map(|d| position_seconds.min(d.as_secs_f32()))
                .unwrap_or(position_seconds);
            let total = duration.map(|d| d.as_secs_f32()).unwrap_or(0.0).max(0.1);
            ui.horizontal(|ui| {
                ui.label(format_duration(Duration::from_secs_f32(displayed_position)));
                let mut value = position_seconds.min(total);
                ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(80.0);
                let response = ui.add_enabled(
                    duration.is_some() && self.player.is_some(),
                    egui::Slider::new(&mut value, 0.0..=total).show_value(false),
                );
                if response.changed() {
                    self.drag_position = Some(value);
                }
                if (response.drag_stopped() || (response.changed() && !response.dragged()))
                    && let (Some(player), Some(value)) = (&self.player, self.drag_position.take())
                {
                    let _ = player.try_seek(Duration::from_secs_f32(value));
                }
                ui.label(
                    duration
                        .map(format_duration)
                        .unwrap_or_else(|| "--:--".into()),
                );
            });
            // buttons
            ui.horizontal(|ui| {
                if ui.button("Previous").clicked() {
                    self.consecutive_failures = 0;
                    self.skip_next = false;
                    self.previous_track(ctx);
                }
                let label = if playing { "Pause" } else { "Play" };
                if ui
                    .add(egui::Button::new(label).min_size(egui::vec2(96.0, 0.0)))
                    .clicked()
                {
                    self.toggle_pause(ctx);
                }
                if ui.button("Next").clicked() {
                    self.consecutive_failures = 0;
                    self.skip_next = false;
                    self.next_track(ctx);
                }
                ui.separator();
                let mut shuffle = self.config.shuffle;
                if ui.checkbox(&mut shuffle, "Shuffle").changed() {
                    self.config.shuffle = shuffle;
                    self.consecutive_failures = 0;
                    self.skip_next = false;
                    self.rebuild_order(self.current);
                    self.config.save();
                }
                ui.separator();
                ui.label("Vol");
                let mut volume = self.config.volume;
                ui.spacing_mut().slider_width = 110.0;
                let response = ui.add(egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false));
                if response.changed() {
                    self.config.volume = volume;
                    if let Some(player) = &self.player {
                        player.set_volume(volume);
                    }
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    self.config.save();
                }
            });
            ui.add_space(6.0);
        });

        // — center: cover + info + list —
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui
                    .allocate_exact_size(egui::vec2(COVER_SIZE, COVER_SIZE), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 2.0, Color32::from_rgb(0x0d, 0x0d, 0x0e));
                if let Some(texture) = &self.cover {
                    let size = texture.size_vec2();
                    let scale = (COVER_SIZE / size.x).min(COVER_SIZE / size.y);
                    let image_rect = egui::Rect::from_center_size(rect.center(), size * scale);
                    ui.painter().image(
                        texture.id(),
                        image_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    if let Some(index) = self.current {
                        let path = &self.tracks[index];
                        let title = self
                            .metadata
                            .as_ref()
                            .and_then(|m| m.title.clone())
                            .unwrap_or_else(|| track_name(path));
                        ui.add(
                            egui::Label::new(RichText::new(title).size(20.0).strong()).truncate(),
                        );
                        if let Some(metadata) = &self.metadata {
                            if let Some(artist) = &metadata.artist {
                                ui.add(egui::Label::new(artist.as_str()).truncate());
                            }
                            if let Some(album) = &metadata.album {
                                ui.add(
                                    egui::Label::new(RichText::new(album.as_str()).weak())
                                        .truncate(),
                                );
                            }
                        }
                    } else {
                        ui.label(RichText::new("Nothing playing").weak());
                    }
                    ui.add_space(8.0);
                    if self.loading {
                        ui.label(RichText::new("Loading…").weak());
                    } else if !self.status.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&self.status)
                                    .color(Color32::from_rgb(0xd0, 0x8a, 0x6a)),
                            )
                            .wrap(),
                        );
                    } else {
                        let text = if self.tracks.len() == 1 {
                            "1 track".to_string()
                        } else {
                            format!("{} tracks", self.tracks.len())
                        };
                        ui.label(RichText::new(text).weak());
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();

            let total = self.tracks.len();
            let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
            if self.scroll_to_current {
                if let Some(index) = self.current {
                    scroll = scroll.vertical_scroll_offset(
                        (index as f32 * (ROW_HEIGHT + ui.spacing().item_spacing.y) - 60.0).max(0.0),
                    );
                }
                self.scroll_to_current = false;
            }
            let mut selected: Option<usize> = None;
            scroll.show_rows(ui, ROW_HEIGHT, total, |ui, range| {
                for index in range {
                    let is_selected = self.current == Some(index);
                    let name = track_name(&self.tracks[index]);
                    let response = ui.add_sized(
                        [ui.available_width(), ROW_HEIGHT],
                        egui::Button::selectable(is_selected, name)
                            .truncate()
                            .frame_when_inactive(false),
                    );
                    if response.clicked() {
                        selected = Some(index);
                    }
                }
            });
            if let Some(index) = selected {
                self.consecutive_failures = 0;
                self.skip_next = false;
                self.rebuild_order(Some(index));
                self.play_index(index, ctx);
            }
        });

        // Only wake up periodically while playing (progress + next track).
        if playing {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.config.save();
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 520.0])
            .with_min_inner_size([520.0, 360.0])
            .with_title("dimtune")
            .with_app_id("dimtune"),
        ..Default::default()
    };
    eframe::run_native(
        "dimtune",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("dimtune-test-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0:00");
        assert_eq!(format_duration(Duration::from_secs(59)), "0:59");
        assert_eq!(format_duration(Duration::from_secs(60)), "1:00");
        assert_eq!(format_duration(Duration::from_secs(3599)), "59:59");
        assert_eq!(format_duration(Duration::from_secs(3600)), "1:00:00");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1:01:01");
    }

    #[test]
    fn name_without_extension() {
        assert_eq!(track_name(Path::new("/music/song.mp3")), "song");
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut values: Vec<usize> = (0..64).collect();
        let original = values.clone();
        Rng(1).shuffle(&mut values);
        let mut sorted = values.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, original);
        assert_ne!(values, original);
    }

    #[test]
    fn config_from_text() {
        let config = Config::from_text("folder=/tmp/music\nshuffle=1\nvolume=1.7\nunknown=value\n");
        assert_eq!(config.folder, Some(PathBuf::from("/tmp/music")));
        assert!(config.shuffle);
        assert_eq!(config.volume, 1.0);

        let config = Config::from_text("volume=-0.5\n");
        assert_eq!(config.volume, 0.0);
        assert_eq!(config.folder, None);
        assert!(!config.shuffle);

        assert_eq!(Config::from_text("volume=NaN\n").volume, 0.8);
        assert_eq!(Config::from_text("folder=\n").folder, None);
    }

    #[test]
    fn recursive_scan_and_extensions() {
        let base = temp_dir();
        let sub = base.join("sub");
        let deep = sub.join("more");
        fs::create_dir_all(&deep).unwrap();
        for name in ["a.mp3", "B.FLAC", "note.txt"] {
            fs::write(base.join(name), b"").unwrap();
        }
        fs::write(sub.join("c.ogg"), b"").unwrap();
        fs::write(deep.join("d.wav"), b"").unwrap();
        fs::write(deep.join("e.opus"), b"").unwrap();

        let mut tracks = Vec::new();
        scan(&base, &mut tracks, 0);
        let names: Vec<String> = tracks
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(tracks.len(), 4);
        for name in ["a.mp3", "B.FLAC", "c.ogg", "d.wav"] {
            assert!(names.iter().any(|n| n == name), "missing {name}");
        }

        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn folder_cover_is_case_insensitive() {
        let base = temp_dir();
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        fs::write(base.join("COVER.PNG"), bytes).unwrap();

        let cover = folder_cover(&base.join("song.mp3")).unwrap();
        assert!(cover.size[0] > 0 && cover.size[1] > 0);

        fs::remove_dir_all(&base).unwrap();
    }
}
