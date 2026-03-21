use serde::{Deserialize, Serialize};

use crate::order::Side;

// delta update for a single price level in the orderbook
// when qty is 0 the level has been fully consumed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookUpdate {
    pub market_id: String,
    pub side: Side,
    pub price: u64,
    pub qty: u64, // new total qty at this level
}
