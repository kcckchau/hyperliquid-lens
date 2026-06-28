use anyhow::Result;
use sqlx::PgPool;

#[derive(Debug, sqlx::FromRow)]
pub struct HeartbeatRow {
    pub coin: String,
    pub last_trade_ts_ms: i64,
}

pub struct HeartbeatRepository {
    pool: PgPool,
}

impl HeartbeatRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Upsert the last seen trade timestamp for a coin.
    pub async fn upsert(&self, coin: &str, last_trade_ts_ms: i64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO system_heartbeats (coin, last_trade_ts_ms, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (coin) DO UPDATE
                SET last_trade_ts_ms = EXCLUDED.last_trade_ts_ms,
                    updated_at       = EXCLUDED.updated_at
            "#,
        )
        .bind(coin)
        .bind(last_trade_ts_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch all heartbeat rows — used by /health.
    pub async fn fetch_all(&self) -> Result<Vec<HeartbeatRow>> {
        let rows = sqlx::query_as::<_, HeartbeatRow>(
            "SELECT coin, last_trade_ts_ms FROM system_heartbeats ORDER BY coin",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
