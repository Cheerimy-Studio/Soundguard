use eframe::egui;
use egui_plot::{Legend, Line, Plot};

use crate::app::{Panel, Phase, SoundGuardApp};

// Brand colors (matching old WebView UI)
const TEAL: egui::Color32 = egui::Color32::from_rgb(13, 138, 146);
const TEAL_LIGHT: egui::Color32 = egui::Color32::from_rgb(20, 160, 168);
const BG: egui::Color32 = egui::Color32::from_rgb(244, 246, 248);
const CARD: egui::Color32 = egui::Color32::WHITE;
const TEXT: egui::Color32 = egui::Color32::from_rgb(30, 40, 50);
const TEXT_SUB: egui::Color32 = egui::Color32::from_rgb(100, 112, 122);
const RED: egui::Color32 = egui::Color32::from_rgb(220, 38, 38);
const AMBER: egui::Color32 = egui::Color32::from_rgb(217, 119, 6);
const GOLD: egui::Color32 = egui::Color32::from_rgb(212, 160, 23);
const GOLD_BRIGHT: egui::Color32 = egui::Color32::from_rgb(255, 215, 0);

impl eframe::App for SoundGuardApp {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        self.tick();
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        match self.phase.clone() {
            Phase::Consent => self.consent(ctx),
            Phase::Setup => self.setup(ctx),
            Phase::Running => self.main(ctx),
        }
    }
}

// Helper: card frame
fn card() -> egui::Frame {
    egui::Frame {
        fill: CARD,
        rounding: egui::Rounding::same(12.0),
        inner_margin: egui::Margin::same(16.0),
        shadow: egui::Shadow {
            offset: egui::Vec2::new(0.0, 2.0),
            blur: 8.0,
            color: egui::Color32::from_black_alpha(15),
            spread: 0.0,
        },
        ..Default::default()
    }
}

