use anyhow::Result;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::Message;
use redis::AsyncCommands;
use tokio_stream::StreamExt;

use common::config::{KafkaConfig, RedisConfig};
use common::events::BookUpdate;

// cache updater service
// consumes from book_updates topic, will write orderbook state to redis HSET
// api servers read from redis for GET /orderbook request
#[tokio::main]
async fn main() -> Result<()> {
    let kafka_config = KafkaConfig::from_env();
    let redis_config = RedisConfig::from_env();

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &kafka_config.brokers)
        .set("group.id", "cache-updater")
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "true")
        .create()?;

    consumer.subscribe(&[&kafka_config.book_updates_topic])?;
    
    eprintln!(
        "cache updater subscribed to {}",
        kafka_config.book_updates_topic
    );

    let redis_client = redis::Client::open(redis_config.url.as_str())?;
    let mut conn = redis_client.get_multiplexed_async_connection().await?;
    
    eprintln!("connected to redis");

    let mut stream = consumer.stream();

    while let Some(msg_result) = stream.next().await {
        match msg_result {
           
            Ok(msg) => {
                
                let payload = match msg.payload_view::<str>() {
                    Some(Ok(s)) => s,
                    _ => continue,
                };

                match serde_json::from_str::<BookUpdate>(payload) {
                    Ok(update) => {
                        
                        let side_str = match update.side {
                            common::order::Side::Buy => "bids",
                            common::order::Side::Sell => "asks",
                        };
                        
                        let key = format!("orderbook:{}:{}", update.market_id, side_str);
                        let price_str = update.price.to_string();

                        if update.qty == 0 {
                            let _: redis::RedisResult<()> =
                                conn.hdel(&key, &price_str).await; // empty level will get removed from redis
                        } else {
                            let _: redis::RedisResult<()> = conn
                                .hset(&key, &price_str, update.qty.to_string())
                                .await;
                        }
                    }
                    
                    Err(e) => {
                        eprintln!("bad book update: {e}");
                    }
                }
            }
            
            Err(e) => {
                eprintln!("kafka error: {e}");
            }
        }
    }

    Ok(())
}
