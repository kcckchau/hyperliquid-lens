use crate::db::events::EventRepository;
use crate::detection::interval_config::IntervalConfig;
use crate::detection::types::{
    CascadeDirection, EventLifecycle, EventType, MarketEvent, OutcomeDetail, OutcomeKind,
    SweepDirection,
};
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Spawns two async tasks per event:
///
/// Phase 1 (cascade guard) — fires after `cascade_guard_candles * interval_ms`.
///   Checks whether a sweep should be reclassified as a cascade by querying
///   subsequent candle data from the DB.
///
/// Phase 2 (outcome resolution) — fires after `outcome_window_ms`.
///   Determines the empirical outcome (reversal, continuation, exhaustion,
///   absorption, expectation failure, reclaim anomaly) and writes it to DB.
///
/// Using DB-poll rather than a live candle subscription keeps this durable:
/// if the process restarts, outcome_tracker can be re-spawned for all events
/// still in `Confirming` state by querying the DB at startup.
pub fn spawn_outcome_tracker(
    event: MarketEvent,
    cfg: IntervalConfig,
    pool: PgPool,
) {
    let event_id = match event.id {
        Some(id) => id,
        None => {
            warn!("outcome_tracker spawned for event without a DB id — skipping");
            return;
        }
    };

    tokio::spawn(async move {
        let repo = EventRepository::new(pool.clone());

        // ── Phase 1: cascade guard ───────────────────────────────────────────
        // Only relevant for sweep events.
        if event.event_type == EventType::LiquiditySweep {
            let guard_wait_ms = (cfg.cascade_guard_candles as i64) * cfg.interval_ms;
            tokio::time::sleep(Duration::from_millis(guard_wait_ms as u64)).await;

            match check_cascade_reclassification(&event, &cfg, &pool, &repo).await {
                Ok(true) => {
                    info!(
                        event_id,
                        coin = %event.coin,
                        interval = %event.interval,
                        "Sweep reclassified as cascade"
                    );
                    // The reclassification path creates a new cascade event and
                    // transitions this sweep to Reclassified. Outcome tracking ends here.
                    return;
                }
                Ok(false) => {}
                Err(e) => {
                    warn!(event_id, error = %e, "cascade guard check failed");
                }
            }
        }

        // ── Phase 2: outcome resolution ──────────────────────────────────────
        let remaining_wait = cfg.outcome_window_ms
            - (cfg.cascade_guard_candles as i64 * cfg.interval_ms).min(cfg.outcome_window_ms);
        if remaining_wait > 0 {
            tokio::time::sleep(Duration::from_millis(remaining_wait as u64)).await;
        }

        match resolve_outcome(&event, &cfg, &pool, &repo).await {
            Ok(()) => {
                debug!(event_id, "Outcome resolved");
            }
            Err(e) => {
                warn!(event_id, error = %e, "Failed to resolve outcome");
                // Mark as expired so it's not stuck in Confirming indefinitely.
                let _ = repo.update_lifecycle(event_id, EventLifecycle::Expired).await;
            }
        }
    });
}

/// Re-attach outcome trackers for all events stuck in `Confirming` state.
/// Call this at startup after the pool is ready.
pub async fn resume_pending_trackers(pool: PgPool) -> anyhow::Result<()> {
    use crate::detection::interval_config::all_configs;

    let repo = EventRepository::new(pool.clone());
    let pending = repo.fetch_confirming().await?;

    info!(count = pending.len(), "Resuming outcome trackers for in-flight events");

    for event in pending {
        let interval_label = event.interval.clone();
        let Some(cfg) = all_configs().into_iter().find(|c| c.label == interval_label) else {
            continue;
        };
        spawn_outcome_tracker(event, cfg, pool.clone());
    }

    Ok(())
}

// ─── helpers ────────────────────────────────────────────────────────────────

