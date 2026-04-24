use async_trait::async_trait;
use std::collections::HashMap;
use anyhow::Result;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SinkType {
    Http,
    Kafka,
    Stdout,
}

#[async_trait]
pub trait DeliverySink: Send + Sync {
    async fn deliver(&self, payload: &str, metadata: &Option<HashMap<String, String>>) -> Result<(), anyhow::Error>;
    #[allow(dead_code)]
    fn sink_type(&self) -> &'static str;
}

pub mod http;
pub mod kafka;
pub mod stdout;

pub use http::HttpSink;
pub use kafka::KafkaSink;
pub use stdout::StdoutSink;
