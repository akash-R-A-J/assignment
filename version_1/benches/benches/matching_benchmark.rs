use criterion::{black_box, criterion_group, criterion_main, Criterion};
use matching_engine::{matcher::match_order, orderbook::OrderBook};
use common::order::{Order, Side};

fn make_order(id: u64, side: Side, price: u64, qty: u64) -> Order {
    Order {
        id,
        market_id: "bench".into(),
        side,
        price,
        qty,
        timestamp: 0,
    }
}

fn bench_matching(c: &mut Criterion) {
    c.bench_function("match_1000_orders", |b| {
        b.iter(|| {
            let mut book = OrderBook::new();

            for i in 0..1000u64 {
                let order = make_order(i, Side::Sell, 10000 + i, 10);
                match_order(&mut book, order);
            }

            let sweep = make_order(9999, Side::Buy, 11000, 10_000);
            let result = match_order(black_box(&mut book), sweep);
            assert_eq!(result.fills.len(), 1000);
        })
    });
}

fn bench_single_insert(c: &mut Criterion) {
    c.bench_function("single_order_insert", |b| {
        let mut book = OrderBook::new();
        let mut id = 0u64;
        b.iter(|| {
            id += 1;
            let order = make_order(id, Side::Buy, 10000 + id, 10);
            match_order(black_box(&mut book), order);
        })
    });
}

fn bench_throughput(c: &mut Criterion) {
    c.bench_function("throughput_10k_orders", |b| {
        b.iter(|| {
            let mut book = OrderBook::new();
            for i in 0..10000u64 {
                let order = make_order(i, Side::Buy, 10000 + (i % 100), 1);
                match_order(black_box(&mut book), order);
            }
        })
    });
}

criterion_group!(benches, bench_matching, bench_single_insert, bench_throughput);
criterion_main!(benches);
