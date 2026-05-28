use crate::db::trades::OhlcvRow;
use anyhow::{anyhow, Result};

/// Indicator profile drives stabilization depth for chart warm-up.
///
/// Professional charting systems do not warm only the visible bars. They also
/// fetch a hidden pre-roll so moving indicators converge before the first bar
/// the user sees. For EMA/SMA families, a practical rule is:
///
///   stabilization_bars = max_indicator_period * 5
///
/// This is enough for a 200 EMA to settle into a trading-grade shape rather
/// than appearing overly reactive on first render.
#[derive(Debug, Clone)]
pub struct IndicatorProfile {
    pub ema_periods: Vec<usize>,
    pub sma_periods: Vec<usize>,
    pub rolling_volume_periods: Vec<usize>,
    pub market_structure_lookbacks: Vec<usize>,
    pub orb_window_bars: Option<usize>,
    pub vwap_sessions: bool,
    pub multi_symbol_context_bars: Vec<usize>,
}

impl IndicatorProfile {
    pub fn stabilization_bars(&self) -> usize {
        let max_period = self
            .ema_periods
            .iter()
            .chain(self.sma_periods.iter())
            .copied()
            .max()
            .unwrap_or(0);

        let indicator_stabilization = max_period.saturating_mul(5);
        let feature_lookback = self
            .rolling_volume_periods
            .iter()
            .chain(self.market_structure_lookbacks.iter())
            .chain(self.multi_symbol_context_bars.iter())
            .copied()
            .max()
            .unwrap_or(0);
        let orb_lookback = self.orb_window_bars.unwrap_or(0);
        let vwap_pre_roll = if self.vwap_sessions { 390 } else { 0 };

        indicator_stabilization
            .max(feature_lookback)
            .max(orb_lookback)
            .max(vwap_pre_roll)
    }
}

#[derive(Debug, Clone)]
pub struct TimeframeWarmupConfig {
    pub interval: String,
    /// Configured floor for this timeframe. This keeps the chart useful even if
    /// the frontend does not yet send a visible-range hint.
    pub floor_bars: usize,
}

#[derive(Debug, Clone)]
pub struct ChartWarmupConfig {
    pub default_visible_bars: usize,
    pub max_remote_bars: usize,
    pub indicator_profile: IndicatorProfile,
    pub per_timeframe: Vec<TimeframeWarmupConfig>,
}

impl ChartWarmupConfig {
    pub fn policy_for(&self, interval: &str) -> Option<&TimeframeWarmupConfig> {
        self.per_timeframe.iter().find(|cfg| cfg.interval == interval)
    }

    pub fn build_plan(&self, request: WarmupRequest<'_>) -> Result<WarmupPlan> {
        let timeframe = self
            .policy_for(request.interval)
            .ok_or_else(|| anyhow!("unsupported warm-up interval: {}", request.interval))?;

        let visible_bars = request.visible_bars.unwrap_or(self.default_visible_bars);
        let visible_range_bars = visible_bars.saturating_mul(2);
        let stabilization_bars = self.indicator_profile.stabilization_bars();

        // The warm-up target balances:
        // 1. Visible range pre-roll for scrolling / zooming comfort
        // 2. Indicator stabilization so EMA/SMA shapes are trustworthy
        // 3. A timeframe-specific floor tuned for professional chart UX
        let requested_bars = timeframe
            .floor_bars
            .max(visible_range_bars)
            .max(stabilization_bars);
        let remote_fetch_bars = requested_bars.min(self.max_remote_bars);
        let needs_remote = request.local_candle_count < requested_bars;

        Ok(WarmupPlan {
            interval: request.interval.to_string(),
            visible_bars,
            visible_range_bars,
            stabilization_bars,
            floor_bars: timeframe.floor_bars,
            requested_bars,
            remote_fetch_bars,
            needs_remote,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WarmupRequest<'a> {
    pub interval: &'a str,
    pub visible_bars: Option<usize>,
    pub local_candle_count: usize,
}

#[derive(Debug, Clone)]
pub struct WarmupPlan {
    pub interval: String,
    pub visible_bars: usize,
    pub visible_range_bars: usize,
    pub stabilization_bars: usize,
    pub floor_bars: usize,
    pub requested_bars: usize,
    pub remote_fetch_bars: usize,
    pub needs_remote: bool,
}

pub fn merge_candles(snapshot: Vec<OhlcvRow>, local: Vec<OhlcvRow>) -> Vec<OhlcvRow> {
    let mut merged = std::collections::BTreeMap::new();

    for candle in snapshot {
        merged.insert(candle.bucket_ms, candle);
    }

    // Local candles win on collisions so the chart prefers the freshest
    // exchange-synchronized data we have computed from the trade DB.
    for candle in local {
        merged.insert(candle.bucket_ms, candle);
    }

    merged.into_values().collect()
}
