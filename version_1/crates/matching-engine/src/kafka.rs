use anyhow::Result;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::Message;
use std::time::Duration;
use tokio_stream::StreamExt;

use common::config::KafkaConfig;
use common::order::Order;

use crate::matcher::{match_order, MatchResult};
use crate::orderbook::OrderBook;

// runs the single-threaded matching loop
// consumes orders from kafka, matches them, publishes fills and book updates
pub async fn run_matching_loop(config: &KafkaConfig) -> Result<()> {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &config.brokers)
        .set("group.id", "matching-engine")
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "true")
        .create()?;

    // consumer subscribes to the order topic
    consumer.subscribe(&[&config.orders_topic])?;

    eprintln!("matching engine subscribed to: {}", config.orders_topic);

    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &config.brokers)
        .set("message.timeout.ms", "5000")
        .create()?;

    let mut book = OrderBook::new(); // in-memory orderbook
    let mut stream = consumer.stream();

    while let Some(msg_result) = stream.next().await {
        match msg_result {
            Ok(msg) => {
                let payload = match msg.payload_view::<str>() {
                    Some(Ok(s)) => s,
                    _ => continue,
                };

                let order: Order = match serde_json::from_str(payload) {
                    Ok(o) => o,
                    Err(e) => {
                        eprintln!("invalid payload: {e}");
                        continue;
                    }
                };

                let result = match_order(&mut book, order);

                publish_results(&producer, config, result).await;
            }

            Err(e) => {
                eprintln!("consumer error: {e}");
            }
        }
    }

    Ok(())
}

// can be parallelize
async fn publish_results(producer: &FutureProducer, config: &KafkaConfig, result: MatchResult) {
    for fill in &result.fills {
        if let Ok(payload) = serde_json::to_string(fill) {
            let record = FutureRecord::to(&config.fills_topic)
                .payload(&payload)
                .key("");

            println!("record (in engine publish_result): {:#?}", record);
            println!("payload (in engine publish result): {payload:#?}");

            if let Err((e, _)) = producer.send(record, Duration::from_secs(5)).await {
                eprintln!("failed to publsh fill: {e}");
            }
        }
    }

    for update in &result.book_updates {
        if let Ok(payload) = serde_json::to_string(update) {
            let record = FutureRecord::to(&config.book_updates_topic)
                .payload(&payload)
                .key("");

            if let Err((e, _)) = producer.send(record, Duration::from_secs(5)).await {
                eprintln!("failed to publish book update: {e}");
            }
        }
    }
}
