use std::sync::Arc;

use crate::{config::Config, hub::Hub, mailer::Mailer, store::Store};

#[derive(Debug, Default)]
pub struct LoginAttempt {
    pub failures: u32,
    pub touched: Option<std::time::Instant>,
    pub locked_until: Option<std::time::Instant>,
}

#[derive(Default)]
pub struct LoginGuard {
    pub accounts: std::collections::HashMap<String, LoginAttempt>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Arc<Store>,
    pub mailer: Arc<Mailer>,
    pub hub: Arc<Hub>,
    pub dummy_password_hash: Arc<String>,
    pub password_jobs: Arc<tokio::sync::Semaphore>,
    pub login_guard: Arc<std::sync::Mutex<LoginGuard>>,
}
