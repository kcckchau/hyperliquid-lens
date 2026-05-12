use crate::detection::candle_builder::Candle;
use crate::detection::interval_config::IntervalConfig;
use crate::detection::swing_detector::SwingLevel;
use crate::detection::types::SweepDirection;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Raw sweep detection result — one per swing level pierced in this candle.
/// Multiple sweeps can fire on a single candle (e.g. if both a high and a low
/// are pierced, though that is rare).
#[derive(Debug, Clone)]
pub struct RawSweep {
    pub direction: SweepDirection,
    /// The swing level that was pierced.
    pub level_price: Decimal,
    /// Wick tip: the furthest point price reached past the level.
    pub sweep_extreme: Decimal,
    /// Fractional distance the wick extended beyond the level.
    pub wick_pct: Decimal,
    pub close_price: Decimal,
    pub candle_volume: Decimal,
    pub liq_count: u32,
    pub candle_ts_ms: i64,
    /// Whether candle volume exceeded the rolling baseline spike threshold.
    pub volume_spike: bool,
    /// Clone of the swing level that was swept (for HTF confluence checks later).
    pub swept_level: SwingLevel,
}

pub struct SweepDetector {
    wick_min_frac: Decimal,
    volume_spike_multiplier: Decimal,
    /// Rolling window of recent candle volumes to compute the baseline average.
    vol_history: VecDeque<Decimal>,
    volume_baseline_candles: usize,
}

impl SweepDetector {
    pub fn new(cfg: &IntervalConfig) -> Self {
        Self {
            wick_min_frac: cfg.wick_min_frac,
            volume_spike_multiplier: cfg.volume_spike_multiplier,
            vol_history: VecDeque::with_capacity(cfg.volume_baseline_candles + 1),
            volume_baseline_candles: cfg.volume_baseline_candles,
        }
    }

    /// Evaluate a closed candle against the live swing level registry.
    ///
    /// A sweep is detected when:
    ///   1. The candle wick extends past a live swing level by >= wick_min_frac.
    ///   2. The candle CLOSE is back inside the level (wick, not a breakout).
    ///
    /// This must be called AFTER the volume history is updated for this candle
    /// (the candle's own volume is included in the baseline).
    pub fn push(
        &mut self,
        candle: &Candle,
        live_highs: &[SwingLevel],
        live_lows: &[SwingLevel],
    ) -> Vec<RawSweep> {
        // Update volume baseline — include current candle volume.
        self.vol_history.push_back(candle.volume);
        if self.vol_history.len() > self.volume_baseline_candles {
            self.vol_history.pop_front();
        }

        let vol_avg: Decimal = if self.vol_history.is_empty() {
            Decimal::ONE
        } else {
            let sum: Decimal = self.vol_history.iter().copied().sum();
            sum / Decimal::from(self.vol_history.len())
        };

        let volume_spike = vol_avg > Decimal::ZERO
            && candle.volume >= vol_avg * self.volume_spike_multiplier;

        let mut sweeps = Vec::new();

        // ── Bearish sweep: wick above a swing high, close back below ──────────
        for level in live_highs {
            if candle.high <= level.price {
                continue; // didn't pierce
            }
            if candle.close >= level.price {
                continue; // close is still above — potential breakout, not a sweep
            }
            let wick_pct = (candle.high - level.price) / level.price;
            if wick_pct < self.wick_min_frac {
                continue; // wick too small to be meaningful at this interval
            }
            sweeps.push(RawSweep {
                direction: SweepDirection::Bearish,
                level_price: level.price,
                sweep_extreme: candle.high,
                wick_pct,
                close_price: candle.close,
                candle_volume: candle.volume,
                liq_count: candle.liq_count,
                candle_ts_ms: candle.bucket_ms,
                volume_spike,
                swept_level: level.clone(),
            });
        }

        // ── Bullish sweep: wick below a swing low, close back above ───────────
        for level in live_lows {
            if candle.low >= level.price {
                continue;
            }
            if candle.close <= level.price {
                continue; // close is still below — breakdown, not a sweep
            }
            let wick_pct = (level.price - candle.low) / level.price;
            if wick_pct < self.wick_min_frac {
                continue;
            }
            sweeps.push(RawSweep {
                direction: SweepDirection::Bullish,
                level_price: level.price,
                sweep_extreme: candle.low,
                wick_pct,
                close_price: candle.close,
                candle_volume: candle.volume,
                liq_count: candle.liq_count,
                candle_ts_ms: candle.bucket_ms,
                volume_spike,
                swept_level: level.clone(),
            });
        }

        sweeps
    }
}
