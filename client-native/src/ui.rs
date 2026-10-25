//! Legacy layout measurements and colors, drawn entirely with egui.
use crate::{
    app::{Action, Phase, SoundGuardApp},
    net::Command,
};
use eframe::egui::{self, pos2, vec2, Align2, Color32 as C, FontId, Rect, RichText as R, Stroke};
const BG: C = C::from_rgb(244, 246, 248);
const CARD: C = C::WHITE;
const BORDER: C = C::from_rgb(229, 233, 239);
const TEXT: C = C::from_rgb(23, 32, 51);
const MUTED: C = C::from_rgb(117, 131, 153);
const TEAL: C = C::from_rgb(13, 122, 130);
const RED: C = C::from_rgb(214, 69, 69);
const GOLD: C = C::from_rgb(212, 160, 23);
fn card() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(15.0)
        .inner_margin(17.0)
}
fn text(p: &egui::Painter, pos: egui::Pos2, size: f32, color: C, s: impl ToString) {
    p.text(
        pos,
        Align2::LEFT_TOP,
        s.to_string(),
        FontId::proportional(size),
        color,
    );
}
fn button(ui: &mut egui::Ui, label: &str, accent: bool) -> egui::Response {
    ui.add(
        egui::Button::new(
            R::new(label)
                .size(13.0)
                .color(if accent { C::WHITE } else { TEXT }),
        )
        .fill(if accent { TEAL } else { BG })
        .rounding(8.0)
        .min_size(vec2(70.0, 36.0)),
    )
}
fn legacy_switch(ui: &mut egui::Ui, value: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(36.0, 20.0), egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    ui.painter()
        .rect_filled(rect, 10.0, if *value { TEAL } else { BORDER });
    ui.painter().circle_filled(
        pos2(
            if *value {
                rect.right() - 10.0
            } else {
                rect.left() + 10.0
            },
            rect.center().y,
        ),
        7.0,
        C::WHITE,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            *value,
            "自动接听",
        )
    });
    response
}
fn brand(ui: &egui::Ui, origin: egui::Pos2) {
    brand_lockup(ui, origin, None);
}
/// Text-first brand lockup; the legacy waveform app icon is intentionally not used here.
fn brand_lockup(ui: &egui::Ui, origin: egui::Pos2, subtitle: Option<&str>) {
    let p = ui.painter();
    let mark = Rect::from_min_size(origin, vec2(36.0, 36.0));
    p.rect_filled(mark, 10.0, TEAL);
    p.text(
        mark.center(),
        Align2::CENTER_CENTER,
        "声",
        FontId::proportional(20.0),
        C::WHITE,
    );
    let name_y = if subtitle.is_some() { 0.0 } else { 5.0 };
    text(p, origin + vec2(46.0, name_y), 20.0, TEXT, "声垣");
    if let Some(subtitle) = subtitle {
        text(p, origin + vec2(46.0, 24.0), 10.0, MUTED, subtitle);
    }
}
#[derive(Clone, Copy)]
enum Icon {
    Settings,
    Notice,
    Calls,
    Mic,
    Link,
}
fn draw_icon(p: &egui::Painter, rect: Rect, icon: Icon, color: C) {
    let o = rect.center();
    let stroke = Stroke::new(1.7_f32, color);
    let line = |a: [f32; 2], b: [f32; 2]| {
        p.line_segment([o + vec2(a[0], a[1]), o + vec2(b[0], b[1])], stroke);
    };
    match icon {
        Icon::Settings => {
            p.circle_stroke(o, 6.0, stroke);
            p.circle_stroke(o, 2.0, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                line(
                    [6.0 * a.cos(), 6.0 * a.sin()],
                    [9.0 * a.cos(), 9.0 * a.sin()],
                );
            }
        }
        Icon::Notice => {
            p.rect_stroke(Rect::from_center_size(o, vec2(13.0, 14.0)), 3.0, stroke);
            line([-4.0, -2.0], [4.0, -2.0]);
            line([-4.0, 2.0], [2.0, 2.0]);
        }
        Icon::Calls => {
            p.circle_stroke(o, 7.0, stroke);
            line([0.0, -4.0], [0.0, 0.0]);
            line([0.0, 0.0], [4.0, 2.0]);
        }
        Icon::Mic => {
            p.rect_stroke(
                Rect::from_center_size(o + vec2(0.0, -2.0), vec2(6.0, 10.0)),
                3.0,
                stroke,
            );
            line([-6.0, 0.0], [-6.0, 3.0]);
            line([-6.0, 3.0], [0.0, 6.0]);
            line([0.0, 6.0], [6.0, 3.0]);
            line([6.0, 3.0], [6.0, 0.0]);
            line([0.0, 6.0], [0.0, 9.0]);
        }
        Icon::Link => {
            p.rect_stroke(Rect::from_center_size(o, vec2(16.0, 11.0)), 2.0, stroke);
            line([-4.0, 8.0], [4.0, 8.0]);
            line([0.0, 6.0], [0.0, 8.0]);
        }
    }
}
fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(34.0, 34.0), egui::Sense::click());
    ui.painter().rect_filled(
        rect,
        8.0,
        if response.hovered() {
            C::from_rgb(230, 244, 245)
        } else {
            BG
        },
    );
    draw_icon(ui.painter(), rect, icon, TEAL);
    response.on_hover_text(label)
}
fn section_heading(ui: &mut egui::Ui, icon: Icon, title: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 8.0, C::from_rgb(230, 244, 245));
        draw_icon(ui.painter(), rect, icon, TEAL);
        ui.label(R::new(title).size(15.0).strong().color(TEXT));
    });
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, secret: bool) {
    ui.label(R::new(label).size(12.0).color(MUTED));
    ui.add_sized(
        [ui.available_width(), 36.0],
        egui::TextEdit::singleline(value)
            .font(FontId::proportional(14.0))
            .password(secret)
            .margin(vec2(10.0, 8.0)),
    );
    ui.add_space(10.0);
}
fn db(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.1}"))
        .unwrap_or_else(|| "--".into())
}
impl eframe::App for SoundGuardApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.tick();
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        self.frames += 1;
        if ctx.input(|i| i.viewport().close_requested()) && !self.exiting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_exit();
        }
        if self.exiting && self.status.exit_ready {
            self.audio.stop();
            self.net.stop();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        match self.phase {
            Phase::Running => self.dashboard(ctx),
            Phase::Setup => self.setup(ctx),
            Phase::Consent => self.consent(ctx),
            Phase::Recovery => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("配置读取失败");
                    ui.colored_label(RED, &self.error);
                    ui.label("原文件保持不变，请恢复 client-config.json 后重新启动。");
                });
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape))
            && self.auth_worker.is_none()
            && self.auth_pending.is_none()
        {
            if self.auth.is_some() {
                self.auth = None;
                self.auth_input.clear();
            } else if self.settings_open {
                self.cancel_settings();
            }
        }
        if self.auth.is_some() || self.settings_open {
            egui::Area::new("modal-scrim".into())
                .fixed_pos(pos2(0.0, 0.0))
                .order(egui::Order::Middle)
                .show(ctx, |ui| {
                    let rect = ctx.screen_rect();
                    ui.allocate_rect(rect, egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, C::from_black_alpha(80));
                });
        }
        if let Some(action) = self.auth {
            self.auth_modal(ctx, action);
        }
        if self.settings_open && self.auth.is_none() {
            self.drawer(ctx);
        }
        if self.status.call_state != "idle" {
            egui::Area::new("active-call".into())
                .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -50.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(TEXT)
                        .rounding(13.0)
                        .inner_margin(14.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let elapsed = self
                                    .status
                                    .call_started
                                    .map(|t| t.elapsed().as_secs())
                                    .unwrap_or(0);
                                ui.label(
                                    R::new(if self.status.call_state == "connecting" {
                                        "正在连接…"
                                    } else if self.status.call_mode == "broadcast" {
                                        "远程广播中"
                                    } else {
                                        "双向对讲中"
                                    })
                                    .color(C::WHITE),
                                );
                                ui.label(
                                    R::new(format!("{:02}:{:02}", elapsed / 60, elapsed % 60))
                                        .color(C::WHITE),
                                );
                                if ui
                                    .button(if self.muted { "取消静音" } else { "静音" })
                                    .clicked()
                                {
                                    self.muted = !self.muted;
                                    self.net.send(Command::Mute(self.muted));
                                }
                                if button(ui, "结束", false).clicked() {
                                    self.net.send(Command::Hangup);
                                    self.muted = false;
                                }
                            });
                        });
                });
        }
        if self.status.incoming.is_some() {
            egui::Window::new("收到管理端呼叫")
                .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("管理员请求与现场建立语音通道。");
                    ui.horizontal(|ui| {
                        if button(ui, "拒绝", false).clicked() {
                            self.net.send(Command::Decline);
                        }
                        if button(ui, "接听", true).clicked() {
                            self.net.send(Command::Accept);
                        }
                    });
                });
        }
        if let Some((message, time, error)) = &self.toast {
            if time.elapsed().as_secs() < 4 {
                egui::Area::new("toast".into())
                    .anchor(Align2::RIGHT_BOTTOM, vec2(-24.0, -48.0))
                    .show(ctx, |ui| {
                        card().show(ui, |ui| {
                            ui.colored_label(if *error { RED } else { TEAL }, message);
                        });
                    });
            }
        }
        if self.exiting && !self.status.exit_ready {
            egui::Window::new("正在关闭")
                .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("正在结束通话并提交关闭审计…");
                });
        }
        if self.frames == 15 && self.screenshot.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
        if let Some(path) = self.screenshot.clone().filter(|_| self.preview) {
            let captured = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = captured {
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(e) = image::save_buffer(
                    &path,
                    &bytes,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("Screenshot failed: {e}");
                }
                self.exiting = true;
                self.status.exit_ready = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
impl SoundGuardApp {
    fn consent(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |_| {});
        egui::Window::new("使用须知")
            .title_bar(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(500.0, 380.0))
            .frame(card())
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    brand_lockup(ui, ui.cursor().min, Some("使用须知"));
                    ui.add_space(30.0);
                    for (title, copy) in [
                        ("通知与邮件", "开启后，声垣会向你推送超标通知，并向管理员设置的邮箱发送告警邮件。"),
                        ("录音保存", "启用超标取证时，声垣会保存最近 30 秒的现场录音，供管理员在后台回放和取证。"),
                        ("责任说明", "声垣仅提供声音监测数据，测量结果仅供参考，不作为任何处理决定的唯一依据。因现场环境、设备故障或使用不当造成的后果，由使用方自行承担。声垣保留对本须知及产品功能的最终解释权。"),
                    ] {
                        ui.add_space(12.0);
                        ui.label(R::new(title).size(14.0).strong());
                        ui.label(R::new(copy).size(12.0).color(MUTED));
                    }
                    if !self.error.is_empty() {
                        ui.colored_label(RED, &self.error);
                    }
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        if button(ui, "退出", false).clicked() {
                            self.request_exit();
                        }
                        if button(ui, "同意并继续", true).clicked() {
                            self.accept_consent();
                        }
                    });
                });
            });
    }
    fn setup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(520.0);
                        ui.add_space(30.0);
                        card().show(ui, |ui| {
                            ui.set_width(470.0);
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                brand_lockup(ui, ui.cursor().min, Some("首次配置"));
                                ui.add_space(42.0);
                                ui.heading(R::new("连接声环境监测").size(24.0).color(TEXT));
                                ui.label(
                                    R::new("填写管理端生成的设备信息，并设置一个只能在本机使用的管理密码。完成后即可开始监测。")
                                        .size(12.0)
                                        .color(MUTED),
                                );
                                ui.add_space(18.0);
                                field(ui, "服务器地址", &mut self.draft.server_url, false);
                                field(ui, "设备编号", &mut self.draft.device_id, false);
                                field(ui, "设备令牌", &mut self.draft.device_token, true);
                                field(ui, "本地密码", &mut self.setup_password, true);
                                field(ui, "确认密码", &mut self.setup_confirm, true);
                                ui.label(
                                    R::new("本地密码用于打开设置、关闭自动接听和关闭客户端，只保存在本机。")
                                        .size(12.0)
                                        .color(MUTED),
                                );
                                if !self.error.is_empty() {
                                    ui.colored_label(RED, &self.error);
                                }
                                ui.add_space(14.0);
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 38.0],
                                        egui::Button::new(
                                            R::new("保存并开始监测").size(13.0).color(C::WHITE),
                                        )
                                        .fill(TEAL)
                                        .rounding(8.0),
                                    )
                                    .clicked()
                                {
                                    self.do_setup();
                                }
                            });
                        });
                    });
                });
            });
    }
    fn dashboard(&mut self, ctx: &egui::Context) {
        let accent = if self.status.policy.war() { GOLD } else { TEAL };
        egui::TopBottomPanel::top("legacy-header")
            .exact_height(64.0)
            .frame(
                egui::Frame::none()
                    .fill(CARD)
                    .inner_margin(egui::Margin::symmetric(22.0, 14.0)),
            )
            .show(ctx, |ui| {
                if self.auth.is_some() || self.settings_open || self.exiting {
                    ui.disable();
                }
                let drag = Rect::from_min_size(
                    ui.cursor().min,
                    vec2((ui.available_width() - 250.0).max(0.0), 36.0),
                );
                if ui
                    .interact(drag, ui.id().with("title-drag"), egui::Sense::drag())
                    .drag_started()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                brand(ui, ui.cursor().min);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_sized(
                            [32.0, 32.0],
                            egui::Button::new("×")
                                .fill(C::WHITE)
                                .stroke(Stroke::new(1.0_f32, BORDER))
                                .rounding(7.0),
                        )
                        .on_hover_text("关闭")
                        .clicked()
                    {
                        self.request_exit();
                    }
                    if icon_button(ui, Icon::Settings, "客户端设置").clicked() {
                        self.request_auth(Action::Settings);
                    }
                    ui.label(
                        R::new(format!("声垣 V{}", env!("CARGO_PKG_VERSION")))
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.label(
                        R::new(if self.status.connected {
                            "● 已连接"
                        } else {
                            "● 连接中"
                        })
                        .size(13.0)
                        .color(if self.status.connected {
                            TEAL
                        } else {
                            MUTED
                        }),
                    );
                });
            });
        egui::TopBottomPanel::bottom("legacy-footer")
            .exact_height(32.0)
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(22.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(R::new("© 2026 Cheerimy Studio").size(12.0).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            R::new(format!("声垣 V{}", env!("CARGO_PKG_VERSION")))
                                .size(12.0)
                                .color(MUTED),
                        );
                        ui.hyperlink_to("GitHub", "https://github.com/Cheerimy-Studio");
                        if self.preview {
                            ui.label(R::new("界面预览").size(12.0).color(MUTED));
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(22.0, 18.0)),
            )
            .show(ctx, |ui| {
                if self.auth.is_some() || self.settings_open || self.exiting {
                    ui.disable();
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let width = ui.available_width();
                    let left = (width - 376.0).max(420.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        ui.allocate_ui_with_layout(
                            vec2(left, 600.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(left);
                                ui.spacing_mut().item_spacing.x = 8.0;
                                self.monitor(ui, accent);
                            },
                        );
                        ui.vertical(|ui| {
                            ui.set_width(360.0);
                            self.sidebar(ui);
                        });
                    });
                });
            });
    }
    fn monitor(&mut self, ui: &mut egui::Ui, accent: C) {
        card().inner_margin(22.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    R::new(&self.status.policy.device_name)
                        .size(25.0)
                        .strong()
                        .color(TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.status.policy.war() {
                        "分贝战争"
                    } else if self.status.call_state != "idle" {
                        "通话中"
                    } else if !self.status.policy.capturing() {
                        "已暂停"
                    } else if self.current_db.is_some() {
                        "监测中"
                    } else {
                        "启动中"
                    };
                    ui.label(R::new(format!("● {label}")).size(12.0).color(accent));
                });
            });
            ui.add_space(8.0);
            let schedule = if self.status.policy.capture_start.is_empty() {
                "手动控制".to_string()
            } else {
                format!(
                    "定时采集 {} — {} · 服务器时区",
                    self.status.policy.capture_start, self.status.policy.capture_end
                )
            };
            ui.label(R::new(schedule).size(12.0).color(MUTED));
            ui.add_space(12.0);
            let over = !self.status.policy.war()
                && self
                    .average_db
                    .is_some_and(|v| v >= self.status.policy.threshold_db);
            let (_, number_rect) = ui.allocate_space(vec2(ui.available_width(), 78.0));
            let color = if over { RED } else { TEXT };
            let mut job = egui::text::LayoutJob::default();
            job.append(
                &db(self.current_db),
                0.0,
                egui::TextFormat {
                    font_id: FontId::new(78.0, egui::FontFamily::Name("legacy-numbers".into())),
                    color,
                    extra_letter_spacing: -4.68,
                    ..Default::default()
                },
            );
            let galley = ui.painter().layout_job(job);
            let number_width = galley.size().x;
            ui.painter()
                .galley(number_rect.min + vec2(0.0, -9.0), galley, color);
            text(
                ui.painter(),
                number_rect.min + vec2(number_width + 8.0, 53.0),
                13.0,
                MUTED,
                "dB(A)",
            );
            ui.label(R::new("瞬时声压").size(12.0).color(MUTED));
            ui.add_space(12.0);
            let (w, r) = ui.allocate_space(vec2(ui.available_width(), 5.0));
            let _ = w;
            ui.painter().rect_filled(r, 3.0, BORDER);
            if let Some(v) = self.current_db {
                let fill = Rect::from_min_size(
                    r.min,
                    vec2(r.width() * ((v as f32 - 30.0) / 80.0).clamp(0.0, 1.0), 5.0),
                );
                ui.painter().rect_filled(fill, 3.0, accent);
            }
            let x = r.left()
                + r.width()
                    * ((self.status.policy.threshold_db as f32 - 30.0) / 80.0).clamp(0.0, 1.0);
            ui.painter().line_segment(
                [pos2(x, r.top() - 3.0), pos2(x, r.bottom() + 3.0)],
                Stroke::new(1.0_f32, GOLD),
            );
            let (_, labels) = ui.allocate_space(vec2(r.width(), 12.0));
            for (i, n) in [30, 50, 70, 90, 110].iter().enumerate() {
                ui.painter().text(
                    pos2(
                        labels.left() + labels.width() * i as f32 / 4.0,
                        labels.top(),
                    ),
                    if i == 4 {
                        Align2::RIGHT_TOP
                    } else {
                        Align2::LEFT_TOP
                    },
                    n.to_string(),
                    FontId::proportional(11.0),
                    MUTED,
                );
            }

            ui.add_space(12.0);
            let width = ui.available_width();
            let values = [
                ("平均", db(self.average_db), "dB"),
                ("峰值", db(self.peak_db), "dB"),
                (
                    "阈值",
                    format!("{:.1}", self.status.policy.threshold_db),
                    "dB",
                ),
                (
                    "下次上报",
                    format!("{:.0}", 30.0 * (1.0 - self.progress)),
                    "s",
                ),
            ];
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                for (i, (name, value, unit)) in values.into_iter().enumerate() {
                    egui::Frame::none()
                        .fill(C::from_rgb(248, 250, 252))
                        .stroke(Stroke::new(1.0_f32, BORDER))
                        .rounding(8.0)
                        .inner_margin(11.0)
                        .show(ui, |ui| {
                            ui.set_width((width - 3.0) / 4.0 - 22.0);
                            ui.vertical(|ui| {
                                ui.label(R::new(name).size(12.0).color(MUTED));
                                ui.horizontal(|ui| {
                                    ui.label(R::new(value).size(18.0).strong().color(if i == 0 {
                                        accent
                                    } else {
                                        TEXT
                                    }));
                                    ui.label(R::new(unit).size(12.0).color(MUTED));
                                });
                            });
                        });
                }
            });
            if over {
                ui.add_space(12.0);
                ui.colored_label(RED, "环境声音超标　30 秒平均值超过设定阈值");
            } else if !self.status.policy.war()
                && self
                    .rolling_15s_db
                    .is_some_and(|v| v >= self.status.policy.threshold_db)
            {
                ui.add_space(10.0);
                ui.colored_label(GOLD, "环境声音预警　15 秒滑动平均值超过阈值");
            }
            if !self.preview {
                if let Some(error) = self.audio.shared.snapshot().error {
                    ui.add_space(8.0);
                    ui.colored_label(RED, error);
                }
            }
            if !self.status.last_error.is_empty() {
                ui.add_space(10.0);
                ui.colored_label(RED, &self.status.last_error);
            }
            ui.add_space(12.0);
            egui::Frame::none()
                .fill(C::from_rgb(248, 250, 252))
                .stroke(Stroke::new(1.0_f32, BORDER))
                .rounding(12.0)
                .inner_margin(14.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(R::new("声压波形").size(13.0).strong());
                        ui.label(R::new("最近 30 秒").size(13.0).color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(R::new("-- 阈值").size(13.0).color(GOLD));
                            ui.label(R::new("— 平均").size(13.0).color(C::from_rgb(37, 99, 235)));
                            ui.label(R::new("— 瞬时").size(13.0).color(accent));
                        });
                    });
                    let (_, r) = ui.allocate_space(vec2(ui.available_width(), 138.0));
                    self.chart(ui.painter(), r, accent);
                });
            ui.add_space(13.0);
            ui.horizontal(|ui| {
                ui.label(R::new("录制窗口").size(12.0).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        R::new(format!("{:.0} / 30 秒", self.progress * 30.0))
                            .size(12.0)
                            .color(MUTED),
                    );
                });
            });
            ui.add(
                egui::ProgressBar::new(self.progress)
                    .desired_height(5.0)
                    .fill(accent),
            );
        });
    }
    fn chart(&self, p: &egui::Painter, r: Rect, accent: C) {
        let plot = Rect::from_min_max(r.min + vec2(34.0, 10.0), r.max - vec2(42.0, 20.0));
        let mut values: Vec<f64> = self.waveform.iter().map(|p| p[1]).collect();
        values.push(self.status.policy.threshold_db);
        values.extend(self.average_db);
        let lo = (values.iter().copied().fold(30.0, f64::min) / 10.0).floor() * 10.0;
        let hi = ((values.iter().copied().fold(80.0, f64::max) + 6.0) / 10.0).ceil() * 10.0;
        let y = |db: f64| {
            plot.bottom() - ((db - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 * plot.height()
        };
        for i in 0..=4 {
            let value = lo + (hi - lo) * i as f64 / 4.0;
            let yy = y(value);
            p.line_segment(
                [pos2(plot.left(), yy), pos2(plot.right(), yy)],
                Stroke::new(0.5_f32, BORDER),
            );
            text(
                p,
                pos2(r.left() + 10.0, yy - 4.0),
                8.0,
                MUTED,
                format!("{value:.0}"),
            );
        }
        text(
            p,
            pos2(plot.left(), plot.bottom() + 6.0),
            8.0,
            MUTED,
            "30 秒前",
        );
        p.text(
            pos2(plot.right(), plot.bottom() + 6.0),
            Align2::RIGHT_TOP,
            "现在",
            FontId::proportional(11.0),
            MUTED,
        );
        let threshold_y = y(self.status.policy.threshold_db);
        text(
            p,
            pos2(plot.right() + 5.0, threshold_y - 4.0),
            8.0,
            GOLD,
            format!("阈值 {:.0}", self.status.policy.threshold_db),
        );
        for i in 0..(plot.width() / 10.0) as usize {
            let xx = plot.left() + i as f32 * 10.0;
            p.line_segment(
                [
                    pos2(xx, threshold_y),
                    pos2((xx + 5.0).min(plot.right()), threshold_y),
                ],
                Stroke::new(1.0_f32, GOLD),
            );
        }
        if let Some(avg) = self.average_db {
            p.line_segment(
                [pos2(plot.left(), y(avg)), pos2(plot.right(), y(avg))],
                Stroke::new(1.0_f32, C::from_rgb(37, 99, 235)),
            );
        }
        if !self.waveform.is_empty() {
            let now = self.waveform.last().unwrap()[0];
            let points: Vec<_> = self
                .waveform
                .iter()
                .map(|v| {
                    pos2(
                        plot.left() + ((v[0] - (now - 30.0)) / 30.0) as f32 * plot.width(),
                        y(v[1]),
                    )
                })
                .collect();
            if points.len() > 1 {
                p.add(egui::Shape::line(points, Stroke::new(2.0_f32, accent)));
            }
        } else {
            p.text(
                plot.center(),
                Align2::CENTER_CENTER,
                "等待音频数据",
                FontId::proportional(11.0),
                MUTED,
            );
        }
    }
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(324.0);
            section_heading(ui, Icon::Notice, "通知");
            ui.add_space(12.0);
            ui.label(
                R::new(if self.status.policy.notification_enabled {
                    self.status.policy.notification_title.as_str()
                } else {
                    "暂无通知"
                })
                .size(13.0)
                .strong(),
            );
            if self.status.policy.notification_enabled {
                ui.label(
                    R::new(&self.status.policy.notification_content)
                        .size(13.0)
                        .color(MUTED),
                );
            }
        });
        ui.add_space(8.0);
        card().show(ui, |ui| {
            ui.set_width(324.0);
            ui.horizontal(|ui| {
                section_heading(ui, Icon::Calls, "通话记录");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("刷新").clicked() {
                        self.net.send(Command::RefreshCalls);
                    }
                });
            });
            ui.label(R::new("管理端对讲与广播回放").size(12.0).color(MUTED));
            ui.add_space(12.0);
            if self.status.records.is_empty() {
                egui::Frame::none()
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .rounding(8.0)
                    .inner_margin(18.0)
                    .show(ui, |ui| {
                        ui.set_width(286.0);
                        ui.vertical_centered(|ui| {
                            ui.label(R::new("暂无通话记录").size(12.0).color(MUTED));
                        });
                    });
            }
            for record in self.status.records.clone() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(if record.mode == "broadcast" {
                            "远程广播"
                        } else {
                            "双向对讲"
                        });
                        ui.label(
                            R::new(format!(
                                "{}　{} 秒",
                                record.started_at, record.duration_secs
                            ))
                            .size(12.0)
                            .color(MUTED),
                        );
                    });
                    if ui
                        .add_enabled(record.has_audio, egui::Button::new("回放"))
                        .clicked()
                    {
                        self.net.send(Command::Play(record.id));
                    }
                });
            }
            if !self.status.records.is_empty() && ui.small_button("停止播放").clicked() {
                self.net.send(Command::StopPlayback);
            }
        });
        ui.add_space(8.0);
        card().show(ui, |ui| {
            ui.set_width(324.0);
            ui.horizontal(|ui| {
                section_heading(ui, Icon::Mic, "远程通话");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        R::new(if self.status.signaling {
                            "已连接"
                        } else {
                            "未连接"
                        })
                        .size(12.0)
                        .color(TEAL),
                    );
                });
            });
            ui.add_space(12.0);
            ui.label(
                R::new(if self.status.call_state == "idle" {
                    "等待管理端呼叫"
                } else if self.status.call_mode == "broadcast" {
                    "远程广播中"
                } else {
                    "双向对讲中"
                })
                .size(13.0)
                .strong(),
            );
            ui.label(
                R::new("管理员可通过管理端发起双向对讲或单向广播。")
                    .size(12.0)
                    .color(MUTED),
            );
            ui.add_space(14.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(R::new("自动接听").size(12.0).color(MUTED));
                let mut enabled = self.config.auto_answer;
                let gap = (ui.available_width() - 38.0).max(0.0);
                ui.add_space(gap);
                if legacy_switch(ui, &mut enabled).changed() {
                    if !enabled {
                        self.request_auth(Action::AutoAnswerOff);
                    } else {
                        let mut next = self.config.clone();
                        next.auto_answer = true;
                        match if self.preview {
                            Ok(())
                        } else {
                            next.save_to(&self.config_path)
                        } {
                            Ok(()) => {
                                self.config = next;
                                self.net.send(Command::AutoAnswer(true));
                            }
                            Err(e) => self.show_toast(e, true),
                        }
                    }
                }
            });
        });
        ui.add_space(8.0);
        card().show(ui, |ui| {
            ui.set_width(324.0);
            ui.horizontal(|ui| {
                section_heading(ui, Icon::Link, "连接状态");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.colored_label(
                        if self.status.connected { TEAL } else { MUTED },
                        if self.status.connected {
                            "● 在线"
                        } else {
                            "● 连接中"
                        },
                    );
                });
            });
            ui.add_space(12.0);
            ui.separator();
            egui::Grid::new("connection-grid")
                .num_columns(2)
                .spacing([12.0, 10.0])
                .show(ui, |ui| {
                    ui.label(R::new("服务器").size(12.0).color(MUTED));
                    ui.label(R::new(&self.config.server_url).size(12.0));
                    ui.end_row();
                    ui.label(R::new("设备编号").size(12.0).color(MUTED));
                    ui.label(R::new(&self.config.device_id).size(12.0));
                    ui.end_row();
                    ui.label(R::new("最后心跳").size(12.0).color(MUTED));
                    ui.label(
                        R::new(
                            self.status
                                .last_heartbeat
                                .map(|v| {
                                    v.with_timezone(&chrono::Local)
                                        .format("%H:%M:%S")
                                        .to_string()
                                })
                                .unwrap_or_else(|| "--".into()),
                        )
                        .size(12.0),
                    );
                    ui.end_row();
                });
            ui.add_space(14.0);
            ui.separator();
            if ui.link("打开客户端设置").clicked() {
                self.request_auth(Action::Settings);
            }
        });
    }
    fn auth_modal(&mut self, ctx: &egui::Context, action: Action) {
        egui::Window::new(if action == Action::Exit {
            "关闭客户端"
        } else {
            "管理员验证"
        })
        .order(egui::Order::Foreground)
        .title_bar(false)
        .movable(false)
        .frame(card().inner_margin(24.0))
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .fixed_size(vec2(400.0, 260.0))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.label(
                    R::new(if action == Action::Exit {
                        "关闭客户端 · 安全确认"
                    } else {
                        "管理员验证"
                    })
                    .size(18.0)
                    .strong(),
                );
                ui.add_space(12.0);
                ui.label(
                    R::new(if action == Action::Exit {
                        "为防止误关闭，请输入本地密码或管理端下发的 6 位动态验证码确认。"
                    } else {
                        "请输入本地密码、管理端固定密码或动态验证码以继续。"
                    })
                    .size(12.0)
                    .color(MUTED),
                );
                ui.add_space(10.0);
                let response = ui.add_sized(
                    [ui.available_width(), 36.0],
                    egui::TextEdit::singleline(&mut self.auth_input)
                        .password(true)
                        .hint_text("密码或 6 位动态验证码"),
                );
                if self.auth_input.is_empty() {
                    response.request_focus();
                }
                if !self.auth_error.is_empty() {
                    ui.colored_label(RED, &self.auth_error);
                }
                let busy = self.auth_worker.is_some() || self.auth_pending.is_some();
                ui.add_space(12.0);
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal(|ui| {
                        if button(ui, "取消", false).clicked() {
                            self.auth = None;
                            self.auth_input.clear();
                        }
                        if button(ui, if action == Action::Exit { "确认关闭" } else { "验证" }, true)
                            .clicked()
                            || response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            self.verify_auth();
                        }
                    });
                });
                if busy {
                    ui.label("正在验证…");
                }
            });
        });
    }
    fn drawer(&mut self, ctx: &egui::Context) {
        egui::Window::new("客户端设置")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .movable(false)
            .anchor(Align2::RIGHT_TOP, vec2(0.0, 0.0))
            .fixed_size(vec2(386.0, ctx.screen_rect().height() - 44.0))
            .frame(
                egui::Frame::none()
                    .fill(CARD)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .inner_margin(22.0),
            )
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_min_height(ctx.screen_rect().height() - 44.0);
                ui.horizontal(|ui| {
                    ui.label(R::new("客户端设置").size(18.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("×").clicked() {
                            self.cancel_settings();
                        }
                    });
                });
                ui.add_space(12.0);
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - 60.0).max(80.0))
                    .show(ui, |ui| {
                        ui.add_space(15.0);
                        ui.label(R::new("服务器与设备").size(13.0).strong());
                        ui.add_space(14.0);
                        field(ui, "服务器地址", &mut self.draft.server_url, false);
                        field(ui, "设备编号", &mut self.draft.device_id, false);
                        ui.label(R::new("告警提示音音量").size(12.0).color(MUTED));
                        ui.scope(|ui| {
                            ui.spacing_mut().interact_size.y = 18.0;
                            ui.spacing_mut().slider_width =
                                (ui.available_width() - 58.0).max(120.0);
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::Slider::new(&mut self.draft.alert_volume, 0.0..=1.0)
                                        .show_value(false),
                                );
                                ui.label(format!("{:.0}%", self.draft.alert_volume * 100.0));
                            });
                        });
                        ui.add_space(10.0);
                        field(
                            ui,
                            "设备令牌",
                            &mut self.draft.device_token,
                            !self.reveal_token,
                        );
                        if ui
                            .small_button(if self.reveal_token {
                                "隐藏"
                            } else {
                                "显示"
                            })
                            .clicked()
                        {
                            self.reveal_token = !self.reveal_token;
                        }
                        ui.label(
                            R::new("设备编号与令牌由管理端“设备连接”页面生成。")
                                .size(12.0)
                                .color(MUTED),
                        );
                        ui.add_space(18.0);
                        ui.separator();
                        ui.add_space(18.0);
                        ui.label(R::new("麦克风校准").size(13.0).strong());
                        ui.add_space(12.0);
                        ui.label(
                            R::new(format!("{:.1} dB 偏移", self.draft.calibration_offset_db))
                                .size(24.0)
                                .strong(),
                        );
                        ui.scope(|ui| {
                            ui.spacing_mut().interact_size.y = 18.0;
                            ui.spacing_mut().slider_width =
                                (ui.available_width() - 64.0).max(120.0);
                            ui.add(
                                egui::Slider::new(
                                    &mut self.draft.calibration_offset_db,
                                    40.0..=140.0,
                                )
                                .step_by(0.5),
                            );
                        });
                        ui.label(
                            R::new("用专业声级计对比当前读数，调整偏移直到数值接近。")
                                .size(12.0)
                                .color(MUTED),
                        );
                        ui.label(
                            R::new("校准后按保存才会应用；取消不会改变正在运行的配置。")
                                .size(12.0)
                                .color(MUTED),
                        );
                        if ui
                            .add_sized(
                                [ui.available_width(), 36.0],
                                egui::Button::new("重新初始化麦克风"),
                            )
                            .clicked()
                        {
                            self.audio.retry();
                        }
                        if !self.preview {
                            let snapshot = self.audio.shared.snapshot();
                            ui.label(
                                R::new(format!(
                                    "{} Hz / {} 声道 · {}",
                                    snapshot.source_rate,
                                    snapshot.channels,
                                    if snapshot.ready {
                                        "已就绪"
                                    } else {
                                        "等待初始化"
                                    }
                                ))
                                .size(12.0)
                                .color(MUTED),
                            );
                            if let Some(error) = snapshot.error {
                                ui.colored_label(RED, error);
                            }
                        }
                        ui.add_space(14.0);
                        ui.label(
                            R::new("降噪、阈值、通知、定时采集和分贝战争由管理端统一设置。")
                                .size(12.0)
                                .color(MUTED),
                        );
                        if !self.error.is_empty() {
                            ui.colored_label(RED, &self.error);
                        }
                    });
                ui.add_space(12.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if button(ui, "取消", false).clicked() {
                        self.cancel_settings();
                    }
                    if button(ui, "保存并重连", true).clicked() {
                        self.save_settings();
                    }
                });
            });
    }
}
