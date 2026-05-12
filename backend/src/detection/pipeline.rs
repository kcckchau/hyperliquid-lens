use crate::db::events::EventRepository;
use crate::detection::candle_builder::CandleBuilder;
use crate::detection::cascade_detector::CascadeDetector;
use crate::detection::interval_config::{all_configs, IntervalConfig};
use crate::detection::outcome_tracker::spawn_outcome_tracker;
use crate::detection::sweep_detector::SweepDetector;
use crate::detection::swing_detector::SwingDetector;
use crate::detection::types::{EventLifecycle, HtfLevel, MarketEvent};
use crate::ingester::parser::Trade;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Spawn one detection pipeline per (coin, interval).
///
/// Each pipeline:
///   1. Subscribes to the trade broadcast channel.
///   2. Builds OHLCV candles from trades.
///   3. On each completed candle, runs swing, sweep, and cascade detectors.
///   4. Persists detected events to DB (lifecycle = Confirming).
///   5. Broadcasts events to connected WebSocket clients.
///   6. Spawns an outcome_tracker per event.
pub fn spawn_all_pipelines(
    coins: &[String],
    pool: PgPool,
    trade_tx: Arc<broadcast::Sender<Trade>>,
    event_tx: Arc<broadcast::Sender<MarketEvent>>,
) {
    for coin in coins {
        for cfg in all_configs() {
            spawn_detection_pipeline(
                coin.clone(),
                cfg,
                pool.clone(),
                trade_tx.subscribe(),
                (*event_tx).clone(),
            );
        }
    }
}

fn spawn_detection_pipeline(
    coin: String,
    cfg: IntervalConfig,
    pool: PgPool,
    mut trade_rx: broadcast::Receiver<Trade>,
    event_tx: broadcast::Sender<MarketEvent>,
) {
    tokio::spawn(async move {
        info!(coin = %coin, interval = %cfg.label, "Detection pipeline started");

        let repo = EventRepository::new(pool.clone());
        let mut candle_builder = CandleBuilder::new(cfg.interval_ms);
        let mut swing_detector = SwingDetector::new(&cfg);
        let mut sweep_detector = SweepDetector::new(&cfg);
        let mut cascade_detector = CascadeDetector::new(&cfg);

        loop {
            match trade_rx.recv().await {
                Ok(trade) if trade.coin == coin => {
                    if let Some(candle) = candle_builder.push(&trade) {
                        // ── Process completed candle ─────────────────────────

                        // 1. Update swing levels.
                        swing_detector.push(candle.clone());

                        // 2. Detect sweeps against current live levels.
                        let raw_sweeps = sweep_detector.push(
                            &candle,
                            &swing_detector.live_highs.clone(),
                            &swing_detector.live_lows.clone(),
                        );

                        for raw in raw_sweeps {
                            let htf = htf_confluence(
                                raw.level_price,
                                &cfg,
                                &pool,
                                &coin,
                                candle.bucket_ms,
                            )
                            .await;

                            let mut event = MarketEvent::new_sweep(
                                coin.clone(),
                                cfg.label.to_string(),
                                raw.direction,
                                raw.level_price,
                                raw.sweep_extreme,
                                raw.wick_pct,
                                raw.close_price,
                                raw.candle_volume,
                                raw.candle_ts_ms,
                            );
                            event.htf_confluence = htf;
                            event.lifecycle = EventLifecycle::Confirming;

                            match repo.insert(&event).await {
                                Ok(id) => {
                                    event.id = Some(id);
                                    debug!(
                                        id,
                                        coin = %coin,
                                        interval = %cfg.label,
                                        "Sweep event persisted"
                                    );
                                    let _ = event_tx.send(event.clone());
                                    spawn_outcome_tracker(event, cfg.clone(), pool.clone());
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to persist sweep event");
                                }
                            }
                        }

                        // 3. Detect cascades.
                        if let Some(raw) = cascade_detector.push(candle.clone()) {
                            let mut event = MarketEvent::new_cascade(
                                coin.clone(),
                                cfg.label.to_string(),
                                raw.direction,
                                raw.start_price,
                                raw.liq_count_total as i32,
                                raw.candles_sustained as i32,
                                raw.volume_acceleration,
                                candle.volume,
                                raw.start_ts_ms,
                            );
                            event.lifecycle = EventLifecycle::Confirming;

                            match repo.insert(&event).await {
                                Ok(id) => {
                                    event.id = Some(id);
                                    debug!(
                                        id,
                                        coin = %coin,
                                        interval = %cfg.label,
                                        "Cascade event persisted"
                                    );
                                    let _ = event_tx.send(event.clone());
                                    spawn_outcome_tracker(event, cfg.clone(), pool.clone());
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to persist cascade event");
                                }
                            }
                        }
                    }
                }
                Ok(_) => {} // different coin — ignore
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!(
                        coin = %coin,
                        interval = %cfg.label,
                        skipped = n,
                        "Detection pipeline lagged on trade channel"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!(coin = %coin, interval = %cfg.label, "Trade channel closed, pipeline stopping");
                    break;
                }
            }
        }
    });
}

/// Look up swing levels on higher timeframes that are within `proximity_frac`
/// of `price`. Used to annotate events with HTF confluence.
async fn htf_confluence(
    price: rust_decimal::Decimal,
    current_cfg: &IntervalConfig,
    pool: &PgPool,
    coin: &str,
    candle_ts_ms: i64,
) -> Vec<HtfLevel> {
    use crate::detection::interval_config::all_configs;

    // Only check intervals higher than the current one.
    let higher_configs: Vec<IntervalConfig> = all_configs()
        .into_iter()
        .filter(|c| c.interval_ms > current_cfg.interval_ms)
        .collect();

    if higher_configs.is_empty() {
        return vec![];
    }

    let proximity_frac = current_cfg.wick_min_frac * rust_decimal::Decimal::new(5, 0);
    let mut result = Vec::new();

    for htf_cfg in higher_configs {
        let repo = crate::db::trades::TradeRepository::new(pool.clone());
        // Query last `swing_lookback * 2` HTF candles to find swing levels.
        let lookback_ms = (htf_cfg.swing_lookback * 2 + 2) as i64 * htf_cfg.interval_ms;
        let from = candle_ts_ms - lookback_ms;

        let Ok(candles) = repo
            .ohlcv(coin, htf_cfg.interval_ms, Some(from), Some(candle_ts_ms))
            .await
        else {
            continue;
        };

        // Check if any candle high or low is within proximity_frac of `price`.
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