/// Fetch OHLCV candles from the DB for the period after this event.
async fn fetch_followup_candles(
    event: &MarketEvent,
    cfg: &IntervalConfig,
    pool: &PgPool,
) -> anyhow::Result<Vec<crate::db::trades::OhlcvRow>> {
    let repo = crate::db::trades::TradeRepository::new(pool.clone());
    let from = event.event_ts_ms + cfg.interval_ms; // start after the triggering candle
    let to = event.event_ts_ms + cfg.outcome_window_ms;
    repo.ohlcv(&event.coin, cfg.interval_ms, Some(from), Some(to))
        .await
}

/// Phase 1: check whether cascade conditions were met in the guard window.
/// If yes: transition sweep → Reclassified, insert a new cascade event, return true.
async fn check_cascade_reclassification(
    event: &MarketEvent,
    cfg: &IntervalConfig,
    pool: &PgPool,
    repo: &EventRepository,
) -> anyhow::Result<bool> {
    let event_id = event.id.unwrap();

    let guard_to = event.event_ts_ms + (cfg.cascade_guard_candles as i64) * cfg.interval_ms;
    let trade_repo = crate::db::trades::TradeRepository::new(pool.clone());
    let candles = trade_repo
        .ohlcv(
            &event.coin,
            cfg.interval_ms,
            Some(event.event_ts_ms + cfg.interval_ms),
            Some(guard_to),
        )
        .await?;

    if candles.len() < cfg.cascade_sustain_candles {
        return Ok(false);
    }

    let window = &candles[..cfg.cascade_sustain_candles];

    let all_bearish = window.iter().all(|c| c.close < c.open);
    let all_bullish = window.iter().all(|c| c.close > c.open);

    if !all_bearish && !all_bullish {
        return Ok(false);
    }

    // If a sweep was bearish (wicked above), a cascade continuation would also
    // be bearish (price keeps falling after the wick). Same direction = cascade,
    // not reversal.
    let cascade_matches_sweep = match (&event.sweep_direction, all_bearish) {
        (Some(SweepDirection::Bearish), true) => true,
        (Some(SweepDirection::Bullish), false) => true,
        _ => false,
    };

    if !cascade_matches_sweep {
        return Ok(false);
    }

    // Reclassify the sweep.
    repo.update_lifecycle(event_id, EventLifecycle::Reclassified)
        .await?;

    // Create a successor cascade event linked via reclassified_from.
    let first = &candles[0];
    let last = &candles[cfg.cascade_sustain_candles - 1];
    let vol_accel = if first.volume == Decimal::ZERO {
        Decimal::ONE
    } else {
        last.volume / first.volume
    };

    let cascade_dir = if all_bearish {
        CascadeDirection::LongLiq
    } else {
        CascadeDirection::ShortLiq
    };

    let mut successor = MarketEvent::new_cascade(
        event.coin.clone(),
        event.interval.clone(),
        cascade_dir,
        first.open,
        window.iter().map(|_| 0i32).sum::<i32>(), // liq_count not available from OHLCV — set 0
        cfg.cascade_sustain_candles as i32,
        vol_accel,
        last.volume,
        first.bucket_ms,
    );
    successor.reclassified_from = Some(event_id);
    successor.lifecycle = EventLifecycle::Confirming;

    let new_id = repo.insert(&successor).await?;
    info!(
        original_id = event_id,
        new_id,
        "Created successor cascade event from reclassified sweep"
    );

    Ok(true)
}

