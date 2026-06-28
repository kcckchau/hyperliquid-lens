use crate::backfill::run_backfill;
use crate::chart::warmup::{merge_candles, ChartWarmupConfig, WarmupRequest};
use crate::db::events::EventRepository;
use crate::db::heartbeat::HeartbeatRepository;
use crate::db::trades::TradeRepository;
use crate::detection::types::MarketEvent;
use crate::hyperliquid::info_client::fetch_candle_snapshot;
use crate::ingester::parser::Trade;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Json as AxumJson, Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info};

/// Shared application state injected into every handler
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub repo: TradeRepository,
    pub event_repo: EventRepository,
    pub broadcast_tx: Arc<broadcast::Sender<Trade>>,
    pub event_tx: Arc<broadcast::Sender<MarketEvent>>,
    pub chart_warmup: Arc<ChartWarmupConfig>,
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let repo = HeartbeatRepository::new(state.pool.clone());
    let now_ms = chrono::Utc::now().timestamp_millis();

    let feeds = match repo.fetch_all().await {
        Ok(rows) => rows
            .into_iter()
            .map(|r| {
                let lag_s = (now_ms - r.last_trade_ts_ms) / 1000;
                let stale = lag_s > 60;
                (
                    r.coin,
                    serde_json::json!({
                        "last_trade_ts_ms": r.last_trade_ts_ms,
                        "lag_s": lag_s,
                        "stale": stale,
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>(),
        Err(_) => serde_json::Map::new(),
    };

    let all_ok = feeds.values().all(|v| !v["stale"].as_bool().unwrap_or(false));
    let status = if feeds.is_empty() {
        "starting"
    } else if all_ok {
        "ok"
    } else {
        "degraded"
    };

    Json(serde_json::json!({
        "status": status,
        "feeds": feeds,
    }))
}

// ---------------------------------------------------------------------------
// GET /trades
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TradesQuery {
    pub coin: String,
    pub from: Option<i64>,
    pub to: Option<i64>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    100
}

pub async fn get_trades(
    State(state): State<AppState>,
    Query(params): Query<TradesQuery>,
) -> impl IntoResponse {
    let limit = params.limit.clamp(1, 1000);
    match state
        .repo
        .fetch(&params.coin, params.from, params.to, limit)
        .await
    {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// GET /trades/summary  → OHLCV
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    pub coin: String,
    /// "1m" | "5m" | "15m" | "1h" | "4h" | "1d"
    #[serde(default = "default_interval")]
    pub interval: String,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub visible_bars: Option<usize>,
}

fn default_interval() -> String {
    "1h".to_string()
}

fn interval_to_ms(interval: &str) -> Option<i64> {
    match interval {
        "1m" => Some(60_000),
        "5m" => Some(300_000),
        "15m" => Some(900_000),
        "1h" => Some(3_600_000),
        "4h" => Some(14_400_000),
        "1d" => Some(86_400_000),
        _ => None,
    }
}

#[derive(Debug, Serialize)]
struct SummaryResponse {
    coin: String,
    interval: String,
    candles: Vec<crate::db::trades::OhlcvRow>,
}

pub async fn get_summary(
    State(state): State<AppState>,
    Query(params): Query<SummaryQuery>,
) -> impl IntoResponse {
    let Some(interval_ms) = interval_to_ms(&params.interval) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid interval, use: 1m 5m 15m 1h 4h 1d" })),
        )
            .into_response();
    };

    match state.repo.ohlcv(&params.coin, interval_ms, params.from, params.to).await {
        Ok(local_candles) => {
            let candles = if params.from.is_none() && params.to.is_none() {
                // Live mode: fill recent gaps from HL API.
                match warm_summary_candles(
                    state.chart_warmup.as_ref(),
                    &params.coin,
                    &params.interval,
                    interval_ms,
                    params.visible_bars,
                    local_candles.clone(),
                )
                .await
                {
                    Ok(candles) => candles,
                    Err(_) => local_candles,
                }
            } else if let (Some(from), Some(to)) = (params.from, params.to) {
                // Historical date range: fetch from HL API and merge with any
                // local trades so the chart always shows a full picture.
                match fetch_candle_snapshot(&params.coin, &params.interval, from, to).await {
                    Ok(remote) => merge_candles(remote, local_candles),
                    Err(_) => local_candles,
                }
            } else {
                local_candles
            };

            Json(SummaryResponse {
                coin: params.coin,
                interval: params.interval,
                candles,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn warm_summary_candles(
    chart_warmup: &ChartWarmupConfig,
    coin: &str,
    interval: &str,
    interval_ms: i64,
    visible_bars: Option<usize>,
    local_candles: Vec<crate::db::trades::OhlcvRow>,
) -> anyhow::Result<Vec<crate::db::trades::OhlcvRow>> {
    let plan = chart_warmup.build_plan(WarmupRequest {
        interval,
        visible_bars,
        local_candle_count: local_candles.len(),
    })?;

    if !plan.needs_remote {
        return Ok(local_candles);
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let start_time = now_ms - interval_ms * plan.remote_fetch_bars as i64;
    let snapshot = fetch_candle_snapshot(coin, interval, start_time, now_ms).await?;
    Ok(merge_candles(snapshot, local_candles))
}

// ---------------------------------------------------------------------------
// WebSocket: GET /ws/trades?coin=ETH
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub coin: String,
}

pub async fn ws_trades(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<WsQuery>,
) -> impl IntoResponse {
    let coin = params.coin.to_uppercase();
    info!(coin = %coin, "WebSocket client connecting (trades)");
    ws.on_upgrade(move |socket| handle_ws_trades(socket, state, coin))
}

async fn handle_ws_trades(mut socket: WebSocket, state: AppState, coin: String) {
    let mut rx = state.broadcast_tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(trade) if trade.coin == coin => {
                        let json = match serde_json::to_string(&trade) {
                            Ok(j) => j,
                            Err(e) => { debug!(error = %e, "Failed to serialize trade"); continue; }
                        };
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(skipped = n, "Trade WS client lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() { break; }
                    }
                    _ => {}
                }
            }
        }
    }

    info!(coin = %coin, "Trade WS client disconnected");
}

// ---------------------------------------------------------------------------
// GET /events — query detected market events
// ?coin=ETH&interval=5m&event_type=liquidity_sweep&lifecycle=confirmed&from=X&to=Y&limit=100
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    pub coin: String,
    pub interval: Option<String>,
    pub event_type: Option<String>,
    pub lifecycle: Option<String>,
    /// Filter by source: "live" or "backfill"
    pub source: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

pub async fn get_events(
    State(state): State<AppState>,
    Query(params): Query<EventsQuery>,
) -> impl IntoResponse {
    let limit = params.limit.clamp(1, 500);
    match state
        .event_repo
        .fetch(
            &params.coin,
            params.interval.as_deref(),
            params.event_type.as_deref(),
            params.lifecycle.as_deref(),
            params.source.as_deref(),
            params.from,
            params.to,
            limit,
        )
        .await
    {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// GET /events/stats — outcome distribution for a coin/interval
// ?coin=ETH&interval=5m&event_type=liquidity_sweep
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct EventStatsQuery {
    pub coin: String,
    pub interval: Option<String>,
    pub event_type: Option<String>,
}

pub async fn get_event_stats(
    State(state): State<AppState>,
    Query(params): Query<EventStatsQuery>,
) -> impl IntoResponse {
    match state
        .event_repo
        .outcome_stats(
            &params.coin,
            params.interval.as_deref(),
            params.event_type.as_deref(),
        )
        .await
    {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// WebSocket: GET /ws/events?coin=ETH  — live market event stream
// ---------------------------------------------------------------------------

pub async fn ws_events(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<WsQuery>,
) -> impl IntoResponse {
    let coin = params.coin.to_uppercase();
    info!(coin = %coin, "WebSocket client connecting (events)");
    ws.on_upgrade(move |socket| handle_ws_events(socket, state, coin))
}

async fn handle_ws_events(mut socket: WebSocket, state: AppState, coin: String) {
    let mut rx = state.event_tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) if event.coin == coin => {
                        let json = match serde_json::to_string(&event) {
                            Ok(j) => j,
                            Err(e) => { debug!(error = %e, "Failed to serialize event"); continue; }
                        };
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(skipped = n, "Events WS client lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() { break; }
                    }
                    _ => {}
                }
            }
        }
    }

    info!(coin = %coin, "Events WS client disconnected");
}

// ---------------------------------------------------------------------------
// POST /backfill — run historical detection for a coin on a given UTC date
// ---------------------------------------------------------------------------
// Body: { "coin": "BTC", "date": "2025-01-15" }
// Response: BackfillSummary JSON with per-interval event counts.
//
// Re-running for the same coin + date is idempotent: existing backfill events
// for that day are deleted first.
//
// Note: cascades are not detected because OHLCV snapshots have no per-trade
// liquidation counts. Only liquidity sweeps are produced.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct BackfillRequest {
    pub coin: String,
    /// UTC date to backfill in "YYYY-MM-DD" format.
    pub date: String,
}

pub async fn post_backfill(
    State(state): State<AppState>,
    AxumJson(req): AxumJson<BackfillRequest>,
) -> impl IntoResponse {
    let date = match NaiveDate::parse_from_str(&req.date, "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid date, expected YYYY-MM-DD" })),
            )
                .into_response();
        }
    };

    let coin = req.coin.trim().to_uppercase();
    if coin.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "coin must not be empty" })),
        )
            .into_response();
    }

    let event_tx = (*state.event_tx).clone();

    match run_backfill(&coin, date, state.pool.clone(), event_tx).await {
        Ok(summary) => Json(summary).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
