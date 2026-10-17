use std::{
    collections::HashMap,
    sync::Arc,
};

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
pub struct Hub {
    admin_tx: broadcast::Sender<String>,
    devices: Arc<Mutex<HashMap<String, DeviceConnection>>>,
}

impl Hub {
    pub fn new() -> Self {
        let (admin_tx, _) = broadcast::channel(512);
        Self {
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
            .insert(device_id, DeviceConnection { id, tx });
        if previous.is_some() {
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
