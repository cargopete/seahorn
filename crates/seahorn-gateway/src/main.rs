use std::sync::Arc;

use alloy_primitives::B256;
use axum::{routing::any, Router};
use reqwest::Client;

mod aggregator;
mod collector;
mod config;
mod db;
mod proxy;
mod tap;

use config::Config;
use db::Pool;

/// Shared state injected into every Axum handler.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: Pool,
    pub http_client: Client,
    pub domain_sep: B256,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "seahorn_gateway=info".into()),
        )
        .init();

    let config = Arc::new(Config::load()?);

    // Connect to Postgres and ensure TAP schema exists.
    let pool = db::connect(&config.database.url).await?;
    tracing::info!(url = %config.database.url, "database connected");

    // Pre-compute EIP-712 domain separator.
    let domain_sep = tap::domain_separator(
        &config.tap.eip712_domain_name,
        config.tap.eip712_chain_id,
        config.tap.eip712_verifying_contract,
    );
    tracing::info!(
        name = %config.tap.eip712_domain_name,
        chain_id = config.tap.eip712_chain_id,
        verifying_contract = %config.tap.eip712_verifying_contract,
        domain_sep = %domain_sep,
        "EIP-712 domain separator computed"
    );

    let http_client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let state = AppState {
        config: Arc::clone(&config),
        pool: pool.clone(),
        http_client,
        domain_sep,
    };

    // Spawn background tasks.
    aggregator::spawn(Arc::clone(&config), pool.clone());
    collector::spawn(Arc::clone(&config), pool.clone());

    // Build Axum router — all paths go through the proxy handler.
    let app = Router::new()
        .route("/{*path}", any(proxy::handler))
        .route("/", any(proxy::handler))
        .with_state(state);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    tracing::info!(%addr, postgrest = %config.backend.postgrest_url, "seahorn-gateway listening");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
