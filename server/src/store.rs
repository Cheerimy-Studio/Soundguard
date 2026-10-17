use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::{error, warn};

use crate::{
    auth::{generate_password, hash_password, hash_token},
    config::Config,
    media_cipher::MediaCipher,
    models::{
        Alert, CallRecord, Device, PersistedState, Reading, Settings, User, UserRole, MAX_ALERTS,
        MAX_CALLS,
    },
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredState {
    settings: Settings,
    #[serde(default)]
    devices: Vec<Device>,
    #[serde(default)]
    device: Option<Device>,
    #[serde(default)]
    users: Vec<User>,
    #[serde(default)]
    alerts: Vec<Alert>,
    #[serde(default)]
    calls: Vec<CallRecord>,
    #[serde(default)]
    last_daily_report_date: Option<String>,
}

pub struct Store {
    data_dir: PathBuf,
    state: Mutex<PersistedState>,
    media_cipher: MediaCipher,
}

impl Store {
    pub async fn load(config: &Config) -> anyhow::Result<Arc<Self>> {
        config.ensure_dirs()?;
        let state_path = config.data_path("state.json");

        let mut stored = match tokio::fs::read_to_string(&state_path).await {
            Ok(raw) => match serde_json::from_str::<StoredState>(&raw) {
                Ok(state) => Some(state),
                Err(error) => {
                    error!("Could not parse state.json: {error}");
                    let backup = config.data_path("state.json.corrupt");
                    let _ = tokio::fs::copy(&state_path, backup).await;
                    None
                }
            },
            Err(_) => None,
        };

        let mut settings = stored
            .as_ref()
            .map(|state| state.settings.clone())
            .unwrap_or_else(|| config.default_settings.clone());
        if let Err(error) = settings.validate_and_normalize() {
            warn!("Persisted settings are invalid ({error}); restoring defaults");
            settings = config.default_settings.clone();
        }
        if !config.env_overrides.is_empty() {
            let persisted = settings.clone();
            config.env_overrides.apply_to(&mut settings);
            if let Err(error) = settings.validate_and_normalize() {
                warn!(
                    "Environment overrides are invalid ({error}); keeping saved settings: {}",
                    config.env_overrides.labels().join(", ")
                );
                settings = persisted;
            } else {
                warn!(
                    "Environment overrides applied over saved settings: {}",
                    config.env_overrides.labels().join(", ")
                );
            }
        }

        let mut devices = stored
            .as_mut()
            .map(|state| std::mem::take(&mut state.devices))
            .unwrap_or_default();
        if devices.is_empty() {
            if let Some(legacy) = stored.as_mut().and_then(|state| state.device.take()) {
                devices.push(legacy);
            }
        }
        if devices.is_empty() {
            let device_id = if config.device_id.trim().is_empty() {
                crate::auth::new_device_id()
            } else {
                config.device_id.clone()
            };
            devices.push(Device::new(
                device_id,
                config.device_name.clone(),
                hash_token(&config.device_token),
            ));
        }

        for device in &mut devices {
            if let Err(error) = device.settings.validate_and_normalize() {
                warn!(
                    "Device {} has invalid settings override ({error}); clearing overrides",
                    device.id
                );
                device.settings = Default::default();
            }
            if device.token_hash.is_empty() {
                if device.id == config.device_id {
                    device.token_hash = hash_token(&config.device_token);
                } else {
                    // A device with no stored token cannot be authenticated. Keep it
                    // visible for recovery, but require an explicit token rotation.
                    device.token_hash = String::new();
                }
            }
            if device.created_at.is_none() {
                device.created_at = Some(Utc::now());
            }
            device.monitoring = false;
        }
        devices.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.name.cmp(&b.name))
        });

        let mut users = stored
            .as_mut()
            .map(|state| std::mem::take(&mut state.users))
            .unwrap_or_default();
        if users.is_empty() {
            let password = if config.admin_password.trim().chars().count() >= 8 {
                config.admin_password.trim().to_string()
            } else {
                generate_password(20)
            };
            let password_hash = hash_password(&password)?;
            let now = Utc::now();
            users.push(User {
                id: crate::auth::new_user_id(),
                username: "admin".to_string(),
                email: config.admin_email.clone().unwrap_or_default(),
                password_hash,
                role: UserRole::SuperAdmin,
                device_ids: Vec::new(),
                disabled: false,
                token_version: 1,
                created_at: now,
                last_login_at: None,
            });
            write_initial_admin_password(config, &password)?;
            tracing::info!(
                "Initial super administrator created. Username: admin. Password is stored in {}",
                config.data_path("initial-admin-password.txt").display()
            );
        }

        if let Some(email) = config.admin_email.as_ref() {
            for user in &mut users {
                if user.role.is_super() && user.email.trim().is_empty() {
                    user.email = email.clone();
                }
            }
        }

        let default_device_id = devices
            .first()
            .map(|device| device.id.clone())
            .unwrap_or_default();
        let mut alerts = stored
            .as_mut()
            .map(|state| std::mem::take(&mut state.alerts))
            .unwrap_or_default();
        for alert in &mut alerts {
            if alert.device_id.is_empty() {
                alert.device_id = default_device_id.clone();
            }
        }
        alerts.truncate(MAX_ALERTS);
        let mut calls = stored
            .as_mut()
            .map(|state| std::mem::take(&mut state.calls))
            .unwrap_or_default();
        calls.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        calls.truncate(MAX_CALLS);

        let state = PersistedState {
            version: 3,
            settings,
            devices,
            users,
            alerts,
            calls,
            last_daily_report_date: stored
                .as_ref()
                .and_then(|value| value.last_daily_report_date.clone()),
            updated_at: Utc::now(),
        };

        let media_cipher = MediaCipher::load_or_create(&config.media_key_file)?;
        migrate_plaintext_media(&config, &media_cipher).await?;
        let store = Arc::new(Self {
            data_dir: config.data_dir.clone(),
            state: Mutex::new(state),
            media_cipher,
        });
        store.persist().await?;
        Ok(store)
    }

    pub async fn snapshot(&self) -> PersistedState {
        self.state.lock().await.clone()
    }

    pub async fn settings(&self) -> Settings {
        self.state.lock().await.settings.clone()
    }

    pub async fn device(&self, device_id: &str) -> Option<Device> {
        self.state
            .lock()
            .await
            .devices
            .iter()
            .find(|device| device.id == device_id)
            .cloned()
    }

    pub async fn user(&self, user_id: &str) -> Option<User> {
        self.state
            .lock()
            .await
            .users
            .iter()
            .find(|user| user.id == user_id)
            .cloned()
    }

    pub async fn user_by_username(&self, username: &str) -> Option<User> {
        let wanted = username.trim().to_ascii_lowercase();
        self.state
            .lock()
            .await
            .users
            .iter()
            .find(|user| user.username.to_ascii_lowercase() == wanted)
            .cloned()
    }

    pub async fn alerts_for(&self, device_id: Option<&str>, limit: usize) -> Vec<Alert> {
        let state = self.state.lock().await;
        state
            .alerts
            .iter()
            .filter(|alert| {
                device_id.is_none_or(|id| alert.device_id.is_empty() || alert.device_id == id)
            })
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn alert(&self, alert_id: &str) -> Option<Alert> {
        self.state
            .lock()
            .await
            .alerts
            .iter()
            .find(|alert| alert.id == alert_id)
            .cloned()
    }

    pub async fn calls_for(&self, device_id: &str, limit: usize) -> Vec<CallRecord> {
        self.state
            .lock()
            .await
            .calls
            .iter()
            .filter(|record| record.device_id == device_id)
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn call(&self, call_id: &str) -> Option<CallRecord> {
        self.state
            .lock()
            .await
            .calls
            .iter()
            .find(|record| record.id == call_id)
            .cloned()
    }

    pub async fn mutate<T>(&self, f: impl FnOnce(&mut PersistedState) -> T) -> anyhow::Result<T> {
        let mut state = self.state.lock().await;
        let result = f(&mut state);
        state.version = 3;
        state.updated_at = Utc::now();
        if state.alerts.len() > MAX_ALERTS {
            state.alerts.truncate(MAX_ALERTS);
        }
        if state.calls.len() > MAX_CALLS {
            state.calls.truncate(MAX_CALLS);
        }
        self.persist_locked(&state).await?;
        Ok(result)
    }

    pub async fn push_alert(&self, alert: Alert) -> anyhow::Result<Alert> {
        self.mutate(|state| {
            state.alerts.insert(0, alert.clone());
            alert
        })
        .await
    }

    pub async fn update_live(
        &self,
        device_id: &str,
        current_db: f64,
        average_db: Option<f64>,
        peak_db: Option<f64>,
        capturing: bool,
        suspended: bool,
        system_playing: bool,
        aec_enabled: bool,
    ) -> Option<crate::models::DeviceView> {
        let now = Utc::now();
        let mut state = self.state.lock().await;
        let settings = state.settings.clone();
        let device = state
            .devices
            .iter_mut()
            .find(|device| device.id == device_id)?;
        let offline = crate::models::effective_settings(&settings, device).offline_timeout_secs;
        device.current_db = Some(current_db);
        device.live_avg_db = average_db;
        device.peak_db = peak_db.or(device.peak_db);
        device.live_suspended = suspended;
        device.live_system_playing = system_playing;
        device.live_aec_enabled = aec_enabled;
        if !suspended {
            device.monitoring = capturing;
        }
        Some(device.view(now, offline))
    }

    pub async fn push_call(&self, record: CallRecord) -> anyhow::Result<CallRecord> {
        self.mutate(|state| {
            state.calls.insert(0, record.clone());
            record
        })
        .await
    }

    pub async fn update_alert_email(
        &self,
        alert_id: &str,
        status: &str,
        error_message: Option<String>,
    ) -> anyhow::Result<Option<Alert>> {
        self.mutate(|state| {
            let found = state.alerts.iter_mut().find(|alert| alert.id == alert_id);
            found.map(|alert| {
                alert.email_status = status.to_string();
                alert.email_error = error_message;
                alert.clone()
            })
        })
        .await
    }

    pub async fn acknowledge_alert(
        &self,
        alert_id: &str,
        acknowledged: bool,
    ) -> anyhow::Result<Option<Alert>> {
        self.mutate(|state| {
            let found = state.alerts.iter_mut().find(|alert| alert.id == alert_id);
            found.map(|alert| {
                alert.acknowledged = acknowledged;
                alert.acknowledged_at = acknowledged.then(Utc::now);
                alert.clone()
            })
        })
        .await
    }

    pub async fn append_reading(&self, reading: &Reading) -> anyhow::Result<()> {
        let dir = self.data_dir.join("readings");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(format!(
            "{}.jsonl",
            reading.recorded_at.format("%Y-%m-%d")
        ));
        let mut line = serde_json::to_string(reading)?;
        line.push('\n');
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        Ok(())
    }

    pub async fn read_readings_for(
        &self,
        device_id: Option<&str>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> anyhow::Result<Vec<Reading>> {
        let mut readings = Vec::new();
        let mut date: NaiveDate = from.date_naive();
        let last_date = to.date_naive();
        while date <= last_date {
            let path = self
                .data_dir
                .join("readings")
                .join(format!("{date}.jsonl"));
            if let Ok(raw) = tokio::fs::read_to_string(path).await {
                for line in raw.lines() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Reading>(line) {
                        Ok(reading)
                            if reading.recorded_at >= from
                                && reading.recorded_at <= to
                                && device_id.is_none_or(|id| reading.device_id == id) =>
                        {
                            readings.push(reading);
                        }
                        Ok(_) => {}
                        Err(error) => warn!("Skipping malformed reading row: {error}"),
                    }
                }
            }
            match date.succ_opt() {
                Some(next) => date = next,
                None => break,
            }
        }
        readings.sort_by_key(|reading| reading.recorded_at);
        Ok(readings)
    }

    pub async fn save_evidence(&self, bytes: &[u8], prefix: &str) -> anyhow::Result<String> {
        let dir = self.data_dir.join("evidence");
        tokio::fs::create_dir_all(&dir).await?;
        let short = uuid::Uuid::new_v4()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>();
        let filename = format!("{prefix}-{}-{short}.wav", Utc::now().format("%Y%m%d-%H%M%S"));
        let encrypted = self.media_cipher.encrypt(bytes)?;
        write_private(&dir.join(&filename), &encrypted).await?;
        Ok(filename)
    }

    pub async fn read_evidence(&self, filename: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let safe = safe_filename(filename)?;
        let path = self.data_dir.join("evidence").join(safe);
        if !path.exists() {
            return Ok(None);
        }
        let stored = tokio::fs::read(path).await?;
        Ok(Some(self.media_cipher.decrypt(&stored)?))
    }

    pub async fn save_call_audio(&self, bytes: &[u8], extension: &str) -> anyhow::Result<String> {
        let dir = self.data_dir.join("calls");
        tokio::fs::create_dir_all(&dir).await?;
        let extension = match extension {
            "wav" => "wav",
            "mp4" => "mp4",
            _ => "webm",
        };
        let short = uuid::Uuid::new_v4()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>();
        let filename = format!(
            "call-{}-{short}.{extension}",
            Utc::now().format("%Y%m%d-%H%M%S")
        );
        let encrypted = self.media_cipher.encrypt(bytes)?;
        write_private(&dir.join(&filename), &encrypted).await?;
        Ok(filename)
    }

    pub async fn read_call_audio(&self, filename: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let safe = safe_filename(filename)?;
        let path = self.data_dir.join("calls").join(safe);
        if !path.exists() {
            return Ok(None);
        }
        let stored = tokio::fs::read(path).await?;
        Ok(Some(self.media_cipher.decrypt(&stored)?))
    }

    pub async fn cleanup_readings(&self, retention_days: u32) -> anyhow::Result<usize> {
        let dir = self.data_dir.join("readings");
        let cutoff = (Utc::now() - chrono::Duration::days(retention_days as i64)).date_naive();
        let mut removed = 0;
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(_) => return Ok(0),
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            if let Ok(date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d") {
                if date < cutoff {
                    if tokio::fs::remove_file(&path).await.is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        Ok(removed)
    }

    async fn persist(&self) -> anyhow::Result<()> {
        let state = self.state.lock().await;
        self.persist_locked(&state).await
    }

    async fn persist_locked(&self, state: &PersistedState) -> anyhow::Result<()> {
        let path = self.data_dir.join("state.json");
        let temp = self.data_dir.join("state.json.tmp");
        let bytes = serde_json::to_vec_pretty(state)?;
        write_private(&temp, &bytes).await?;
        if tokio::fs::rename(&temp, &path).await.is_err() {
            let _ = tokio::fs::remove_file(&path).await;
            tokio::fs::rename(&temp, &path).await?;
        }
        Ok(())
    }
}

pub fn safe_filename(raw: &str) -> anyhow::Result<String> {
    let name = Path::new(raw)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid filename"))?;
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        anyhow::bail!("invalid filename");
    }
    Ok(name.to_string())
}


async fn migrate_plaintext_media(
    config: &Config,
    cipher: &MediaCipher,
) -> anyhow::Result<()> {
    let mut migrated = 0usize;
    for folder in ["evidence", "calls"] {
        let dir = config.data_path(folder);
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let stored = tokio::fs::read(&path).await?;
            if stored.starts_with(b"SGME1") {
                continue;
            }
            let encrypted = cipher.encrypt(&stored)?;
            write_private(&path, &encrypted).await?;
            migrated += 1;
        }
    }
    if migrated > 0 {
        tracing::info!(
            "Encrypted {migrated} existing media file(s) with AES-256-GCM"
        );
    }
    Ok(())
}

async fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    tokio::fs::write(path, bytes).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(0o600),
        )
        .await;
    }
    Ok(())
}

fn write_initial_admin_password(config: &Config, password: &str) -> anyhow::Result<()> {
    let path = config.data_path("initial-admin-password.txt");
    std::fs::write(
        &path,
        format!(
            "# 声垣初始超级管理员账号\n用户名: admin\n密码: {password}\n请首次登录后立即在用户管理中修改密码，并删除本文件。\n"
        ),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
