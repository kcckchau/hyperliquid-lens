use crate::risk::types::ApprovedSignal;
use crate::trading::{OrderIntent, OrderType, TimeInForce, TradeDirection};
use anyhow::Result;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub id: Uuid,
    pub signal_id: Uuid,
    pub coin: String,
    pub side: TradeDirection,
    pub intent: OrderIntent,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub quantity: Decimal,
    pub limit_price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub reduce_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerOrderStatus {
    Accepted,
    Rejected,
    Filled,
    PartiallyFilled,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderAck {
    pub broker_order_id: String,
    pub client_order_id: Uuid,
    pub status: BrokerOrderStatus,
    pub message: Option<String>,
}

pub trait Broker: Send + Sync {
    fn submit_order(&self, request: &OrderRequest) -> Result<OrderAck>;
    fn cancel_order(&self, broker_order_id: &str) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct PaperBroker;

impl PaperBroker {
    pub fn entry_request(signal: &ApprovedSignal) -> OrderRequest {
        OrderRequest {
            id: Uuid::new_v4(),
            signal_id: signal.signal.id,
            coin: signal.signal.coin.clone(),
            side: signal.signal.side,
            intent: OrderIntent::Entry,
            order_type: signal.signal.entry.order_type,
            time_in_force: signal.signal.entry.time_in_force,
            quantity: signal.sized_quantity,
            limit_price: signal.signal.entry.limit_price,
            stop_price: None,
            reduce_only: false,
        }
    }
}

impl Broker for PaperBroker {
    fn submit_order(&self, request: &OrderRequest) -> Result<OrderAck> {
        Ok(OrderAck {
            broker_order_id: format!("paper-{}", request.id),
            client_order_id: request.id,
            status: BrokerOrderStatus::Accepted,
            message: Some("paper broker accepted order".to_string()),
        })
    }

    fn cancel_order(&self, _broker_order_id: &str) -> Result<()> {
        Ok(())
    }
}
