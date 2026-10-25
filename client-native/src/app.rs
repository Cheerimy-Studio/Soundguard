use crate::{
    audio::AudioEngineHandle,
    config::ClientConfig,
    net::{Command, NetManager, NetState},
    totp::TotpEngine,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
#[path = "../../client/common/security.rs"]
pub mod security;
#[derive(Clone, PartialEq)]
pub enum Phase {
    Consent,
    Setup,
    Running,
    Recovery,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Settings,
    Exit,
    AutoAnswerOff,
}
pub struct SoundGuardApp {
    pub phase: Phase,
    pub config: ClientConfig,
    pub config_path: PathBuf,
    pub draft: ClientConfig,
    pub audio: AudioEngineHandle,
    pub net: NetManager,
    pub status: NetState,
    pub totp: TotpEngine,
    pub setup_password: String,
    pub setup_confirm: String,
    pub error: String,
    pub settings_open: bool,
    pub reveal_token: bool,
    pub auth: Option<Action>,
    pub auth_input: String,
    pub auth_error: String,
    pub guard: security::PasswordGuard,
    pub auth_worker: Option<std::sync::mpsc::Receiver<bool>>,
    pub auth_pending: Option<(u64, Instant)>,
    pub next_auth_id: u64,
    pub current_db: Option<f64>,
    pub average_db: Option<f64>,
    pub peak_db: Option<f64>,
    pub rolling_15s_db: Option<f64>,
    pub progress: f32,
    pub waveform: Vec<[f64; 2]>,
    pub last_wave: Instant,
    pub muted: bool,
    pub toast: Option<(String, Instant, bool)>,
    pub exiting: bool,
    pub preview: bool,
    pub screenshot: Option<String>,
    pub frames: u32,
    pub last_alarm: Option<Instant>,
}
impl SoundGuardApp {
    pub fn new(_: &eframe::CreationContext<'_>) -> Self {
        Self::initialize(ClientConfig::config_path(), std::env::args().collect())
    }
    fn initialize(path: PathBuf, args: Vec<String>) -> Self {
        let loaded = ClientConfig::load(&path);
        let recovery = loaded.as_ref().err().cloned();
        let config = loaded.unwrap_or_default();
        let phase = if recovery.is_some() {
            Phase::Recovery
        } else if !config.consent_accepted {
            Phase::Consent
        } else if config.needs_setup() {
            Phase::Setup
        } else {
            Phase::Running
        };
        let audio = AudioEngineHandle::start(config.calibration_offset_db as f32);
        let mut net = NetManager::new();
        let preview = args.iter().any(|s| s == "--preview");
        if phase == Phase::Running && !preview {
            net.start(config.clone(), audio.clone());
        }
        let screenshot = args
            .iter()
            .position(|s| s == "--screenshot")
            .and_then(|i| args.get(i + 1))
            .cloned();
        let mut app = Self {
            phase,
            config: config.clone(),
            config_path: path,
            draft: config,
            audio,
            net,
            status: NetState::default(),
            totp: TotpEngine::new(),
            setup_password: String::new(),
            setup_confirm: String::new(),
            error: recovery.unwrap_or_default(),
            settings_open: false,
            reveal_token: false,
            auth: None,
            auth_input: String::new(),
            auth_error: String::new(),
            guard: security::PasswordGuard::default(),
            auth_worker: None,
            auth_pending: None,
            next_auth_id: 0,
            current_db: None,
            average_db: None,
            peak_db: None,
            rolling_15s_db: None,
            progress: 0.0,
            waveform: Vec::new(),
            last_wave: Instant::now() - Duration::from_secs(1),
            muted: false,
            toast: None,
            exiting: false,
            preview,
            screenshot,
            frames: 0,
            last_alarm: None,
        };
        if preview {
            app.phase = Phase::Running;
            app.status.connected = true;
            app.status.signaling = true;
            app.status.policy.device_name = "现场采集节点".into();
            app.status.last_heartbeat = Some(chrono::Utc::now());
            app.config.device_id = "dev-preview".into();
            app.draft = app.config.clone();
            app.current_db = Some(48.6);
            app.average_db = Some(47.8);
            app.peak_db = Some(56.3);
            app.progress = 0.4;
            let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
            app.waveform = (0..120)
                .map(|i| {
                    [
                        now - 30.0 + i as f64 * 0.25,
                        47.0 + (i as f64 * 0.2).sin() * 5.0,
                    ]
                })
                .collect();
        }
        if preview {
            if let Some(screen) = args
                .iter()
                .position(|v| v == "--preview-screen")
                .and_then(|i| args.get(i + 1))
            {
                match screen.as_str() {
                    "settings" => app.settings_open = true,
                    "auth" => app.auth = Some(Action::Settings),
                    "consent" => app.phase = Phase::Consent,
                    "setup" => app.phase = Phase::Setup,
                    "war" => {
                        app.status.policy.war_mode_enabled = true;
                        app.status.policy.war_mode_start = "00:00".into();
                        app.status.policy.war_mode_end = "00:00".into();
                    }
                    _ => {}
                }
            }
        }
        app
    }
    pub fn show_toast(&mut self, msg: impl Into<String>, err: bool) {
        self.toast = Some((msg.into(), Instant::now(), err));
    }
    pub fn tick(&mut self) {
        if !self.preview {
            let s = self.audio.shared.snapshot();
            self.current_db = s.current_db;
            self.average_db = s.average_db;
            self.rolling_15s_db = s.rolling_15s_db;
            self.peak_db = s.peak_db;
            self.progress = s.progress;
            if self.last_wave.elapsed() >= Duration::from_secs(1) {
                self.last_wave = Instant::now();
                let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
                if let Some(db) = s.current_db {
                    self.waveform.push([now, db]);
                }
                self.waveform.retain(|p| p[0] >= now - 30.0);
            }
            self.status = self.net.poll();
            if self.status.call_state == "idle" {
                self.muted = false;
            }
            let over = s.capturing
                && !s.suspended
                && self.status.call_state == "idle"
                && !self.status.policy.war()
                && s.average_db
                    .is_some_and(|v| v >= self.status.policy.threshold_db);
            if over
                && self.config.alert_volume > 0.0
                && self.last_alarm.is_none_or(|t| {
                    t.elapsed().as_secs() >= self.status.policy.alert_cooldown_secs.max(30)
                })
            {
                self.last_alarm = Some(Instant::now());
                let volume = self.config.alert_volume;
                std::thread::spawn(move || {
                    if let Ok(out) = crate::playback::AudioOutput::new() {
                        out.volume(volume);
                        out.pcm(1, 48000, crate::playback::alarm_samples());
                        std::thread::sleep(Duration::from_millis(1100));
                    }
                });
            }
            self.totp.set_secret(self.status.policy.totp_secret.clone());
            if let Some(server) = self.status.policy.server_time {
                self.totp
                    .set_offset(server.timestamp() - chrono::Utc::now().timestamp());
            }
        }
        if let Some(rx) = &self.auth_worker {
            if let Ok(valid) = rx.try_recv() {
                self.auth_worker = None;
                if valid {
                    self.finish_auth(true, String::new());
                } else if self.status.connected && self.status.policy.has_fixed_password {
                    self.next_auth_id += 1;
                    let id = self.next_auth_id;
                    if self.net.send(Command::Verify(id, self.auth_input.clone())) {
                        self.auth_pending = Some((id, Instant::now()));
                    } else {
                        self.finish_auth(false, "服务器未连接".into());
                    }
                } else {
                    self.finish_auth(false, "验证码或密码不正确".into());
                }
            }
        }
        if let Some((id, time)) = self.auth_pending {
            if let Some((result_id, ok, error)) = &self.status.auth_result {
                if *result_id == id {
                    let (ok, error) = (*ok, error.clone());
                    self.auth_pending = None;
                    self.finish_auth(ok, error);
                }
            }
            if self.auth_pending.is_some() && time.elapsed() > Duration::from_secs(10) {
                self.auth_pending = None;
                self.finish_auth(false, "验证超时，请稍后再试".into());
            }
        }
    }
    pub fn request_auth(&mut self, action: Action) {
        if self.auth.is_some() {
            return;
        }
        if self.preview {
            if action == Action::Settings {
                self.settings_open = true;
            } else if action == Action::Exit {
                self.exiting = true;
                self.status.exit_ready = true;
            }
            return;
        }
        self.auth = Some(action);
        self.auth_input.clear();
        self.auth_error.clear();
    }
    pub fn verify_auth(&mut self) {
        if self.auth_worker.is_some() || self.auth_pending.is_some() {
            return;
        }
        if self.auth_input.is_empty() || self.auth_input.len() > 512 {
            self.auth_error = "请输入有效的密码或六位验证码".into();
            return;
        }
        if let Err(e) = self.guard.check_unlocked() {
            self.auth_error = e;
            return;
        }
        let hash = self.config.local_password_hash.clone();
        let input = self.auth_input.clone();
        let totp = self.totp.verify(&input);
        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_worker = Some(rx);
        std::thread::spawn(move || {
            use argon2::{Argon2, PasswordHash, PasswordVerifier};
            let ok = totp
                || PasswordHash::new(&hash).ok().is_some_and(|h| {
                    Argon2::default()
                        .verify_password(input.as_bytes(), &h)
                        .is_ok()
                });
            let _ = tx.send(ok);
        });
    }
    fn finish_auth(&mut self, ok: bool, error: String) {
        if let Err(e) = self.guard.verify(|| ok) {
            self.auth_error = if error.is_empty() {
                e
            } else {
                format!("{error}；{e}")
            };
            return;
        }
        self.auth_input.clear();
        self.auth_error.clear();
        match self.auth.take() {
            Some(Action::Settings) => {
                self.draft = self.config.clone();
                self.settings_open = true;
                self.error.clear();
            }
            Some(Action::AutoAnswerOff) => {
                let mut next = self.config.clone();
                next.auto_answer = false;
                if let Err(e) = next.save_to(&self.config_path) {
                    self.show_toast(e, true);
                } else {
                    self.config = next;
                    self.net.send(Command::AutoAnswer(false));
                }
            }
            Some(Action::Exit) => {
                self.exiting = true;
                if !self.net.send(Command::Quit) {
                    self.status.exit_ready = true;
                }
            }
            None => {}
        }
    }
    pub fn save_settings(&mut self) {
        if self.preview {
            self.settings_open = false;
            self.show_toast("预览模式：设置未写入磁盘", false);
            return;
        }
        if let Err(e) = self.guard.require_authorized() {
            self.error = e;
            self.settings_open = false;
            self.request_auth(Action::Settings);
            return;
        }
        let mut next = match self.draft.clone().normalize() {
            Ok(v) => v,
            Err(e) => {
                self.error = e;
                return;
            }
        };
        next.local_password_hash = self.config.local_password_hash.clone();
        if let Err(e) = next.save_to(&self.config_path) {
            self.error = e;
            return;
        }
        let rebind = next.server_url != self.config.server_url
            || next.device_id != self.config.device_id
            || next.device_token != self.config.device_token;
        self.config = next;
        self.audio
            .shared
            .set_offset(self.config.calibration_offset_db as f32);
        if rebind {
            self.net.send(Command::Hangup);
            self.net.start(self.config.clone(), self.audio.clone());
            self.status = NetState::default();
            self.waveform.clear();
        }
        self.settings_open = false;
        self.show_toast("设置已保存", false);
    }
    pub fn cancel_settings(&mut self) {
        self.settings_open = false;
        self.reveal_token = false;
        self.draft = self.config.clone();
        self.error.clear();
    }
    pub fn accept_consent(&mut self) {
        let mut next = self.config.clone();
        next.consent_accepted = true;
        if let Err(e) = next.save_to(&self.config_path) {
            self.error = e;
            return;
        }
        self.config = next;
        self.phase = if self.config.needs_setup() {
            Phase::Setup
        } else {
            self.net.start(self.config.clone(), self.audio.clone());
            Phase::Running
        };
    }
    pub fn do_setup(&mut self) {
        if self.setup_password != self.setup_confirm {
            self.error = "两次输入的本地密码不一致".into();
            return;
        }
        if !(8..=128).contains(&self.setup_password.chars().count()) {
            self.error = "本地密码需要 8–128 个字符".into();
            return;
        }
        let mut next = match self.draft.clone().normalize() {
            Ok(c) => c,
            Err(e) => {
                self.error = e;
                return;
            }
        };
        use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
        let salt = SaltString::generate(&mut rand_core::OsRng);
        next.local_password_hash =
            match Argon2::default().hash_password(self.setup_password.as_bytes(), &salt) {
                Ok(h) => h.to_string(),
                Err(e) => {
                    self.error = e.to_string();
                    return;
                }
            };
        next.consent_accepted = true;
        if let Err(e) = next.save_to(&self.config_path) {
            self.error = e;
            return;
        }
        self.config = next;
        self.setup_password.clear();
        self.setup_confirm.clear();
        self.error.clear();
        self.phase = Phase::Running;
        self.net.start(self.config.clone(), self.audio.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> SoundGuardApp {
        let path = std::env::temp_dir().join(format!("sg-ui-{}.json", uuid::Uuid::new_v4()));
        let config = ClientConfig {
            device_id: "room-test".into(),
            device_token: "test-token".into(),
            local_password_hash: "hash".into(),
            consent_accepted: false,
            ..Default::default()
        };
        config.save_to(&path).unwrap();
        SoundGuardApp::initialize(path, vec![])
    }
    #[test]
    fn settings_cancel_discards_draft_without_changing_disk_or_meter() {
        let mut app = app();
        let original = std::fs::read(&app.config_path).unwrap();
        app.settings_open = true;
        app.draft.calibration_offset_db = 120.0;
        app.draft.server_url = "https://other.example".into();
        app.cancel_settings();
        assert!(!app.settings_open);
        assert_eq!(app.config.calibration_offset_db, 90.0);
        assert_eq!(app.draft.calibration_offset_db, 90.0);
        assert_eq!(app.audio.shared.offset(), 90.0);
        assert_eq!(std::fs::read(&app.config_path).unwrap(), original);
        std::fs::remove_file(&app.config_path).unwrap();
        app.audio.stop();
    }
    #[test]
    fn settings_require_authorization_then_apply_only_after_valid_save() {
        let mut app = app();
        app.settings_open = true;
        app.draft.calibration_offset_db = 100.0;
        app.save_settings();
        assert_eq!(app.config.calibration_offset_db, 90.0);
        assert_eq!(app.auth, Some(Action::Settings));
        app.auth = None;
        app.guard.verify(|| true).unwrap();
        app.draft.calibration_offset_db = 100.0;
        app.save_settings();
        assert_eq!(app.audio.shared.offset(), 100.0);
        assert_eq!(
            ClientConfig::load(&app.config_path)
                .unwrap()
                .calibration_offset_db,
            100.0
        );
        app.draft.server_url = "file:///secret".into();
        app.save_settings();
        assert!(!app.error.is_empty());
        assert_eq!(app.config.calibration_offset_db, 100.0);
        std::fs::remove_file(&app.config_path).unwrap();
        app.audio.stop();
    }
    #[test]
    fn repeated_close_does_not_replace_inflight_authentication() {
        let mut app = app();
        app.request_auth(Action::Settings);
        app.auth_input = "in-progress".into();
        app.request_auth(Action::Exit);
        assert_eq!(app.auth, Some(Action::Settings));
        assert_eq!(app.auth_input, "in-progress");
        std::fs::remove_file(&app.config_path).unwrap();
        app.audio.stop();
    }
}
