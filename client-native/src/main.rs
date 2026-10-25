#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod app;
#[cfg_attr(not(windows), allow(dead_code))] // WASAPI backend is intentionally Windows-only.
mod audio;
mod config;
mod net;
mod playback;
#[path = "../../client/common/rnnoise_engine.rs"]
mod rnnoise_engine;
mod rtc;
#[path = "../../client/common/schedule.rs"]
#[allow(dead_code)] // Legacy clients also use the shared anchored clock.
mod schedule;
mod totp;
mod ui;
use eframe::egui;
fn main() -> Result<(), eframe::Error> {
    let compact = std::env::args().any(|a| a == "--preview-compact");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(if compact {
                [900.0, 640.0]
            } else {
                [1120.0, 760.0]
            })
            .with_min_inner_size([900.0, 640.0])
            .with_title("声垣")
            .with_decorations(true)
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "声垣",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(app::SoundGuardApp::new(cc)))
        }),
    )
}
fn app_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../assets/128x128.png"))
        .expect("bundled application icon")
        .into_rgba8();
    egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}
fn setup_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = egui::Color32::from_rgb(244, 246, 248);
    style.visuals.window_fill = egui::Color32::WHITE;
    style.visuals.override_text_color = Some(egui::Color32::from_rgb(23, 32, 51));
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(207, 236, 238);
    style.visuals.widgets.inactive.bg_stroke =
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(213, 221, 231));
    style.visuals.extreme_bg_color = egui::Color32::WHITE;
    style.visuals.window_rounding = egui::Rounding::same(15.0);
    // Labels are informational; only real text inputs should allow selection.
    style.interaction.selectable_labels = false;
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(9.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(9.0);
    style.visuals.widgets.active.rounding = egui::Rounding::same(9.0);
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(244, 247, 250);
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(234, 242, 244);
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(221, 237, 238);
    style.visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(188, 213, 215));
    style.visuals.widgets.inactive.fg_stroke =
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(23, 32, 51));
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.interact_size.y = 32.0;
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
    ctx.set_style(style);
    let mut fonts = egui::FontDefinitions::default();
    // The bundled CJK face is borrowed from the executable image, so it adds no
    // heap copy. Microsoft YaHei is deliberately not loaded: reading its 19 MB
    // TTC into memory would cost more than it improves typography.
    fonts.font_data.insert(
        "soundguard-cjk".into(),
        egui::FontData::from_static(include_bytes!("../assets/NotoSansCJKsc-Regular.otf")),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push("soundguard-cjk".into());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("soundguard-cjk".into());
    #[cfg(windows)]
    for (name, file) in [
        ("legacy-ui", "segoeui.ttf"),
        ("legacy-numbers", "seguisb.ttf"),
    ] {
        let root = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into());
        if let Ok(data) = std::fs::read(root.join("Fonts").join(file)) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(data));
            if name == "legacy-ui" {
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, name.into());
            }
        }
    }
    let numbers = if fonts.font_data.contains_key("legacy-numbers") {
        "legacy-numbers"
    } else {
        "soundguard-cjk"
    };
    fonts.families.insert(
        egui::FontFamily::Name("legacy-numbers".into()),
        vec![numbers.into()],
    );
    ctx.set_fonts(fonts);
}
