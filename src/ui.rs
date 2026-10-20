use std::time::Duration;

use eframe::egui::{self, Color32, Key, RichText, TextureOptions};

use crate::app::App;
use crate::library::track_name;
use crate::util::format_duration;

const ROW_HEIGHT: f32 = 22.0;
const COVER_SIZE: f32 = 148.0;

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // — thread events —
        self.poll_track_load(ctx);
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
                    && let Some(value) = self.drag_position.take()
                {
                    self.seek(Duration::from_secs_f32(value), ctx);
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
                let label = if self.loading_track {
                    "Loading…"
                } else if playing {
                    "Pause"
                } else {
                    "Play"
                };
                if ui
                    .add_enabled(
                        !self.loading_track,
                        egui::Button::new(label).min_size(egui::vec2(96.0, 0.0)),
                    )
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
                    if self.loading || self.loading_track {
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

        // Keep progress and automatic track changes moving. Recompute this after
        // the UI so starting playback from a click also schedules the next frame.
        let playing_now = self
            .player
            .as_ref()
            .is_some_and(|player| !player.is_paused());
        if playing_now || self.loading_track {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.config.save();
    }
}
