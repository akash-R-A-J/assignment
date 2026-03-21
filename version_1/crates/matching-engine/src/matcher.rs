use common::events::BookUpdate;
use common::order::{Fill, Order, Side};

use crate::orderbook::OrderBook;

pub struct MatchResult {
    pub fills: Vec<Fill>,
    pub book_updates: Vec<BookUpdate>,
}

// match the incoming order against the book, uses price-time priority
// buys match against lowest ask, sells against highest bid, any leftover qty rests on the book
pub fn match_order(book: &mut OrderBook, mut order: Order) -> MatchResult {
    let mut fills: Vec<Fill> = Vec::new();
    let mut book_updates: Vec<BookUpdate> = Vec::new();

    match order.side {
        Side::Buy => {
            
            while order.qty > 0 {
                let best_ask = match book.best_ask() {
                    Some(p) if p <= order.price => p,
                    _ => break,
                };

                let matched = book.take_from_top(Side::Sell, best_ask, order.qty);
                
                for (maker, fill_qty) in &matched {
                    order.qty -= fill_qty;

                    fills.push(Fill {
                        maker_order_id: maker.id,
                        taker_order_id: order.id,
                        price: best_ask,
                        qty: *fill_qty,
                    });
                }

                let remaining = book.qty_at_level(Side::Sell, best_ask);
                
                book_updates.push(BookUpdate {
                    market_id: order.market_id.clone(),
                    side: Side::Sell,
                    price: best_ask,
                    qty: remaining,
                });
            }
        }
        
        Side::Sell => {
            while order.qty > 0 {
                
                let best_bid = match book.best_bid() {
                    Some(p) if p >= order.price => p,
                    _ => break,
                };

                let matched = book.take_from_top(Side::Buy, best_bid, order.qty);
                
                for (maker, fill_qty) in &matched {
                    order.qty -= fill_qty;

                    fills.push(Fill {
                        maker_order_id: maker.id,
                        taker_order_id: order.id,
                        price: best_bid,
                        qty: *fill_qty,
                    });
                }

                let remaining = book.qty_at_level(Side::Buy, best_bid);
                
                book_updates.push(BookUpdate {
                    market_id: order.market_id.clone(),
                    side: Side::Buy,
                    price: best_bid,
                    qty: remaining,
                });
            }
        }
    }

    // remaining qty rests on the book
    if order.qty > 0 {
        let resting_side = order.side;
        let resting_price = order.price;

        book.add_order(order.clone());

        let new_level_qty = book.qty_at_level(resting_side, resting_price);
        
        book_updates.push(BookUpdate {
            market_id: order.market_id.clone(),
            side: resting_side,
            price: resting_price,
            qty: new_level_qty,
        });
    }

    MatchResult { fills, book_updates }
}

// added some tests to test the engine
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
            timestamp: chrono::Utc::now().timestamp_micros(),
        }
    }

    #[test]
    fn test_no_match_rests_on_book() {
        let mut book = OrderBook::new();
        let order = make_order(1, Side::Buy, 100, 10);
        let result = match_order(&mut book, order);

        assert!(result.fills.is_empty());
        assert_eq!(book.best_bid(), Some(100));
    }

    #[test]
    fn test_full_match() {
        let mut book = OrderBook::new();

        let sell = make_order(1, Side::Sell, 100, 10);
        
        match_order(&mut book, sell);

        let buy = make_order(2, Side::Buy, 100, 10);
        let result = match_order(&mut book, buy);

        assert_eq!(result.fills.len(), 1);
        assert_eq!(result.fills[0].qty, 10);
        assert_eq!(result.fills[0].price, 100);
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
    }

    #[test]
    fn test_partial_fill() {
        let mut book = OrderBook::new();

        let sell = make_order(1, Side::Sell, 100, 5);
        
        match_order(&mut book, sell);

        let buy = make_order(2, Side::Buy, 100, 8);
        let result = match_order(&mut book, buy);

        assert_eq!(result.fills.len(), 1);
        assert_eq!(result.fills[0].qty, 5); // remaining 3 rests as a bid
        assert_eq!(book.best_bid(), Some(100));
        assert_eq!(book.best_ask(), None);
    }

    #[test]
    fn test_price_improvement() {
        let mut book = OrderBook::new();

        let sell = make_order(1, Side::Sell, 95, 5);
        
        match_order(&mut book, sell);

        // buy at 100 should match at 95 (makers price)
        let buy = make_order(2, Side::Buy, 100, 5);
        let result = match_order(&mut book, buy);

        assert_eq!(result.fills.len(), 1);
        assert_eq!(result.fills[0].price, 95);
    }

    #[test]
    fn test_multi_level_match() {
        let mut book = OrderBook::new();

        let s1 = make_order(1, Side::Sell, 100, 3);
        let s2 = make_order(2, Side::Sell, 101, 4);
        
        match_order(&mut book, s1);
        match_order(&mut book, s2);

        // buy at 101 for qty 6 sweeps [100 (3) + 101 (3 of 4)]
        let buy = make_order(3, Side::Buy, 101, 6);
        let result = match_order(&mut book, buy);

        assert_eq!(result.fills.len(), 2);
        assert_eq!(result.fills[0].price, 100);
        assert_eq!(result.fills[0].qty, 3);
        assert_eq!(result.fills[1].price, 101);
        assert_eq!(result.fills[1].qty, 3);
        assert_eq!(book.best_ask(), Some(101));
        assert_eq!(book.qty_at_level(Side::Sell, 101), 1);
    }

    #[test]
    fn test_book_updates_emitted() {
        let mut book = OrderBook::new();

        let sell = make_order(1, Side::Sell, 100, 10);
        
        match_order(&mut book, sell);

        let buy = make_order(2, Side::Buy, 100, 4);
        let result = match_order(&mut book, buy);

        let update = result
            .book_updates
            .iter()
            .find(|u| u.side == Side::Sell && u.price == 100)
            .expect("should have book update for ask 100");
        assert_eq!(update.qty, 6);
    }
}
