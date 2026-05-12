use crate::detection::candle_builder::Candle;
use crate::detection::interval_config::IntervalConfig;
use rust_decimal::Decimal;
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwingKind {
    High,
    Low,
}

/// A confirmed swing level that is still within its expiry window.
#[derive(Debug, Clone)]
pub struct SwingLevel {
    pub price: Decimal,
    /// Candle open time when this level was originally formed (the pivot candle).
    pub formed_ts_ms: i64,
    pub kind: SwingKind,
    /// Candles on each side that qualified the pivot. Equal to swing_lookback.
    pub strength: usize,
    /// How many candles have passed since this level was detected.
    /// Starts at `lookback` because detection is inherently delayed by the
    /// right-side lookback window.
    pub age_candles: usize,
}

/// Maintains a rolling window of candles, identifies pivot swing highs/lows,
/// and tracks a live registry of unexpired levels.
///
/// Swing identification is intentionally lagged by `lookback` candles: a pivot
/// at position N can only be confirmed once positions N+1..N+lookback have been
/// observed. This lag is real and expected — the detector produces levels that
/// were "recently" significant, not the current candle.
pub struct SwingDetector {
    lookback: usize,
    level_expiry_candles: usize,
    /// Ring buffer of the last (2 * lookback + 1) candles.
    history: VecDeque<Candle>,
    pub live_highs: Vec<SwingLevel>,
    pub live_lows: Vec<SwingLevel>,
}

impl SwingDetector {
    pub fn new(cfg: &IntervalConfig) -> Self {
        let capacity = cfg.swing_lookback * 2 + 2;
        Self {
            lookback: cfg.swing_lookback,
            level_expiry_candles: cfg.level_expiry_candles,
            history: VecDeque::with_capacity(capacity),
            live_highs: Vec::new(),
            live_lows: Vec::new(),
        }
    }

    /// Feed a completed candle. Returns any newly detected swing levels.
    ///
    /// Detection evaluates the pivot candle at position `lookback` from the
    /// back of the history window — i.e. the candle that now has `lookback`
    /// neighbours on both sides.
    pub fn push(&mut self, candle: Candle) -> Vec<SwingLevel> {
        self.history.push_back(candle);

        // Age all live levels and evict expired ones.
        for lvl in self.live_highs.iter_mut().chain(self.live_lows.iter_mut()) {
            lvl.age_candles += 1;
        }
        self.live_highs.retain(|l| l.age_candles < self.level_expiry_candles);
        self.live_lows.retain(|l| l.age_candles < self.level_expiry_candles);

        // Trim history to exactly the window we need.
        let window = self.lookback * 2 + 1;
        while self.history.len() > window {
            self.history.pop_front();
        }

        if self.history.len() < window {
            return vec![];
        }

        let mid = self.lookback;
        let pivot_high = self.history[mid].high;
        let pivot_low = self.history[mid].low;
        let pivot_ts = self.history[mid].bucket_ms;

        // Strict inequality: adjacent candles must not match the pivot.
        let is_swing_high = self.history.iter().enumerate().all(|(i, c)| {
            i == mid || c.high < pivot_high
        });
        let is_swing_low = self.history.iter().enumerate().all(|(i, c)| {
            i == mid || c.low > pivot_low
        });

        let mut new_levels = Vec::new();

        if is_swing_high {
            // Avoid adding a duplicate level at the same price formed close in time.
            let is_dup = self.live_highs.iter().any(|l| l.price == pivot_high);
            if !is_dup {
                let level = SwingLevel {
                    price: pivot_high,
                    formed_ts_ms: pivot_ts,
                    kind: SwingKind::High,
                    strength: self.lookback,
                    age_candles: self.lookback, // already `lookback` candles old at detection time
                };
                self.live_highs.push(level.clone());
                new_levels.push(level);
            }
        }

        if is_swing_low {
            let is_dup = self.live_lows.iter().any(|l| l.price == pivot_low);
            if !is_dup {
                let level = SwingLevel {
                    price: pivot_low,
                    formed_ts_ms: pivot_ts,
                    kind: SwingKind::Low,
                    strength: self.lookback,
                    age_candles: self.lookback,
                };
                self.live_lows.push(level.clone());
                new_levels.push(level);
            }
        }

        new_levels
    }
}