impl SoundGuardApp {
    fn consent(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("声垣").size(32.0).color(TEAL).strong());
                ui.label(egui::RichText::new("使用须知").size(14.0).color(TEXT_SUB));
            });
            ui.add_space(24.0);

            ui.vertical_centered(|ui| {
                card().show(ui, |ui| {
                    ui.set_width(480.0);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("1. 通知与邮件").size(13.0).color(TEXT).strong());
                    ui.label(egui::RichText::new("开启后会推送超标通知并发送告警邮件。").size(12.0).color(TEXT_SUB));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("2. 录音保存").size(13.0).color(TEXT).strong());
                    ui.label(egui::RichText::new("超标时保存 30 秒现场录音供管理员回放。").size(12.0).color(TEXT_SUB));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("3. 责任说明").size(13.0).color(TEXT).strong());
                    ui.label(egui::RichText::new("测量结果仅供参考，声垣保留最终解释权。").size(12.0).color(TEXT_SUB));
                    ui.add_space(4.0);
                });
            });

            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    let btn_no = egui::Button::new(egui::RichText::new("不同意").color(TEXT_SUB)).fill(egui::Color32::from_rgb(240, 240, 240));
                    if ui.add(btn_no).clicked() { std::process::exit(0); }
                    ui.add_space(8.0);
                    let btn_yes = egui::Button::new(egui::RichText::new("同意并继续").color(egui::Color32::WHITE)).fill(TEAL);
                    if ui.add(btn_yes).clicked() {
                        self.consent_accepted = true;
                        self.config.consent_accepted = true;
                        let _ = self.config.save();
                        self.phase = if self.config.needs_setup() { Phase::Setup } else { Phase::Running };
                    }
                });
            });
        });
    }

    fn setup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("首次配置").size(22.0).color(TEXT).strong());
                ui.label(egui::RichText::new("填写管理端生成的设备信息").size(12.0).color(TEXT_SUB));
            });
            ui.add_space(16.0);

            ui.vertical_centered(|ui| {
                card().show(ui, |ui| {
                    ui.set_width(420.0);
                    egui::Grid::new("setup").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                        ui.label(egui::RichText::new("服务器").size(12.0).color(TEXT_SUB));
                        ui.add(egui::TextEdit::singleline(&mut self.setup_server_url).desired_width(260.0).hint_text("https://db.gzmath.cn"));
                        ui.end_row();
                        ui.label(egui::RichText::new("设备编号").size(12.0).color(TEXT_SUB));
                        ui.add(egui::TextEdit::singleline(&mut self.setup_device_id).desired_width(260.0));
                        ui.end_row();
                        ui.label(egui::RichText::new("设备令牌").size(12.0).color(TEXT_SUB));
                        ui.add(egui::TextEdit::singleline(&mut self.setup_device_token).desired_width(260.0).password(true));
                        ui.end_row();
                        ui.label(egui::RichText::new("本地密码").size(12.0).color(TEXT_SUB));
                        ui.add(egui::TextEdit::singleline(&mut self.setup_password).desired_width(260.0).password(true));
                        ui.end_row();
                        ui.label(egui::RichText::new("确认密码").size(12.0).color(TEXT_SUB));
                        ui.add(egui::TextEdit::singleline(&mut self.setup_confirm).desired_width(260.0).password(true));
                        ui.end_row();
                    });

                    if !self.setup_error.is_empty() {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(&self.setup_error).size(11.0).color(RED));
                    }

                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        let btn = egui::Button::new(egui::RichText::new("保存并开始监测").color(egui::Color32::WHITE)).fill(TEAL);
                        if ui.add(btn).clicked() { self.do_setup(); }
                    });
                });
            });
        });
    }

    fn do_setup(&mut self) {
        if self.setup_password != self.setup_confirm { self.setup_error = "密码不一致".into(); return; }
        if self.setup_password.len() < 6 { self.setup_error = "密码至少 6 位".into(); return; }
        use argon2::{Argon2, PasswordHasher};
        use argon2::password_hash::{SaltString, rand_core::OsRng};
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default().hash_password(self.setup_password.as_bytes(), &salt).map(|h| h.to_string()).unwrap_or_default();
        self.config.server_url = self.setup_server_url.trim().to_string();
        self.config.device_id = self.setup_device_id.trim().to_string();
        self.config.device_token = self.setup_device_token.trim().to_string();
        self.config.local_password_hash = hash;
        let _ = self.config.save();
        self.net.start(self.config.server_url.clone(), self.config.device_id.clone(), self.config.device_token.clone());
        self.phase = Phase::Running;
    }

    fn main(&mut self, ctx: &egui::Context) {
        let war = self.war_mode;
        let accent = if war { GOLD_BRIGHT } else { TEAL };

        // Header
        egui::TopBottomPanel::top("hdr").frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(16.0, 10.0))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Brand
                ui.label(egui::RichText::new("声垣").size(18.0).strong().color(accent));
                if war {
                    ui.label(egui::RichText::new("分贝战争").size(12.0).color(GOLD));
                }
                ui.separator();

                // Connection status
                let (dot, txt) = if self.connected {
                    (egui::Color32::from_rgb(34, 197, 94), "已连接")
                } else {
                    (RED, "重连中")
                };
                ui.colored_label(dot, egui::RichText::new("●").size(10.0));
                ui.label(egui::RichText::new(txt).size(11.0).color(TEXT_SUB));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let gear = if self.panel == Panel::Settings { "⚙" } else { "⚙" };
                    if ui.add(egui::Button::new(egui::RichText::new(gear).size(16.0)).fill(egui::Color32::TRANSPARENT)).clicked() {
                        self.panel = if self.panel == Panel::Settings { Panel::Dashboard } else { Panel::Settings };
                    }
                });
            });
        });

        // Main content
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                match self.panel {
                    Panel::Dashboard => self.dashboard(ui, accent),
                    Panel::Settings => self.settings(ui),
                }
            });
        });

        // Toast
        if let Some((msg, t, err)) = &self.toast {
            if t.elapsed().as_secs() < 4 {
                egui::Area::new(egui::Id::new("toast"))
                    .fixed_pos(egui::pos2(20.0, 60.0))
                    .show(ctx, |ui| {
                        card().show(ui, |ui| {
                            let c = if *err { RED } else { TEAL };
                            ui.label(egui::RichText::new(msg).size(12.0).color(c));
                        });
                    });
            } else {
                self.toast = None;
            }
        }
    }

    fn dashboard(&mut self, ui: &mut egui::Ui, accent: egui::Color32) {
        ui.add_space(8.0);

        // dB display card
        card().show(ui, |ui| {
            ui.horizontal(|ui| {
                // Big number
                ui.vertical(|ui| {
                    let db = self.current_db.map(|d| format!("{:.1}", d)).unwrap_or("--".into());
                    let over30 = self.average_db.map(|d| d >= self.threshold_db).unwrap_or(false);
                    let over15 = self.rolling_15s_db.map(|d| d >= self.threshold_db).unwrap_or(false);
                    let c = if over30 { RED } else if over15 { AMBER } else { accent };
                    ui.label(egui::RichText::new(db).size(52.0).strong().color(c));
                    ui.label(egui::RichText::new("dB(A)").size(12.0).color(TEXT_SUB));
                });

                ui.separator();

                // Stats
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("阈值 ").size(11.0).color(TEXT_SUB));
                        ui.label(egui::RichText::new(format!("{:.1} dB", self.threshold_db)).size(11.0).color(TEXT).strong());
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("15s  ").size(11.0).color(TEXT_SUB));
                        let v = self.rolling_15s_db.map(|d| format!("{:.1}", d)).unwrap_or("--".into());
                        let c = if over15_now(self) { AMBER } else { TEXT };
                        ui.label(egui::RichText::new(format!("{} dB", v)).size(11.0).color(c).strong());
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("30s  ").size(11.0).color(TEXT_SUB));
                        let v = self.average_db.map(|d| format!("{:.1}", d)).unwrap_or("--".into());
                        ui.label(egui::RichText::new(format!("{} dB", v)).size(11.0).color(TEXT).strong());
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("峰值 ").size(11.0).color(TEXT_SUB));
                        let v = self.peak_db.map(|d| format!("{:.1}", d)).unwrap_or("--".into());
                        ui.label(egui::RichText::new(format!("{} dB", v)).size(11.0).color(TEXT).strong());
                    });
                });
            });
        });

        // TOTP
        if !self.totp_code.is_empty() {
            ui.add_space(6.0);
            card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("2FA").size(10.0).color(TEXT_SUB));
                    ui.label(egui::RichText::new(&self.totp_code).size(20.0).monospace().color(accent).strong());
                    ui.label(egui::RichText::new("每 30 秒刷新").size(9.0).color(TEXT_SUB));
                });
            });
        }

        // Waveform
        ui.add_space(6.0);
        if self.waveform.len() > 1 {
            card().show(ui, |ui| {
                ui.set_height(ui.available_height().max(200.0));
                ui.label(egui::RichText::new("声压波形").size(13.0).color(TEXT).strong());

                let pts = self.waveform.clone();
                let thr = self.threshold_db;
                let line_c = accent;
                Plot::new("wave")
                    .height(ui.available_height().min(220.0))
                    .show_axes([false, true])
                    .legend(Legend::default())
                    .show(ui, move |p| {
                        p.line(Line::new(pts.clone()).name("分贝").color(line_c).width(2.0));
                        p.line(Line::new(vec![[pts[0][0], thr], [pts[pts.len()-1][0], thr]])
                            .name("阈值").color(RED).width(1.0));
                    });
            });
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);

        // Calibration
        card().show(ui, |ui| {
            ui.label(egui::RichText::new("麦克风校准").size(13.0).color(TEXT).strong());
            ui.add_space(4.0);
            ui.add(egui::Slider::new(&mut self.config.calibration_offset_db, 40.0..=140.0).text("dB 偏移"));
            if ui.add(egui::Button::new(egui::RichText::new("保存校准").color(egui::Color32::WHITE)).fill(TEAL)).clicked() {
                let _ = self.config.save();
                self.audio.shared.set_offset(self.config.calibration_offset_db as f32);
                self.show_toast("校准已保存", false);
            }
        });

        ui.add_space(8.0);

        // Volume
        card().show(ui, |ui| {
            ui.label(egui::RichText::new("告警提示音").size(13.0).color(TEXT).strong());
            ui.add_space(4.0);
            ui.add(egui::Slider::new(&mut self.alert_volume, 0.0..=1.0).text("音量"));
        });

        ui.add_space(8.0);

        // Connection info
        card().show(ui, |ui| {
            ui.label(egui::RichText::new("连接").size(13.0).color(TEXT).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("服务器 ").size(11.0).color(TEXT_SUB));
                ui.label(egui::RichText::new(&self.config.server_url).size(11.0).color(TEXT));
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("设备    ").size(11.0).color(TEXT_SUB));
                ui.label(egui::RichText::new(&self.config.device_id).size(11.0).color(TEXT));
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("状态    ").size(11.0).color(TEXT_SUB));
                let s = if self.connected { "已连接" } else { "离线" };
                let c = if self.connected { egui::Color32::from_rgb(34, 197, 94) } else { RED };
                ui.label(egui::RichText::new(s).size(11.0).color(c));
            });
        });

        ui.add_space(16.0);

        // Exit
        if ui.add(egui::Button::new(egui::RichText::new("退出声垣").color(RED)).fill(egui::Color32::from_rgb(254, 226, 226))).clicked() {
            self.audio.stop();
            self.net.stop();
            std::process::exit(0);
        }
    }
}

fn over15_now(app: &SoundGuardApp) -> bool {
    app.rolling_15s_db.map(|d| d >= app.threshold_db).unwrap_or(false)
}
