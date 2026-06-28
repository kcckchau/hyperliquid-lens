use crate::db::regime::RegimeRepository;
use crate::detection::regime_classifier::classify;
use crate::hyperliquid::info_client::fetch_candle_snapshot;
use sqlx::PgPool;
use std::time::Duration;
use tracing::{info, warn};

const REGIME_INTERVALS: &[(&str, i64)] = &[
    ("1h", 3_600_000),
    ("4h", 14_400_000),
];
const CANDLES_TO_FETCH: i64 = 100;
const RUN_INTERVAL_SECS: u64 = 15 * 60;

/// Spawn the background regime engine.
///
/// Runs once immediately on startup, then every 15 minutes.
/// For each (coin, "1h") and (coin, "4h"), fetches the last 100 candles
/// from the Hyperliquid API, classifies regime, and upserts with hysteresis.
pub fn spawn_regime_engine(coins: Vec<String>, pool: PgPool) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(RUN_INTERVAL_SECS));
        loop {
            ticker.tick().await;
            run_once(&coins, &pool).await;
        }
    });
}

async fn run_once(coins: &[String], pool: &PgPool) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let repo = RegimeRepository::new(pool.clone());

    for coin in coins {
        for &(interval, interval_ms) in REGIME_INTERVALS {
            let start_ms = now_ms - CANDLES_TO_FETCH * interval_ms;

            let candles = match fetch_candle_snapshot(coin, interval, start_ms, now_ms).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(coin, interval, "Regime: candle fetch failed: {e}");
                    continue;
                }
            };

            let Some((kind, confidence)) = classify(&candles) else {
                warn!(coin, interval, candles = candles.len(), "Regime: not enough candles");
                continue;
            };

            let candle_ts_ms = candles.last().map(|c| c.bucket_ms).unwrap_or(now_ms);

            if let Err(e) = repo
                .upsert_with_hysteresis(coin, interval, kind.as_str(), confidence, candle_ts_ms)
                .await
            {
                warn!(coin, interval, "Regime: upsert failed: {e}");
                continue;
            }

            info!(
                coin,
                interval,
                regime = kind.as_str(),
                confidence = format!("{:.3}", confidence),
                "Regime updated"
            );
        }
    }
}
