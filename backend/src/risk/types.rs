use crate::signals::types::TradeSignal;
use crate::trading::{MarketSnapshot, PositionSide};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskLimits {
    pub max_notional_usd: Decimal,
    pub max_risk_per_trade_usd: Decimal,
    pub max_concurrent_positions: usize,
    pub max_daily_loss_usd: Decimal,
    pub max_slippage_bps: Decimal,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_notional_usd: Decimal::new(10_000, 0),
            max_risk_per_trade_usd: Decimal::new(100, 0),
            max_concurrent_positions: 3,
            max_daily_loss_usd: Decimal::new(300, 0),
            max_slippage_bps: Decimal::new(25, 0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioState {
    pub open_positions: usize,
    pub daily_realized_pnl_usd: Decimal,
    pub coin_position_side: PositionSide,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskContext {
    pub signal: TradeSignal,
    pub market: MarketSnapshot,
    pub portfolio: PortfolioState,
    pub limits: RiskLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskRejectionReason {
    MissingEntryPrice,
    InvalidStopDistance,
    PositionLimitReached,
    DailyLossLimitReached,
    NotionalLimitExceeded,
    SlippageTooHigh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovedSignal {
    pub signal: TradeSignal,
    pub sized_notional_usd: Decimal,
    pub sized_quantity: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskDecision {
    Approved(ApprovedSignal),
    Rejected {
        signal: TradeSignal,
        reason: RiskRejectionReason,
        message: String,
    },
}
