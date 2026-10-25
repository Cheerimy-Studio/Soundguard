#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

mod app;
mod audio;
mod config;
mod net;
mod rtc;
#[allow(unused_imports)]
use rtc::*;
mod totp;
mod ui;

use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::Vec2::new(960.0, 640.0))
            .with_min_inner_size(egui::Vec2::new(720.0, 480.0))
            .with_title("声垣 SoundGuard")
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "声垣 SoundGuard",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(app::SoundGuardApp::new(cc)))
        }),
    )
}

fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::light();
    v.panel_fill = egui::Color32::from_rgb(244, 246, 248);
    v.window_fill = egui::Color32::WHITE;
    v.widgets.inactive.bg_fill = egui::Color32::WHITE;
    v.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(50, 60, 70);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(13, 138, 146);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(11, 122, 130);
    v.selection.bg_fill = egui::Color32::from_rgb(13, 138, 146);
    ctx.set_visuals(v);

    let mut fonts = egui::FontDefinitions::default();
    for p in ["C:\\Windows\\Fonts\\msyh.ttc", "C:\\Windows\\Fonts\\msyh.ttf", "C:\\Windows\\Fonts\\simsun.ttc"] {
        if let Ok(d) = std::fs::read(p) {
            fonts.font_data.insert("zh".to_owned(), egui::FontData::from_owned(d));
            if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                fam.insert(0, "zh".to_owned());
            }
            if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                fam.push("zh".to_owned());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn load_icon() -> egui::IconData {
    let size = 32usize;
    let mut px: Vec<u8> = Vec::with_capacity(size * size * 4);
    let c = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let dist = (dx * dx + dy * dy).sqrt();
            let inside = dist < c * 0.85;
            let r: u8 = if inside { 13 } else { 0 };
            let g: u8 = if inside { 138 } else { 0 };
            let b: u8 = if inside { 146 } else { 0 };
            let a: u8 = if inside { 255 } else { 0 };
            px.extend_from_slice(&[r, g, b, a]);
        }
    }
    egui::IconData { rgba: px, width: size as u32, height: size as u32 }
}
