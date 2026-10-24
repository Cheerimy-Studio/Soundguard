#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;

#[path = "../../common/rnnoise_engine.rs"]
mod rnnoise_engine;
#[path = "../../common/config_file.rs"]
mod config_file;
#[path = "../../common/security.rs"]
mod security;

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc, Mutex, RwLock,
    },
    time::{Duration, Instant},
};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use audio::{AudioCommand, AudioShared, MeterSnapshot, WINDOW_SECONDS};
use base64::Engine as _;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{
    webview::{PermissionKind, PermissionResponse, WebviewWindowBuilder},
    AppHandle, Emitter, Manager, State, WebviewUrl,
};

const HEARTBEAT_SECS: u64 = 10;
const MAX_PENDING_REPORTS: usize = 12;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ClientConfig {
    server_url: String,
    device_id: String,
    device_token: String,
    calibration_offset_db: f64,
    #[serde(default, skip_serializing)]
    local_password_hash: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: "https://db.gzmath.cn".to_string(),
            device_id: String::new(),
            device_token: String::new(),
            calibration_offset_db: 90.0,
            local_password_hash: String::new(),
        }
    }
}

impl ClientConfig {
    fn normalize(mut self) -> Result<Self, String> {
        self.server_url = self.server_url.trim().trim_end_matches('/').to_string();
        self.device_id = self.device_id.trim().to_string();
        self.device_token = self.device_token.trim().to_string();
        if self.server_url.is_empty() {
            return Err("服务器地址不能为空".to_string());
        }
        let parsed = reqwest::Url::parse(&self.server_url)
            .map_err(|_| "服务器地址格式无效，应类似 https://db.gzmath.cn".to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("服务器地址必须使用 http 或 https".to_string());
        }
        if parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err("服务器地址不能包含凭据、查询或片段".into());
        }
        if self.device_id.is_empty() {
            return Err("设备编号不能为空".to_string());
        }
        if self.device_token.is_empty() {
            return Err("设备令牌不能为空".to_string());
        }
        if !(40.0..=140.0).contains(&self.calibration_offset_db) {
            return Err("校准偏移必须在 40–140 dB 之间".to_string());
        }
        self.calibration_offset_db = (self.calibration_offset_db * 10.0).round() / 10.0;
        Ok(self)
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.server_url, path)
    }

    fn needs_setup(&self) -> bool {
        self.device_id.trim().is_empty()
            || self.device_token.trim().is_empty()
            || self.local_password_hash.trim().is_empty()
    }
}

fn hash_local_password(password: &str) -> Result<String, String> {
    if !(8..=128).contains(&password.chars().count()) {
        return Err("本地密码需要 8–128 个字符".to_string());
    }
    let salt = argon2::password_hash::SaltString::generate(&mut rand_core::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| format!("密码哈希失败: {error}"))
}

fn verify_local_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct StatusSnapshot {
    connected: bool,
    desired_monitoring: bool,
    monitoring: bool,
    server_url: String,
    device_id: String,
    device_name: String,
    app_version: String,
    threshold_db: f64,
    alert_cooldown_secs: u64,
    offline_timeout_secs: u64,
    evidence_seconds: u32,
    auto_record_evidence: bool,
    audio_ready: bool,
    capture_error: Option<String>,
    last_error: Option<String>,
    last_heartbeat_at: Option<String>,
    last_report_at: Option<String>,
    last_alert_at: Option<String>,
    reports_sent: u64,
    pending_reports: usize,
    server_time: Option<String>,
    ice_servers: Value,
    notification_enabled: bool,
    notification_title: String,
    notification_content: String,
}

#[derive(Debug, Clone)]
struct RemoteConfig {
    desired_monitoring: bool,
    threshold_db: f64,
    alert_cooldown_secs: u64,
    evidence_seconds: u32,
    auto_record_evidence: bool,
    ice_servers: Value,
    server_time: Option<String>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            desired_monitoring: true,
            threshold_db: 65.0,
            alert_cooldown_secs: 300,
            evidence_seconds: 30,
            auto_record_evidence: true,
            ice_servers: json!([]),
            server_time: None,
        }
    }
}

