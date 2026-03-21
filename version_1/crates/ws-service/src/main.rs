use std::sync::Arc;

use anyhow::Result;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::Message;
use tokio::sync::broadcast;

use common::config::KafkaConfig;

struct AppState {
    tx: broadcast::Sender<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let kafka_config = KafkaConfig::from_env();

    // for fan-out to all connected ws clients
    let (tx, _) = broadcast::channel::<String>(4096);

    // reads fills and forwards to broadcast
    {
        let tx = tx.clone();
        let brokers = kafka_config.brokers.clone();
        let topic = kafka_config.fills_topic.clone();
        
        tokio::spawn(async move {
            if let Err(e) = consume_fills(&brokers, &topic, tx).await {
                eprintln!("fills consumer died: {e}");
            }
        });
    }

    let state = Arc::new(AppState { tx });

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state);

    let port = std::env::var("WS_PORT").unwrap_or_else(|_| "3001".into());
    let addr = format!("0.0.0.0:{port}");
    
    eprintln!("ws service listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}

// consume fill events from kafka and forward them to the broadcast channel
async fn consume_fills(
    brokers: &str,
    topic: &str,
    tx: broadcast::Sender<String>,
) -> Result<()> {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("group.id", "ws-service")
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "true")
        .create()?;

    consumer.subscribe(&[topic])?;
    
    eprintln!("ws consumer subscribed to {topic}");

    let mut stream = consumer.stream();

    while let Some(msg_result) = stream.next().await {
        match msg_result {
            
            Ok(msg) => {
                if let Some(Ok(payload)) = msg.payload_view::<str>() {
                    let _ = tx.send(payload.to_owned());
                }
            }
            
            Err(e) => {
                eprintln!("kafka error: {e}");
            }
        }
    }

    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

// handle a single websocket connection, forward all fill events to the connected client
async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(WsMessage::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // read from client side, mostly just waiting for close
    while let Some(Ok(msg)) = receiver.next().await {
        if matches!(msg, WsMessage::Close(_)) {
            break;
        }
    }

    send_task.abort();
}
