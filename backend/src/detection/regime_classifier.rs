/// Score-based regime classifier.
///
/// Input:  the last ~100 OHLCV candles for a single (coin, timeframe).
/// Output: (RegimeKind, confidence 0..1)
///
/// Five regimes:
///   TREND_UP            — directional, bulls in control
///   TREND_DOWN          — directional, bears in control
///   RANGE               — mean-reverting, bounded
///   HIGH_VOL_CHOP       — volatile, no direction, both sides getting stopped
///   LOW_VOL_COMPRESSION — coiling, breakout pending
///
/// Detection is NOT based on individual candle patterns (engulfing, sweep count).
/// It is based on structural state across 50–100 candles:
///   1. EMA fan/slope  — direction and momentum of trend
///   2. HH/HL/LH/LL   — market structure continuity
///   3. VWAP behavior  — mean-reverting vs trending
///   4. ATR percentile — volatility regime
///   5. Close position — candle-by-candle buying/selling pressure
use crate::db::trades::OhlcvRow;

// ---------------------------------------------------------------------------
// Regime enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RegimeKind {
    TrendUp,
    TrendDown,
    Range,
    HighVolChop,
    LowVolCompression,
}

impl RegimeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TrendUp => "TREND_UP",
            Self::TrendDown => "TREND_DOWN",
            Self::Range => "RANGE",
            Self::HighVolChop => "HIGH_VOL_CHOP",
            Self::LowVolCompression => "LOW_VOL_COMPRESSION",
        }
    }
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

struct Features {
    // EMA structure
    ema9: f64,
    ema20: f64,
    ema50: f64,
    ema20_slope: f64,   // fractional slope over last 4 bars
    ema_spread: f64,    // (ema9 - ema50).abs() / close — low = compressed

    // VWAP (computed over last 50 candles)
    bars_above_vwap_frac: f64,
    vwap_cross_count: usize,

    // Market structure
    hh: usize, // higher highs (last 10 swing pivots)
    hl: usize, // higher lows
    lh: usize, // lower highs
    ll: usize, // lower lows

    // Volatility
    atr_percentile: f64,    // current ATR vs last-N distribution
    avg_wick_ratio: f64,    // wick / range, last 20 candles
    direction_flips: usize, // candle direction changes, last 20

    // Close quality
    avg_close_pos: f64, // (close - low) / (high - low), last 20 candles
}

// ---------------------------------------------------------------------------
// Indicator helpers
// ---------------------------------------------------------------------------

