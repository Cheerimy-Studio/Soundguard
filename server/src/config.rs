use std::{
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use serde_json::{json, Value};
use tracing::warn;

use crate::models::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpTls {
    StartTls,
    Wrapper,
    None,
}

#[derive(Clone, Debug)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
    pub from_name: String,
    pub tls: SmtpTls,
}

/// Values explicitly configured in `.env` take precedence over the global
/// settings persisted in `data/state.json`. When an env var is absent, the
/// value saved by the administrator is kept.
#[derive(Clone, Debug, Default)]
pub struct SettingsOverrides {
    pub threshold_db: Option<f64>,
    pub alert_recipients: Option<Vec<String>>,
    pub report_recipients: Option<Vec<String>>,
    pub daily_report_enabled: Option<bool>,
    pub daily_report_time: Option<String>,
    pub offline_timeout_secs: Option<u64>,
    pub alert_cooldown_secs: Option<u64>,
    pub auto_record_evidence: Option<bool>,
    pub retention_days: Option<u32>,
}

impl SettingsOverrides {
    pub fn is_empty(&self) -> bool {
        self.threshold_db.is_none()
            && self.alert_recipients.is_none()
            && self.report_recipients.is_none()
            && self.daily_report_enabled.is_none()
            && self.daily_report_time.is_none()
            && self.offline_timeout_secs.is_none()
            && self.alert_cooldown_secs.is_none()
            && self.auto_record_evidence.is_none()
            && self.retention_days.is_none()
    }

    pub fn apply_to(&self, settings: &mut Settings) {
        if let Some(value) = self.threshold_db {
            settings.threshold_db = value;
        }
        if let Some(value) = &self.alert_recipients {
            settings.alert_recipients = value.clone();
        }
        if let Some(value) = &self.report_recipients {
            settings.report_recipients = value.clone();
        }
        if let Some(value) = self.daily_report_enabled {
            settings.daily_report_enabled = value;
        }
        if let Some(value) = &self.daily_report_time {
            settings.daily_report_time = value.clone();
        }
        if let Some(value) = self.offline_timeout_secs {
            settings.offline_timeout_secs = value;
        }
        if let Some(value) = self.alert_cooldown_secs {
            settings.alert_cooldown_secs = value;
        }
        if let Some(value) = self.auto_record_evidence {
            settings.auto_record_evidence = value;
        }
        if let Some(value) = self.retention_days {
            settings.retention_days = value;
        }
    }

    pub fn labels(&self) -> Vec<&'static str> {
        let mut labels = Vec::new();
        if self.threshold_db.is_some() {
            labels.push("THRESHOLD_DB");
        }
        if self.alert_recipients.is_some() {
            labels.push("ALERT_TO");
        }
        if self.report_recipients.is_some() {
            labels.push("REPORT_TO");
        }
        if self.daily_report_enabled.is_some() {
            labels.push("DAILY_REPORT_ENABLED");
        }
        if self.daily_report_time.is_some() {
            labels.push("DAILY_REPORT_TIME");
        }
        if self.offline_timeout_secs.is_some() {
            labels.push("OFFLINE_TIMEOUT_SECS");
        }
        if self.alert_cooldown_secs.is_some() {
            labels.push("ALERT_COOLDOWN_SECS");
        }
        if self.auto_record_evidence.is_some() {
            labels.push("AUTO_RECORD_EVIDENCE");
        }
        if self.retention_days.is_some() {
            labels.push("RETENTION_DAYS");
        }
        labels
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub _data_lock: std::sync::Arc<fs::File>,
    pub bind: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub media_key_file: PathBuf,
    pub web_dist: PathBuf,
    pub admin_password: String,
    pub admin_email: Option<String>,
    pub jwt_secret: String,
    pub jwt_ttl_hours: i64,
    pub device_id: String,
    pub device_name: String,
    pub device_token: String,
    pub stop_pin: String,
    pub public_base_url: String,
    pub smtp: Option<SmtpConfig>,
    pub ice_servers: Value,
    pub cors_origins: Vec<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub default_settings: Settings,
    pub env_overrides: SettingsOverrides,
}

