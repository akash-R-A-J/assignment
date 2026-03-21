use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use redis::AsyncCommands;

use common::config::{KafkaConfig, RedisConfig};
use common::order::{CreateOrderRequest, Order};

struct AppState {
    kafka_producer: FutureProducer,
    kafka_config: KafkaConfig,
    redis_client: redis::Client,
    id_counter: AtomicU64,
}

#[tokio::main]
async fn main() -> Result<()> {
    
    let kafka_config = KafkaConfig::from_env();
    let redis_config = RedisConfig::from_env();

    let kafka_producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &kafka_config.brokers)
        .set("message.timeout.ms", "5000")
        .create()?;

    let redis_client = redis::Client::open(redis_config.url.as_str())?;

    // seed the counter with current time in micros so multiple api server instance get non-overlapping id range
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;

    let state = Arc::new(AppState {
        kafka_producer,
        kafka_config,
        redis_client,
        id_counter: AtomicU64::new(seed),
    });

    let app = Router::new()
        .route("/orders", post(create_order))
        .route("/orderbook/:market_id", get(get_orderbook))
        .with_state(state);

    let port = std::env::var("API_PORT").unwrap_or_else(|_| "3000".into());
    let addr = format!("0.0.0.0:{port}");
    
    eprintln!("api server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}

// POST /orders : submit a new order and return the assigned id
async fn create_order(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateOrderRequest>,
) -> impl IntoResponse {
    // edge case
    if req.qty == 0 {
        return (StatusCode::BAD_REQUEST, "qty must be greater than 0").into_response();
    }
    
    if req.price == 0 {
        return (StatusCode::BAD_REQUEST, "price must be greater than 0").into_response();
    }

    let id = state.id_counter.fetch_add(1, Ordering::Relaxed);

    let order = Order {
        id,
        market_id: req.market_id,
        side: req.side,
        price: req.price,
        qty: req.qty,
        timestamp: chrono::Utc::now().timestamp_micros(),
    };

    let payload = match serde_json::to_string(&order) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "serialization error").into_response();
        }
    };

    let record = FutureRecord::to(&state.kafka_config.orders_topic)
        .payload(&payload)
        .key(&order.market_id);

    match state
        .kafka_producer
        .send(record, Duration::from_secs(5))
        .await
    {
        Ok(_) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "order_id": id })),
        )
            .into_response(),
            
        Err((e, _)) => {
            eprintln!("kafka send failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to submit order").into_response()
        }
    }
}

// GET /orderbook/:market_id — read cached orderbook from redis
async fn get_orderbook(
    State(state): State<Arc<AppState>>,
    Path(market_id): Path<String>,
) -> impl IntoResponse {
    
    let conn = state.redis_client.get_multiplexed_async_connection().await;
    
    let mut conn = match conn {
        Ok(c) => c,
        Err(_) => {
            return (StatusCode::SERVICE_UNAVAILABLE, "redis unavailable").into_response();
        }
    };

    let bids_key = format!("orderbook:{market_id}:bids");
    let asks_key = format!("orderbook:{market_id}:asks");

    let bids: std::collections::HashMap<String, String> =
        conn.hgetall(&bids_key).await.unwrap_or_default();
    
    let asks: std::collections::HashMap<String, String> =
        conn.hgetall(&asks_key).await.unwrap_or_default();

    let mut bids_vec: Vec<(u64, u64)> = bids
        .iter()
        .filter_map(|(k, v)| Some((k.parse().ok()?, v.parse().ok()?)))
        .collect();
    
    let mut asks_vec: Vec<(u64, u64)> = asks
        .iter()
        .filter_map(|(k, v)| Some((k.parse().ok()?, v.parse().ok()?)))
        .collect();

    bids_vec.sort_by(|a, b| b.0.cmp(&a.0)); // highest first
    asks_vec.sort_by(|a, b| a.0.cmp(&b.0)); // lowest first

    Json(serde_json::json!({
        "market_id": market_id,
        "bids": bids_vec.iter().map(|(p, q)| serde_json::json!({"price": p, "qty": q})).collect::<Vec<_>>(),
        "asks": asks_vec.iter().map(|(p, q)| serde_json::json!({"price": p, "qty": q})).collect::<Vec<_>>(),
    }))
    .into_response()
}
