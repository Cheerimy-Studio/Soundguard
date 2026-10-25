//! Client configuration — persisted as JSON next to the executable.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientConfig {
    pub server_url: String,
    pub device_id: String,
    pub device_token: String,
    pub calibration_offset_db: f64,
    pub local_password_hash: String,
    pub consent_accepted: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: "https://db.gzmath.cn".to_string(),
            device_id: String::new(),
            device_token: String::new(),
            calibration_offset_db: 90.0,
            local_password_hash: String::new(),
            consent_accepted: false,
        }
    }
}

impl ClientConfig {
    pub fn config_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("client-config.json")))
            .unwrap_or_else(|| PathBuf::from("client-config.json"))
    }

    pub fn load_or_default() -> Self {
        let path = Self::config_path();
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<ClientConfig>(&raw) {
                return config;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn needs_setup(&self) -> bool {
        self.server_url.is_empty()
            || self.device_id.is_empty()
            || self.device_token.is_empty()
            || self.local_password_hash.is_empty()
    }
}
