use crate::detection::types::{
    CascadeDirection, EventLifecycle, EventSource, EventType, MarketEvent, SweepDirection,
};
use anyhow::Result;
use reqwest::Client;
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{info, warn};

const COOLDOWN: Duration = Duration::from_secs(5 * 60); // 5 min per coin+type+direction

#[derive(Debug, Serialize)]
struct TelegramMessage<'a> {
    chat_id: &'a str,
    text: String,
    parse_mode: &'static str,
}

pub struct TelegramAlerter {
    client: Client,
    token: String,
    chat_id: String,
}

impl TelegramAlerter {
    pub fn new(token: String, chat_id: String) -> Self {
        Self {
            client: Client::new(),
            token,
            chat_id,
        }
    }

    async fn send(&self, text: String) -> Result<()> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
        let body = TelegramMessage {
            chat_id: &self.chat_id,
            text,
            parse_mode: "HTML",
        };
        let resp = self.client.post(&url).json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!("Telegram API error {status}: {body}");
        }
        Ok(())
    }

    fn format_event(event: &MarketEvent) -> String {
        match event.event_type {
            EventType::LiquiditySweep => {
                let direction = match &event.sweep_direction {
                    Some(SweepDirection::Bullish) => "BULLISH",
                    Some(SweepDirection::Bearish) => "BEARISH",
                    None => "UNKNOWN",
                };
                let level = event
                    .level_price
                    .as_ref()
                    .map(|p| format!("${p}"))
                    .unwrap_or_default();
                let wick = event
                    .wick_pct
                    .as_ref()
                    .map(|w| {
                        let pct = w * rust_decimal::Decimal::new(100, 0);
                        format!("{:.3}%", pct)
                    })
                    .unwrap_or_default();
                let close = event
                    .close_price
                    .as_ref()
                    .map(|p| format!("${p}"))
                    .unwrap_or_default();

                format!(
                    "<b>[SWEEP] {} {}</b>\n{} sweep of {}\nWick {} → close {}",
                    event.coin, event.interval, direction, level, wick, close
                )
            }
            EventType::LiquidationCascade => {
                let direction = match &event.cascade_direction {
                    Some(CascadeDirection::LongLiq) => "LONG LIQ",
                    Some(CascadeDirection::ShortLiq) => "SHORT LIQ",
                    None => "UNKNOWN",
                };
                let start = event
                    .cascade_start_price
                    .as_ref()
                    .map(|p| format!("${p}"))
                    .unwrap_or_default();
                let liqs = event
                    .liq_count_total
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                let candles = event
                    .candles_sustained
                    .map(|n| n.to_string())
                    .unwrap_or_default();

                format!(
                    "<b>[CASCADE] {} {}</b>\n{} liquidation cascade\nStart {} | {} liqs | {} candles",
                    event.coin, event.interval, direction, start, liqs, candles
                )
            }
        }
    }
}

/// Spawn a background task that listens for live detected events and
/// sends Telegram alerts with per-coin+type cooldown to suppress noise.
pub fn spawn_alert_task(
    token: String,
    chat_id: String,
    mut event_rx: broadcast::Receiver<MarketEvent>,
) {
    tokio::spawn(async move {
        let alerter = TelegramAlerter::new(token, chat_id);
        // cooldown_key = "{coin}_{event_type}_{direction}"
        let mut cooldowns: HashMap<String, Instant> = HashMap::new();

        info!("Telegram alert task started");

        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    // Only alert on live events entering the observation window.
                    // Skip backfill events (would spam on startup gap fill).
                    if event.source != EventSource::Live {
                        continue;
                    }
                    if event.lifecycle != EventLifecycle::Confirming {
                        continue;
                    }

                    // Build cooldown key from coin + type + direction
                    let key = match (&event.event_type, &event.sweep_direction, &event.cascade_direction) {
                        (EventType::LiquiditySweep, Some(d), _) => format!("{}_sweep_{d:?}", event.coin),
                        (EventType::LiquidationCascade, _, Some(d)) => format!("{}_cascade_{d:?}", event.coin),
                        _ => format!("{}_unknown", event.coin),
                    };

                    let now = Instant::now();
                    if let Some(last) = cooldowns.get(&key) {
                        if now.duration_since(*last) < COOLDOWN {
                            continue; // still in cooldown
                        }
                    }
                    cooldowns.insert(key, now);

                    let text = TelegramAlerter::format_event(&event);
                    info!(coin = %event.coin, "Sending Telegram alert");
                    if let Err(e) = alerter.send(text).await {
                        warn!("Failed to send Telegram alert: {e}");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "Alert task lagged behind event broadcast");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