fn var(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn var_or(key: &str, fallback: impl Into<String>) -> String {
    var(key).unwrap_or_else(|| fallback.into())
}

fn parse_u64(key: &str, fallback: u64) -> u64 {
    var(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(fallback)
}

fn parse_port(key: &str, fallback: u16) -> anyhow::Result<u16> {
    let raw = var(key).unwrap_or_else(|| fallback.to_string());
    let port: u16 = raw
        .parse()
        .map_err(|_| anyhow::anyhow!("{key} must be 1–65535"))?;
    anyhow::ensure!(port != 0, "{key} must be 1–65535");
    Ok(port)
}

fn parse_f64(key: &str, fallback: f64) -> f64 {
    var(key)
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(fallback)
}

fn parse_bool(key: &str, fallback: bool) -> bool {
    var(key)
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(fallback)
}

fn opt_u64(key: &str) -> Option<u64> {
    var(key).and_then(|value| match value.parse::<u64>() {
        Ok(parsed) => Some(parsed),
        Err(_) => {
            warn!("忽略无效的环境变量 {key}={value}，需要无符号整数");
            None
        }
    })
}

fn opt_f64(key: &str) -> Option<f64> {
    var(key).and_then(|value| match value.parse::<f64>() {
        Ok(parsed) if parsed.is_finite() => Some(parsed),
        _ => {
            warn!("忽略无效的环境变量 {key}={value}，需要数字");
            None
        }
    })
}

fn opt_bool(key: &str) -> Option<bool> {
    var(key).and_then(|value| match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => {
            warn!("忽略无效的环境变量 {key}={value}，需要 true/false");
            None
        }
    })
}

pub fn parse_recipients(raw: &str) -> Vec<String> {
    let mut recipients: Vec<String> = raw
        .split([',', ';', ' ', '\n'])
        .map(str::trim)
        .filter(|item| item.contains('@') && !item.starts_with('@') && !item.ends_with('@'))
        .map(ToString::to_string)
        .collect();
    recipients.sort();
    recipients.dedup();
    recipients
}

fn generated_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn load_or_create_secret(data_dir: &Path, filename: &str, prefix: &str) -> anyhow::Result<String> {
    let path = data_dir.join(filename);
    match fs::read_to_string(&path) {
        Ok(raw) if !raw.trim().is_empty() => return Ok(raw.trim().to_string()),
        Ok(_) => anyhow::bail!(
            "Secret file {} is empty; restore it from backup",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let value = format!("{prefix}{}", generated_secret());
    crate::private_file::atomic_write(&path, format!("{value}\n").as_bytes())?;
    Ok(value)
}

fn load_or_create_stop_pin(data_dir: &Path) -> anyhow::Result<String> {
    load_or_create_secret(data_dir, "initial-stop-pin.txt", "")
}

fn inboxes(key: &str, fallback_key: Option<&str>) -> Vec<String> {
    var(key)
        .or_else(|| fallback_key.and_then(var))
        .map(|value| parse_recipients(&value))
        .unwrap_or_default()
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let port = parse_port("PORT", 8080)?;
        let bind = var_or("BIND_ADDR", "0.0.0.0");
        let data_dir = PathBuf::from(var_or("DATA_DIR", "data"));
        ensure_dir(&data_dir)?;
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let data_lock = options.open(data_dir.join(".soundguard.lock"))?;
        data_lock.try_lock().map_err(|error| {
            anyhow::anyhow!("DATA_DIR is already in use or cannot be locked: {error}")
        })?;
        let web_dist = PathBuf::from(var_or("WEB_DIST", "web/dist"));
        let media_key_file = var("MEDIA_KEY_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("media-key"));
        let smtp = match var("SMTP_HOST") {
            Some(host) => Some(SmtpConfig {
                port: parse_port("SMTP_PORT", 587)?,
                username: var("SMTP_USER"),
                password: var("SMTP_PASS"),
                from: var_or("SMTP_FROM", "soundguard@localhost"),
                from_name: var_or("SMTP_FROM_NAME", "声垣 SoundGuard"),
                tls: match var("SMTP_TLS")
                    .unwrap_or_else(|| "starttls".to_string())
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "none" | "false" | "plain" => SmtpTls::None,
                    "ssl" | "wrapper" | "implicit" => SmtpTls::Wrapper,
                    _ => SmtpTls::StartTls,
                },
                host,
            }),
            None => None,
        };

        let ice_servers = var("ICE_SERVERS")
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .unwrap_or_else(|| {
                json!([
                    { "urls": "stun:stun.l.google.com:19302" },
                    { "urls": "stun:stun1.l.google.com:19302" }
                ])
            });

        let jwt_secret = match var("JWT_SECRET") {
            Some(value) if value.len() >= 32 => value,
            _ => load_or_create_secret(&data_dir, "jwt-secret", "")?,
        };
        let device_token = match var("DEVICE_TOKEN") {
            Some(value) if value.len() >= 16 => value,
            _ => load_or_create_secret(&data_dir, "initial-device-token", "sg_")?,
        };
        let stop_pin = match var("STOP_PIN") {
            Some(value) => value,
            _ => load_or_create_stop_pin(&data_dir)?,
        };

        let cors_origins = var("CORS_ORIGINS")
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for origin in &cors_origins {
            let uri: axum::http::Uri = origin
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid CORS_ORIGINS entry"))?;
            anyhow::ensure!(
                matches!(uri.scheme_str(), Some("http" | "https"))
                    && uri.authority().is_some()
                    && uri.path() == "/"
                    && uri.query().is_none(),
                "CORS_ORIGINS entries must be HTTP(S) origins"
            );
            let _: axum::http::HeaderValue = origin.parse()?;
        }
        let tls_cert = var("TLS_CERT").map(PathBuf::from);
        let tls_key = var("TLS_KEY").map(PathBuf::from);
        let (tls_cert, tls_key) = match (tls_cert, tls_key) {
            (Some(cert), Some(key)) if cert.is_file() && key.is_file() => (Some(cert), Some(key)),
            (Some(_), Some(_)) => {
                return Err(anyhow::anyhow!("TLS_CERT/TLS_KEY 文件不存在"));
            }
            (None, None) => (None, None),
            _ => anyhow::bail!("TLS_CERT 和 TLS_KEY 必须同时配置"),
        };

        let alert_recipients = inboxes("ALERT_TO", None);
        let report_recipients = inboxes("REPORT_TO", Some("ALERT_TO"));
        let alert_override = var("ALERT_TO").map(|value| parse_recipients(&value));
        let report_override = var("REPORT_TO")
            .map(|value| parse_recipients(&value))
            .or_else(|| alert_override.clone());
        let admin_email = var("ADMIN_EMAIL").or_else(|| alert_recipients.first().cloned());
        let env_overrides = SettingsOverrides {
            threshold_db: opt_f64("THRESHOLD_DB"),
            alert_recipients: alert_override,
            report_recipients: report_override,
            daily_report_enabled: opt_bool("DAILY_REPORT_ENABLED"),
            daily_report_time: var("DAILY_REPORT_TIME"),
            offline_timeout_secs: opt_u64("OFFLINE_TIMEOUT_SECS"),
            alert_cooldown_secs: opt_u64("ALERT_COOLDOWN_SECS"),
            auto_record_evidence: opt_bool("AUTO_RECORD_EVIDENCE"),
            retention_days: opt_u64("RETENTION_DAYS")
                .map(|value| value.min(u32::MAX as u64) as u32),
        };
        let mut default_settings = Settings {
            threshold_db: parse_f64("THRESHOLD_DB", 65.0),
            alert_recipients,
            report_recipients,
            daily_report_enabled: parse_bool("DAILY_REPORT_ENABLED", true),
            daily_report_time: var_or("DAILY_REPORT_TIME", "08:00"),
            offline_timeout_secs: parse_u64("OFFLINE_TIMEOUT_SECS", 45),
            alert_cooldown_secs: parse_u64("ALERT_COOLDOWN_SECS", 300),
            evidence_seconds: 30,
            auto_record_evidence: parse_bool("AUTO_RECORD_EVIDENCE", true),
            retention_days: u32::try_from(parse_u64("RETENTION_DAYS", 30))?,
            notification_enabled: false,
            notification_title: String::new(),
            notification_content: String::new(),
            denoise_mode: "off".to_string(),
            voice_enhance: false,
        };

        default_settings
            .validate_and_normalize()
            .map_err(anyhow::Error::msg)?;
        let jwt_ttl_hours = parse_u64("JWT_TTL_HOURS", 24);
        anyhow::ensure!(
            (1..=720).contains(&jwt_ttl_hours),
            "JWT_TTL_HOURS must be 1–720"
        );
        Ok(Self {
            _data_lock: std::sync::Arc::new(data_lock),
            bind,
            port,
            data_dir,
            media_key_file,
            web_dist,
            admin_password: var_or("ADMIN_PASSWORD", ""),
            admin_email,
            jwt_secret,
            jwt_ttl_hours: jwt_ttl_hours as i64,
            device_id: var_or("DEVICE_ID", ""),
            device_name: var_or("DEVICE_NAME", "未命名设备"),
            device_token,
            stop_pin,
            public_base_url: var_or("PUBLIC_BASE_URL", format!("http://127.0.0.1:{port}")),
            default_settings,
            env_overrides,
            smtp,
            ice_servers,
            cors_origins,
            tls_cert,
            tls_key,
        })
    }

    pub fn socket_addr(&self) -> anyhow::Result<SocketAddr> {
        self.bind
            .parse::<std::net::IpAddr>()
            .map(|ip| SocketAddr::new(ip, self.port))
            .map_err(Into::into)
    }

    pub fn smtp_configured(&self) -> bool {
        self.smtp.is_some()
    }

    pub fn data_path(&self, name: &str) -> PathBuf {
        self.data_dir.join(name)
    }

    pub fn web_index(&self) -> PathBuf {
        self.web_dist.join("index.html")
    }

    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        for dir in [
            self.data_dir.clone(),
            self.data_path("readings"),
            self.data_path("evidence"),
            self.data_path("calls"),
            self.data_path("outbox"),
        ] {
            ensure_dir(&dir)?;
        }
        Ok(())
    }
}

fn ensure_dir(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_settings() -> Settings {
        Settings {
            threshold_db: 65.0,
            alert_recipients: vec!["saved@example.com".to_string()],
            report_recipients: vec!["report@example.com".to_string()],
            daily_report_enabled: true,
            daily_report_time: "08:00".to_string(),
            offline_timeout_secs: 45,
            alert_cooldown_secs: 300,
            evidence_seconds: 30,
            auto_record_evidence: true,
            retention_days: 30,
            notification_enabled: false,
            notification_title: String::new(),
            notification_content: String::new(),
            denoise_mode: "off".to_string(),
            voice_enhance: false,
        }
    }

    #[test]
    fn env_overrides_replace_only_configured_fields() {
        let mut settings = sample_settings();
        let overrides = SettingsOverrides {
            threshold_db: Some(58.5),
            alert_recipients: Some(vec!["env@example.com".to_string()]),
            ..Default::default()
        };
        overrides.apply_to(&mut settings);
        assert_eq!(settings.threshold_db, 58.5);
        assert_eq!(settings.alert_recipients, vec!["env@example.com"]);
        assert_eq!(settings.report_recipients, vec!["report@example.com"]);
        assert_eq!(settings.offline_timeout_secs, 45);
        assert_eq!(overrides.labels(), vec!["THRESHOLD_DB", "ALERT_TO"]);
    }

    #[test]
    fn empty_overrides_leave_saved_settings_untouched() {
        let mut settings = sample_settings();
        let original = settings.clone();
        SettingsOverrides::default().apply_to(&mut settings);
        assert_eq!(settings.threshold_db, original.threshold_db);
        assert_eq!(settings.alert_recipients, original.alert_recipients);
        assert!(SettingsOverrides::default().is_empty());
    }
}
