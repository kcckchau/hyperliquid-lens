use rust_decimal::Decimal;

/// Per-interval detection parameters.
///
/// All thresholds are interval-aware: what constitutes a meaningful wick on 1m
/// is noise on 4h. The outcome_window_ms is time-based (not candle-count) so
/// that outcome resolution is comparable across intervals.
#[derive(Debug, Clone)]
pub struct IntervalConfig {
    /// Label used in API queries and DB rows.
    pub label: &'static str,
    pub interval_ms: i64,

    /// Number of candles that must be strictly lower/higher on both sides of a
    /// pivot candle for it to qualify as a swing high/low.
    pub swing_lookback: usize,

    /// Minimum fractional wick extension past a swing level to classify as sweep.
    /// e.g. 0.0003 = 0.03%.
    pub wick_min_frac: Decimal,

    /// Rolling window size (in candles) used to compute the volume baseline.
    pub volume_baseline_candles: usize,

    /// Volume must be >= baseline * this multiplier to flag as a spike.
    pub volume_spike_multiplier: Decimal,

    /// Time window (ms) the outcome_tracker observes after an event fires.
    /// Sleeping then querying DB candles — durable across restarts.
    pub outcome_window_ms: i64,

    /// How many candles after a sweep fires to check for cascade reclassification
    /// before the full outcome_window expires (first phase of two-phase tracking).
    pub cascade_guard_candles: usize,

    /// Consecutive candles in the same direction with elevated liq volume needed
    /// to confirm a liquidation cascade.
    pub cascade_sustain_candles: usize,

    /// Minimum fractional move after event close to count as reversal or continuation.
    pub reversal_threshold_frac: Decimal,

    /// A swing level expires after this many candles without being swept.
    pub level_expiry_candles: usize,
}

impl IntervalConfig {
    pub fn interval_label_to_ms(label: &str) -> Option<i64> {
        all_configs()
            .iter()
            .find(|c| c.label == label)
            .map(|c| c.interval_ms)
    }
}

/// Returns the full parameter table for all supported intervals.
pub fn all_configs() -> Vec<IntervalConfig> {
    vec![
        IntervalConfig {
            label: "1m",
            interval_ms: 60_000,
            swing_lookback: 10,
            wick_min_frac: Decimal::new(3, 4),       // 0.0003
            volume_baseline_candles: 20,
            volume_spike_multiplier: Decimal::new(15, 1), // 1.5
            outcome_window_ms: 15 * 60_000,
            cascade_guard_candles: 3,
            cascade_sustain_candles: 3,
            reversal_threshold_frac: Decimal::new(1, 3), // 0.001
            level_expiry_candles: 200,
        },
        IntervalConfig {
            label: "5m",
            interval_ms: 300_000,
            swing_lookback: 10,
            wick_min_frac: Decimal::new(5, 4),       // 0.0005
            volume_baseline_candles: 20,
            volume_spike_multiplier: Decimal::new(15, 1),
            outcome_window_ms: 60 * 60_000,
            cascade_guard_candles: 3,
            cascade_sustain_candles: 3,
            reversal_threshold_frac: Decimal::new(2, 3), // 0.002
            level_expiry_candles: 100,
        },
        IntervalConfig {
            label: "15m",
            interval_ms: 900_000,
            swing_lookback: 8,
            wick_min_frac: Decimal::new(8, 4),       // 0.0008
            volume_baseline_candles: 20,
            volume_spike_multiplier: Decimal::new(14, 1), // 1.4
            outcome_window_ms: 4 * 3_600_000,
            cascade_guard_candles: 3,
            cascade_sustain_candles: 2,
            reversal_threshold_frac: Decimal::new(3, 3), // 0.003
            level_expiry_candles: 100,
        },
        IntervalConfig {
            label: "1h",
            interval_ms: 3_600_000,
            swing_lookback: 6,
            wick_min_frac: Decimal::new(12, 4),      // 0.0012
            volume_baseline_candles: 14,
            volume_spike_multiplier: Decimal::new(14, 1),
            outcome_window_ms: 24 * 3_600_000,
            cascade_guard_candles: 2,
            cascade_sustain_candles: 2,
            reversal_threshold_frac: Decimal::new(5, 3), // 0.005
            level_expiry_candles: 50,
        },
        IntervalConfig {
            label: "4h",
            interval_ms: 14_400_000,
            swing_lookback: 5,
            wick_min_frac: Decimal::new(2, 3),       // 0.002
            volume_baseline_candles: 14,
            volume_spike_multiplier: Decimal::new(13, 1), // 1.3
            outcome_window_ms: 72 * 3_600_000,
            cascade_guard_candles: 2,
            cascade_sustain_candles: 2,
            reversal_threshold_frac: Decimal::new(8, 3), // 0.008
            level_expiry_candles: 50,
        },
        IntervalConfig {
            label: "1d",
            interval_ms: 86_400_000,
            swing_lookback: 5,
            wick_min_frac: Decimal::new(3, 3),       // 0.003
            volume_baseline_candles: 14,
            volume_spike_multiplier: Decimal::new(13, 1),
            outcome_window_ms: 7 * 86_400_000,
            cascade_guard_candles: 2,
            cascade_sustain_candles: 2,
            reversal_threshold_frac: Decimal::new(10, 3), // 0.010
            level_expiry_candles: 30,
        },
    ]
}

/// Returns the config for the given label, or None if not supported.
pub fn config_for(label: &str) -> Option<IntervalConfig> {
    all_configs().into_iter().find(|c| c.label == label)
}