/// Phase 2: determine empirical outcome and write to DB.
async fn resolve_outcome(
    event: &MarketEvent,
    cfg: &IntervalConfig,
    pool: &PgPool,
    repo: &EventRepository,
) -> anyhow::Result<()> {
    let event_id = event.id.unwrap();
    let candles = fetch_followup_candles(event, cfg, pool).await?;

    if candles.is_empty() {
        repo.update_lifecycle(event_id, EventLifecycle::Expired)
            .await?;
        return Ok(());
    }

    // Reference price: the close of the triggering candle (or start price for cascade).
    let ref_price = match event.event_type {
        EventType::LiquiditySweep => event.close_price.unwrap_or(candles[0].open),
        EventType::LiquidationCascade => event.cascade_start_price.unwrap_or(candles[0].open),
    };

    // Find max upward and downward excursions from ref_price.
    let max_high = candles.iter().map(|c| c.high).fold(ref_price, Decimal::max);
    let min_low = candles.iter().map(|c| c.low).fold(ref_price, Decimal::min);

    let up_pct = if ref_price > Decimal::ZERO {
        (max_high - ref_price) / ref_price
    } else {
        Decimal::ZERO
    };
    let down_pct = if ref_price > Decimal::ZERO {
        (ref_price - min_low) / ref_price
    } else {
        Decimal::ZERO
    };

    let threshold = cfg.reversal_threshold_frac;

    // Absorption detection: high volume + small range relative to volume.
    // A candle is "absorbing" when volume > baseline but (high-low)/close is small.
    let vol_sum: Decimal = candles.iter().map(|c| c.volume).sum();
    let vol_avg = if candles.is_empty() {
        Decimal::ONE
    } else {
        vol_sum / Decimal::from(candles.len())
    };
    let absorption_candle = candles.iter().any(|c| {
        let range_pct = if c.close > Decimal::ZERO {
            (c.high - c.low) / c.close
        } else {
            Decimal::ZERO
        };
        c.volume > vol_avg * cfg.volume_spike_multiplier && range_pct < threshold
    });

    let (outcome, detail) = match event.event_type {
        EventType::LiquiditySweep => {
            classify_sweep_outcome(
                event,
                cfg,
                &candles,
                ref_price,
                up_pct,
                down_pct,
                threshold,
                absorption_candle,
            )
        }
        EventType::LiquidationCascade => {
            classify_cascade_outcome(
                event,
                cfg,
                &candles,
                up_pct,
                down_pct,
                absorption_candle,
            )
        }
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    repo.update_outcome(event_id, outcome, detail, now_ms).await?;
    repo.update_lifecycle(event_id, EventLifecycle::Confirmed).await?;

    Ok(())
}

fn classify_sweep_outcome(
    event: &MarketEvent,
    cfg: &IntervalConfig,
    candles: &[crate::db::trades::OhlcvRow],
    ref_price: Decimal,
    up_pct: Decimal,
    down_pct: Decimal,
    threshold: Decimal,
    absorption_candle: bool,
) -> (OutcomeKind, Option<OutcomeDetail>) {

    // For a bullish sweep: expected followup = price moves up (reversal from low).
    // For a bearish sweep: expected followup = price moves down (reversal from high).
    let (reversal_pct, continuation_pct, _reversal_direction, continuation_direction) =
        match event.sweep_direction {
            Some(SweepDirection::Bullish) => (up_pct, down_pct, "up", "down"),
            Some(SweepDirection::Bearish) => (down_pct, up_pct, "down", "up"),
            None => return (OutcomeKind::Pending, None),
        };

    // Reclaim anomaly: for a bearish sweep, price aggressively reclaims above the level.
    let reclaim_anomaly = match event.sweep_direction {
        Some(SweepDirection::Bearish) => {
            event.level_price.map(|lp| up_pct > threshold && ref_price < lp && candles.iter().any(|c| c.close > lp)).unwrap_or(false)
        }
        Some(SweepDirection::Bullish) => {
            event.level_price.map(|lp| down_pct > threshold && ref_price > lp && candles.iter().any(|c| c.close < lp)).unwrap_or(false)
        }
        None => false,
    };

    if reclaim_anomaly {
        return (
            OutcomeKind::ReclaimAnomaly,
            Some(OutcomeDetail {
                magnitude_pct: Some(reversal_pct),
                duration_ms: None,
                max_extension: None,
                failure_note: Some("price reclaimed sweep level against sweep direction".into()),
            }),
        );
    }

    if absorption_candle && reversal_pct < threshold && continuation_pct < threshold {
        return (
            OutcomeKind::AbsorptionDetected,
            Some(OutcomeDetail {
                magnitude_pct: Some(reversal_pct.max(continuation_pct)),
                duration_ms: None,
                max_extension: None,
                failure_note: Some("high-volume, small-range candle absorbed the move".into()),
            }),
        );
    }

    if reversal_pct >= threshold && reversal_pct > continuation_pct {
        return (
            OutcomeKind::ReversalFollowed,
            Some(OutcomeDetail {
                magnitude_pct: Some(reversal_pct),
                duration_ms: None,
                max_extension: None,
                failure_note: None,
            }),
        );
    }

    if continuation_pct >= threshold && continuation_pct > reversal_pct {
        return (
            OutcomeKind::ContinuationFollowed,
            Some(OutcomeDetail {
                magnitude_pct: Some(continuation_pct),
                duration_ms: None,
                max_extension: None,
                failure_note: Some(format!(
                    "sweep direction continued ({}); reversal expectation failed",
                    continuation_direction
                )),
            }),
        );
    }

    if reversal_pct < threshold && continuation_pct < threshold {
        return (
            OutcomeKind::ExhaustionDetected,
            Some(OutcomeDetail {
                magnitude_pct: Some(reversal_pct.max(continuation_pct)),
                duration_ms: None,
                max_extension: None,
                failure_note: Some("price stalled; no directional follow-through".into()),
            }),
        );
    }

    (
        OutcomeKind::ExpectationFailed,
        Some(OutcomeDetail {
            magnitude_pct: Some(reversal_pct.max(continuation_pct)),
            duration_ms: None,
            max_extension: None,
            failure_note: Some("ambiguous outcome; expected reversal did not clearly materialise".into()),
        }),
    )
}

fn classify_cascade_outcome(
    event: &MarketEvent,
    cfg: &IntervalConfig,
    _candles: &[crate::db::trades::OhlcvRow],
    up_pct: Decimal,
    down_pct: Decimal,
    absorption_candle: bool,
) -> (OutcomeKind, Option<OutcomeDetail>) {
    let threshold = cfg.reversal_threshold_frac;
    // For a cascade there is no single "expected" outcome — we record what happened.

    // Continuation: price kept moving in cascade direction.
    let (continuation_pct, reversal_pct) = match event.cascade_direction {
        Some(CascadeDirection::LongLiq) => (down_pct, up_pct),  // longs liq → price falling
        Some(CascadeDirection::ShortLiq) => (up_pct, down_pct), // shorts liq → price rising
        None => return (OutcomeKind::Pending, None),
    };

    if absorption_candle && continuation_pct < threshold && reversal_pct < threshold {
        return (
            OutcomeKind::AbsorptionDetected,
            Some(OutcomeDetail {
                magnitude_pct: Some(continuation_pct.max(reversal_pct)),
                duration_ms: None,
                max_extension: None,
                failure_note: Some("large opposing volume absorbed the cascade".into()),
            }),
        );
    }

    // Exhaustion: volume decelerates and price stalls.
    let vol_decel = event
        .volume_acceleration
        .map(|va| va < Decimal::ONE)
        .unwrap_or(false);
    if vol_decel && continuation_pct < threshold && reversal_pct < threshold {
        return (
            OutcomeKind::ExhaustionDetected,
            Some(OutcomeDetail {
                magnitude_pct: Some(continuation_pct.max(reversal_pct)),
                duration_ms: None,
                max_extension: None,
                failure_note: Some("cascade volume decelerated; price stalled".into()),
            }),
        );
    }

    if reversal_pct >= threshold && reversal_pct > continuation_pct {
        return (
            OutcomeKind::ReversalFollowed,
            Some(OutcomeDetail {
                magnitude_pct: Some(reversal_pct),
                duration_ms: None,
                max_extension: None,
                failure_note: Some("violent reversal after cascade (possible short squeeze / long squeeze)".into()),
            }),
        );
    }

    if continuation_pct >= threshold {
        return (
            OutcomeKind::ContinuationFollowed,
            Some(OutcomeDetail {
                magnitude_pct: Some(continuation_pct),
                duration_ms: None,
                max_extension: None,
                failure_note: None,
            }),
        );
    }

    (
        OutcomeKind::ExpectationFailed,
        Some(OutcomeDetail {
            magnitude_pct: Some(continuation_pct.max(reversal_pct)),
            duration_ms: None,
            max_extension: None,
            failure_note: Some("cascade ended without clear continuation or reversal".into()),
        }),
    )
}
