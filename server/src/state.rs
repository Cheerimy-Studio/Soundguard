use std::sync::Arc;

use crate::{config::Config, hub::Hub, mailer::Mailer, store::Store};

#[derive(Debug, Default)]
pub struct LoginGuard {
    pub failures: u32,
    pub locked_until: Option<std::time::Instant>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Arc<Store>,
    pub mailer: Arc<Mailer>,
    pub hub: Arc<Hub>,
    pub login_guard: Arc<std::sync::Mutex<LoginGuard>>,
}
