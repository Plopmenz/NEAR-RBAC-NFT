use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use payrole_oidc::clock::{Clock, SystemClock};
use payrole_oidc::config::Config;
use payrole_oidc::keys::KeyMaterial;
use payrole_oidc::roles::{NearRpcRoleSource, RoleSource};
use payrole_oidc::{build_router, AppState};
use tokio::time::MissedTickBehavior;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("payrole_oidc=info,tower_http=info")),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("PAYROLE_OIDC_CONFIG").ok())
        .unwrap_or_else(|| "config.toml".to_owned());
    let config = Config::load(&config_path)?;

    let keys = match &config.signing_key_path {
        Some(path) => {
            let pem = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read signing key {}", path.display()))?;
            KeyMaterial::from_pem(&pem)?
        }
        None => {
            tracing::warn!(
                "no signing_key_path configured; generating an ephemeral RSA key. \
                 Tokens will become invalid on restart."
            );
            KeyMaterial::generate()?
        }
    };

    let listener = tokio::net::TcpListener::bind(&config.listen)
        .await
        .with_context(|| format!("failed to bind {}", config.listen))?;
    let address = listener.local_addr()?;

    let roles: Arc<dyn RoleSource> = Arc::new(NearRpcRoleSource::new(
        config.rpc_url.clone(),
        config.contract_id.clone(),
    ));
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let state = AppState::new(config, keys, roles, clock);

    spawn_store_sweeper(state.clone());

    tracing::info!(%address, "payrole-oidc listening");

    axum::serve(listener, build_router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server failed")?;
    Ok(())
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!(%err, "failed to install the shutdown signal handler");
    }
}

/// Periodically removes expired pending logins, codes and sessions.
fn spawn_store_sweeper(state: AppState) {
    let interval_secs = state.config.store_sweep_interval_secs;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let removed = state.store.prune_expired(state.clock.now()).await;
            if removed > 0 {
                tracing::debug!(removed, "pruned expired OIDC entries");
            }
        }
    });
}
