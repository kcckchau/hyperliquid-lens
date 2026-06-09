mod api;
mod backfill;
mod chart;
mod config;
mod db;
mod detection;
mod execution;
mod hyperliquid;
mod ingester;
mod risk;
mod setups;
mod signals;
mod trading;

use crate::api::routes::{
    get_event_stats, get_events, get_summary, get_trades, health, post_backfill, ws_events,
    ws_trades, AppState,
};
use crate::config::Config;
use crate::db::events::EventRepository;
use crate::db::trades::TradeRepository;
use crate::detection::outcome_tracker::resume_pending_trackers;
use crate::detection::pipeline::spawn_all_pipelines;
use crate::detection::types::MarketEvent;
use crate::ingester::parser::Trade;
use crate::ingester::ws_client::spawn_coin_ingester;

use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer())
        .init();

    let config = Config::from_env()?;
    info!(coins = ?config.coins, port = config.port, "Starting hyperliquid-lens");

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("Database migrations applied");

    // ── Broadcast channels ────────────────────────────────────────────────────
    let (trade_tx, _) = broadcast::channel::<Trade>(1024);
    let trade_tx = Arc::new(trade_tx);

    // Event channel: detection pipelines → WebSocket clients.
    // Buffer is larger because events are rarer but clients may subscribe late.
    let (event_tx, _) = broadcast::channel::<MarketEvent>(256);
    let event_tx = Arc::new(event_tx);

    // ── Ingesters (one per coin) ──────────────────────────────────────────────
    for coin in &config.coins {
        spawn_coin_ingester(coin.clone(), pool.clone(), (*trade_tx).clone());
    }

    // ── Detection pipelines (one per coin × interval) ─────────────────────────
    spawn_all_pipelines(&config.coins, pool.clone(), trade_tx.clone(), event_tx.clone());

    // ── Resume outcome trackers for any events left in Confirming state ───────
    // (covers the case where the process was restarted mid-observation-window)
    resume_pending_trackers(pool.clone()).await?;

    // ── Axum app ──────────────────────────────────────────────────────────────
    let state = AppState {
        pool: pool.clone(),
        repo: TradeRepository::new(pool.clone()),
        event_repo: EventRepository::new(pool),
        broadcast_tx: trade_tx,
        event_tx,
        chart_warmup: Arc::new(config.chart_warmup),
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
        .route("/events", get(get_events))
        .route("/events/stats", get(get_event_stats))
        .route("/ws/events", get(ws_events))
        .route("/backfill", post(post_backfill))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let addr = format!("0.0.0.0:{}", config.port);
    info!(addr = %addr, "API server listening");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
