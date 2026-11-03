use std::sync::Arc;

use anyhow::{Context, Result};
use rbac_oidc::clock::{Clock, SystemClock};
use rbac_oidc::config::Config;
use rbac_oidc::keys::KeyMaterial;
use rbac_oidc::roles::{NearRpcRoleSource, RoleSource};
use rbac_oidc::{build_router, AppState};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("rbac_oidc=info,tower_http=info")),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("RBAC_OIDC_CONFIG").ok())
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

    tracing::info!(%address, "rbac-oidc listening");

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
