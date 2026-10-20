use eframe::egui::{self, Color32};

pub(crate) fn apply_style(ctx: &egui::Context) {
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
