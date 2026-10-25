use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::config::parse_recipients;

pub const MAX_ALERTS_PER_DEVICE: usize = 50;
pub const MAX_CALLS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub threshold_db: f64,
    pub alert_recipients: Vec<String>,
    pub report_recipients: Vec<String>,
    pub daily_report_enabled: bool,
    pub daily_report_time: String,
    pub offline_timeout_secs: u64,
    pub alert_cooldown_secs: u64,
    pub evidence_seconds: u32,
    pub auto_record_evidence: bool,
    pub retention_days: u32,
    #[serde(default)]
    pub notification_enabled: bool,
    #[serde(default)]
    pub notification_title: String,
    #[serde(default)]
    pub notification_content: String,
    /// AI 降噪模式：off / light / standard / strong（RNNoise）。
    #[serde(default = "default_denoise_mode")]
    pub denoise_mode: String,
    /// 人声增强：降噪后对语音频段做临场感提升。
    #[serde(default)]
    pub voice_enhance: bool,
    /// 分贝战争模式：在指定时段内启用金色 UI 和排行榜。
    #[serde(default)]
    pub war_mode_enabled: bool,
    #[serde(default = "default_war_mode_start")]
    pub war_mode_start: String,
    #[serde(default = "default_war_mode_end")]
    pub war_mode_end: String,
    /// 固定操作密码的 Argon2 哈希。
    #[serde(default)]
    pub fixed_password_hash: String,
    /// 自动开启采集的时段（服务器时间），空则始终手动控制。
    #[serde(default)]
    pub capture_start: String,
    #[serde(default)]
    pub capture_end: String,
}

fn default_denoise_mode() -> String {
    "off".to_string()
}

fn default_war_mode_start() -> String {
    "07:30".to_string()
}

fn default_war_mode_end() -> String {
    "08:00".to_string()
}

fn parse_hhmm(value: &str) -> Option<u32> {
    let (hour, minute) = value.split_once(':')?;
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

pub fn normalize_hhmm(name: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let (hour, minute) = value
        .split_once(':')
        .and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)))
        .filter(|(h, m)| *h < 24 && *m < 60)
        .ok_or_else(|| format!("{name} 格式应为 HH:MM"))?;
    Ok(format!("{hour:02}:{minute:02}"))
}

