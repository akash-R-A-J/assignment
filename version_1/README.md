# Order Matching Engine

A toy order matching engine for a prediction market. Orders submitted via HTTP are matched by price-time priority. Fills are broadcast over WebSocket in real time.

Supports multiple API server instances running simultaneously.

### Benchmark Results (single core, CPU-pinned Linux)

| Metric | Latency | Throughput |
|--------|---------|------------|
| Order insertion | 338 ns | ~2.96M ops/sec |
| Sweep match (1000 levels) | 380 ns/trade | ~2.63M matches/sec |
| Bulk throughput (10k orders) | 2.02 ms | ~4.94M orders/sec |

Measured with `taskset -c 0 cargo bench -p benchmarks` on a single core, clean environment.

## Architecture

```
                   ┌────────────┐   ┌────────────┐
                   │ API Server │   │ API Server │   (N instances)
                   └─────┬──────┘   └─────┬──────┘
                         │ produce        │
                         └────────┬───────┘
                                  ▼
                        ┌── Kafka ──────────┐
                        │  topic: orders    │
                        └────────┬──────────┘
                                 │ single consumer
                                 ▼
                        ┌────────────────────┐
                        │  Matching Engine   │
                        │  (single-threaded) │
                        └───┬────────────┬───┘
                            │            │
                            ▼            ▼
                    ┌── Kafka ──┐  ┌── Kafka ──────┐
                    │  fills    │  │  book_updates  │
                    └─────┬────┘  └──────┬─────────┘
                          │              │
                          ▼              ▼
                    ┌──────────┐  ┌──────────────┐
                    │ WS Feed  │  │ Cache Updater│
                    └────┬─────┘  └──────┬───────┘
                         │               │
                         ▼               ▼
                    WS clients        Redis HSET
                                   (orderbook cache)
```

All API servers produce to the same Kafka `orders` topic. A single matching engine instance consumes from this topic, guaranteeing total ordering and preventing double-matching. Fills go to the `fills` topic (consumed by WebSocket service), book deltas go to `book_updates` (consumed by cache-updater which writes to Redis). API servers read the orderbook from Redis.

## Quick Start

```bash
# 1. start infra
docker-compose -f docker/docker-compose.yml up -d

# 2. build
cargo build --workspace

# 3. run each service in a separate terminal
cargo run -p matching-engine
cargo run -p api-server
cargo run -p ws-service
cargo run -p cache-updater
```

## API

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/orders` | Submit a new order, returns `order_id` |
| `GET` | `/orderbook/:market_id` | Read current bids and asks |
| `WS` | `ws://host:3001/ws` | Real-time fill events |

### POST /orders

```json
{
  "market_id": "btc-usd",
  "side": "buy",
  "price": 10050,
  "qty": 5
}
```

Response: `{ "order_id": 123456789 }`

Prices are integer ticks (`u64`). No floats anywhere in the system.

## Data Model

```rust
pub enum Side { Buy, Sell }

pub struct Order {
    pub id: u64,
    pub market_id: String,
    pub side: Side,
    pub price: u64,
    pub qty: u64,
    pub timestamp: i64,
}

pub struct Fill {
    pub maker_order_id: u64,
    pub taker_order_id: u64,
    pub price: u64,
    pub qty: u64,
}
```

---

## README Questions

### 1. How does your system handle multiple API server instances without double-matching?

All API servers produce orders to a single Kafka topic (`orders`). A single matching engine instance consumes from this topic as the sole consumer in its consumer group. Because Kafka guarantees total ordering within a partition and there is exactly one consumer, every order is processed exactly once, in order. There is no possibility of double-matching regardless of how many API server instances are running.

Order IDs are generated using an `AtomicU64` counter seeded with the system clock, giving each API instance a non-overlapping ID space.


### 2. What data structure did you use for the order book and why?

`BTreeMap<u64, VecDeque<Order>>` — one for bids, one for asks.

- **BTreeMap** gives O(log N) lookup and insertion by price level, and the keys are always sorted. Getting the best bid (highest) or best ask (lowest) is O(1) via `keys().next_back()` / `keys().next()`.
- **VecDeque** at each price level maintains FIFO time priority. New orders push to the back, matching takes from the front.
- A `HashMap<u64, (Side, u64)>` index maps `order_id → (side, price)` for O(1) cancel lookups.

This is the common approach used by most exchange implementations. It's simple, correct, and cache-friendly because BTreeMap nodes are contiguous.

### 3. What breaks first under real production load?

1. **Single Kafka partition** — the `orders` topic has one partition, so there's one consumer thread. At roughly 100k+ orders/sec, this becomes the bottleneck. Fix: partition by `market_id`, run one matcher per partition.

2. **VecDeque cancel cost** — cancelling an order in a VecDeque is O(N) in the number of orders at that price level (linear scan + shift). Under heavy cancel load (common in HFT), this becomes a problem. Fix: replace VecDeque with a doubly-linked list or arena allocator for O(1) removal.

3. **Redis write contention** — the cache-updater writes every book delta to Redis. Under extreme throughput, it'll lag behind the matching engine. This is acceptable since the cache is eventually consistent, but latency-sensitive reads would suffer.

### 4. What would you build next with another 4 hours?

- **Order acknowledgment via Kafka** — API server subscribes to an ack topic so it can return the match result synchronously instead of fire-and-forget. Each server would have a unique ID and a HashMap of pending futures, resolving them when acks arrive.
- **Rate limiting** on the API server (per-IP or per-API-key).
- **Periodic orderbook snapshots** — instead of relying on full Kafka replay for recovery, snapshot the book to Redis every N seconds for fast cold-start.

## Project Structure

```
├── Cargo.toml                 workspace root
├── crates/
│   ├── common/                shared types
│   │   └── src/
│   │       ├── order.rs       Order, Fill, Side
│   │       ├── events.rs      BookUpdate
│   │       └── config.rs      KafkaConfig, RedisConfig
│   ├── matching-engine/       core matcher
│   │   └── src/
│   │       ├── orderbook.rs   BTreeMap order book
│   │       ├── matcher.rs     price-time priority matching
│   │       ├── kafka.rs       consumer/producer loop
│   │       └── main.rs
│   ├── api-server/            REST API (Axum)
│   ├── ws-service/            WebSocket feed
│   └── cache-updater/         Redis orderbook cache
├── docker/
│   └── docker-compose.yml     Kafka, Redis
└── benches/
    └── matching_benchmark.rs  Criterion benchmarks
```

## Running Tests

```bash
cargo test --workspace
```

## Environment Variables

| Variable | Default |
|----------|---------|
| `KAFKA_BROKERS` | `localhost:9092` |
| `REDIS_URL` | `redis://127.0.0.1:6379` |
| `API_PORT` | `3000` |
| `WS_PORT` | `3001` |
