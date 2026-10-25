use egui::{self, Color32, RichText};
use std::time::{Duration, Instant};

use crate::audio::AudioEngineHandle;
use crate::config::ClientConfig;
use crate::net::NetManager;
use crate::totp::TotpEngine;

#[derive(Debug, Clone, PartialEq)]
pub enum Panel { Dashboard, Settings }

#[derive(Debug, Clone, PartialEq)]
pub enum Phase { Consent, Setup, Running }


pub struct SoundGuardApp {
    pub phase: Phase,
    pub panel: Panel,
    pub config: ClientConfig,
    pub audio: AudioEngineHandle,
    pub net: NetManager,
    pub totp: TotpEngine,
    pub consent_accepted: bool,
    pub setup_server_url: String,
    pub setup_device_id: String,
    pub setup_device_token: String,
    pub setup_password: String,
    pub setup_confirm: String,
    pub setup_error: String,
    pub totp_code: String,
    pub totp_last_refresh: Instant,
    pub current_db: Option<f64>,
    pub rolling_15s_db: Option<f64>,
    pub average_db: Option<f64>,
    pub peak_db: Option<f64>,
    pub threshold_db: f64,
    pub waveform: Vec<[f64; 2]>,
    pub connected: bool,
    pub war_mode: bool,
    #[allow(dead_code)]
    pub call_active: bool,
    pub alert_volume: f32,
    #[allow(dead_code)]
    pub auto_answer: bool,
    pub toast: Option<(String, Instant, bool)>,
}

impl SoundGuardApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = ClientConfig::load_or_default();
        let consent = config.consent_accepted;
        let needs_setup = config.needs_setup();
        let audio = AudioEngineHandle::start(config.calibration_offset_db as f32);
        Self {
            phase: if !consent { Phase::Consent } else if needs_setup { Phase::Setup } else { Phase::Running },
            panel: Panel::Dashboard,
            config, audio,
            net: NetManager::new(),
            totp: TotpEngine::new(),
            consent_accepted: consent,
            setup_server_url: String::new(),
            setup_device_id: String::new(),
            setup_device_token: String::new(),
            setup_password: String::new(),
            setup_confirm: String::new(),
            setup_error: String::new(),
            totp_code: String::new(),
            totp_last_refresh: Instant::now(),
            current_db: None,
            rolling_15s_db: None,
            average_db: None,
            peak_db: None,
            threshold_db: 65.0,
            waveform: Vec::with_capacity(600),
            connected: false,
            war_mode: false,
            call_active: false,
            alert_volume: 0.22,
            auto_answer: true,
            toast: None,
        }
    }

    pub fn show_toast(&mut self, msg: impl Into<String>, err: bool) {
        self.toast = Some((msg.into(), Instant::now(), err));
    }

    pub fn tick(&mut self) {
        {
        let snap = self.audio.shared.snapshot();
        {
            self.current_db = snap.current_db;
            self.rolling_15s_db = snap.rolling_15s_db;
            self.average_db = snap.average_db;
            self.peak_db = snap.peak_db;
            if let Some(db) = snap.current_db {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64();
                self.waveform.push([now, db]);
                let cut = now - 60.0;
                self.waveform.retain(|p| p[0] >= cut);
            }
        }
        }
        if let Some(state) = self.net.poll() {
            self.connected = state.connected;
            self.threshold_db = state.threshold_db;
            self.war_mode = state.war_mode;
            if let Some(s) = &state.totp_secret {
                self.totp.set_secret(s.clone());
            }
        }
        if self.totp_last_refresh.elapsed() >= Duration::from_secs(5) {
            self.totp_code = self.totp.current_code();
            self.totp_last_refresh = Instant::now();
        }
    }
}
