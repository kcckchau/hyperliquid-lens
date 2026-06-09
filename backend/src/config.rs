use anyhow::{Context, Result};

use crate::chart::warmup::{ChartWarmupConfig, IndicatorProfile, TimeframeWarmupConfig};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub port: u16,
    pub coins: Vec<String>,
    pub chart_warmup: ChartWarmupConfig,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .context("DATABASE_URL must be set")?;

        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "3001".to_string())
            .parse::<u16>()
            .context("PORT must be a valid port number")?;

        let coins = std::env::var("COINS")
            .unwrap_or_else(|_| "BTC,ETH,SOL,HYPE".to_string())
            .split(',')
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .collect();

        let chart_warmup = ChartWarmupConfig {
            default_visible_bars: std::env::var("CHART_DEFAULT_VISIBLE_BARS")
                .unwrap_or_else(|_| "300".to_string())
                .parse::<usize>()
                .context("CHART_DEFAULT_VISIBLE_BARS must be a valid positive integer")?,
            max_remote_bars: std::env::var("CHART_MAX_REMOTE_BARS")
                .unwrap_or_else(|_| "5000".to_string())
                .parse::<usize>()
                .context("CHART_MAX_REMOTE_BARS must be a valid positive integer")?,
            indicator_profile: IndicatorProfile {
                ema_periods: parse_usize_list(
                    &std::env::var("CHART_EMA_PERIODS")
                        .unwrap_or_else(|_| "50,100,200".to_string()),
                )?,
                sma_periods: parse_usize_list(
                    &std::env::var("CHART_SMA_PERIODS")
                        .unwrap_or_else(|_| "".to_string()),
                )?,
                rolling_volume_periods: parse_usize_list(
                    &std::env::var("CHART_ROLLING_VOLUME_PERIODS")
                        .unwrap_or_else(|_| "20".to_string()),
                )?,
                market_structure_lookbacks: parse_usize_list(
                    &std::env::var("CHART_MARKET_STRUCTURE_LOOKBACKS")
                        .unwrap_or_else(|_| "20,50".to_string()),
                )?,
                orb_window_bars: parse_optional_usize(
                    std::env::var("CHART_ORB_WINDOW_BARS").ok().as_deref(),
                )?,
                vwap_sessions: std::env::var("CHART_ENABLE_VWAP")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse::<bool>()
                    .context("CHART_ENABLE_VWAP must be true or false")?,
                multi_symbol_context_bars: parse_usize_list(
                    &std::env::var("CHART_MULTI_SYMBOL_CONTEXT_BARS")
                        .unwrap_or_else(|_| "".to_string()),
                )?,
            },
            per_timeframe: vec![
                timeframe_config("1m", 2_000)?,
                timeframe_config("5m", 1_500)?,
                timeframe_config("15m", 1_000)?,
                timeframe_config("1h", 1_000)?,
                timeframe_config("4h", 500)?,
                timeframe_config("1d", 500)?,
            ],
        };

        Ok(Config {
            database_url,
            port,
            coins,
            chart_warmup,
        })
    }
}

fn timeframe_config(label: &str, default_floor: usize) -> Result<TimeframeWarmupConfig> {
    let env_key = format!("CHART_WARMUP_{}_BARS", label.to_uppercase());
    let floor_bars = std::env::var(&env_key)
        .unwrap_or_else(|_| default_floor.to_string())
        .parse::<usize>()
        .with_context(|| format!("{env_key} must be a valid positive integer"))?;

    Ok(TimeframeWarmupConfig {
        interval: label.to_string(),
        floor_bars,
    })
}

fn parse_usize_list(raw: &str) -> Result<Vec<usize>> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<usize>()
                .with_context(|| format!("invalid integer value: {value}"))
        })
        .collect()
}

fn parse_optional_usize(raw: Option<&str>) -> Result<Option<usize>> {
    match raw.map(str::trim) {
        Some("") | None => Ok(None),
        Some(value) => value
            .parse::<usize>()
            .map(Some)
            .with_context(|| format!("invalid integer value: {value}")),
    }
}
