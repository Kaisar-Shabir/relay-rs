use async_trait::async_trait;
use super::{DeliverySink};
use std::collections::HashMap;
use anyhow::Result;
use tracing::info;

pub struct StdoutSink;

impl StdoutSink {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DeliverySink for StdoutSink {
    async fn deliver(&self, payload: &str, metadata: &Option<HashMap<String, String>>) -> Result<(), anyhow::Error> {
        println!("=== STDOUT SINK DELIVERY ===");
        println!("Timestamp: {}", chrono::Utc::now());
        println!("Payload: {}", payload);
        if let Some(meta) = metadata {
            println!("Metadata: {:?}", meta);
        }
        println!("===============================");
        
        info!("Payload delivered to stdout sink");
        Ok(())
    }

    fn sink_type(&self) -> &'static str {
        "stdout"
    }
}
