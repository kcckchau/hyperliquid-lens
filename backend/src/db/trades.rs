use crate::ingester::parser::Trade;
use anyhow::Result;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct TradeRepository {
    pool: PgPool,
}

impl TradeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a trade, ignoring duplicates via the unique hash index
    pub async fn insert(&self, trade: &Trade) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO trades (coin, side, price, size, timestamp_ms, trade_hash, is_liquidation)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (trade_hash) DO NOTHING
            "#,
        )
        .bind(&trade.coin)
        .bind(&trade.side)
        .bind(trade.price)
        .bind(trade.size)
        .bind(trade.timestamp_ms)
        .bind(&trade.trade_hash)
        .bind(trade.is_liquidation)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch recent trades for a coin, optionally bounded by time range
    pub async fn fetch(
        &self,
        coin: &str,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: i64,
    ) -> Result<Vec<TradeRow>> {
        let rows = sqlx::query_as::<_, TradeRow>(
            r#"
            SELECT id, coin, side, price, size, timestamp_ms, trade_hash, is_liquidation
            FROM trades
            WHERE coin = $1
              AND ($2::BIGINT IS NULL OR timestamp_ms >= $2)
              AND ($3::BIGINT IS NULL OR timestamp_ms <= $3)
            ORDER BY timestamp_ms DESC
            LIMIT $4
            "#,
        )
        .bind(coin)
        .bind(from_ms)
        .bind(to_ms)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// OHLCV aggregation bucketed by `interval_ms` milliseconds.
    /// Uses a CTE to build clean per-bucket candles without window-function duplication.
    pub async fn ohlcv(
        &self,
        coin: &str,
        interval_ms: i64,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<Vec<OhlcvRow>> {
        let rows = sqlx::query_as::<_, OhlcvRow>(
            r#"
            WITH bucketed AS (
                SELECT
                    (timestamp_ms / $2) * $2 AS bucket_ms,
                    price,
                    size,
                    ROW_NUMBER() OVER (PARTITION BY (timestamp_ms / $2) ORDER BY timestamp_ms ASC)  AS rn_asc,
                    ROW_NUMBER() OVER (PARTITION BY (timestamp_ms / $2) ORDER BY timestamp_ms DESC) AS rn_desc
                FROM trades
                WHERE coin = $1
                  AND ($3::BIGINT IS NULL OR timestamp_ms >= $3)
                  AND ($4::BIGINT IS NULL OR timestamp_ms <= $4)
            )
            SELECT
                bucket_ms,
                MAX(price) FILTER (WHERE rn_asc  = 1)  AS open,
                MAX(price)                              AS high,
                MIN(price)                              AS low,
                MAX(price) FILTER (WHERE rn_desc = 1)  AS close,
                SUM(size)                               AS volume
            FROM bucketed
            GROUP BY bucket_ms
            ORDER BY bucket_ms
            "#,
        )
        .bind(coin)
        .bind(interval_ms)
        .bind(from_ms)
        .bind(to_ms)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}

/// Flat trade row returned by the REST API
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TradeRow {
    pub id: i64,
    pub coin: String,
    pub side: String,
    pub price: Decimal,
    pub size: Decimal,
    pub timestamp_ms: i64,
    pub trade_hash: String,
    pub is_liquidation: bool,
}

/// OHLCV candle returned by the summary endpoint
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OhlcvRow {
    pub bucket_ms: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}
