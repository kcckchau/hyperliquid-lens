use crate::db::trades::TradeRepository;
use crate::ingester::parser::{parse_ws_message, Trade};
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use sqlx::PgPool;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

const HL_WS_URL: &str = "wss://api.hyperliquid.xyz/ws";
const INITIAL_BACKOFF_MS: u64 = 1_000;
const MAX_BACKOFF_MS: u64 = 60_000;

/// Build the subscription JSON for a given coin
fn subscribe_msg(coin: &str) -> String {
    serde_json::json!({
        "method": "subscribe",
        "subscription": { "type": "trades", "coin": coin }
    })
    .to_string()
}

/// Spawn a persistent ingester for a single coin.
/// On disconnect it will retry with exponential backoff.
pub fn spawn_coin_ingester(
    coin: String,
    pool: PgPool,
    tx: broadcast::Sender<Trade>,
) {
    tokio::spawn(async move {
        let mut backoff_ms = INITIAL_BACKOFF_MS;

        loop {
            match run_ws_session(&coin, &pool, &tx).await {
                Ok(_) => {
                    info!(coin = %coin, "WebSocket session ended cleanly, reconnecting…");
                }
                Err(e) => {
                    error!(coin = %coin, error = %e, backoff_ms, "WebSocket error, retrying…");
                }
            }

            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
        }
    });
}

async fn run_ws_session(
    coin: &str,
    pool: &PgPool,
    tx: &broadcast::Sender<Trade>,
) -> Result<()> {
    info!(coin = %coin, url = HL_WS_URL, "Connecting to Hyperliquid WebSocket");

    let (ws_stream, _) = connect_async(HL_WS_URL).await?;
    let (mut write, mut read) = ws_stream.split();

    // Subscribe to the trades feed for this coin
    write
        .send(Message::Text(subscribe_msg(coin).into()))
        .await?;

    info!(coin = %coin, "Subscribed to trades feed");

    let repo = TradeRepository::new(pool.clone());

    while let Some(msg) = read.next().await {
        let msg = msg?;

        match msg {
            Message::Text(text) => {
                match parse_ws_message(&text) {
                    Ok(trades) if !trades.is_empty() => {
                        for trade in trades {
                            // Persist to database (best-effort — don't abort on duplicate)
                            if let Err(e) = repo.insert(&trade).await {
                                warn!(coin = %coin, error = %e, "Failed to insert trade");
                            }

                            // Broadcast to live WebSocket subscribers
                            // Ignore send errors — no active subscribers is fine
                            let _ = tx.send(trade);
                        }
                    }
                    Ok(_) => {} // non-trades message, ignore
                    Err(e) => {
                        warn!(coin = %coin, error = %e, "Failed to parse message");
                    }
                }
            }
            Message::Ping(data) => {
                write.send(Message::Pong(data)).await?;
            }
            Message::Close(_) => {
                info!(coin = %coin, "WebSocket connection closed by server");
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
