use criterion::{black_box, criterion_group, criterion_main, Criterion};
use matching_engine::{matcher::match_order, orderbook::OrderBook};
use common::order::{Order, Side};
use rustc_hash::FxHashMap;

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

// ─── EXISTING BENCHMARKS (baseline) ──────────────────────────

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

// ─── BOTTLENECK ISOLATION BENCHMARKS ─────────────────────────

// measures just the String clone cost (market_id)
fn bench_order_clone(c: &mut Criterion) {
    let order = make_order(1, Side::Buy, 10000, 10);
    c.bench_function("order_clone_cost", |b| {
        b.iter(|| {
            let _ = black_box(order.clone());
        })
    });
}

// measures HashMap insert + SipHash cost (the order index)
fn bench_hashmap_insert(c: &mut Criterion) {
    c.bench_function("hashmap_u64_insert_10k", |b| {
        b.iter(|| {
            let mut map: FxHashMap<u64, (Side, u64)> = FxHashMap::default();
            for i in 0..10_000u64 {
                map.insert(i, (Side::Buy, 10000 + i));
            }
            black_box(&map);
        })
    });
}

// measures Vec<Fill> allocation cost
fn bench_vec_fill_alloc(c: &mut Criterion) {
    use common::order::Fill;
    c.bench_function("vec_fill_alloc_100", |b| {
        b.iter(|| {
            let mut fills = Vec::new();
            for i in 0..100u64 {
                fills.push(Fill {
                    maker_order_id: i,
                    taker_order_id: i + 1,
                    price: 10000,
                    qty: 10,
                });
            }
            black_box(&fills);
        })
    });
}

// measures serde_json serialization (kafka hot path)
fn bench_json_serialize(c: &mut Criterion) {
    let order = make_order(1, Side::Buy, 10000, 10);
    c.bench_function("json_serialize_order", |b| {
        b.iter(|| {
            let s = serde_json::to_string(black_box(&order)).unwrap();
            black_box(s);
        })
    });
}

fn bench_json_deserialize(c: &mut Criterion) {
    let json = r#"{"id":1,"market_id":"bench","side":"buy","price":10000,"qty":10,"timestamp":0}"#;
    c.bench_function("json_deserialize_order", |b| {
        b.iter(|| {
            let o: Order = serde_json::from_str(black_box(json)).unwrap();
            black_box(o);
        })
    });
}

// measures BTreeMap operations in isolation
fn bench_btreemap_insert_remove(c: &mut Criterion) {
    use std::collections::BTreeMap;
    c.bench_function("btreemap_insert_remove_1k", |b| {
        b.iter(|| {
            let mut tree: BTreeMap<u64, u64> = BTreeMap::new();
            for i in 0..1000u64 {
                tree.insert(i, i);
            }
            for i in 0..1000u64 {
                tree.remove(&i);
            }
            black_box(&tree);
        })
    });
}

criterion_group!(
    benches,
    // baseline
    bench_matching,
    bench_single_insert,
    bench_throughput,
    // bottleneck isolation
    bench_order_clone,
    bench_hashmap_insert,
    bench_vec_fill_alloc,
    bench_json_serialize,
    bench_json_deserialize,
    bench_btreemap_insert_remove,
);
criterion_main!(benches);
