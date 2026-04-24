use async_trait::async_trait;
use super::{DeliverySink};
use std::collections::HashMap;
use anyhow::Result;
use tracing::{info, error};

pub struct HttpSink {
    client: reqwest::Client,
    remote_url: String,
}

impl HttpSink {
    pub fn new(remote_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            remote_url,
        }
    }
}

#[async_trait]
impl DeliverySink for HttpSink {
    async fn deliver(&self, payload: &str, _metadata: &Option<HashMap<String, String>>) -> Result<(), anyhow::Error> {
        let response = self.client
            .post(&self.remote_url)
            .header("Content-Type", "application/json")
            .body(payload.to_string())
            .send()
            .await?;

        if response.status().is_success() {
            info!("Successfully delivered payload to HTTP endpoint: {}", self.remote_url);
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            error!("HTTP delivery failed: {} - {}", status, error_text);
            Err(anyhow::anyhow!("HTTP delivery failed: {} - {}", status, error_text))
        }
    }

    fn sink_type(&self) -> &'static str {
        "http"
    }
}
