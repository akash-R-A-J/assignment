use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

// core order type, prices are integer ticks to avoid floating point issues
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: u64,
    pub market_id: String,
    pub side: Side,
    pub price: u64, // tick price i.e. 10050 = $100.50
    pub qty: u64,
    pub timestamp: i64,
}

// a fill represents a single trade between a maker and taker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub maker_order_id: u64,
    pub taker_order_id: u64,
    pub price: u64,
    pub qty: u64,
}

// incoming request body for POST /orders
#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest {
    pub market_id: String,
    pub side: Side,
    pub price: u64,
    pub qty: u64,
}