fn d2f(d: rust_decimal::Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

/// Exponential moving average of a close-price slice.
fn ema(closes: &[f64], period: usize) -> Vec<f64> {
    let k = 2.0 / (period as f64 + 1.0);
    let mut out = Vec::with_capacity(closes.len());
    for (i, &c) in closes.iter().enumerate() {
        if i == 0 {
            out.push(c);
        } else if i < period {
            // SMA seed phase
            let sum: f64 = closes[..=i].iter().sum();
            out.push(sum / (i + 1) as f64);
        } else {
            let prev = out[i - 1];
            out.push(c * k + prev * (1.0 - k));
        }
    }
    out
}

/// Wilder ATR.
fn atr(candles: &[OhlcvRow], period: usize) -> Vec<f64> {
    let mut trs = Vec::with_capacity(candles.len());
    for (i, c) in candles.iter().enumerate() {
        let h = d2f(c.high);
        let l = d2f(c.low);
        let tr = if i == 0 {
            h - l
        } else {
            let pc = d2f(candles[i - 1].close);
            (h - l).max((h - pc).abs()).max((l - pc).abs())
        };
        trs.push(tr);
    }
    let mut out = Vec::with_capacity(candles.len());
    for (i, &tr) in trs.iter().enumerate() {
        if i < period {
            let sum: f64 = trs[..=i].iter().sum();
            out.push(sum / (i + 1) as f64);
        } else {
            let prev = out[i - 1];
            out.push((prev * (period as f64 - 1.0) + tr) / period as f64);
        }
    }
    out
}

/// Pivot highs: indices where high > all highs within `lookback` on each side.
fn pivot_highs(highs: &[f64], lookback: usize) -> Vec<usize> {
    let n = highs.len();
    let mut out = Vec::new();
    for i in lookback..n.saturating_sub(lookback) {
        let h = highs[i];
        let left_ok = highs[i.saturating_sub(lookback)..i].iter().all(|&x| x <= h);
        let right_ok = highs[i + 1..=(i + lookback).min(n - 1)]
            .iter()
            .all(|&x| x <= h);
        if left_ok && right_ok {
            out.push(i);
        }
    }
    out
}

/// Pivot lows: indices where low < all lows within `lookback` on each side.
fn pivot_lows(lows: &[f64], lookback: usize) -> Vec<usize> {
    let n = lows.len();
    let mut out = Vec::new();
    for i in lookback..n.saturating_sub(lookback) {
        let l = lows[i];
        let left_ok = lows[i.saturating_sub(lookback)..i].iter().all(|&x| x >= l);
        let right_ok = lows[i + 1..=(i + lookback).min(n - 1)]
            .iter()
            .all(|&x| x >= l);
        if left_ok && right_ok {
            out.push(i);
        }
    }
    out
}

/// Count HH/HL/LH/LL from the last `max_pivots` pivot points.
fn count_structure(
    highs: &[f64],
    lows: &[f64],
    lookback: usize,
    max_pivots: usize,
) -> (usize, usize, usize, usize) {
    let ph = pivot_highs(highs, lookback);
    let pl = pivot_lows(lows, lookback);

    // Take last N pivots
    let ph: Vec<_> = ph.iter().rev().take(max_pivots).rev().collect();
    let pl: Vec<_> = pl.iter().rev().take(max_pivots).rev().collect();

    let mut hh = 0usize;
    let mut lh = 0usize;
    for pair in ph.windows(2) {
        if highs[*pair[1]] > highs[*pair[0]] {
            hh += 1;
        } else {
            lh += 1;
        }
    }

    let mut hl = 0usize;
    let mut ll = 0usize;
    for pair in pl.windows(2) {
        if lows[*pair[1]] > lows[*pair[0]] {
            hl += 1;
        } else {
            ll += 1;
        }
    }

    (hh, hl, lh, ll)
}

// ---------------------------------------------------------------------------
// Feature extraction
// ---------------------------------------------------------------------------

fn compute_features(candles: &[OhlcvRow]) -> Option<Features> {
    let n = candles.len();
    if n < 60 {
        return None; // need at least 60 bars
    }

    let closes: Vec<f64> = candles.iter().map(|c| d2f(c.close)).collect();
    let highs: Vec<f64> = candles.iter().map(|c| d2f(c.high)).collect();
    let lows: Vec<f64> = candles.iter().map(|c| d2f(c.low)).collect();
    let opens: Vec<f64> = candles.iter().map(|c| d2f(c.open)).collect();
    let volumes: Vec<f64> = candles.iter().map(|c| d2f(c.volume)).collect();

    // ── EMAs ──────────────────────────────────────────────────────────────────
    let ema9_series = ema(&closes, 9);
    let ema20_series = ema(&closes, 20);
    let ema50_series = ema(&closes, 50);

    let ema9 = *ema9_series.last()?;
    let ema20 = *ema20_series.last()?;
    let ema50 = *ema50_series.last()?;
    let last_close = *closes.last()?;

    let ema20_slope = if n >= 5 && ema20_series[n - 5] != 0.0 {
        (ema20_series[n - 1] - ema20_series[n - 5]) / ema20_series[n - 5]
    } else {
        0.0
    };

    let ema_spread = if last_close != 0.0 {
        (ema9 - ema50).abs() / last_close
    } else {
        0.0
    };

    // ── VWAP (last 50 candles) ─────────────────────────────────────────────
    let w_start = n.saturating_sub(50);
    let vwap_num: f64 = candles[w_start..]
        .iter()
        .zip(volumes[w_start..].iter())
        .map(|(c, &v)| ((d2f(c.high) + d2f(c.low) + d2f(c.close)) / 3.0) * v)
        .sum();
    let vwap_den: f64 = volumes[w_start..].iter().sum();
    let vwap = if vwap_den > 0.0 {
        vwap_num / vwap_den
    } else {
        last_close
    };

    let mut bars_above_vwap = 0usize;
    let mut vwap_cross_count = 0usize;
    let mut prev_above = closes[w_start] > vwap;
    for &c in &closes[w_start..] {
        let above = c > vwap;
        if above {
            bars_above_vwap += 1;
        }
        if above != prev_above {
            vwap_cross_count += 1;
        }
        prev_above = above;
    }
    let bars_total = closes[w_start..].len();
    let bars_above_vwap_frac = bars_above_vwap as f64 / bars_total as f64;

    // ── Market structure ───────────────────────────────────────────────────
    let (hh, hl, lh, ll) = count_structure(&highs, &lows, 3, 10);

    // ── ATR percentile ─────────────────────────────────────────────────────
    let atrs = atr(candles, 14);
    let last_atr = *atrs.last()?;
    let mut sorted_atrs: Vec<f64> = atrs.clone();
    sorted_atrs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = sorted_atrs.partition_point(|&x| x <= last_atr);
    let atr_percentile = rank as f64 / sorted_atrs.len() as f64;

    // ── Close position + wick ratio + direction flips (last 20) ───────────
    let w20 = n.saturating_sub(20);
    let mut close_pos_sum = 0.0f64;
    let mut wick_ratio_sum = 0.0f64;
    let mut direction_flips = 0usize;
    let mut prev_bullish = opens[w20] < closes[w20];

    for i in w20..n {
        let h = highs[i];
        let l = lows[i];
        let c = closes[i];
        let o = opens[i];
        let range = h - l;

        close_pos_sum += if range > 0.0 { (c - l) / range } else { 0.5 };
        let body = (c - o).abs();
        wick_ratio_sum += if range > 0.0 { (range - body) / range } else { 0.0 };

        let bullish = c >= o;
        if bullish != prev_bullish {
            direction_flips += 1;
        }
        prev_bullish = bullish;
    }

    let count20 = (n - w20) as f64;
    let avg_close_pos = close_pos_sum / count20;
    let avg_wick_ratio = wick_ratio_sum / count20;

    Some(Features {
        ema9,
        ema20,
        ema50,
        ema20_slope,
        ema_spread,
        bars_above_vwap_frac,
        vwap_cross_count,
        hh,
        hl,
        lh,
        ll,
        atr_percentile,
        avg_wick_ratio,
        direction_flips,
        avg_close_pos,
    })
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------
// Order: [TrendUp, TrendDown, Range, HighVolChop, LowVolCompression]

fn score_all(f: &Features) -> [f64; 5] {
    let mut s = [0f64; 5];

    // ── TREND_UP ────────────────────────────────────────────────────────────
    if f.ema9 > f.ema20 && f.ema20 > f.ema50 {
        s[0] += 15.0;
    }
    if f.ema20_slope > 0.001 {
        s[0] += 8.0;
    }
    s[0] += f.bars_above_vwap_frac * 12.0;
    s[0] += (f.hh as f64).min(5.0) * 2.0;
    s[0] += (f.hl as f64).min(5.0) * 2.0;
    if f.avg_close_pos > 0.55 {
        s[0] += 8.0;
    }
    if f.vwap_cross_count <= 4 {
        s[0] += 4.0;
    }

    // ── TREND_DOWN ──────────────────────────────────────────────────────────
    if f.ema9 < f.ema20 && f.ema20 < f.ema50 {
        s[1] += 15.0;
    }
    if f.ema20_slope < -0.001 {
        s[1] += 8.0;
    }
    s[1] += (1.0 - f.bars_above_vwap_frac) * 12.0;
    s[1] += (f.lh as f64).min(5.0) * 2.0;
    s[1] += (f.ll as f64).min(5.0) * 2.0;
    if f.avg_close_pos < 0.45 {
        s[1] += 8.0;
    }
    if f.vwap_cross_count <= 4 {
        s[1] += 4.0;
    }

    // ── RANGE ───────────────────────────────────────────────────────────────
    if f.vwap_cross_count >= 6 {
        s[2] += 12.0;
    }
    if f.atr_percentile >= 0.25 && f.atr_percentile <= 0.70 {
        s[2] += 8.0;
    }
    if f.ema_spread < 0.006 {
        s[2] += 8.0;
    } // EMAs tangled
    if f.avg_close_pos >= 0.40 && f.avg_close_pos <= 0.60 {
        s[2] += 8.0;
    }
    if f.ema20_slope.abs() < 0.002 {
        s[2] += 6.0;
    }

    // ── HIGH_VOL_CHOP ────────────────────────────────────────────────────────
    if f.atr_percentile > 0.70 {
        s[3] += 15.0;
    }
    if f.avg_wick_ratio > 0.50 {
        s[3] += 10.0;
    }
    if f.vwap_cross_count >= 8 {
        s[3] += 10.0;
    }
    s[3] += (f.direction_flips as f64).min(10.0) * 1.5;

    // ── LOW_VOL_COMPRESSION ──────────────────────────────────────────────────
    if f.atr_percentile < 0.25 {
        s[4] += 15.0;
    }
    if f.ema_spread < 0.003 {
        s[4] += 12.0;
    }
    if f.avg_wick_ratio < 0.35 {
        s[4] += 8.0;
    }
    if f.vwap_cross_count <= 4 {
        s[4] += 6.0;
    }
    if f.direction_flips <= 5 {
        s[4] += 6.0;
    }

    s
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Classify regime from a candle slice.
/// Returns None if there are fewer than 60 candles (not enough data).
pub fn classify(candles: &[OhlcvRow]) -> Option<(RegimeKind, f64)> {
    let f = compute_features(candles)?;
    let scores = score_all(&f);

    let total: f64 = scores.iter().sum();
    if total == 0.0 {
        return None;
    }

    let (best_idx, &best_score) = scores
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?;

    let kind = match best_idx {
        0 => RegimeKind::TrendUp,
        1 => RegimeKind::TrendDown,
        2 => RegimeKind::Range,
        3 => RegimeKind::HighVolChop,
        4 => RegimeKind::LowVolCompression,
        _ => return None,
    };

    // confidence = winning_score / total_score (your definition)
    let confidence = (best_score / total).clamp(0.0, 1.0);
    Some((kind, confidence))
}
