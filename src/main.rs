//! dimtune — an ultra-minimal music player.
//! Fixed dark theme, no animations, no network, no telemetry.

mod app;
mod config;
mod library;
mod metadata;
mod style;
mod ui;
mod util;

use eframe::egui;

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
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
