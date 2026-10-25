//! UTF-8 configuration, compatible with the legacy client-config.json.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[path = "../../client/common/config_file.rs"]
mod private_file;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientConfig {
    pub server_url: String,
    pub device_id: String,
    pub device_token: String,
    pub calibration_offset_db: f64,
    pub local_password_hash: String,
    pub consent_accepted: bool,
    pub auto_answer: bool,
    pub alert_volume: f32,
}
impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: "https://db.gzmath.cn".into(),
            device_id: String::new(),
            device_token: String::new(),
            calibration_offset_db: 90.0,
            local_password_hash: String::new(),
            consent_accepted: false,
            auto_answer: true,
            alert_volume: 0.22,
        }
    }
}
impl ClientConfig {
    pub fn config_path() -> PathBuf {
        std::env::var_os("SOUNDGUARD_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("client-config.json")))
                    .unwrap_or_else(|| PathBuf::from("client-config.json"))
            })
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(raw) => {
                let raw = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&raw);
                let config: Self = serde_json::from_slice(raw)
                    .map_err(|e| format!("配置文件损坏，请恢复备份（不会覆盖原文件）：{e}"))?;
                if !config.needs_setup() {
                    return config.normalize();
                }
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("读取配置失败：{e}")),
        }
    }
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        private_file::atomic_write(
            path,
            &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
    }
    pub fn needs_setup(&self) -> bool {
        self.device_id.trim().is_empty()
            || self.device_token.trim().is_empty()
            || self.local_password_hash.is_empty()
    }
    pub fn normalize(mut self) -> Result<Self, String> {
        self.server_url = self.server_url.trim().trim_end_matches('/').into();
        self.device_id = self.device_id.trim().into();
        self.device_token = self.device_token.trim().into();
        let url = reqwest::Url::parse(&self.server_url).map_err(|_| "服务器地址格式无效")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("服务器地址必须为不含凭据、查询或片段的 HTTP(S) 地址".into());
        }
        if self.device_id.is_empty() || self.device_token.is_empty() {
            return Err("请填写设备编号和设备令牌".into());
        }
        if self.device_id.len() > 128
            || !self
                .device_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        {
            return Err("设备编号仅支持 128 位以内的英文字母、数字、下划线、点和短横线".into());
        }
        if self.device_token.len() > 512 || !self.device_token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err("设备令牌格式无效，请复制管理端生成的令牌".into());
        }
        if !(40.0..=140.0).contains(&self.calibration_offset_db) || !self.alert_volume.is_finite() {
            return Err("校准或音量数据无效".into());
        }
        self.alert_volume = self.alert_volume.clamp(0.0, 1.0);
        Ok(self)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bom_and_corrupt_files_are_handled_without_overwrite() {
        let path = std::env::temp_dir().join(format!("sg-config-{}.json", uuid::Uuid::new_v4()));
        let bytes = b"\xef\xbb\xbf{\"deviceId\":\"room\",\"consentAccepted\":true}";
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(ClientConfig::load(&path).unwrap().device_id, "room");
        std::fs::write(&path, b"{broken").unwrap();
        assert!(ClientConfig::load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn legacy_config_and_utf8_bom_migrate() {
        let c: ClientConfig = serde_json::from_str(
            r#"{"deviceId":"教室一","deviceToken":"令牌","localPasswordHash":"hash"}"#,
        )
        .unwrap();
        assert_eq!(c.device_id, "教室一");
        assert!(c.auto_answer);
        assert!(!c.consent_accepted);
    }
    #[test]
    fn rejects_credential_urls_and_invalid_offsets() {
        let mut c = ClientConfig {
            device_id: "room".into(),
            device_token: "token".into(),
            ..Default::default()
        };
        c.server_url = "https://user:pass@example.com".into();
        assert!(c.clone().normalize().is_err());
        c.server_url = "http://localhost:8080/".into();
        c.calibration_offset_db = f64::NAN;
        assert!(c.normalize().is_err());
    }
}
