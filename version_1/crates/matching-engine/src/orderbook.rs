use std::collections::{BTreeMap, VecDeque};
use rustc_hash::FxHashMap;

use common::order::{Order, Side};

// in-memory order book using BTreeMap for sorted price levels
// bids: highest price first (reverse iter)
// asks: lowest price first (forward iter)
pub struct OrderBook {
    pub bids: BTreeMap<u64, VecDeque<Order>>,
    pub asks: BTreeMap<u64, VecDeque<Order>>,
    orders_index: FxHashMap<u64, (Side, u64)>, // index for cancel lookup: order_id -> (side, price)
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            orders_index: FxHashMap::default(),
        }
    }

    // insert a resting order at its price level
    pub fn add_order(&mut self, order: Order) {
        let side = order.side;
        let price = order.price;
        let id = order.id;

        let book_side = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        book_side.entry(price).or_default().push_back(order);
        self.orders_index.insert(id, (side, price));
    }

    // cancel an order by id, returns the removed order if found
    pub fn cancel_order(&mut self, order_id: u64) -> Option<Order> {
        let (side, price) = self.orders_index.remove(&order_id)?;

        let book_side = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        if let Some(level) = book_side.get_mut(&price) {
            if let Some(pos) = level.iter().position(|o| o.id == order_id) {
                
                let order = level.remove(pos).unwrap();
                if level.is_empty() {
                    book_side.remove(&price);
                }
                
                return Some(order);
            }
        }

        None
    }

    pub fn best_bid(&self) -> Option<u64> {
        self.bids.keys().next_back().copied()
    }

    pub fn best_ask(&self) -> Option<u64> {
        self.asks.keys().next().copied()
    }

    // total qty resting at a given price level
    pub fn qty_at_level(&self, side: Side, price: u64) -> u64 {
        let book_side = match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };
        
        book_side
            .get(&price)
            .map(|level| level.iter().map(|o| o.qty).sum())
            .unwrap_or(0)
    }

    // take qty from the front of a price level (oldest orders first) and return vec of (maker_order, filled_qty) pair
    pub fn take_from_top(&mut self, side: Side, price: u64, mut qty: u64) -> Vec<(Order, u64)> {
        let book_side = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        let mut fills: Vec<(Order, u64)> = Vec::new();

        if let Some(level) = book_side.get_mut(&price) {
            while qty > 0 && !level.is_empty() {
                
                let front = level.front_mut().unwrap();
                let fill_qty = qty.min(front.qty);
                let maker_snapshot = front.clone();

                front.qty -= fill_qty;
                qty -= fill_qty;

                if front.qty == 0 {
                    level.pop_front();
                    self.orders_index.remove(&maker_snapshot.id);
                }

                fills.push((maker_snapshot, fill_qty));
            }

            if level.is_empty() {
                book_side.remove(&price);
            }
        }

        fills
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

// added some tests for the orderbook
#[cfg(test)]
mod tests {
    use super::*;

    fn make_order(id: u64, side: Side, price: u64, qty: u64) -> Order {
        Order {
            id,
            market_id: "test".into(),
            side,
            price,
            qty,
            timestamp: 1_000_000,
        }
    }

    #[test]
    fn test_add_and_best_prices() {
        let mut book = OrderBook::new();

        book.add_order(make_order(1, Side::Buy, 100, 10));
        book.add_order(make_order(2, Side::Buy, 105, 5));
        book.add_order(make_order(3, Side::Sell, 110, 8));
        book.add_order(make_order(4, Side::Sell, 115, 3));

        assert_eq!(book.best_bid(), Some(105));
        assert_eq!(book.best_ask(), Some(110));
    }

    #[test]
    fn test_cancel_order() {
        let mut book = OrderBook::new();
        let order = make_order(1, Side::Buy, 100, 10);

        book.add_order(order);
        assert_eq!(book.best_bid(), Some(100));

        let cancelled = book.cancel_order(1);
        assert!(cancelled.is_some());
        assert_eq!(book.best_bid(), None);
    }

    #[test]
    fn test_cancel_unknown_order() {
        let mut book = OrderBook::new();
        let result = book.cancel_order(999);
        assert!(result.is_none());
    }

    #[test]
    fn test_qty_at_level() {
        let mut book = OrderBook::new();
        book.add_order(make_order(1, Side::Sell, 200, 10));
        book.add_order(make_order(2, Side::Sell, 200, 7));

        assert_eq!(book.qty_at_level(Side::Sell, 200), 17);
        assert_eq!(book.qty_at_level(Side::Sell, 201), 0);
    }

    #[test]
    fn test_take_from_top_partial() {
        let mut book = OrderBook::new();
        book.add_order(make_order(1, Side::Sell, 200, 10));

        let fills = book.take_from_top(Side::Sell, 200, 4);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].1, 4);
        assert_eq!(book.qty_at_level(Side::Sell, 200), 6);
    }

    #[test]
    fn test_take_from_top_multi_order() {
        let mut book = OrderBook::new();
        book.add_order(make_order(1, Side::Sell, 200, 3));
        book.add_order(make_order(2, Side::Sell, 200, 5));

        let fills = book.take_from_top(Side::Sell, 200, 6);
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].1, 3); // first order fully filled
        assert_eq!(fills[1].1, 3); // second partially
        assert_eq!(book.qty_at_level(Side::Sell, 200), 2);
    }

    #[test]
    fn test_fifo_priority() {
        let mut book = OrderBook::new();
        book.add_order(make_order(1, Side::Buy, 100, 5));
        book.add_order(make_order(2, Side::Buy, 100, 5));

        let fills = book.take_from_top(Side::Buy, 100, 3);
        assert_eq!(fills[0].0.id, 1); // fifo
    }
}