impl Settings {
    pub fn war_active(&self, now: chrono::DateTime<chrono::Local>) -> bool {
        use chrono::Timelike;
        self.war_mode_enabled
            && Self::time_window(
                &self.war_mode_start,
                &self.war_mode_end,
                now.hour() * 60 + now.minute(),
            )
    }
    pub fn capture_active(&self, now: chrono::DateTime<chrono::Local>) -> bool {
        use chrono::Timelike;
        Self::time_window(
            &self.capture_start,
            &self.capture_end,
            now.hour() * 60 + now.minute(),
        )
    }
    /// Empty start means "from midnight"; empty end means "until midnight".
    /// This allows an auto-start-only or auto-stop-only daily schedule.
    pub fn time_window(start: &str, end: &str, minute: u32) -> bool {
        match (parse_hhmm(start), parse_hhmm(end)) {
            (None, None) => true,
            (Some(a), None) => minute >= a,
            (None, Some(b)) => minute < b,
            (Some(a), Some(b)) if a < b => minute >= a && minute < b,
            (Some(a), Some(b)) if a > b => minute >= a || minute < b,
            (Some(_), Some(_)) => true,
        }
    }
    pub fn validate_and_normalize(&mut self) -> Result<(), String> {
        if !self.threshold_db.is_finite() || !(30.0..=130.0).contains(&self.threshold_db) {
            return Err("分贝阈值必须在 30–130 dB 之间".to_string());
        }
        if !(20..=3600).contains(&self.offline_timeout_secs) {
            return Err("离线判定时间必须在 20–3600 秒之间".to_string());
        }
        if !(30..=86_400).contains(&self.alert_cooldown_secs) {
            return Err("告警冷却时间必须在 30–86400 秒之间".to_string());
        }
        // Alerts must retain the complete 30-second averaging interval.
        self.evidence_seconds = 30;
        if !(1..=365).contains(&self.retention_days) {
            return Err("数据保留天数必须在 1–365 天之间".to_string());
        }
        let (hour, minute) = self
            .daily_report_time
            .split_once(':')
            .and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)))
            .ok_or_else(|| "日报时间格式应为 HH:MM".to_string())?;
        if hour > 23 || minute > 59 {
            return Err("日报时间格式应为 HH:MM".to_string());
        }
        self.daily_report_time = format!("{hour:02}:{minute:02}");

        for raw in self.alert_recipients.iter().chain(&self.report_recipients) {
            for address in raw
                .split([',', ';', ' ', '\n'])
                .map(str::trim)
                .filter(|a| !a.is_empty())
            {
                if address.parse::<lettre::Address>().is_err() {
                    return Err("收件人邮箱格式不正确".to_string());
                }
            }
        }
        self.alert_recipients = parse_recipients(&self.alert_recipients.join(","));
        self.report_recipients = parse_recipients(&self.report_recipients.join(","));
        if self.alert_recipients.len() > 20 || self.report_recipients.len() > 20 {
            return Err("收件人数量不能超过 20 个".to_string());
        }
        self.threshold_db = (self.threshold_db * 10.0).round() / 10.0;
        self.denoise_mode = self.denoise_mode.trim().to_ascii_lowercase();
        for (name, val) in [
            ("war_mode_start", &mut self.war_mode_start),
            ("war_mode_end", &mut self.war_mode_end),
            ("capture_start", &mut self.capture_start),
            ("capture_end", &mut self.capture_end),
        ] {
            *val = normalize_hhmm(name, val)?;
        }
        if self.war_mode_enabled && (self.war_mode_start.is_empty() || self.war_mode_end.is_empty())
        {
            return Err("开启分贝战争时请填写完整时段".into());
        }
        if !matches!(
            self.denoise_mode.as_str(),
            "off" | "light" | "standard" | "strong"
        ) {
            return Err("降噪模式必须为 off / light / standard / strong".to_string());
        }
        self.notification_title = self.notification_title.trim().chars().take(80).collect();
        self.notification_content = self
            .notification_content
            .trim()
            .chars()
            .take(2000)
            .collect();
        if self.notification_enabled
            && self.notification_title.is_empty()
            && self.notification_content.is_empty()
        {
            return Err("启用客户端通知时，请至少填写标题或内容".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSettings {
    #[serde(default)]
    pub threshold_db: Option<f64>,
    #[serde(default)]
    pub alert_cooldown_secs: Option<u64>,
    #[serde(default)]
    pub offline_timeout_secs: Option<u64>,
    #[serde(default)]
    pub auto_record_evidence: Option<bool>,
    #[serde(default)]
    pub capture_start: Option<String>,
    #[serde(default)]
    pub capture_end: Option<String>,
}

impl DeviceSettings {
    pub fn validate_and_normalize(&mut self) -> Result<(), String> {
        if let Some(value) = self.threshold_db {
            if !value.is_finite() || !(30.0..=130.0).contains(&value) {
                return Err("设备分贝阈值必须在 30–130 dB 之间".to_string());
            }
            self.threshold_db = Some((value * 10.0).round() / 10.0);
        }
        if let Some(value) = self.alert_cooldown_secs {
            if !(30..=86_400).contains(&value) {
                return Err("设备告警冷却时间必须在 30–86400 秒之间".to_string());
            }
        }
        if let Some(value) = self.offline_timeout_secs {
            if !(20..=3600).contains(&value) {
                return Err("设备离线判定时间必须在 20–3600 秒之间".to_string());
            }
        }
        for (name, value) in [
            ("captureStart", &mut self.capture_start),
            ("captureEnd", &mut self.capture_end),
        ] {
            if let Some(raw) = value {
                *raw = normalize_hhmm(name, raw)?;
            }
        }
        Ok(())
    }

    pub fn apply(&self, base: &Settings) -> Settings {
        let mut result = base.clone();
        if let Some(value) = self.threshold_db {
            result.threshold_db = value;
        }
        if let Some(value) = self.alert_cooldown_secs {
            result.alert_cooldown_secs = value;
        }
        if let Some(value) = self.offline_timeout_secs {
            result.offline_timeout_secs = value;
        }
        if let Some(value) = self.auto_record_evidence {
            result.auto_record_evidence = value;
        }
        if let Some(value) = &self.capture_start {
            result.capture_start = value.clone();
        }
        if let Some(value) = &self.capture_end {
            result.capture_end = value.clone();
        }
        // Evidence must cover the complete 30-second averaging window.
        result.evidence_seconds = 30;
        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub totp_secret: String,
    #[serde(default)]
    pub settings: DeviceSettings,
    #[serde(default)]
    pub token_hash: String,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    pub desired_monitoring: bool,
    pub monitoring: bool,
    pub current_db: Option<f64>,
    pub peak_db: Option<f64>,
    pub last_seen: Option<DateTime<Utc>>,
    pub last_reading_at: Option<DateTime<Utc>>,
    pub last_stop_at: Option<DateTime<Utc>>,
    pub last_high_alert_at: Option<DateTime<Utc>>,
    pub expected_offline: bool,
    pub offline_alert_active: bool,
    pub app_version: Option<String>,
    pub capture_error: Option<String>,
    #[serde(default)]
    pub live_avg_db: Option<f64>,
    #[serde(default)]
    pub live_rolling_15s_db: Option<f64>,
    #[serde(default)]
    pub live_suspended: bool,
    #[serde(default)]
    pub live_system_playing: bool,
    #[serde(default)]
    pub live_aec_enabled: bool,
    #[serde(default)]
    pub stop_failures: u32,
    #[serde(default)]
    pub stop_locked_until: Option<DateTime<Utc>>,
}

impl Device {
    pub fn new(id: String, name: String, token_hash: String) -> Self {
        Self {
            id,
            name,
            settings: DeviceSettings::default(),
            token_hash,
            created_at: Some(Utc::now()),
            desired_monitoring: true,
            monitoring: false,
            current_db: None,
            peak_db: None,
            last_seen: None,
            last_reading_at: None,
            last_stop_at: None,
            last_high_alert_at: None,
            expected_offline: false,
            offline_alert_active: false,
            app_version: None,
            capture_error: None,
            live_avg_db: None,
            live_rolling_15s_db: None,
            live_suspended: false,
            live_system_playing: false,
            live_aec_enabled: false,
            stop_failures: 0,
            stop_locked_until: None,
            totp_secret: crate::totp::generate_secret(),
        }
    }

    pub fn is_online(&self, now: DateTime<Utc>, offline_timeout_secs: u64) -> bool {
        !self.expected_offline
            && self.last_seen.is_some_and(|seen| {
                now.signed_duration_since(seen).num_seconds() <= offline_timeout_secs as i64
            })
    }

    pub fn view(&self, now: DateTime<Utc>, offline_timeout_secs: u64) -> DeviceView {
        let online = self.is_online(now, offline_timeout_secs);
        let status = if self.expected_offline {
            "stopped"
        } else if online {
            if self.capture_error.is_some() {
                "error"
            } else if self.monitoring {
                "monitoring"
            } else if self.desired_monitoring {
                "starting"
            } else {
                "paused"
            }
        } else if self.offline_alert_active {
            "offline_alert"
        } else if self.last_seen.is_some() {
            "offline"
        } else {
            "waiting"
        };

        let has_measurement = online
            && self.monitoring
            && self.capture_error.is_none()
            && !self.live_suspended
            && !(self.live_system_playing && !self.live_aec_enabled);
        DeviceView {
            id: self.id.clone(),
            name: self.name.clone(),
            settings: self.settings.clone(),
            created_at: self.created_at,
            desired_monitoring: self.desired_monitoring,
            monitoring: self.monitoring,
            current_db: has_measurement.then_some(self.current_db).flatten(),
            peak_db: has_measurement.then_some(self.peak_db).flatten(),
            last_seen: self.last_seen,
            last_reading_at: self.last_reading_at,
            last_stop_at: self.last_stop_at,
            last_high_alert_at: self.last_high_alert_at,
            expected_offline: self.expected_offline,
            offline_alert_active: self.offline_alert_active,
            app_version: self.app_version.clone(),
            capture_error: self.capture_error.clone(),
            live_avg_db: has_measurement.then_some(self.live_avg_db).flatten(),
            live_rolling_15s_db: has_measurement
                .then_some(self.live_rolling_15s_db)
                .flatten(),
            live_suspended: self.live_suspended,
            live_system_playing: self.live_system_playing,
            live_aec_enabled: self.live_aec_enabled,
            online,
            status: status.to_string(),
        }
    }
}

pub fn effective_settings(base: &Settings, device: &Device) -> Settings {
    device.settings.apply(base)
}

/// Recipients for an event scoped to a device: configured global recipients,
/// all active super administrators, and device administrators assigned to it.
pub fn notification_recipients(
    snapshot: &PersistedState,
    device_id: &str,
    base: &[String],
) -> Vec<String> {
    recipients_from_users(&snapshot.users, device_id, base)
}

fn recipients_from_users(users: &[User], device_id: &str, base: &[String]) -> Vec<String> {
    let mut recipients = base.to_vec();
    for user in users {
        if user.disabled || user.email.trim().is_empty() {
            continue;
        }
        if user.role.is_update_admin()
            || user.device_ids.iter().any(|assigned| assigned == device_id)
        {
            recipients.push(user.email.trim().to_string());
        }
    }
    let mut seen = std::collections::HashSet::new();
    recipients
        .into_iter()
        .filter(|email| !email.is_empty())
        .filter(|email| seen.insert(email.to_ascii_lowercase()))
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub settings: DeviceSettings,
    pub created_at: Option<DateTime<Utc>>,
    pub desired_monitoring: bool,
    pub monitoring: bool,
    pub current_db: Option<f64>,
    pub peak_db: Option<f64>,
    pub last_seen: Option<DateTime<Utc>>,
    pub last_reading_at: Option<DateTime<Utc>>,
    pub last_stop_at: Option<DateTime<Utc>>,
    pub last_high_alert_at: Option<DateTime<Utc>>,
    pub expected_offline: bool,
    pub offline_alert_active: bool,
    pub app_version: Option<String>,
    pub capture_error: Option<String>,
    pub live_avg_db: Option<f64>,
    #[serde(default)]
    pub live_rolling_15s_db: Option<f64>,
    pub live_suspended: bool,
    pub live_system_playing: bool,
    pub live_aec_enabled: bool,
    pub online: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    Noise,
    Offline,
    Recovered,
    Stopped,
    Tamper,
    Control,
    Report,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: String,
    #[serde(default)]
    pub device_id: String,
    pub kind: AlertKind,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub acknowledged: bool,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub db: Option<f64>,
    pub evidence_file: Option<String>,
    pub evidence_duration_secs: Option<u32>,
    pub email_status: String,
    pub email_error: Option<String>,
}

impl Alert {
    pub fn new(
        device_id: impl Into<String>,
        kind: AlertKind,
        severity: impl Into<String>,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            device_id: device_id.into(),
            kind,
            severity: severity.into(),
            title: title.into(),
            message: message.into(),
            created_at: Utc::now(),
            acknowledged: false,
            acknowledged_at: None,
            db: None,
            evidence_file: None,
            evidence_duration_secs: None,
            email_status: "pending".to_string(),
            email_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub device_id: String,
    pub recorded_at: DateTime<Utc>,
    pub avg_db: f64,
    pub peak_db: f64,
    pub min_db: f64,
    pub over_threshold: bool,
    pub duration_secs: u32,
    pub sample_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallRecord {
    pub id: String,
    pub device_id: String,
    pub mode: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub duration_secs: u32,
    pub status: String,
    pub audio_file: Option<String>,
    pub audio_mime: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl CallRecord {
    pub fn public_view(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "deviceId": self.device_id,
            "mode": self.mode,
            "startedAt": self.started_at,
            "endedAt": self.ended_at,
            "durationSecs": self.duration_secs,
            "status": self.status,
            "hasAudio": self.audio_file.is_some(),
            "createdAt": self.created_at,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)] // Public role names are part of persisted user records.
pub enum UserRole {
    SuperAdmin,
    /// Operational administrator: all devices and operational settings;
    /// user management, secrets, retention and update installation stay super-only.
    UpdateAdmin,
    DeviceAdmin,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SuperAdmin => "super_admin",
            Self::UpdateAdmin => "update_admin",
            Self::DeviceAdmin => "device_admin",
        }
    }

    pub fn is_super(&self) -> bool {
        matches!(self, Self::SuperAdmin)
    }

    /// UpdateAdmin and SuperAdmin can manage devices, users, and settings.
    pub fn is_update_admin(&self) -> bool {
        matches!(self, Self::SuperAdmin | Self::UpdateAdmin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub email: String,
    pub password_hash: String,
    pub role: UserRole,
    #[serde(default)]
    pub device_ids: Vec<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub token_version: u64,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_login_at: Option<DateTime<Utc>>,
}

impl User {
    pub fn view(&self) -> UserView {
        UserView {
            id: self.id.clone(),
            username: self.username.clone(),
            email: self.email.clone(),
            role: self.role.clone(),
            device_ids: self.device_ids.clone(),
            disabled: self.disabled,
            created_at: self.created_at,
            last_login_at: self.last_login_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    pub id: String,
    pub username: String,
    pub email: String,
    pub role: UserRole,
    pub device_ids: Vec<String>,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub role: UserRole,
    #[serde(default)]
    pub device_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub role: Option<UserRole>,
    #[serde(default)]
    pub device_ids: Option<Vec<String>>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationUpdate {
    pub enabled: bool,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSettingsUpdate {
    #[serde(default)]
    pub threshold_db: Option<f64>,
    #[serde(default)]
    pub alert_cooldown_secs: Option<u64>,
    #[serde(default)]
    pub offline_timeout_secs: Option<u64>,
    #[serde(default)]
    pub auto_record_evidence: Option<bool>,
    #[serde(default)]
    pub capture_start: Option<String>,
    #[serde(default)]
    pub capture_end: Option<String>,
}

impl DeviceSettingsUpdate {
    pub fn into_settings(self) -> DeviceSettings {
        DeviceSettings {
            threshold_db: self.threshold_db,
            alert_cooldown_secs: self.alert_cooldown_secs,
            offline_timeout_secs: self.offline_timeout_secs,
            auto_record_evidence: self.auto_record_evidence,
            capture_start: self.capture_start,
            capture_end: self.capture_end,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub version: u32,
    pub settings: Settings,
    pub devices: Vec<Device>,
    #[serde(default)]
    pub users: Vec<User>,
    #[serde(default)]
    pub alerts: Vec<Alert>,
    #[serde(default)]
    pub calls: Vec<CallRecord>,
    pub last_daily_report_date: Option<String>,
    #[serde(default)]
    pub daily_report_dates: std::collections::HashMap<String, String>,
    /// NVIDIA build.nvidia.com API key for cloud evidence denoising (BNR).
    /// Server-only secret; never included in client-visible snapshots.
    #[serde(default)]
    pub nvidia_api_key: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub avg_db: Option<f64>,
    pub peak_db: Option<f64>,
    pub min_db: Option<f64>,
    pub over_count: u64,
    pub sample_count: usize,
}

impl Stats {
    pub fn from_readings(readings: &[Reading]) -> Self {
        if readings.is_empty() {
            return Self {
                avg_db: None,
                peak_db: None,
                min_db: None,
                over_count: 0,
                sample_count: 0,
            };
        }
        let avg = leq(readings);
        Self {
            avg_db: Some(round1(avg)),
            peak_db: Some(round1(
                readings
                    .iter()
                    .map(|r| r.peak_db)
                    .fold(f64::NEG_INFINITY, f64::max),
            )),
            min_db: Some(round1(
                readings
                    .iter()
                    .map(|r| r.min_db)
                    .fold(f64::INFINITY, f64::min),
            )),
            over_count: readings.iter().filter(|r| r.over_threshold).count() as u64,
            sample_count: readings.len(),
        }
    }
}

/// Equivalent continuous sound level, weighted by each measured duration.
pub fn leq(readings: &[Reading]) -> f64 {
    let duration: f64 = readings.iter().map(|r| r.duration_secs.max(1) as f64).sum();
    let energy: f64 = readings
        .iter()
        .map(|r| 10f64.powf(r.avg_db / 10.0) * r.duration_secs.max(1) as f64)
        .sum();
    10.0 * (energy / duration).max(1e-16).log10()
}

pub fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub remember: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub user: UserView,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatRequest {
    pub device_id: String,
    pub monitoring: bool,
    #[serde(default)]
    pub app_version: Option<String>,
    #[serde(default)]
    pub capture_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatResponse {
    pub manual_monitoring: bool,
    pub war_active: bool,
    pub desired_monitoring: bool,
    pub threshold_db: f64,
    pub alert_cooldown_secs: u64,
    pub offline_timeout_secs: u64,
    pub evidence_seconds: u32,
    pub auto_record_evidence: bool,
    pub server_time: DateTime<Utc>,
    pub server_utc_offset_secs: i32,
    pub ice_servers: Value,
    pub device_id: String,
    pub device_name: String,
    pub notification_enabled: bool,
    pub notification_title: String,
    pub notification_content: String,
    pub denoise_mode: String,
    pub voice_enhance: bool,
    #[serde(default)]
    pub war_mode_enabled: bool,
    #[serde(default)]
    pub war_mode_start: String,
    #[serde(default)]
    pub war_mode_end: String,
    #[serde(default)]
    pub capture_start: String,
    #[serde(default)]
    pub capture_end: String,
    #[serde(default)]
    pub totp_secret: String,
    #[serde(default)]
    pub has_fixed_password: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryRequest {
    pub device_id: String,
    pub avg_db: f64,
    pub peak_db: f64,
    #[serde(default)]
    pub min_db: Option<f64>,
    pub duration_secs: u32,
    pub sample_count: u32,
    #[serde(default)]
    pub recorded_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub audio_base64: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryResponse {
    pub accepted: bool,
    pub over_threshold: bool,
    pub threshold_db: f64,
    pub desired_monitoring: bool,
    pub alert_cooldown_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert: Option<Alert>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveRequest {
    pub device_id: String,
    pub current_db: f64,
    #[serde(default)]
    pub average_db: Option<f64>,
    #[serde(default)]
    pub peak_db: Option<f64>,
    #[serde(default)]
    pub rolling_15s_db: Option<f64>,
    #[serde(default)]
    pub capturing: bool,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub system_playing: bool,
    #[serde(default)]
    pub aec_enabled: bool,
    #[serde(default)]
    pub recorded_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopIntentRequest {
    pub device_id: String,
    pub pin: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopIntentResponse {
    pub accepted: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyPinRequest {
    pub device_id: String,
    pub pin: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyPinResponse {
    pub verified: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlRequest {
    pub device_id: String,
    pub monitoring: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceControlResponse {
    pub device: DeviceView,
    pub settings: Settings,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewResponse {
    pub devices: Vec<DeviceView>,
    pub device: DeviceView,
    pub settings: Settings,
    pub stats: Stats,
    pub alerts: Vec<Alert>,
    pub alert_count_today: usize,
    pub smtp_configured: bool,
    pub server_time: DateTime<Utc>,
    pub server_utc_offset_secs: i32,
    pub ice_servers: Value,
    pub signaling_connected: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponse {
    pub settings: Settings,
    pub smtp_configured: bool,
    pub ice_servers: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportResult {
    pub device_id: String,
    pub device_name: String,
    pub recipients: Vec<String>,
    pub email_status: String,
    pub alert_id: String,
    pub sample_count: usize,
    pub period_from: DateTime<Utc>,
    pub period_to: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeviceRequest {
    pub name: String,
    #[serde(default)]
    pub device_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeviceResponse {
    pub device: DeviceView,
    pub token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeviceRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTokenResponse {
    pub device_id: String,
    pub token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCallRequest {
    pub device_id: String,
    pub mode: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub audio_base64: Option<String>,
    #[serde(default)]
    pub audio_mime: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(email: &str, role: UserRole, device_ids: &[&str], disabled: bool) -> User {
        User {
            id: uuid::Uuid::new_v4().to_string(),
            username: "tester".to_string(),
            email: email.to_string(),
            password_hash: String::new(),
            role,
            device_ids: device_ids.iter().map(|id| id.to_string()).collect(),
            disabled,
            token_version: 1,
            created_at: Utc::now(),
            last_login_at: None,
        }
    }

    #[test]
    fn capture_window_allows_one_sided_daily_schedules() {
        assert!(Settings::time_window("", "", 100));
        assert!(!Settings::time_window("08:00", "", 470));
        assert!(Settings::time_window("08:00", "", 480));
        assert!(Settings::time_window("", "18:00", 600));
        assert!(!Settings::time_window("", "18:00", 1080));
        assert!(Settings::time_window("22:00", "06:00", 23 * 60));
    }

    #[test]
    fn routes_notifications_to_super_and_assigned_device_admins() {
        let users = vec![
            user("super@example.com", UserRole::SuperAdmin, &[], false),
            user(
                "assigned@example.com",
                UserRole::DeviceAdmin,
                &["dev-a"],
                false,
            ),
            user(
                "other@example.com",
                UserRole::DeviceAdmin,
                &["dev-b"],
                false,
            ),
            user(
                "disabled@example.com",
                UserRole::DeviceAdmin,
                &["dev-a"],
                true,
            ),
            user("", UserRole::DeviceAdmin, &["dev-a"], false),
        ];
        let recipients =
            recipients_from_users(&users, "dev-a", &["global@example.com".to_string()]);
        assert_eq!(
            recipients,
            vec![
                "global@example.com".to_string(),
                "super@example.com".to_string(),
                "assigned@example.com".to_string(),
            ]
        );
    }
}
