use std::{
    env,
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use serde_json::{json, Value};

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

#[derive(Clone, Debug)]
pub struct Config {
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

fn parse_f64(key: &str, fallback: f64) -> f64 {
    var(key)
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(fallback)
}

fn parse_bool(key: &str, fallback: bool) -> bool {
    var(key)
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(fallback)
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

fn load_or_create_secret(data_dir: &Path, filename: &str, prefix: &str) -> String {
    let path = data_dir.join(filename);
    if let Ok(raw) = fs::read_to_string(&path) {
        let value = raw.trim().to_string();
        if !value.is_empty() {
            return value;
        }
    }
    let value = format!("{prefix}{}", generated_secret());
    let _ = fs::create_dir_all(data_dir);
    if fs::write(&path, format!("{value}\n")).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
    }
    value
}

fn load_or_create_stop_pin(data_dir: &Path) -> String {
    let path = data_dir.join("initial-stop-pin.txt");
    if let Ok(raw) = fs::read_to_string(&path) {
        let value = raw.trim().to_string();
        if !value.is_empty() {
            return value;
        }
    }
    let value = format!("{:06}", rand::random::<u32>() % 1_000_000);
    let _ = fs::create_dir_all(data_dir);
    if fs::write(&path, format!("{value}
")).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
    }
    value
}

fn inboxes(key: &str, fallback_key: Option<&str>) -> Vec<String> {
    var(key)
        .or_else(|| fallback_key.and_then(var))
        .map(|value| parse_recipients(&value))
        .unwrap_or_default()
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let port = parse_u64("PORT", 8080) as u16;
        let bind = var_or("BIND_ADDR", "0.0.0.0");
        let data_dir = PathBuf::from(var_or("DATA_DIR", "data"));
        let web_dist = PathBuf::from(var_or("WEB_DIST", "web/dist"));
        let media_key_file = var("MEDIA_KEY_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("media-key"));
        let smtp = var("SMTP_HOST").map(|host| SmtpConfig {
            port: parse_u64("SMTP_PORT", 587) as u16,
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
        });

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
            _ => load_or_create_secret(&data_dir, "jwt-secret", ""),
        };
        let device_token = match var("DEVICE_TOKEN") {
            Some(value) if value.len() >= 16 => value,
            _ => load_or_create_secret(&data_dir, "initial-device-token", "sg_"),
        };
        let stop_pin = match var("STOP_PIN") {
            Some(value) => value,
            _ => load_or_create_stop_pin(&data_dir),
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

        let tls_cert = var("TLS_CERT").map(PathBuf::from);
        let tls_key = var("TLS_KEY").map(PathBuf::from);
        let (tls_cert, tls_key) = match (tls_cert, tls_key) {
            (Some(cert), Some(key)) if cert.is_file() && key.is_file() => (Some(cert), Some(key)),
            (Some(_), Some(_)) => {
                return Err(anyhow::anyhow!("TLS_CERT/TLS_KEY 文件不存在"));
            }
            _ => (None, None),
        };

        let alert_recipients = inboxes("ALERT_TO", None);
        let report_recipients = inboxes("REPORT_TO", Some("ALERT_TO"));
        let admin_email = var("ADMIN_EMAIL").or_else(|| alert_recipients.first().cloned());
        let default_settings = Settings {
            threshold_db: parse_f64("THRESHOLD_DB", 65.0),
            alert_recipients,
            report_recipients,
            daily_report_enabled: parse_bool("DAILY_REPORT_ENABLED", true),
            daily_report_time: var_or("DAILY_REPORT_TIME", "08:00"),
            offline_timeout_secs: parse_u64("OFFLINE_TIMEOUT_SECS", 45),
            alert_cooldown_secs: parse_u64("ALERT_COOLDOWN_SECS", 300),
            evidence_seconds: 30,
            auto_record_evidence: parse_bool("AUTO_RECORD_EVIDENCE", true),
            retention_days: parse_u64("RETENTION_DAYS", 30) as u32,
            notification_enabled: false,
            notification_title: String::new(),
            notification_content: String::new(),
        };

        Ok(Self {
            bind,
            port,
            data_dir,
            media_key_file,
            web_dist,
            admin_password: var_or("ADMIN_PASSWORD", ""),
            admin_email,
            jwt_secret,
            jwt_ttl_hours: parse_u64("JWT_TTL_HOURS", 24) as i64,
            device_id: var_or("DEVICE_ID", ""),
            device_name: var_or("DEVICE_NAME", "未命名设备"),
            device_token,
            stop_pin,
            public_base_url: var_or("PUBLIC_BASE_URL", &format!("http://127.0.0.1:{port}")),
            default_settings,
            smtp,
            ice_servers,
            cors_origins,
            tls_cert,
            tls_key,
        })
    }

    pub fn socket_addr(&self) -> anyhow::Result<SocketAddr> {
        format!("{}:{}", self.bind, self.port)
            .parse()
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
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}
