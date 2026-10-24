use std::{collections::HashMap, sync::Arc};

use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::warn;
use uuid::Uuid;

#[derive(Clone)]
struct DeviceConnection {
    id: Uuid,
    tx: mpsc::Sender<String>,
}

#[derive(Clone)]
struct CallSession {
    owner: Uuid,
    call_id: String,
    created_at: std::time::Instant,
}

#[derive(Clone)]
pub struct Hub {
    calls: Arc<Mutex<HashMap<String, CallSession>>>,
    admin_tx: broadcast::Sender<String>,
    devices: Arc<Mutex<HashMap<String, DeviceConnection>>>,
}

impl Hub {
    pub fn new() -> Self {
        let (admin_tx, _) = broadcast::channel(512);
        Self {
            calls: Arc::new(Mutex::new(HashMap::new())),
            admin_tx,
            devices: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe_admin(&self) -> broadcast::Receiver<String> {
        self.admin_tx.subscribe()
    }

    pub async fn register_device(&self, device_id: String, tx: mpsc::Sender<String>) -> Uuid {
        let id = Uuid::new_v4();
        let previous = self
            .devices
            .lock()
            .await
            .insert(device_id.clone(), DeviceConnection { id, tx });
        if previous.is_some() {
            self.disconnect_call_device(&device_id).await;
            warn!("A previous device signaling connection was replaced");
        }
        id
    }

    pub async fn unregister_device(&self, device_id: &str, id: Uuid) {
        let mut guard = self.devices.lock().await;
        if guard
            .get(device_id)
            .is_some_and(|connection| connection.id == id)
        {
            guard.remove(device_id);
        }
    }

    pub async fn device_connected(&self, device_id: &str) -> bool {
        self.devices.lock().await.contains_key(device_id)
    }

    pub async fn signaling_snapshot(&self) -> HashMap<String, bool> {
        self.devices
            .lock()
            .await
            .keys()
            .map(|device_id| (device_id.clone(), true))
            .collect()
    }

    pub async fn send_to_device(&self, device_id: &str, message: String) -> bool {
        let guard = self.devices.lock().await;
        match guard.get(device_id) {
            Some(connection) => {
                if let Err(error) = connection.tx.try_send(message) {
                    warn!("Could not deliver signaling message to device {device_id}: {error}");
                    false
                } else {
                    true
                }
            }
            None => false,
        }
    }

    pub async fn is_current_device(&self, device_id: &str, id: Uuid) -> bool {
        self.devices
            .lock()
            .await
            .get(device_id)
            .is_some_and(|c| c.id == id)
    }

    /// Sessions older than this are treated as stale and replaced regardless
    /// of owner, preventing a dead admin page from blocking future calls.
    const STALE_SESSION_SECS: u64 = 120;

    pub async fn claim_call(&self, device_id: &str, owner: Uuid, call_id: &str) -> bool {
        let mut calls = self.calls.lock().await;
        match calls.get(device_id) {
            // Same admin + same call = keepalive for the existing session.
            Some(call) if call.owner == owner && call.call_id == call_id => true,
            // Same admin + new call = the previous call from this admin is
            // dead (page refresh, dropped socket). Replace it instead of
            // blocking the admin with "another administrator is calling".
            Some(call) if call.owner == owner => {
                tracing::warn!(
                    "Replacing stale call session for device {device_id}                      (old call {}, new call {call_id})",
                    call.call_id
                );
                calls.insert(
                    device_id.to_string(),
                    CallSession {
                        owner,
                        call_id: call_id.to_string(),
                        created_at: std::time::Instant::now(),
                    },
                );
                true
            }
            // Any session older than STALE_SESSION_SECS is considered dead;
            // replace it so a hung admin cannot block the device forever.
            Some(call) if call.created_at.elapsed().as_secs() > Self::STALE_SESSION_SECS => {
                tracing::warn!(
                    "Expiring stale call session for device {device_id}                      (age {}s, owner {})",
                    call.created_at.elapsed().as_secs(),
                    call.owner
                );
                calls.insert(
                    device_id.to_string(),
                    CallSession {
                        owner,
                        call_id: call_id.to_string(),
                        created_at: std::time::Instant::now(),
                    },
                );
                true
            }
            // A different admin has an active, recent session = reject.
            Some(_) => false,
            None => {
                calls.insert(
                    device_id.to_string(),
                    CallSession {
                        owner,
                        call_id: call_id.to_string(),
                        created_at: std::time::Instant::now(),
                    },
                );
                true
            }
        }
    }

    pub async fn call_owner(&self, device_id: &str, call_id: &str) -> Option<Uuid> {
        self.calls
            .lock()
            .await
            .get(device_id)
            .filter(|c| c.call_id == call_id)
            .map(|c| c.owner)
    }

    pub async fn finish_call(&self, device_id: &str, call_id: &str) {
        let mut calls = self.calls.lock().await;
        if calls.get(device_id).is_some_and(|c| c.call_id == call_id) {
            calls.remove(device_id);
        }
    }

    pub async fn disconnect_admin(&self, owner: Uuid) {
        let sessions = {
            let mut calls = self.calls.lock().await;
            let sessions: Vec<_> = calls
                .iter()
                .filter(|(_, c)| c.owner == owner)
                .map(|(device_id, c)| (device_id.clone(), c.call_id.clone()))
                .collect();
            calls.retain(|_, c| c.owner != owner);
            sessions
        };
        for (device_id, call_id) in sessions {
            self.send_to_device(
                &device_id,
                json!({"type":"hangup", "from":"admin", "callId":call_id}).to_string(),
            )
            .await;
        }
    }

    pub async fn disconnect_call_device(&self, device_id: &str) {
        if let Some(call) = self.calls.lock().await.remove(device_id) {
            self.broadcast_raw(
                json!({"type":"hangup", "from":"device", "deviceId":device_id,
                "callId":call.call_id, "adminConnectionId":call.owner})
                .to_string(),
            );
        }
    }

    pub fn broadcast_raw(&self, message: String) {
        let _ = self.admin_tx.send(message);
    }

    pub fn broadcast_event(&self, event_type: &str, data: Value) {
        let message = json!({ "type": event_type, "data": data }).to_string();
        let _ = self.admin_tx.send(message);
    }

    pub fn broadcast_alert(&self, alert: &crate::models::Alert) {
        if let Ok(value) = serde_json::to_value(alert) {
            self.broadcast_event("alert", value);
        }
    }
}
