use crate::detection::candle_builder::Candle;
use crate::detection::interval_config::IntervalConfig;
use crate::detection::types::CascadeDirection;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Raw cascade detection result.
/// Uses `CascadeDirection` from `types` so no conversion is needed in the pipeline.
#[derive(Debug, Clone)]
pub struct RawCascade {
    pub direction: CascadeDirection,
    pub start_price: Decimal,
    pub end_price: Decimal,
    pub liq_count_total: u32,
    pub candles_sustained: u32,
    /// Volume of the final cascade candle / volume of the first. Values > 1
    /// indicate acceleration; < 1 indicate deceleration (may signal exhaustion).
    pub volume_acceleration: Decimal,
    pub start_ts_ms: i64,
    pub end_ts_ms: i64,
}

/// Detects liquidation cascades: sustained directional price movement
/// accompanied by elevated liquidation activity and above-average volume.
///
/// A cascade fires once when the sustain window is first satisfied.
/// To avoid re-firing on every subsequent candle of the same cascade, the
/// detector tracks whether a cascade has already been reported and requires
/// a break in direction before it can fire again.
pub struct CascadeDetector {
    sustain_candles: usize,
    volume_spike_multiplier: Decimal,
    volume_baseline_candles: usize,
    window: VecDeque<Candle>,
    vol_history: VecDeque<Decimal>,
    /// Direction of the last reported cascade, to suppress duplicate fires.
    last_reported: Option<CascadeDirection>,
}

impl CascadeDetector {
    pub fn new(cfg: &IntervalConfig) -> Self {
        Self {
            sustain_candles: cfg.cascade_sustain_candles,
            volume_spike_multiplier: cfg.volume_spike_multiplier,
            volume_baseline_candles: cfg.volume_baseline_candles,
            window: VecDeque::with_capacity(cfg.cascade_sustain_candles + 1),
            vol_history: VecDeque::with_capacity(cfg.volume_baseline_candles + 1),
            last_reported: None,
        }
    }

    /// Feed a completed candle. Returns `Some(RawCascade)` when cascade
    /// conditions are first met.
    pub fn push(&mut self, candle: Candle) -> Option<RawCascade> {
        // Update volume baseline.
        self.vol_history.push_back(candle.volume);
        if self.vol_history.len() > self.volume_baseline_candles {
            self.vol_history.pop_front();
        }

        self.window.push_back(candle);
        if self.window.len() > self.sustain_candles {
            self.window.pop_front();
        }

        if self.window.len() < self.sustain_candles {
            return None;
        }

        // ── Direction check ───────────────────────────────────────────────────
        // All candles in the window must close in the same direction as their open.
        let all_bearish = self.window.iter().all(|c| c.close < c.open);
        let all_bullish = self.window.iter().all(|c| c.close > c.open);

        if !all_bearish && !all_bullish {
            // Window is mixed — reset suppression so the next cascade can fire.
            self.last_reported = None;
            return None;
        }

        let direction = if all_bearish {
            CascadeDirection::LongLiq   // price falling → long positions liquidated
        } else {
            CascadeDirection::ShortLiq  // price rising → short positions liquidated
        };

        // ── Suppress duplicate fires in same direction ────────────────────────
        if self.last_reported.as_ref() == Some(&direction) {
            return None;
        }

        // ── Liquidation requirement ───────────────────────────────────────────
        let total_liq: u32 = self.window.iter().map(|c| c.liq_count).sum();
        if total_liq == 0 {
            return None;
        }

        // ── Volume elevation check ────────────────────────────────────────────
        let vol_baseline: Decimal = if self.vol_history.is_empty() {
            return None;
        } else {
            let sum: Decimal = self.vol_history.iter().copied().sum();
            sum / Decimal::from(self.vol_history.len())
        };

        let avg_window_vol: Decimal = {
            let sum: Decimal = self.window.iter().map(|c| c.volume).sum();
            sum / Decimal::from(self.window.len())
        };

        if avg_window_vol < vol_baseline * self.volume_spike_multiplier {
            return None;
        }

        // ── All conditions met — emit cascade ─────────────────────────────────
        self.last_reported = Some(direction.clone());

        let first = self.window.front().unwrap();
        let last = self.window.back().unwrap();

        let volume_acceleration = if first.volume == Decimal::ZERO {
            Decimal::ONE
        } else {
            last.volume / first.volume
        };

        Some(RawCascade {
            direction,
            start_price: first.open,
            end_price: last.close,
            liq_count_total: total_liq,
            candles_sustained: self.window.len() as u32,
            volume_acceleration,
            start_ts_ms: first.bucket_ms,
            end_ts_ms: last.bucket_ms,
        })
    }
}
