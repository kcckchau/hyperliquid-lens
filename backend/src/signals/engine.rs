use crate::setups::types::SetupEvent;
use crate::signals::types::TradeSignal;
use anyhow::Result;

pub trait SignalEngine: Send + Sync {
    fn generate(&self, setup: &SetupEvent, now_ms: i64) -> Result<Option<TradeSignal>>;
}

#[derive(Debug, Clone)]
pub struct RuleBasedSignalEngine {
    pub ttl_ms: i64,
}

impl Default for RuleBasedSignalEngine {
    fn default() -> Self {
        Self {
            ttl_ms: 15 * 60_000,
        }
    }
}

impl SignalEngine for RuleBasedSignalEngine {
    fn generate(&self, setup: &SetupEvent, now_ms: i64) -> Result<Option<TradeSignal>> {
        Ok(TradeSignal::from_setup(setup, now_ms + self.ttl_ms))
    }
}
