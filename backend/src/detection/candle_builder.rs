use crate::ingester::parser::Trade;
use rust_decimal::Decimal;

/// A completed OHLCV candle enriched with liquidation trade count.
#[derive(Debug, Clone)]
pub struct Candle {
    /// Interval-aligned open time: (trade.timestamp_ms / interval_ms) * interval_ms
    pub bucket_ms: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub trade_count: u32,
    /// Number of trades in this candle flagged as liquidations.
    pub liq_count: u32,
}

/// Accumulates trades into OHLCV candles and emits a completed candle whenever
/// the interval boundary is crossed.
///
/// Assumes trades arrive in non-decreasing timestamp order (guaranteed by the
/// Hyperliquid WS stream for a single coin). Out-of-order trades within the
/// current bucket are silently merged; out-of-order trades that fall into a
/// past bucket are dropped.
pub struct CandleBuilder {
    interval_ms: i64,
    current_bucket: Option<i64>,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: Decimal,
    trade_count: u32,
    liq_count: u32,
}

impl CandleBuilder {
    pub fn new(interval_ms: i64) -> Self {
        Self {
            interval_ms,
            current_bucket: None,
            open: Decimal::ZERO,
            high: Decimal::ZERO,
            low: Decimal::ZERO,
            close: Decimal::ZERO,
            volume: Decimal::ZERO,
            trade_count: 0,
            liq_count: 0,
        }
    }

    /// Feed one trade into the builder.
    ///
    /// Returns `Some(Candle)` if this trade falls into a new bucket, completing
    /// the previous one. Returns `None` if the trade extends the current candle.
    pub fn push(&mut self, trade: &Trade) -> Option<Candle> {
        let bucket = (trade.timestamp_ms / self.interval_ms) * self.interval_ms;

        match self.current_bucket {
            None => {
                self.start_bucket(bucket, trade);
                None
            }
            Some(current) if current == bucket => {
                self.accumulate(trade);
                None
            }
            Some(current) if bucket > current => {
                // Trade opens a new bucket — emit the completed candle first.
                let completed = self.snapshot();
                self.start_bucket(bucket, trade);
                Some(completed)
            }
            Some(_) => {
                // Trade is older than current bucket — drop silently.
                None
            }
        }
    }

    fn start_bucket(&mut self, bucket: i64, trade: &Trade) {
        self.current_bucket = Some(bucket);
        self.open = trade.price;
        self.high = trade.price;
        self.low = trade.price;
        self.close = trade.price;
        self.volume = trade.size;
        self.trade_count = 1;
        self.liq_count = u32::from(trade.is_liquidation);
    }

    fn accumulate(&mut self, trade: &Trade) {
        if trade.price > self.high {
            self.high = trade.price;
        }
        if trade.price < self.low {
            self.low = trade.price;
        }
        self.close = trade.price;
        self.volume += trade.size;
        self.trade_count += 1;
        if trade.is_liquidation {
            self.liq_count += 1;
        }
    }

    fn snapshot(&self) -> Candle {
        Candle {
            bucket_ms: self.current_bucket.expect("snapshot called with no open bucket"),
            open: self.open,
            high: self.high,
            low: self.low,
            close: self.close,
            volume: self.volume,
            trade_count: self.trade_count,
            liq_count: self.liq_count,
        }
    }
}
