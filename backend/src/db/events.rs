use crate::detection::types::{
    CascadeDirection, EventLifecycle, EventType, HtfLevel, MarketEvent, OutcomeDetail, OutcomeKind,
    SweepDirection,
};
use anyhow::Result;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct EventRepository {
    pool: PgPool,
}

impl EventRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a new event. Returns the auto-generated DB id.
    pub async fn insert(&self, event: &MarketEvent) -> Result<i64> {
        let htf_json = serde_json::to_value(&event.htf_confluence)?;

        let row: (i64,) = sqlx::query_as(
            r#"
            INSERT INTO market_events (
                coin, interval, event_type, lifecycle,
                sweep_direction, level_price, sweep_extreme, wick_pct, close_price,
                cascade_direction, cascade_start_price, liq_count_total,
                candles_sustained, volume_acceleration,
                event_ts_ms, candle_volume, htf_confluence,
                outcome, reclassified_from
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8, $9,
                $10, $11, $12,
                $13, $14,
                $15, $16, $17,
                $18, $19
            )
            RETURNING id
            "#,
        )
        .bind(&event.coin)
        .bind(&event.interval)
        .bind(&event.event_type)
        .bind(&event.lifecycle)
        .bind(&event.sweep_direction)
        .bind(event.level_price)
        .bind(event.sweep_extreme)
        .bind(event.wick_pct)
        .bind(event.close_price)
        .bind(&event.cascade_direction)
        .bind(event.cascade_start_price)
        .bind(event.liq_count_total)
        .bind(event.candles_sustained)
        .bind(event.volume_acceleration)
        .bind(event.event_ts_ms)
        .bind(event.candle_volume)
        .bind(htf_json)
        .bind(&event.outcome)
        .bind(event.reclassified_from)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// Transition an event's lifecycle state.
    pub async fn update_lifecycle(&self, id: i64, lifecycle: EventLifecycle) -> Result<()> {
        sqlx::query("UPDATE market_events SET lifecycle = $1 WHERE id = $2")
            .bind(&lifecycle)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Write the empirical outcome after the observation window closes.
    /// Also transitions lifecycle to Confirmed.
    pub async fn update_outcome(
        &self,
        id: i64,
        outcome: OutcomeKind,
        detail: Option<OutcomeDetail>,
        resolved_ts_ms: i64,
    ) -> Result<()> {
        let detail_json = detail.map(|d| serde_json::to_value(d)).transpose()?;
        sqlx::query(
            r#"
            UPDATE market_events
            SET outcome = $1,
                outcome_detail = $2,
                outcome_resolved_ts = $3,
                lifecycle = 'confirmed'
            WHERE id = $4
            "#,
        )
        .bind(&outcome)
        .bind(detail_json)
        .bind(resolved_ts_ms)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch all events still in `Confirming` state (for restart recovery).
    pub async fn fetch_confirming(&self) -> Result<Vec<MarketEvent>> {
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT * FROM market_events WHERE lifecycle = 'confirming' ORDER BY event_ts_ms",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(MarketEvent::from).collect())
    }

    /// Fetch events with flexible filtering for the REST API.
    pub async fn fetch(
        &self,
        coin: &str,
        interval: Option<&str>,
        event_type: Option<&str>,
        lifecycle: Option<&str>,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: i64,
    ) -> Result<Vec<EventRow>> {
        let rows = sqlx::query_as::<_, EventRow>(
            r#"
            SELECT * FROM market_events
            WHERE coin = $1
              AND ($2::TEXT IS NULL OR interval = $2)
              AND ($3::TEXT IS NULL OR event_type::TEXT = $3)
              AND ($4::TEXT IS NULL OR lifecycle::TEXT = $4)
              AND ($5::BIGINT IS NULL OR event_ts_ms >= $5)
              AND ($6::BIGINT IS NULL OR event_ts_ms <= $6)
            ORDER BY event_ts_ms DESC
            LIMIT $7
            "#,
        )
        .bind(coin)
        .bind(interval)
        .bind(event_type)
        .bind(lifecycle)
        .bind(from_ms)
        .bind(to_ms)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Return aggregate outcome distribution for a coin/interval.
    pub async fn outcome_stats(
        &self,
        coin: &str,
        interval: Option<&str>,
        event_type: Option<&str>,
    ) -> Result<Vec<OutcomeStatRow>> {
        let rows = sqlx::query_as::<_, OutcomeStatRow>(
            r#"
            SELECT
                event_type::TEXT  AS event_type,
                outcome::TEXT     AS outcome,
                COUNT(*)          AS count,
                AVG((outcome_detail->>'magnitude_pct')::NUMERIC) AS avg_magnitude_pct
            FROM market_events
            WHERE coin = $1
              AND lifecycle = 'confirmed'
              AND ($2::TEXT IS NULL OR interval = $2)
              AND ($3::TEXT IS NULL OR event_type::TEXT = $3)
            GROUP BY event_type, outcome
            ORDER BY event_type, count DESC
            "#,
        )
        .bind(coin)
        .bind(interval)
        .bind(event_type)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

// ─── DB row types ────────────────────────────────────────────────────────────

/// Flat row returned by SELECT * from market_events. Used for REST API responses
/// and for rehydrating MarketEvent structs on restart.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EventRow {
    pub id: i64,
    pub coin: String,
    pub interval: String,
    pub event_type: EventType,
    pub lifecycle: EventLifecycle,
    pub sweep_direction: Option<SweepDirection>,
    pub level_price: Option<Decimal>,
    pub sweep_extreme: Option<Decimal>,
    pub wick_pct: Option<Decimal>,
    pub close_price: Option<Decimal>,
    pub cascade_direction: Option<CascadeDirection>,
    pub cascade_start_price: Option<Decimal>,
    pub liq_count_total: Option<i32>,
    pub candles_sustained: Option<i32>,
    pub volume_acceleration: Option<Decimal>,
    pub event_ts_ms: i64,
    pub candle_volume: Decimal,
    pub htf_confluence: serde_json::Value,
    pub outcome: OutcomeKind,
    pub outcome_detail: Option<serde_json::Value>,
    pub outcome_resolved_ts: Option<i64>,
    pub regime: Option<String>,
    pub reclassified_from: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OutcomeStatRow {
    pub event_type: String,
    pub outcome: String,
    pub count: i64,
    pub avg_magnitude_pct: Option<Decimal>,
}

// ─── EventRow → MarketEvent conversion (for restart recovery) ────────────────

impl From<EventRow> for MarketEvent {
    fn from(r: EventRow) -> Self {
        let htf: Vec<HtfLevel> =
            serde_json::from_value(r.htf_confluence).unwrap_or_default();
        let outcome_detail: Option<OutcomeDetail> = r
            .outcome_detail
            .and_then(|v| serde_json::from_value(v).ok());

        MarketEvent {
            id: Some(r.id),
            coin: r.coin,
            interval: r.interval,
            event_type: r.event_type,
            lifecycle: r.lifecycle,
            sweep_direction: r.sweep_direction,
            level_price: r.level_price,
            sweep_extreme: r.sweep_extreme,
            wick_pct: r.wick_pct,
            close_price: r.close_price,
            cascade_direction: r.cascade_direction,
            cascade_start_price: r.cascade_start_price,
            liq_count_total: r.liq_count_total,
            candles_sustained: r.candles_sustained,
            volume_acceleration: r.volume_acceleration,
            event_ts_ms: r.event_ts_ms,
            candle_volume: r.candle_volume,
            htf_confluence: htf,
            outcome: r.outcome,
            outcome_detail,
            outcome_resolved_ts: r.outcome_resolved_ts,
            regime: None,
            reclassified_from: r.reclassified_from,
        }
    }
}
