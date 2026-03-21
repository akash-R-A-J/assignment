use anyhow::Result;
use common::config::KafkaConfig;

#[tokio::main]
async fn main() -> Result<()> {
    let config = KafkaConfig::from_env();
    
    eprintln!("starting matching engine");

    // single consumer, single thread - deterministic ordering guaranteed
    matching_engine::kafka::run_matching_loop(&config).await
}
