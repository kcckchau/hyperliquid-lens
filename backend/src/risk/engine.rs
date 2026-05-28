use crate::risk::types::{
    ApprovedSignal, RiskContext, RiskDecision, RiskRejectionReason,
};
use anyhow::Result;
use rust_decimal::Decimal;

pub trait RiskEngine: Send + Sync {
    fn evaluate(&self, ctx: &RiskContext) -> Result<RiskDecision>;
}

#[derive(Debug, Default)]
pub struct BasicRiskEngine;

impl RiskEngine for BasicRiskEngine {
    fn evaluate(&self, ctx: &RiskContext) -> Result<RiskDecision> {
        if ctx.portfolio.open_positions >= ctx.limits.max_concurrent_positions {
            return Ok(RiskDecision::Rejected {
                signal: ctx.signal.clone(),
                reason: RiskRejectionReason::PositionLimitReached,
                message: "max concurrent positions reached".to_string(),
            });
        }

        if ctx.portfolio.daily_realized_pnl_usd <= -ctx.limits.max_daily_loss_usd {
            return Ok(RiskDecision::Rejected {
                signal: ctx.signal.clone(),
                reason: RiskRejectionReason::DailyLossLimitReached,
                message: "daily loss limit reached".to_string(),
            });
        }

        let Some(entry_price) = ctx.signal.entry.limit_price else {
            return Ok(RiskDecision::Rejected {
                signal: ctx.signal.clone(),
                reason: RiskRejectionReason::MissingEntryPrice,
                message: "entry plan requires an explicit price for sizing".to_string(),
            });
        };

        let stop_distance = match ctx.signal.side {
            crate::trading::TradeDirection::Long => entry_price - ctx.signal.stop.stop_price,
            crate::trading::TradeDirection::Short => ctx.signal.stop.stop_price - entry_price,
        };

        if stop_distance <= Decimal::ZERO {
            return Ok(RiskDecision::Rejected {
                signal: ctx.signal.clone(),
                reason: RiskRejectionReason::InvalidStopDistance,
                message: "stop must be beyond entry in the loss direction".to_string(),
            });
        }

        let sized_notional = ctx.limits.max_notional_usd.min(
            ctx.signal
                .notional_usd
                .unwrap_or(ctx.limits.max_risk_per_trade_usd * Decimal::new(10, 0)),
        );

        if sized_notional > ctx.limits.max_notional_usd {
            return Ok(RiskDecision::Rejected {
                signal: ctx.signal.clone(),
                reason: RiskRejectionReason::NotionalLimitExceeded,
                message: "signal notional exceeds risk limits".to_string(),
            });
        }

        let sized_quantity = if entry_price > Decimal::ZERO {
            sized_notional / entry_price
        } else {
            Decimal::ZERO
        };

        Ok(RiskDecision::Approved(ApprovedSignal {
            signal: ctx.signal.clone(),
            sized_notional_usd: sized_notional,
            sized_quantity,
        }))
    }
}
