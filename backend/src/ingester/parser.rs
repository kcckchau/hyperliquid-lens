use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

fn normalize_side(side: &str) -> Option<String> {
    match side.trim().to_uppercase().as_str() {
        // Hyperliquid may emit Bid/Ask style markers.
        "B" | "BID" | "BUY" => Some("B".to_string()),
        "S" | "ASK" | "SELL" | "A" => Some("S".to_string()),
        _ => None,
    }
}

/// Raw trade event as it arrives from Hyperliquid WebSocket
#[derive(Debug, Deserialize)]
pub struct WsMessage {
    pub channel: String,
    pub data: serde_json::Value,
}

/// Individual raw trade item inside the `data` array
#[derive(Debug, Deserialize)]
pub struct RawTrade {
    pub coin: String,
    pub side: String,
    /// Price as string
    pub px: String,
    /// Size as string
    pub sz: String,
    /// Timestamp in milliseconds
    pub time: i64,
    pub hash: String,
    pub liquidation: Option<serde_json::Value>,
}

/// Parsed, typed trade ready for DB insertion and broadcast
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub coin: String,
    pub side: String,
    pub price: Decimal,
    pub size: Decimal,
    pub timestamp_ms: i64,
    pub trade_hash: String,
    pub is_liquidation: bool,
}

impl TryFrom<RawTrade> for Trade {
    type Error = anyhow::Error;

    fn try_from(raw: RawTrade) -> Result<Self> {
        let price = Decimal::from_str(&raw.px)
            .with_context(|| format!("invalid price: {}", raw.px))?;
        let size = Decimal::from_str(&raw.sz)
            .with_context(|| format!("invalid size: {}", raw.sz))?;
        let side = normalize_side(&raw.side)
            .with_context(|| format!("invalid side: {}", raw.side))?;

        Ok(Trade {
            coin: raw.coin,
            side,
            price,
            size,
            timestamp_ms: raw.time,
            trade_hash: raw.hash,
            is_liquidation: raw.liquidation.is_some(),
        })
    }
}

/// Parse a raw JSON string arriving from the WebSocket into a list of trades.
/// Returns an empty vec if the message is not a trades channel message.
pub fn parse_ws_message(raw: &str) -> Result<Vec<Trade>> {
    let msg: WsMessage = serde_json::from_str(raw).context("failed to deserialize ws message")?;

    if msg.channel != "trades" {
        return Ok(vec![]);
    }

    let raw_trades: Vec<RawTrade> = serde_json::from_value(msg.data)
        .context("failed to deserialize trades data")?;

    let trades = raw_trades
        .into_iter()
        .filter_map(|raw_trade| Trade::try_from(raw_trade).ok())
        .collect::<Vec<_>>();

    Ok(trades)
}
