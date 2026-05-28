use crate::detection::types::{EventLifecycle, EventType, MarketEvent};
use crate::trading::TradeDirection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupType {
    LiquiditySweep,
    LiquidationCascade,
    FailedReclaim,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    Detected,
    Confirming,
    Confirmed,
    Invalidated,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupContext {
    pub event_id: Option<i64>,
    pub reference_price: Option<Decimal>,
    pub trigger_price: Option<Decimal>,
    pub invalidation_price: Option<Decimal>,
    pub htf_confluence_count: usize,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupEvent {
    pub id: Uuid,
    pub coin: String,
    pub interval: String,
    pub setup_type: SetupType,
    pub status: SetupStatus,
    pub detected_at_ms: i64,
    pub direction_bias: Option<TradeDirection>,
    pub strength: Decimal,
    pub context: SetupContext,
}

impl SetupEvent {
    pub fn from_market_event(event: &MarketEvent) -> Self {
        let (setup_type, direction_bias, reference_price, trigger_price, invalidation_price) =
            match event.event_type {
                EventType::LiquiditySweep => (
                    SetupType::LiquiditySweep,
                    event.sweep_direction.as_ref().map(|direction| match direction {
                        crate::detection::types::SweepDirection::Bullish => TradeDirection::Long,
                        crate::detection::types::SweepDirection::Bearish => TradeDirection::Short,
                    }),
                    event.level_price,
                    event.sweep_extreme,
                    event.close_price,
                ),
                EventType::LiquidationCascade => (
                    SetupType::LiquidationCascade,
                    event.cascade_direction.as_ref().map(|direction| match direction {
                        crate::detection::types::CascadeDirection::LongLiq => TradeDirection::Short,
                        crate::detection::types::CascadeDirection::ShortLiq => TradeDirection::Long,
                    }),
                    event.cascade_start_price,
                    event.close_price.or(event.cascade_start_price),
                    None,
                ),
            };

        Self {
            id: Uuid::new_v4(),
            coin: event.coin.clone(),
            interval: event.interval.clone(),
            setup_type,
            status: map_lifecycle(&event.lifecycle),
            detected_at_ms: event.event_ts_ms,
            direction_bias,
            strength: event
                .wick_pct
                .or(event.volume_acceleration)
                .unwrap_or(Decimal::ONE),
            context: SetupContext {
                event_id: event.id,
                reference_price,
                trigger_price,
                invalidation_price,
                htf_confluence_count: event.htf_confluence.len(),
                notes: vec![],
            },
        }
    }
}

fn map_lifecycle(lifecycle: &EventLifecycle) -> SetupStatus {
    match lifecycle {
        EventLifecycle::Detected => SetupStatus::Detected,
        EventLifecycle::Confirming => SetupStatus::Confirming,
        EventLifecycle::Confirmed => SetupStatus::Confirmed,
        EventLifecycle::Reclassified => SetupStatus::Invalidated,
        EventLifecycle::Expired => SetupStatus::Expired,
    }
}
