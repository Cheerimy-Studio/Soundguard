//! Network manager — HTTP heartbeat/telemetry + WebSocket signaling.
//!
//! Runs in a background tokio task and communicates with the GUI thread
//! via a shared state struct.

use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct NetState {
    pub connected: bool,
    pub threshold_db: f64,
    pub war_mode: bool,
    pub totp_secret: Option<String>,
    pub device_name: String,
    pub server_time: Option<String>,
}

pub struct NetManager {
    state: Arc<Mutex<NetState>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl NetManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(NetState::default())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn start(&self, server_url: String, device_id: String, device_token: String) {
        if self.running.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);

        let state = self.state.clone();
        let running = self.running.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create tokio runtime");
            rt.block_on(async move {
                net_loop(server_url, device_id, device_token, state, running).await;
            });
        });
    }

    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn poll(&self) -> Option<NetState> {
        self.state.try_lock().ok().map(|s| s.clone())
    }
}

async fn net_loop(
    server_url: String,
    device_id: String,
    device_token: String,
    state: Arc<Mutex<NetState>>,
    running: Arc<std::sync::atomic::AtomicBool>,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build HTTP client");

    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));
    let mut telemetry_interval = tokio::time::interval(Duration::from_secs(30));

    loop {
        if !running.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }

        tokio::select! {
            _ = heartbeat_interval.tick() => {
                let url = format!("{}/api/device/heartbeat", server_url);
                let resp = client
                    .post(&url)
                    .header("x-device-token", &device_token)
                    .json(&serde_json::json!({
                        "deviceId": device_id,
                        "monitoring": true,
                    }))
                    .send()
                    .await;

                match resp {
                    Ok(r) if r.status().is_success() => {
                        if let Ok(data) = r.json::<serde_json::Value>().await {
                            if let Ok(mut s) = state.try_lock() {
                                s.connected = true;
                                s.threshold_db = data.get("thresholdDb").and_then(|v| v.as_f64()).unwrap_or(65.0);
                                s.war_mode = data.get("warModeEnabled").and_then(|v| v.as_bool()).unwrap_or(false);
                                s.totp_secret = data.get("totpSecret").and_then(|v| v.as_str()).map(String::from);
                                s.device_name = data.get("deviceName").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            }
                        }
                    }
                    _ => {
                        if let Ok(mut s) = state.try_lock() {
                            s.connected = false;
                        }
                    }
                }
            }
            _ = telemetry_interval.tick() => {
                // Telemetry is sent by the GUI thread with actual dB values
                // This is a keepalive
            }
        }
    }
}
