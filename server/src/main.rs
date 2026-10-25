mod api;
mod auth;
mod config;
mod error;
mod hub;
mod mailer;
mod media_cipher;
mod models;
mod monitor;
mod notify;
mod nvidia_bnr;
mod private_file;
mod reports;
mod state;
mod store;
mod totp;
mod update;
mod wav;

use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{config::Config, hub::Hub, mailer::Mailer, state::AppState, store::Store};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "soundguard_server=info,tower_http=warn,axum=warn".into()),
        )
        .init();

    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = Arc::new(Config::from_env()?);
    config.ensure_dirs()?;
    let store = Store::load(&config).await?;
    let mailer = Arc::new(Mailer::new(&config)?);
    let hub = Arc::new(Hub::new());
    let state = AppState {
        config: config.clone(),
        store,
        mailer,
        hub,
        dummy_password_hash: Arc::new(auth::hash_password(&auth::generate_password(24))?),
        password_jobs: Arc::new(tokio::sync::Semaphore::new(4)),
        login_guard: Arc::new(std::sync::Mutex::new(crate::state::LoginGuard::default())),
    };

    monitor::spawn_offline_monitor(state.clone());
    monitor::spawn_maintenance(state.clone());
    reports::spawn_daily_report_scheduler(state.clone());

    let addr = config.socket_addr()?;
    let tls_enabled = config.tls_cert.is_some() && config.tls_key.is_some();
    let scheme = if tls_enabled { "https" } else { "http" };
    let local_url = format!("{scheme}://127.0.0.1:{}", config.port);

    info!("SoundGuard server listening on {scheme}://{addr}");
    info!("Admin dashboard: {local_url}");
    info!("Device API base: {local_url}/api");
    if config.smtp_configured() {
        info!("SMTP delivery is enabled");
    } else {
        warn!("SMTP is not configured. Emails will be written to data/outbox (dry-run).");
    }
    let app = api::build_router(state);

    if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key) {
        info!("TLS enabled: certificate {}", cert.display());
        let rustls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
        let handle = axum_server::Handle::new();
        let shutdown = handle.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            shutdown.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
        });
        axum_server::bind_rustls(addr, rustls)
            .handle(handle)
            .serve(app.into_make_service())
            .await?;
    } else {
        let listener = TcpListener::bind(addr).await?;
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;
    }
    Ok(())
}

async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                warn!("Could not listen for SIGTERM: {error}");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = interrupt => {}, _ = terminate => {} }
    info!("Shutdown signal received");
}
