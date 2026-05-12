use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core enums — all map directly to PostgreSQL enum types in migration 002.
// The rename_all = "snake_case" attribute makes sqlx encode Rust variants
// into the exact lowercase_snake strings the DB expects.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "event_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    LiquiditySweep,
    LiquidationCascade,
}

/// State machine lifecycle for every market event.
///
/// Detected   — raw detection fired; not yet confirmed.
/// Confirming — event is in the observation window (outcome_tracker is running).
/// Confirmed  — observation window closed; outcome has been written.
/// Reclassified — event identity changed (e.g. sweep → cascade); a successor
///                event row is linked via reclassified_from.
/// Expired    — observation window closed without enough data to determine outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "event_lifecycle", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EventLifecycle {
    Detected,
    Confirming,
    Confirmed,
    Reclassified,
    Expired,
}

/// What actually happened after the event — observed empirically, never assumed.
///
/// ExpectationFailed is a first-class outcome: the move the statistical base
/// rate would predict did not materialise. Failure states may contain stronger
/// edge than canonical outcomes.
///
/// ReclaimAnomaly: price recovers a key level in a manner inconsistent with
/// how the event classified it (e.g. a level swept bearishly is aggressively
/// reclaimed — implying the sweep itself may have been absorbed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "outcome_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Pending,
    ReversalFollowed,
    ContinuationFollowed,
    ExhaustionDetected,
    AbsorptionDetected,
    ExpectationFailed,
    ReclaimAnomaly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "sweep_direction", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SweepDirection {
    /// Price wicked below a swing low and closed back above it.
    Bullish,
    /// Price wicked above a swing high and closed back below it.
    Bearish,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "cascade_direction", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum CascadeDirection {
    /// Price falling; long positions being liquidated.
    LongLiq,
    /// Price rising; short positions being liquidated.
    ShortLiq,
}

/// Regime is a context tag attached to events for future filtering.
/// Detection logic for regime is not yet implemented; this enum reserves
/// the vocabulary so the schema and API are stable when it is added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "market_regime", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MarketRegime {
    Trend,
    Range,
    VolatilityExpansion,
    VolatilityCompression,
    MomentumAcceleration,
    Chop,
    Unknown,
}

// ---------------------------------------------------------------------------
// Composite value types
// ---------------------------------------------------------------------------

/// A higher-timeframe swing level that coincides with this event's price zone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtfLevel {
    pub interval: String,
    pub level_price: Decimal,
    /// How many candles of that higher timeframe ago the level was formed.
    pub age_candles: usize,
}

/// Empirical detail recorded when the outcome observation window closes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeDetail {
    /// Percentage move from close_price to max extension in outcome direction.
    pub magnitude_pct: Option<Decimal>,
    /// Milliseconds from event_ts_ms to max extension.
    pub duration_ms: Option<i64>,
    /// Absolute price of the max extension point.
    pub max_extension: Option<Decimal>,
    /// Human-readable note for failure or anomaly outcomes.
    pub failure_note: Option<String>,
}

// ---------------------------------------------------------------------------
// MarketEvent — the central struct flowing through the detection pipeline,
// persisted to DB, and broadcast to WebSocket clients.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketEvent {
    /// None before DB insert; Some after.
    pub id: Option<i64>,
    pub coin: String,
    pub interval: String,
    pub event_type: EventType,
    pub lifecycle: EventLifecycle,

    // --- Sweep fields (None for cascade events) ---
    pub sweep_direction: Option<SweepDirection>,
    /// The swing level price that was pierced.
    pub level_price: Option<Decimal>,
    /// Wick tip — the furthest point price reached past the level.
    pub sweep_extreme: Option<Decimal>,
    /// Fractional distance the wick extended past the level.
    pub wick_pct: Option<Decimal>,
    /// Candle close price — must be back inside the level for a valid sweep.
    pub close_price: Option<Decimal>,

    // --- Cascade fields (None for sweep events) ---
    pub cascade_direction: Option<CascadeDirection>,
    pub cascade_start_price: Option<Decimal>,
    pub liq_count_total: Option<i32>,
    pub candles_sustained: Option<i32>,
    /// Volume of last cascade candle divided by volume of first.
    pub volume_acceleration: Option<Decimal>,

    // --- Shared ---
    /// Candle open timestamp (interval-aligned) of the triggering candle.
    pub event_ts_ms: i64,
    pub candle_volume: Decimal,
    pub htf_confluence: Vec<HtfLevel>,

    // --- Outcome (written by outcome_tracker) ---
    pub outcome: OutcomeKind,
    pub outcome_detail: Option<OutcomeDetail>,
    pub outcome_resolved_ts: Option<i64>,

    // --- Context ---
    pub regime: Option<MarketRegime>,
    /// ID of the event this was reclassified from, if applicable.
    pub reclassified_from: Option<i64>,
}

impl MarketEvent {
    /// Construct a new liquidity sweep event in Detected state.
    pub fn new_sweep(
        coin: String,
        interval: String,
        direction: SweepDirection,
        level_price: Decimal,
        sweep_extreme: Decimal,
        wick_pct: Decimal,
        close_price: Decimal,
        candle_volume: Decimal,
        event_ts_ms: i64,
    ) -> Self {
        Self {
            id: None,
            coin,
            interval,
            event_type: EventType::LiquiditySweep,
            lifecycle: EventLifecycle::Detected,
            sweep_direction: Some(direction),
            level_price: Some(level_price),
            sweep_extreme: Some(sweep_extreme),
            wick_pct: Some(wick_pct),
            close_price: Some(close_price),
            cascade_direction: None,
            cascade_start_price: None,
            liq_count_total: None,
            candles_sustained: None,
            volume_acceleration: None,
            event_ts_ms,
            candle_volume,
            htf_confluence: vec![],
            outcome: OutcomeKind::Pending,
            outcome_detail: None,
            outcome_resolved_ts: None,
            regime: None,
            reclassified_from: None,
        }
    }

    /// Construct a new liquidation cascade event in Detected state.
    pub fn new_cascade(
        coin: String,
        interval: String,
        direction: CascadeDirection,
        start_price: Decimal,
        liq_count_total: i32,
        candles_sustained: i32,
        volume_acceleration: Decimal,
        candle_volume: Decimal,
        event_ts_ms: i64,
    ) -> Self {
        Self {
            id: None,
            coin,
            interval,
            event_type: EventType::LiquidationCascade,
            lifecycle: EventLifecycle::Detected,
            sweep_direction: None,
            level_price: None,
            sweep_extreme: None,
            wick_pct: None,
            close_price: None,
            cascade_direction: Some(direction),
            cascade_start_price: Some(start_price),
            liq_count_total: Some(liq_count_total),
            candles_sustained: Some(candles_sustained),
            volume_acceleration: Some(volume_acceleration),
            event_ts_ms,
            candle_volume,
            htf_confluence: vec![],
            outcome: OutcomeKind::Pending,
            outcome_detail: None,
            outcome_resolved_ts: None,
            regime: None,
            reclassified_from: None,
        }
    }
}