struct RuntimeState {
    local_auth: Mutex<security::PasswordGuard>,
    network_gate: tokio::sync::Mutex<()>,
    config_path: PathBuf,
    config: RwLock<ClientConfig>,
    status: Mutex<StatusSnapshot>,
    remote: RwLock<RemoteConfig>,
    audio: Arc<AudioShared>,
    audio_tx: Mutex<Sender<AudioCommand>>,
    pending: Mutex<VecDeque<TelemetryPayload>>,
    last_audio_sent: Mutex<Option<Instant>>,
    exiting: AtomicBool,
    force_probe: AtomicBool,
    http: reqwest::Client,
}

impl RuntimeState {
    fn config(&self) -> ClientConfig {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn remote(&self) -> RemoteConfig {
        self.remote
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientStateResponse {
    config: ClientConfig,
    status: StatusSnapshot,
    version: String,
    needs_setup: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeterEvent {
    current_db: Option<f64>,
    average_db: Option<f64>,
    peak_db: Option<f64>,
    progress: f32,
    window_seconds: f64,
    capturing: bool,
    suspended: bool,
    ready: bool,
    aec_enabled: bool,
    system_playing: bool,
    error: Option<String>,
    source_rate: u32,
    channels: u16,
    threshold_db: f64,
    auto_record_evidence: bool,
    evidence_seconds: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TelemetryPayload {
    device_id: String,
    avg_db: f64,
    peak_db: f64,
    min_db: f64,
    duration_secs: u32,
    sample_count: u32,
    recorded_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_base64: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LivePayload {
    device_id: String,
    current_db: f64,
    average_db: Option<f64>,
    peak_db: Option<f64>,
    capturing: bool,
    suspended: bool,
    system_playing: bool,
    aec_enabled: bool,
    recorded_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HeartbeatPayload {
    device_id: String,
    monitoring: bool,
    app_version: String,
    capture_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HeartbeatResponse {
    desired_monitoring: bool,
    threshold_db: f64,
    alert_cooldown_secs: u64,
    offline_timeout_secs: u64,
    evidence_seconds: u32,
    auto_record_evidence: bool,
    server_time: DateTime<Utc>,
    ice_servers: Value,
    device_name: String,
    notification_enabled: bool,
    notification_title: String,
    notification_content: String,
    #[serde(default)]
    denoise_mode: String,
    #[serde(default)]
    voice_enhance: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TelemetryResponse {
    #[allow(dead_code)]
    accepted: bool,
    #[allow(dead_code)]
    over_threshold: bool,
    threshold_db: f64,
    desired_monitoring: bool,
    alert_cooldown_secs: u64,
    alert: Option<Value>,
}

#[tauri::command]
fn get_client_state(state: State<'_, Arc<RuntimeState>>) -> ClientStateResponse {
    let status = state
        .status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let config = state.config();
    ClientStateResponse {
        needs_setup: config.needs_setup(),
        config,
        status,
        version: APP_VERSION.to_string(),
    }
}

fn persist_client_config(
    app: &AppHandle,
    state: &RuntimeState,
    config: ClientConfig,
) -> Result<ClientConfig, String> {
    let config = config.normalize()?;
    if let Some(parent) = state.config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建配置目录失败: {error}"))?;
    }
    let mut stored = serde_json::to_value(&config).map_err(|e| e.to_string())?;
    stored["localPasswordHash"] = Value::String(config.local_password_hash.clone());
    let bytes = serde_json::to_vec_pretty(&stored).map_err(|error| error.to_string())?;
    config_file::atomic_write(&state.config_path, &bytes)?;

    let previous = state.config();
    {
        let mut guard = state
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = config.clone();
    }
    state.audio.set_offset(config.calibration_offset_db as f32);
    if previous.server_url != config.server_url || previous.device_id != config.device_id {
        state
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
    state.audio.set_suspended(true);
    state.audio.set_suspended(false);
    *state
        .last_audio_sent
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = None;
    state.force_probe.store(true, Ordering::SeqCst);
    {
        let mut status = state
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        status.server_url = config.server_url.clone();
        status.device_id = config.device_id.clone();
        status.last_error = None;
        if previous.server_url != config.server_url {
            status.connected = false;
        }
    }
    emit_status(app, state);
    let _ = app.emit("config-changed", &config);
    Ok(config)
}

#[tauri::command]
fn save_config(
    app: AppHandle,
    state: State<'_, Arc<RuntimeState>>,
    config: ClientConfig,
) -> Result<ClientConfig, String> {
    state
        .local_auth
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .require_authorized()?;
    let previous = state.config();
    let mut config = config.normalize()?;
    // The webview must never replace the password hash supplied by Rust.
    config.local_password_hash = previous.local_password_hash;
    if config.local_password_hash.is_empty() {
        return Err("请先完成首次配置".into());
    }
    persist_client_config(&app, &state, config)
}

#[tauri::command]
fn setup_client(
    app: AppHandle,
    state: State<'_, Arc<RuntimeState>>,
    config: ClientConfig,
    password: String,
    confirm_password: String,
) -> Result<ClientConfig, String> {
    if !state.config().needs_setup() {
        return Err("客户端已配置，请通过密码验证后修改设置".into());
    }
    if password != confirm_password {
        return Err("两次输入的本地密码不一致".to_string());
    }
    let mut config = config.normalize()?;
    config.local_password_hash = hash_local_password(&password)?;
    persist_client_config(&app, &state, config)
}

#[tauri::command]
fn retry_audio(app: AppHandle, state: State<'_, Arc<RuntimeState>>) -> Result<(), String> {
    let sender = state
        .audio_tx
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    sender
        .send(AudioCommand::Retry)
        .map_err(|_| "音频线程不可用".to_string())?;
    {
        let mut status = state
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        status.capture_error = None;
    }
    emit_status(&app, &state);
    Ok(())
}

#[tauri::command]
fn open_github() {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", "https://github.com/Cheerimy-Studio"])
            .spawn();
    }
}

#[tauri::command]
fn set_call_active(state: State<'_, Arc<RuntimeState>>, active: bool) -> Result<(), String> {
    state.audio.set_suspended(active);
    Ok(())
}

#[tauri::command]
fn verify_pin(state: State<'_, Arc<RuntimeState>>, pin: String) -> Result<(), String> {
    if pin.trim().is_empty() {
        return Err("请输入本地密码".to_string());
    }
    let config = state.config();
    if config.local_password_hash.trim().is_empty() {
        return Err("尚未设置本地密码，请重新打开客户端完成首次配置".to_string());
    }
    state
        .local_auth
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .verify(|| pin.len() <= 512 && verify_local_password(&config.local_password_hash, &pin))?;
    Ok(())
}

#[tauri::command]
async fn request_exit(
    app: AppHandle,
    state: State<'_, Arc<RuntimeState>>,
    pin: String,
) -> Result<(), String> {
    let config = state.config();
    if config.needs_setup() {
        state.exiting.store(true, Ordering::SeqCst);
        app.exit(0);
        return Ok(());
    }
    if pin.trim().is_empty() {
        return Err("请输入本地密码".to_string());
    }
    state
        .local_auth
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .verify(|| pin.len() <= 512 && verify_local_password(&config.local_password_hash, &pin))?;
    state.exiting.store(true, Ordering::SeqCst);
    // The server audit is best effort; the local password already authorized exit.
    let _ = post_json::<serde_json::Value, _>(
        &state,
        &config,
        "/api/device/local-stop",
        &json!({ "deviceId": config.device_id }),
    )
    .await;
    state.exiting.store(true, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

/// Immediate shutdown used when the user declines the first-run notice.
/// No password is required: the app must not keep running without consent.
#[tauri::command]
fn exit_now(app: AppHandle, state: State<'_, Arc<RuntimeState>>) {
    state.exiting.store(true, Ordering::SeqCst);
    app.exit(0);
}

fn emit_status(app: &AppHandle, state: &RuntimeState) {
    let status = state
        .status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let _ = app.emit("status", status);
}

fn update_status(app: &AppHandle, state: &RuntimeState, update: impl FnOnce(&mut StatusSnapshot)) {
    {
        let mut status = state
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut status);
    }
    emit_status(app, state);
}

fn apply_audio_state(state: &RuntimeState, desired_monitoring: bool) {
    let currently_capturing = state.audio.capturing();
    if desired_monitoring && !currently_capturing {
        if let Ok(sender) = state.audio_tx.lock() {
            let _ = sender.send(AudioCommand::Start);
        }
    } else if !desired_monitoring && currently_capturing {
        if let Ok(sender) = state.audio_tx.lock() {
            let _ = sender.send(AudioCommand::Pause);
        }
    }
}

async fn post_json<T: DeserializeOwned, B: Serialize>(
    state: &RuntimeState,
    config: &ClientConfig,
    path: &str,
    body: &B,
) -> Result<T, String> {
    let _network_guard = state.network_gate.lock().await;
    if state.exiting.load(Ordering::SeqCst) && path != "/api/device/local-stop" {
        return Err("客户端正在关闭".into());
    }
    let response = state
        .http
        .post(config.api_url(path))
        .header("X-Device-Token", &config.device_token)
        .json(body)
        .send()
        .await
        .map_err(|error| format!("无法连接服务器: {error}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("服务器返回 {}", status.as_u16()));
        return Err(message);
    }
    serde_json::from_str::<T>(&text).map_err(|error| format!("服务器响应格式错误: {error}"))
}

async fn network_loop(app: AppHandle, state: Arc<RuntimeState>) {
    let mut next_heartbeat = Instant::now();

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if state.exiting.load(Ordering::SeqCst) {
            break;
        }
        let config = state.config();
        if config.needs_setup() {
            continue;
        }

        if state.force_probe.swap(false, Ordering::SeqCst) || Instant::now() >= next_heartbeat {
            next_heartbeat = Instant::now() + Duration::from_secs(HEARTBEAT_SECS);
            let monitoring = state.audio.capturing();
            let capture_error = state.audio.error();
            let payload = HeartbeatPayload {
                device_id: config.device_id.clone(),
                monitoring,
                app_version: APP_VERSION.to_string(),
                capture_error: capture_error.clone(),
            };
            match post_json::<HeartbeatResponse, _>(
                &state,
                &config,
                "/api/device/heartbeat",
                &payload,
            )
            .await
            {
                Ok(response) => {
                    let current = state.config();
                    if current.server_url != config.server_url
                        || current.device_id != config.device_id
                        || current.device_token != config.device_token
                    {
                        continue;
                    }
                    apply_audio_state(&state, response.desired_monitoring);
                    {
                        let mut remote = state
                            .remote
                            .write()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        remote.desired_monitoring = response.desired_monitoring;
                        remote.threshold_db = response.threshold_db;
                        remote.alert_cooldown_secs = response.alert_cooldown_secs;
                        remote.evidence_seconds = response.evidence_seconds;
                        remote.auto_record_evidence = response.auto_record_evidence;
                        remote.ice_servers = response.ice_servers.clone();
                        remote.server_time = Some(
                            response
                                .server_time
                                .to_rfc3339_opts(SecondsFormat::Secs, true),
                        );
                    }
                    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
                    let capturing = state.audio.capturing();
                    update_status(&app, &state, |status| {
                        status.connected = true;
                        status.last_error = None;
                        status.desired_monitoring = response.desired_monitoring;
                        status.monitoring = capturing;
                        status.threshold_db = response.threshold_db;
                        status.alert_cooldown_secs = response.alert_cooldown_secs;
                        status.offline_timeout_secs = response.offline_timeout_secs;
                        status.evidence_seconds = response.evidence_seconds;
                        status.auto_record_evidence = response.auto_record_evidence;
                        status.audio_ready = state.audio.ready();
                        status.capture_error = capture_error.clone();
                        status.last_heartbeat_at = Some(now);
                        status.device_name = response.device_name.clone();
                        status.server_time = Some(
                            response
                                .server_time
                                .to_rfc3339_opts(SecondsFormat::Secs, true),
                        );
                        status.notification_enabled = response.notification_enabled;
                        status.notification_title = response.notification_title.clone();
                        status.notification_content = response.notification_content.clone();
                        let denoise_mode = match response.denoise_mode.as_str() {
                            "light" => audio::DENOISE_LIGHT,
                            "standard" => audio::DENOISE_STANDARD,
                            "strong" => audio::DENOISE_STRONG,
                            _ => audio::DENOISE_OFF,
                        };
                        state
                            .audio
                            .set_denoise(denoise_mode, response.voice_enhance);
                        status.ice_servers = response.ice_servers;
                    });
                }
                Err(error) => {
                    update_status(&app, &state, |status| {
                        status.connected = false;
                        status.last_error = Some(error.clone());
                        status.monitoring = state.audio.capturing();
                        status.audio_ready = state.audio.ready();
                        status.capture_error = capture_error.clone();
                    });
                }
            }
        }

        let connected = state
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .connected;
        if connected && !state.exiting.load(Ordering::SeqCst) {
            for _ in 0..2 {
                let payload = {
                    let guard = state
                        .pending
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.front().cloned()
                };
                let Some(payload) = payload else {
                    break;
                };
                match post_json::<TelemetryResponse, _>(
                    &state,
                    &config,
                    "/api/device/telemetry",
                    &payload,
                )
                .await
                {
                    Ok(response) => {
                        let current = state.config();
                        if current.server_url != config.server_url
                            || current.device_id != config.device_id
                            || current.device_token != config.device_token
                        {
                            break;
                        }
                        {
                            let mut queue = state
                                .pending
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            queue.retain(|item| item.recorded_at != payload.recorded_at);
                        }
                        apply_audio_state(&state, response.desired_monitoring);
                        {
                            let mut remote = state
                                .remote
                                .write()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            remote.desired_monitoring = response.desired_monitoring;
                            remote.threshold_db = response.threshold_db;
                            remote.alert_cooldown_secs = response.alert_cooldown_secs;
                        }
                        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
                        let pending_reports = state
                            .pending
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .len();
                        update_status(&app, &state, |status| {
                            status.last_report_at = Some(now.clone());
                            status.reports_sent = status.reports_sent.saturating_add(1);
                            status.pending_reports = pending_reports;
                            status.desired_monitoring = response.desired_monitoring;
                            status.threshold_db = response.threshold_db;
                            status.alert_cooldown_secs = response.alert_cooldown_secs;
                            status.monitoring = state.audio.capturing();
                            status.last_error = None;
                            if response.alert.is_some() {
                                status.last_alert_at = Some(now.clone());
                            }
                        });
                        if let Some(alert) = response.alert {
                            let _ = app.emit("remote-alert", alert);
                        }
                    }
                    Err(error) => {
                        update_status(&app, &state, |status| {
                            status.last_error = Some(format!("上报失败：{error}"));
                            status.pending_reports = state
                                .pending
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .len();
                        });
                        break;
                    }
                }
            }
        }
    }
}

async fn live_loop(state: Arc<RuntimeState>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if state.exiting.load(Ordering::SeqCst) {
            break;
        }
        let connected = state
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .connected;
        if !connected {
            continue;
        }
        let config = state.config();
        let snapshot = state.audio.snapshot();
        let payload = LivePayload {
            device_id: config.device_id.clone(),
            current_db: snapshot.current_db.unwrap_or(0.0),
            average_db: snapshot.average_db,
            peak_db: snapshot.peak_db,
            capturing: snapshot.capturing,
            suspended: snapshot.suspended,
            system_playing: snapshot.system_playing,
            aec_enabled: snapshot.aec_enabled,
            recorded_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        };
        let _ = post_json::<Value, _>(&state, &config, "/api/device/live", &payload).await;
    }
}

async fn meter_loop(app: AppHandle, state: Arc<RuntimeState>) {
    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if state.exiting.load(Ordering::SeqCst) {
            break;
        }
        if state.config().needs_setup() {
            state.audio.set_capturing(false);
            continue;
        }
        let remote = state.remote();
        let snapshot: MeterSnapshot = state.audio.snapshot();
        let event = MeterEvent {
            current_db: snapshot.current_db,
            average_db: snapshot.average_db,
            peak_db: snapshot.peak_db,
            progress: snapshot.progress,
            window_seconds: WINDOW_SECONDS,
            capturing: snapshot.capturing,
            suspended: snapshot.suspended,
            ready: snapshot.ready,
            aec_enabled: snapshot.aec_enabled,
            system_playing: snapshot.system_playing,
            error: snapshot.error,
            source_rate: snapshot.source_rate,
            channels: snapshot.channels,
            threshold_db: remote.threshold_db,
            auto_record_evidence: remote.auto_record_evidence,
            evidence_seconds: remote.evidence_seconds,
        };
        let _ = app.emit("meter", event);

        let Some(report) = state.audio.take_window() else {
            continue;
        };
        let offset = state.audio.offset() as f64;
        let avg_db = (report.avg_dbfs + offset).clamp(0.0, 160.0);
        let peak_db = (report.peak_dbfs + offset).clamp(0.0, 160.0);
        let min_db = (report.min_dbfs + offset).clamp(0.0, 160.0);
        let over_threshold = avg_db >= remote.threshold_db;

        let mut audio_base64 = None;
        if over_threshold && remote.auto_record_evidence {
            let cooldown_ready = {
                let last = state
                    .last_audio_sent
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                last.map(|sent| sent.elapsed().as_secs() >= remote.alert_cooldown_secs)
                    .unwrap_or(true)
            };
            if cooldown_ready {
                let seconds = remote.evidence_seconds.clamp(1, 30).max(1);
                if let Some(wav) = state.audio.take_evidence_wav(seconds) {
                    audio_base64 = Some(base64::engine::general_purpose::STANDARD.encode(wav));
                    let mut last = state
                        .last_audio_sent
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *last = Some(Instant::now());
                }
            }
        }

        let config = state.config();
        let payload = TelemetryPayload {
            device_id: config.device_id,
            avg_db: (avg_db * 10.0).round() / 10.0,
            peak_db: (peak_db * 10.0).round() / 10.0,
            min_db: (min_db * 10.0).round() / 10.0,
            duration_secs: report.duration_secs.round().max(1.0) as u32,
            sample_count: report.frames.min(u32::MAX as u64) as u32,
            recorded_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            audio_base64,
        };
        let pending_len = {
            let mut queue = state
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if queue.len() >= MAX_PENDING_REPORTS {
                queue.pop_front();
            }
            queue.push_back(payload);
            queue.len()
        };
        update_status(&app, &state, |status| {
            status.pending_reports = pending_len;
        });
    }
}

fn load_or_create_config(path: &PathBuf) -> Result<ClientConfig, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => {
            return serde_json::from_str::<ClientConfig>(&raw)
                .map_err(|error| format!("配置文件损坏，请恢复备份：{error}"))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("读取配置失败：{error}")),
    }

    let config = ClientConfig::default();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&config) {
        config_file::atomic_write(path, &bytes)?;
    }
    Ok(config)
}

fn build_status(config: &ClientConfig, remote: &RemoteConfig) -> StatusSnapshot {
    StatusSnapshot {
        connected: false,
        desired_monitoring: remote.desired_monitoring,
        monitoring: true,
        server_url: config.server_url.clone(),
        device_id: config.device_id.clone(),
        device_name: "现场采集节点".to_string(),
        app_version: APP_VERSION.to_string(),
        threshold_db: remote.threshold_db,
        alert_cooldown_secs: remote.alert_cooldown_secs,
        offline_timeout_secs: 45,
        evidence_seconds: remote.evidence_seconds,
        auto_record_evidence: remote.auto_record_evidence,
        audio_ready: false,
        capture_error: None,
        last_error: None,
        last_heartbeat_at: None,
        last_report_at: None,
        last_alert_at: None,
        reports_sent: 0,
        pending_reports: 0,
        server_time: None,
        ice_servers: remote.ice_servers.clone(),
        notification_enabled: false,
        notification_title: String::new(),
        notification_content: String::new(),
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_client_state,
            save_config,
            setup_client,
            retry_audio,
            set_call_active,
            verify_pin,
            open_github,
            request_exit,
            exit_now
        ])
        .setup(|app| {
            let legacy_config_dir = app
                .path()
                .app_config_dir()
                .map_err(|error| std::io::Error::other(format!("无法定位配置目录: {error}")))?;
            std::fs::create_dir_all(&legacy_config_dir)?;
            let legacy_config_path = legacy_config_dir.join("client-config.json");
            let config_path = std::env::current_exe()
                .ok()
                .and_then(|path| {
                    path.parent()
                        .map(|parent| parent.join("client-config.json"))
                })
                .unwrap_or(legacy_config_path.clone());
            if !config_path.exists() && legacy_config_path.exists() {
                let _ = std::fs::copy(&legacy_config_path, &config_path);
            }
            let config = load_or_create_config(&config_path).map_err(std::io::Error::other)?;
            let audio = AudioShared::new(config.calibration_offset_db as f32);
            audio.set_engine(Box::new(
                rnnoise_engine::RnnoiseEngine::new(audio::DENOISE_OFF, false),
            ));
            let audio_tx = audio::spawn_audio_engine(audio.clone());
            let remote = RemoteConfig::default();
            let status = build_status(&config, &remote);
            let runtime = Arc::new(RuntimeState {
                local_auth: Mutex::new(security::PasswordGuard::default()),
                network_gate: tokio::sync::Mutex::new(()),
                config_path,
                config: RwLock::new(config),
                status: Mutex::new(status),
                remote: RwLock::new(remote),
                audio,
                audio_tx: Mutex::new(audio_tx),
                pending: Mutex::new(VecDeque::new()),
                last_audio_sent: Mutex::new(None),
                exiting: AtomicBool::new(false),
                force_probe: AtomicBool::new(true),
                http: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(Duration::from_secs(9))
                    .build()?,
            });
            app.manage(runtime.clone());

            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("声垣")
                    .inner_size(1120.0, 760.0)
                    .min_inner_size(900.0, 640.0)
                    .center()
                    .on_permission_request(|_, kind| match kind {
                        PermissionKind::Microphone => PermissionResponse::Allow,
                        _ => PermissionResponse::Default,
                    })
                    .build()?;

            let close_state = runtime.clone();
            let close_window = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if close_state.exiting.load(Ordering::SeqCst) {
                        return;
                    }
                    api.prevent_close();
                    let _ = close_window.emit("close-requested", ());
                    let _ = close_window.show();
                    let _ = close_window.set_focus();
                }
            });

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(network_loop(app_handle.clone(), runtime.clone()));
            tauri::async_runtime::spawn(meter_loop(app_handle, runtime.clone()));
            tauri::async_runtime::spawn(live_loop(runtime));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running SoundGuard client");
}

#[cfg(test)]
mod local_auth_tests {
    use super::*;

    #[test]
    fn local_password_hash_round_trip() {
        let hash = hash_local_password("local-pass-123").expect("hash");
        assert!(verify_local_password(&hash, "local-pass-123"));
        assert!(!verify_local_password(&hash, "wrong-pass"));
    }

    #[test]
    fn setup_is_required_until_device_and_password_exist() {
        let mut config = ClientConfig::default();
        assert!(config.needs_setup());
        config.server_url = "https://db.gzmath.cn".to_string();
        config.device_id = "dev-test".to_string();
        config.device_token = "token".to_string();
        config.local_password_hash = "hash".to_string();
        assert!(!config.needs_setup());
    }
}
