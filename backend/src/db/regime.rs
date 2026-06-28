use anyhow::Result;
use sqlx::PgPool;

const MIN_CONFIDENCE_TO_SWITCH: f64 = 0.65;
const MIN_PENDING_COUNT_TO_SWITCH: i32 = 3;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RegimeRow {
    pub coin: String,
    pub interval_label: String,
    pub regime: String,
    pub confidence: rust_decimal::Decimal,
    pub candles_in_regime: i32,
    pub pending_regime: Option<String>,
    pub pending_count: i32,
    pub previous_regime: Option<String>,
}

pub struct RegimeRepository {
    pool: PgPool,
}

impl RegimeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Fetch all current regime rows — used by /health.
    pub async fn fetch_all(&self) -> Result<Vec<RegimeRow>> {
        let rows = sqlx::query_as::<_, RegimeRow>(
            r#"
            SELECT coin, interval_label, regime, confidence, candles_in_regime,
                   pending_regime, pending_count, previous_regime
            FROM regime_snapshots
            ORDER BY coin, interval_label
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Fetch the current regime for one (coin, interval) — used by pipeline to tag events.
    pub async fn fetch_current(&self, coin: &str, interval: &str) -> Result<Option<RegimeRow>> {
        let row = sqlx::query_as::<_, RegimeRow>(
            r#"
            SELECT coin, interval_label, regime, confidence, candles_in_regime,
                   pending_regime, pending_count, previous_regime
            FROM regime_snapshots
            WHERE coin = $1 AND interval_label = $2
            "#,
        )
        .bind(coin)
        .bind(interval)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Upsert regime with hysteresis.
    ///
    /// Rules:
    /// - Same as current → increment candles_in_regime, update confidence, clear pending.
    /// - New regime == pending challenger → increment pending_count.
    ///   If pending_count >= 3 AND confidence >= 0.65 → commit the switch.
    /// - New challenger → reset pending to this regime, pending_count = 1.
    pub async fn upsert_with_hysteresis(
        &self,
        coin: &str,
        interval: &str,
        new_regime: &str,
        confidence: f64,
        candle_ts_ms: i64,
    ) -> Result<()> {
        let current = self.fetch_current(coin, interval).await?;

        match current {
            None => {
                // First ever compute — just insert.
                sqlx::query(
                    r#"
                    INSERT INTO regime_snapshots
                        (coin, interval_label, regime, confidence, candles_in_regime,
                         pending_regime, pending_count, computed_at, candle_ts_ms)
                    VALUES ($1, $2, $3, $4, 1, NULL, 0, NOW(), $5)
                    "#,
                )
                .bind(coin)
                .bind(interval)
                .bind(new_regime)
                .bind(confidence)
                .bind(candle_ts_ms)
                .execute(&self.pool)
                .await?;
            }

            Some(row) => {
                let current_regime = &row.regime;
                let pending = row.pending_regime.as_deref();
                let pending_count = row.pending_count;

                if new_regime == current_regime {
                    // Regime continues — update confidence + candle count.
                    sqlx::query(
                        r#"
                        UPDATE regime_snapshots
                        SET confidence        = $1,
                            candles_in_regime = candles_in_regime + 1,
                            pending_regime    = NULL,
                            pending_count     = 0,
                            computed_at       = NOW(),
                            candle_ts_ms      = $2
                        WHERE coin = $3 AND interval_label = $4
                        "#,
                    )
                    .bind(confidence)
                    .bind(candle_ts_ms)
                    .bind(coin)
                    .bind(interval)
                    .execute(&self.pool)
                    .await?;
                } else if Some(new_regime) == pending {
                    let new_count = pending_count + 1;
                    if new_count >= MIN_PENDING_COUNT_TO_SWITCH
                        && confidence >= MIN_CONFIDENCE_TO_SWITCH
                    {
                        // Challenger confirmed — switch regime.
                        sqlx::query(
                            r#"
                            UPDATE regime_snapshots
                            SET previous_regime   = regime,
                                regime            = $1,
                                confidence        = $2,
                                candles_in_regime = 1,
                                pending_regime    = NULL,
                                pending_count     = 0,
                                changed_at        = NOW(),
                                computed_at       = NOW(),
                                candle_ts_ms      = $3
                            WHERE coin = $4 AND interval_label = $5
                            "#,
                        )
                        .bind(new_regime)
                        .bind(confidence)
                        .bind(candle_ts_ms)
                        .bind(coin)
                        .bind(interval)
                        .execute(&self.pool)
                        .await?;
                    } else {
                        // Challenger accumulating.
                        sqlx::query(
                            r#"
                            UPDATE regime_snapshots
                            SET pending_count = $1,
                                computed_at   = NOW(),
                                candle_ts_ms  = $2
                            WHERE coin = $3 AND interval_label = $4
                            "#,
                        )
                        .bind(new_count)
                        .bind(candle_ts_ms)
                        .bind(coin)
                        .bind(interval)
                        .execute(&self.pool)
                        .await?;
                    }
                } else {
                    // New challenger — reset pending.
                    sqlx::query(
                        r#"
                        UPDATE regime_snapshots
                        SET pending_regime = $1,
                            pending_count  = 1,
                            computed_at    = NOW(),
                            candle_ts_ms   = $2
                        WHERE coin = $3 AND interval_label = $4
                        "#,
                    )
                    .bind(new_regime)
                    .bind(candle_ts_ms)
                    .bind(coin)
                    .bind(interval)
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        Ok(())
    }
}
