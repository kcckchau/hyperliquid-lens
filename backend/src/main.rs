mod api;
mod config;
mod db;
mod ingester;

use crate::api::routes::{get_summary, get_trades, health, ws_trades, AppState};
use crate::config::Config;
use crate::db::trades::TradeRepository;
use crate::ingester::ws_client::spawn_coin_ingester;

use anyhow::Result;
use axum::{routing::get, Router};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env (ignore if not present — Docker provides env vars directly)
    let _ = dotenvy::dotenv();

    // Initialise structured logging
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer())
        .init();

    let config = Config::from_env()?;
    info!(coins = ?config.coins, port = config.port, "Starting hyperliquid-lens");

    // Database pool
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    // Run pending migrations at startup
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("Database migrations applied");

    // Broadcast channel: ingester → WebSocket clients
    // 1024-message buffer per subscriber; laggy clients are dropped gracefully
    let (broadcast_tx, _) = broadcast::channel::<ingester::parser::Trade>(1024);
    let broadcast_tx = Arc::new(broadcast_tx);

    // Spawn one ingester per coin
    for coin in &config.coins {
        spawn_coin_ingester(coin.clone(), pool.clone(), (*broadcast_tx).clone());
    }

    // Build Axum app
    let state = AppState {
        repo: TradeRepository::new(pool),
        broadcast_tx,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/trades", get(get_trades))
        .route("/trades/summary", get(get_summary))
        .route("/ws/trades", get(ws_trades))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let addr = format!("0.0.0.0:{}", config.port);
    info!(addr = %addr, "API server listening");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
