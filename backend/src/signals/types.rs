use crate::setups::types::{SetupEvent, SetupType};
use crate::trading::{OrderType, SignalStatus, TimeInForce, TradeDirection};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPlan {
    pub order_type: OrderType,
    pub limit_price: Option<Decimal>,
    pub time_in_force: TimeInForce,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopPlan {
    pub stop_price: Decimal,
    pub invalidates_setup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetPlan {
    pub price: Decimal,
    pub size_fraction: Decimal,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeSignal {
    pub id: Uuid,
    pub setup_id: Uuid,
    pub source_setup_type: SetupType,
    pub coin: String,
    pub interval: String,
    pub side: TradeDirection,
    pub status: SignalStatus,
    pub confidence: Decimal,
    pub notional_usd: Option<Decimal>,
    pub expires_at_ms: i64,
    pub entry: EntryPlan,
    pub stop: StopPlan,
    pub targets: Vec<TargetPlan>,
    pub rationale: Vec<String>,
}

impl TradeSignal {
    pub fn from_setup(setup: &SetupEvent, expires_at_ms: i64) -> Option<Self> {
        let side = setup.direction_bias?;
        let entry_price = setup.context.invalidation_price.or(setup.context.reference_price)?;
        let stop_price = setup.context.trigger_price.or(setup.context.reference_price)?;
        let risk_per_unit = match side {
            TradeDirection::Long => entry_price - stop_price,
            TradeDirection::Short => stop_price - entry_price,
        };

        if risk_per_unit <= Decimal::ZERO {
            return None;
        }

        let target_one = match side {
            TradeDirection::Long => entry_price + risk_per_unit,
            TradeDirection::Short => entry_price - risk_per_unit,
        };
        let target_two = match side {
            TradeDirection::Long => entry_price + risk_per_unit * Decimal::TWO,
            TradeDirection::Short => entry_price - risk_per_unit * Decimal::TWO,
        };

        Some(Self {
            id: Uuid::new_v4(),
            setup_id: setup.id,
            source_setup_type: setup.setup_type.clone(),
            coin: setup.coin.clone(),
            interval: setup.interval.clone(),
            side,
            status: SignalStatus::Candidate,
            confidence: setup.strength,
            notional_usd: None,
            expires_at_ms,
            entry: EntryPlan {
                order_type: OrderType::Limit,
                limit_price: Some(entry_price),
                time_in_force: TimeInForce::Gtc,
            },
            stop: StopPlan {
                stop_price,
                invalidates_setup: true,
            },
            targets: vec![
                TargetPlan {
                    price: target_one,
                    size_fraction: Decimal::new(5, 1),
                    label: "t1".to_string(),
                },
                TargetPlan {
                    price: target_two,
                    size_fraction: Decimal::new(5, 1),
                    label: "t2".to_string(),
                },
            ],
            rationale: vec![],
        })
    }
}
