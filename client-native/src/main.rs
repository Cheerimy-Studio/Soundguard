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
            .with_decorations(false),
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
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    ctx.set_style(style);
    let mut fonts = egui::FontDefinitions::default();
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
    // Same Windows font families as the legacy WebView; bundled CJK stays the portable fallback.
    #[cfg(windows)]
    for (name, file) in [
        ("legacy-ui", "segoeui.ttf"),
        ("legacy-cjk", "msyh.ttc"),
        ("legacy-numbers", "seguisb.ttf"),
    ] {
        let root = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into());
        if let Ok(data) = std::fs::read(root.join("Fonts").join(file)) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(data));
            if name != "legacy-numbers" {
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(if name == "legacy-ui" { 0 } else { 1 }, name.into());
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
