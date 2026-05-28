use crate::detection::types::MarketEvent;
use crate::setups::types::SetupEvent;
use anyhow::Result;

pub trait SetupEngine: Send + Sync {
    fn evaluate_event(&self, event: &MarketEvent) -> Result<Option<SetupEvent>>;
}

#[derive(Debug, Default)]
pub struct EventAdapterSetupEngine;

impl SetupEngine for EventAdapterSetupEngine {
    fn evaluate_event(&self, event: &MarketEvent) -> Result<Option<SetupEvent>> {
        Ok(Some(SetupEvent::from_market_event(event)))
    }
}
