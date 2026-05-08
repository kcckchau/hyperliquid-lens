use crate::db::trades::TradeRepository;
use crate::ingester::parser::Trade;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info};

/// Shared application state injected into every handler
#[derive(Clone)]
pub struct AppState {
    pub repo: TradeRepository,
    pub broadcast_tx: Arc<broadcast::Sender<Trade>>,
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
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

    match state
        .repo
        .ohlcv(&params.coin, interval_ms, params.from, params.to)
        .await
    {
        Ok(candles) => Json(SummaryResponse {
            coin: params.coin,
            interval: params.interval,
            candles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
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
    info!(coin = %coin, "WebSocket client connecting");
    ws.on_upgrade(move |socket| handle_ws(socket, state, coin))
}

async fn handle_ws(mut socket: WebSocket, state: AppState, coin: String) {
    let mut rx = state.broadcast_tx.subscribe();

    loop {
        tokio::select! {
            // Forward live trades that match the requested coin
            result = rx.recv() => {
                match result {
                    Ok(trade) if trade.coin == coin => {
                        let json = match serde_json::to_string(&trade) {
                            Ok(j) => j,
                            Err(e) => {
                                debug!(error = %e, "Failed to serialize trade");
                                continue;
                            }
                        };
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            // Client disconnected
                            break;
                        }
                    }
                    Ok(_) => {} // different coin
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(skipped = n, "WebSocket client lagged, dropping old messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Handle incoming messages from the client (ping / close)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    info!(coin = %coin, "WebSocket client disconnected");
}
