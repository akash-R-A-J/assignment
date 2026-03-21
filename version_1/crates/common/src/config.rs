use std::env;

pub struct KafkaConfig {
    pub brokers: String,
    pub orders_topic: String,
    pub fills_topic: String,
    pub book_updates_topic: String,
}

impl KafkaConfig {
    pub fn from_env() -> Self {
        Self {
            brokers: env::var("KAFKA_BROKERS").unwrap_or_else(|_| "localhost:9092".into()),
            orders_topic: env::var("KAFKA_ORDERS_TOPIC").unwrap_or_else(|_| "orders".into()),
            fills_topic: env::var("KAFKA_FILLS_TOPIC").unwrap_or_else(|_| "fills".into()),
            book_updates_topic: env::var("KAFKA_BOOK_UPDATES_TOPIC")
                .unwrap_or_else(|_| "book_updates".into()),
        }
    }
}

pub struct RedisConfig {
    pub url: String,
}

impl RedisConfig {
    pub fn from_env() -> Self {
        Self {
            url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into()),
        }
    }
}
