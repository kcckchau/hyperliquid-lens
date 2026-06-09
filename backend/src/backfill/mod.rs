use crate::db::events::EventRepository;
use crate::db::trades::TradeRepository;
use crate::detection::candle_builder::Candle;
use crate::detection::cascade_detector::CascadeDetector;
use crate::detection::interval_config::{all_configs, IntervalConfig};
use crate::detection::sweep_detector::SweepDetector;
use crate::detection::swing_detector::SwingDetector;
use crate::detection::types::{EventLifecycle, EventSource, HtfLevel, MarketEvent};
use crate::hyperliquid::info_client::fetch_candle_snapshot;
use anyhow::Result;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct BackfillSummary {
    pub coin: String,
    /// UTC date that was backfilled ("YYYY-MM-DD")
    pub date: String,
    pub events_detected: usize,
    /// Per-interval breakdown
    pub by_interval: Vec<IntervalSummary>,
    /// Informational notes (e.g. cascade limitations)
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct IntervalSummary {
    pub interval: String,
    pub sweeps: usize,
    pub cascades: usize,
}

/// Run detection over a full UTC day for one coin.
///
/// For each supported interval:
///   1. Fetch OHLCV candles from the Hyperliquid API, including warmup candles
///      before the day to prime the swing detector's rolling window.
///   2. Replay them through SwingDetector → SweepDetector → CascadeDetector.
///   3. Persist any detected events to DB with `source = backfill`.
///
/// Events are marked `lifecycle = confirmed` immediately — no async outcome
/// tracking is spawned because raw trades are not available for historical
/// periods. Cascades will not fire because OHLCV snapshots carry no
/// per-trade liquidation counts.
///
/// Re-running for the same coin + date is idempotent: existing backfill events
/// for the day are deleted before the new pass begins.
pub async fn run_backfill(
    coin: &str,
    date: NaiveDate,
    pool: PgPool,
    event_tx: broadcast::Sender<MarketEvent>,
) -> Result<BackfillSummary> {
    // UTC day boundaries (milliseconds).
    let naive_dt = date
        .and_hms_opt(0, 0, 0)
        .expect("midnight is always valid");
    let start_ms = naive_dt.and_utc().timestamp_millis();
    let end_ms = start_ms + 86_400_000i64; // exclusive

    info!(coin = %coin, date = %date, start_ms, end_ms, "Starting backfill");

    let repo = EventRepository::new(pool.clone());

    // Clear any previous backfill events for this day so re-runs are clean.
    let deleted = repo
        .delete_backfill_range(coin, start_ms, end_ms)
        .await
        .unwrap_or(0);
    if deleted > 0 {
        info!(coin = %coin, date = %date, deleted, "Cleared previous backfill events");
    }

    let mut by_interval = Vec::new();
    let mut total_events = 0usize;
    let mut notes = vec![
        "Cascades require per-trade liquidation counts which are not available \
         in OHLCV snapshots. Only liquidity sweeps are detected during backfill."
            .to_string(),
    ];

    for cfg in all_configs() {
        match run_interval_backfill(
            coin,
            start_ms,
            end_ms,
            &cfg,
            &pool,
            &repo,
            &event_tx,
        )
        .await
        {
            Ok(summary) => {
                info!(
                    coin = %coin,
                    interval = cfg.label,
                    sweeps = summary.sweeps,
                    "Interval backfill complete"
                );
                total_events += summary.sweeps + summary.cascades;
                by_interval.push(summary);
            }
            Err(e) => {
                warn!(coin = %coin, interval = cfg.label, error = %e, "Interval backfill failed");
                notes.push(format!("Interval {} failed: {}", cfg.label, e));
            }
        }
    }

    Ok(BackfillSummary {
        coin: coin.to_string(),
        date: date.to_string(),
        events_detected: total_events,
        by_interval,
        notes,
    })
}

// ---------------------------------------------------------------------------
// Per-interval runner
// ---------------------------------------------------------------------------

async fn run_interval_backfill(
    coin: &str,
    start_ms: i64,
    end_ms: i64,
    cfg: &IntervalConfig,
    pool: &PgPool,
    repo: &EventRepository,
    event_tx: &broadcast::Sender<MarketEvent>,
) -> Result<IntervalSummary> {
    // Fetch extra candles before the day to warm up the swing detector's
    // rolling window. The swing detector needs `2 * lookback + 1` candles
    // before it can confirm any level, so we fetch that many extra.
    let warmup_candles = (cfg.swing_lookback * 2 + 2) as i64;
    let fetch_start = start_ms - warmup_candles * cfg.interval_ms;

    let ohlcv =
        fetch_candle_snapshot(coin, cfg.label, fetch_start, end_ms).await?;

    if ohlcv.is_empty() {
        return Ok(IntervalSummary {
            interval: cfg.label.to_string(),
            sweeps: 0,
            cascades: 0,
        });
    }

    // Convert OhlcvRow → Candle.
    // liq_count is zero because OHLCV snapshots do not carry per-trade flags.
    let candles: Vec<Candle> = ohlcv
        .iter()
        .map(|r| Candle {
            bucket_ms: r.bucket_ms,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
            trade_count: 0,
            liq_count: 0,
        })
        .collect();

    let mut swing = SwingDetector::new(cfg);
    let mut sweep_det = SweepDetector::new(cfg);
    let mut cascade_det = CascadeDetector::new(cfg);

    let mut sweeps = 0usize;
    let mut cascades = 0usize;

    for candle in &candles {
        // Always feed all detectors so their internal windows warm up correctly,
        // but only persist events for candles within the target day.
        swing.push(candle.clone());
        let raw_sweeps = sweep_det.push(
            candle,
            &swing.live_highs.clone(),
            &swing.live_lows.clone(),
        );
        let raw_cascade = cascade_det.push(candle.clone());

        if candle.bucket_ms < start_ms {
            continue; // warmup phase — detectors run but results are discarded
        }

        // ── Sweep events ──────────────────────────────────────────────────
        for raw in raw_sweeps {
            let htf = htf_confluence(raw.level_price, cfg, pool, coin, candle.bucket_ms).await;

            let mut event = MarketEvent::new_sweep(
                coin.to_string(),
                cfg.label.to_string(),
                raw.direction,
                raw.level_price,
                raw.sweep_extreme,
                raw.wick_pct,
                raw.close_price,
                raw.candle_volume,
                candle.bucket_ms,
            );
            event.htf_confluence = htf;
            event.source = EventSource::Backfill;
            event.lifecycle = EventLifecycle::Confirmed;

            match repo.insert(&event).await {
                Ok(id) => {
                    event.id = Some(id);
                    sweeps += 1;
                    let _ = event_tx.send(event);
                }
                Err(e) => {
                    warn!(
                        coin = %coin,
                        interval = cfg.label,
                        error = %e,
                        "Failed to persist backfill sweep"
                    );
                }
            }
        }

        // ── Cascade events ────────────────────────────────────────────────
        // In practice these never fire for OHLCV backfill because liq_count
        // is always 0, but the code path is correct for completeness.
        if let Some(raw) = raw_cascade {
            let mut event = MarketEvent::new_cascade(
                coin.to_string(),
                cfg.label.to_string(),
                raw.direction,
                raw.start_price,
                raw.liq_count_total as i32,
                raw.candles_sustained as i32,
                raw.volume_acceleration,
                candle.volume,
                raw.start_ts_ms,
            );
            event.source = EventSource::Backfill;
            event.lifecycle = EventLifecycle::Confirmed;

            match repo.insert(&event).await {
                Ok(id) => {
                    event.id = Some(id);
                    cascades += 1;
                    let _ = event_tx.send(event);
                }
                Err(e) => {
                    warn!(
                        coin = %coin,
                        interval = cfg.label,
                        error = %e,
                        "Failed to persist backfill cascade"
                    );
                }
            }
        }
    }

    Ok(IntervalSummary {
        interval: cfg.label.to_string(),
        sweeps,
        cascades,
    })
}

// ---------------------------------------------------------------------------
// HTF confluence — mirrors the logic in detection/pipeline.rs, querying the
// DB for higher-timeframe swing levels near `price`.
// ---------------------------------------------------------------------------

async fn htf_confluence(
    price: Decimal,
    current_cfg: &IntervalConfig,
    pool: &PgPool,
    coin: &str,
    candle_ts_ms: i64,
) -> Vec<HtfLevel> {
    let higher_configs: Vec<IntervalConfig> = all_configs()
        .into_iter()
        .filter(|c| c.interval_ms > current_cfg.interval_ms)
        .collect();

    if higher_configs.is_empty() {
        return vec![];
    }

    let proximity_frac = current_cfg.wick_min_frac * Decimal::new(5, 0);
    let mut result = Vec::new();

    for htf_cfg in higher_configs {
        let repo = TradeRepository::new(pool.clone());
        let lookback_ms = (htf_cfg.swing_lookback * 2 + 2) as i64 * htf_cfg.interval_ms;
        let from = candle_ts_ms - lookback_ms;

        let Ok(candles) = repo
            .ohlcv(coin, htf_cfg.interval_ms, Some(from), Some(candle_ts_ms))
            .await
        else {
            continue;
        };

        for (i, c) in candles.iter().enumerate() {
            let age = candles.len() - i;

            let high_dist = if c.high > price {
                (c.high - price) / price
            } else {
                (price - c.high) / price
            };
            let low_dist = if c.low > price {
                (c.low - price) / price
            } else {
                (price - c.low) / price
            };

            if high_dist <= proximity_frac {
                result.push(HtfLevel {
                    interval: htf_cfg.label.to_string(),
                    level_price: c.high,
                    age_candles: age,
                });
            } else if low_dist <= proximity_frac {
                result.push(HtfLevel {
                    interval: htf_cfg.label.to_string(),
                    level_price: c.low,
                    age_candles: age,
                });
            }
        }
    }

    result
}
