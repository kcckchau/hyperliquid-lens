use crate::db::trades::OhlcvRow;
use anyhow::{Context, Result};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

const HL_INFO_URL: &str = "https://api.hyperliquid.xyz/info";

#[derive(Debug, Serialize)]
struct CandleSnapshotRequest<'a> {
    #[serde(rename = "type")]
    request_type: &'static str,
    req: CandleSnapshotInner<'a>,
}

#[derive(Debug, Serialize)]
struct CandleSnapshotInner<'a> {
    coin: &'a str,
    interval: &'a str,
    #[serde(rename = "startTime")]
    start_time: i64,
    #[serde(rename = "endTime")]
    end_time: i64,
}

#[derive(Debug, Deserialize)]
struct HyperliquidCandle {
    t: i64,
    o: String,
    h: String,
    l: String,
    c: String,
    v: String,
}

pub async fn fetch_candle_snapshot(
    coin: &str,
    interval: &str,
    start_time: i64,
    end_time: i64,
) -> Result<Vec<OhlcvRow>> {
    let client = Client::new();
    let response = client
        .post(HL_INFO_URL)
        .json(&CandleSnapshotRequest {
            request_type: "candleSnapshot",
            req: CandleSnapshotInner {
                coin,
                interval,
                start_time,
                end_time,
            },
        })
        .send()
        .await
        .context("failed to request Hyperliquid candle snapshot")?
        .error_for_status()
        .context("Hyperliquid candle snapshot returned error status")?;

    let candles: Vec<HyperliquidCandle> = response
        .json()
        .await
        .context("failed to decode Hyperliquid candle snapshot response")?;

    candles
        .into_iter()
        .map(|c| {
            Ok(OhlcvRow {
                bucket_ms: c.t,
                open: Decimal::from_str(&c.o).with_context(|| format!("invalid open {}", c.o))?,
                high: Decimal::from_str(&c.h).with_context(|| format!("invalid high {}", c.h))?,
                low: Decimal::from_str(&c.l).with_context(|| format!("invalid low {}", c.l))?,
                close: Decimal::from_str(&c.c).with_context(|| format!("invalid close {}", c.c))?,
                volume: Decimal::from_str(&c.v).with_context(|| format!("invalid volume {}", c.v))?,
            })
        })
        .collect()
}
