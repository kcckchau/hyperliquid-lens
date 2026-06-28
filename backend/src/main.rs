mod alert;
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
use crate::alert::spawn_alert_task;
use crate::backfill::backfill_gap;
use crate::config::Config;
use crate::db::events::EventRepository;
use crate::db::heartbeat::HeartbeatRepository;
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
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
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

    // ── Startup gap detection ─────────────────────────────────────────────────
    // Check heartbeats from the previous run. If any coin has a gap > 2 min,
    // fire backfill concurrently. We do this BEFORE spawning live ingesters so
    // the event channel exists when gap fill tries to broadcast.
    // (The broadcast channel is created just below; gap tasks are spawned after.)

    // ── Broadcast channels ────────────────────────────────────────────────────
    let (trade_tx, _) = broadcast::channel::<Trade>(1024);
    let trade_tx = Arc::new(trade_tx);

    // Event channel: detection pipelines → WebSocket clients.
    // Buffer is larger because events are rarer but clients may subscribe late.
    let (event_tx, _) = broadcast::channel::<MarketEvent>(256);
    let event_tx = Arc::new(event_tx);

    // ── Startup gap fill (concurrent, non-blocking) ───────────────────────────
    {
        let hb_repo = HeartbeatRepository::new(pool.clone());
        let now_ms = chrono::Utc::now().timestamp_millis();

        match hb_repo.fetch_all().await {
            Ok(heartbeats) if !heartbeats.is_empty() => {
                for hb in heartbeats {
                    let gap_s = (now_ms - hb.last_trade_ts_ms) / 1000;
                    if gap_s < 120 {
                        continue; // normal — no gap
                    }
                    info!(
                        coin = %hb.coin,
                        gap_hours = gap_s / 3600,
                        "Detected startup gap — spawning backfill"
                    );
                    let gap_pool = pool.clone();
                    let gap_event_tx = (*event_tx).clone();
                    let coin = hb.coin.clone();
                    let start_ms = hb.last_trade_ts_ms;
                    tokio::spawn(async move {
                        if let Err(e) =
                            backfill_gap(&coin, start_ms, now_ms, gap_pool, gap_event_tx).await
                        {
                            warn!(coin, "Startup gap fill failed: {e}");
                        }
                    });
                }
            }
            Ok(_) => {
                info!("No heartbeats found — first run, skipping gap check");
            }
            Err(e) => {
                warn!("Could not read heartbeats for gap check: {e}");
            }
        }
    }

    // ── Ingesters (one per coin) ──────────────────────────────────────────────
    for coin in &config.coins {
        spawn_coin_ingester(coin.clone(), pool.clone(), (*trade_tx).clone());
    }

    // ── Telegram alert task ───────────────────────────────────────────────────
    match (config.telegram_bot_token.clone(), config.telegram_chat_id.clone()) {
        (Some(token), Some(chat_id)) => {
            info!("Telegram alerts enabled");
            spawn_alert_task(token, chat_id, event_tx.subscribe());
        }
        _ => {
            info!("Telegram alerts disabled (TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID not set)");
        }
    }

    // ── Heartbeat task ────────────────────────────────────────────────────────
    // Subscribes to the trade broadcast and writes last_trade_ts_ms per coin
    // to system_heartbeats every 30 s. Powers the /health feed-lag check.
    {
        let mut rx = trade_tx.subscribe();
        let hb_pool = pool.clone();
        tokio::spawn(async move {
            let repo = HeartbeatRepository::new(hb_pool);
            let mut last_seen: HashMap<String, i64> = HashMap::new();
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    result = rx.recv() => {
                        match result {
                            Ok(trade) => {
                                last_seen
                                    .entry(trade.coin.clone())
                                    .and_modify(|ts| *ts = (*ts).max(trade.timestamp_ms))
                                    .or_insert(trade.timestamp_ms);
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!(skipped = n, "heartbeat task lagged behind trade broadcast");
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    _ = tick.tick() => {
                        for (coin, ts) in &last_seen {
                            if let Err(e) = repo.upsert(coin, *ts).await {
                                warn!(coin, "heartbeat upsert failed: {e}");
                            }
                        }
                    }
                }
            }
        });
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
