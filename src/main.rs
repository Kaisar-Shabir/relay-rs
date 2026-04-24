mod storage;
mod sinks;

use storage::{StorageEngine, StorageStats, StorageConfig, StorageEngineType};
use sinks::{DeliverySink, SinkType, HttpSink, KafkaSink, StdoutSink};
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use dotenv::dotenv;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};
use tracing_subscriber;
use chrono::{DateTime, Utc};

#[derive(Clone)]
struct AppState {
    storage: Arc<Box<dyn StorageEngine>>,
    sink: Arc<Box<dyn DeliverySink>>,
    config: Arc<tokio::sync::RwLock<RelayConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RelayConfig {
    storage: StorageConfig,
    network: NetworkConfig,
    worker: WorkerConfig,
    sink: SinkConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetworkConfig {
    remote_url: String,
    callback_url: String,
    retry_backoff_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkerConfig {
    poll_interval_seconds: u64,
    config_sync_interval_seconds: u64,
    max_retries: u32,
    batch_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SinkConfig {
    sink_type: SinkType,
    http_remote_url: Option<String>,
    kafka_topic: Option<String>,
    kafka_bootstrap_servers: Option<String>,
}

fn create_sink_engine(config: &SinkConfig) -> Box<dyn DeliverySink> {
    match config.sink_type {
        SinkType::Http => {
            let url = config.http_remote_url.clone().unwrap_or_else(|| "http://localhost:3000/webhook".to_string());
            Box::new(HttpSink::new(url))
        }
        SinkType::Kafka => {
            let topic = config.kafka_topic.clone().unwrap_or_else(|| "relay-topic".to_string());
            let bootstrap_servers = config.kafka_bootstrap_servers.clone().unwrap_or_else(|| "localhost:9092".to_string());
            Box::new(KafkaSink::new(topic, bootstrap_servers))
        }
        SinkType::Stdout => {
            Box::new(StdoutSink::new())
        }
    }
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
            network: NetworkConfig {
                remote_url: "http://localhost:3000/webhook".to_string(),
                callback_url: "http://localhost:8080/callback".to_string(),
                retry_backoff_sec: 2,
            },
            worker: WorkerConfig {
                poll_interval_seconds: 1,
                config_sync_interval_seconds: 60,
                max_retries: 5,
                batch_size: 10,
            },
            sink: SinkConfig {
                sink_type: SinkType::Http,
                http_remote_url: Some("http://localhost:3000/webhook".to_string()),
                kafka_topic: None,
                kafka_bootstrap_servers: None,
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct IngestPayload {
    data: serde_json::Value,
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CallbackPayload {
    queue_id: String,
    status: String,
    message: Option<String>,
    processed_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    timestamp: DateTime<Utc>,
    storage_engine: String,
    storage_stats: StorageStats,
}

#[derive(Debug, Serialize)]
struct IngestResponse {
    queue_id: String,
    status: String,
    message: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .json() // for jq in run command
        .init();
    
    // Load environment variables
    dotenv().ok();
    
    // Initialize storage engine
    let mut storage = storage::create_storage_engine(
        StorageEngineType::SQLite, 
        &StorageConfig::default()
    )?;
    storage.initialize().await?;
    let storage = Arc::new(storage);
    
    // Initialize configuration
    let config = Arc::new(tokio::sync::RwLock::new(RelayConfig::default()));
    
    // Initialize sink
    let sink_config = config.read().await.sink.clone();
    let sink = Arc::new(create_sink_engine(&sink_config));
    
    // Create shared state
    let state = AppState { storage, sink, config };
    
    // Spawn background tasks
    let worker_state = state.clone();
    let config_sync_state = state.clone();
    
    tokio::spawn(async move {
        worker_task(worker_state).await;
    });
    
    tokio::spawn(async move {
        config_sync_task(config_sync_state).await;
    });
    
    // Create Axum router
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/ingest", post(ingest_endpoint))
        .route("/stats", get(get_stats))
        .route("/metrics", get(metrics_endpoint))
        .with_state(state);
    
    // Start server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8089").await?;
    info!("RelayRS server listening on 127.0.0.1:8089");
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

async fn health_check(State(state): State<AppState>) -> Result<Json<HealthResponse>, StatusCode> {
    let storage_stats = state.storage.get_stats().await
        .map_err(|e| {
            error!("Failed to get storage stats: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    let is_healthy = state.storage.health_check().await.unwrap_or(false);
    
    Ok(Json(HealthResponse {
        status: if is_healthy { "healthy".to_string() } else { "unhealthy".to_string() },
        timestamp: Utc::now(),
        storage_engine: state.storage.engine_type().to_string(),
        storage_stats,
    }))
}

async fn ingest_endpoint(
    State(state): State<AppState>,
    Json(payload): Json<IngestPayload>,
) -> Result<Json<IngestResponse>, StatusCode> {
    // Check queue capacity before enqueuing
    let stats = state.storage.get_stats().await
        .map_err(|e| {
            error!("Failed to get storage stats: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    let config = state.config.read().await.clone();
    if stats.total_entries >= config.storage.max_entries as i64 {
        warn!("Queue capacity exceeded: {} >= {}", stats.total_entries, config.storage.max_entries);
        return Err(StatusCode::INSUFFICIENT_STORAGE);
    }
    
    let payload_str = serde_json::to_string(&payload)
        .map_err(|e| {
            error!("Failed to serialize payload: {}", e);
            StatusCode::BAD_REQUEST
        })?;
    
    // Convert metadata from serde_json::Value to HashMap<String, String>
    let metadata_map = match payload.metadata {
        Some(metadata_value) => {
            match serde_json::from_value::<std::collections::HashMap<String, String>>(metadata_value) {
                Ok(map) => Some(map),
                Err(e) => {
                    warn!("Failed to convert metadata to HashMap<String, String>: {}. Using empty metadata.", e);
                    None
                }
            }
        }
        None => None,
    };
    
    let queue_item = state.storage.push(payload_str, metadata_map).await
        .map_err(|e| {
            error!("Failed to enqueue payload: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    info!("Payload enqueued with ID: {}", queue_item.id);
    
    Ok(Json(IngestResponse {
        queue_id: queue_item.id,
        status: "queued".to_string(),
        message: "Payload successfully queued for processing".to_string(),
    }))
}

async fn get_stats(State(state): State<AppState>) -> Result<Json<StorageStats>, StatusCode> {
    let stats = state.storage.get_stats().await
        .map_err(|e| {
            error!("Failed to get storage stats: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    Ok(Json(stats))
}

async fn metrics_endpoint(State(state): State<AppState>) -> Result<String, StatusCode> {
    let stats = state.storage.get_stats().await
        .map_err(|e| {
            error!("Failed to get storage stats: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Simple text format metrics for monitoring
    let metrics = format!(
        "# HELP relayrs_total_entries Total number of entries in the queue\n\
         # TYPE relayrs_total_entries gauge\n\
         relayrs_total_entries {}\n\
         # HELP relayrs_pending_entries Number of pending entries\n\
         # TYPE relayrs_pending_entries gauge\n\
         relayrs_pending_entries {}\n\
         # HELP relayrs_failed_entries Number of failed entries\n\
         # TYPE relayrs_failed_entries gauge\n\
         relayrs_failed_entries {}\n",
        stats.total_entries,
        stats.pending_count,
        stats.failed_count
    );
    
    Ok(metrics)
}

async fn worker_task(state: AppState) {
    let config = state.config.read().await.clone();
    let mut interval = interval(Duration::from_secs(
        config.worker.poll_interval_seconds
    ));
    
    let mut prune_interval = tokio::time::interval(Duration::from_secs(
        config.storage.batch_prune_interval_sec
    ));
    
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let config = state.config.read().await.clone();
                
                match process_pending_items(&state.storage, &state.sink, &config).await {
                    Ok(count) => {
                        if count > 0 {
                            info!("Processed {} pending items", count);
                        }
                    }
                    Err(e) => {
                        error!("Error processing pending items: {}", e);
                    }
                }
            }
            _ = prune_interval.tick() => {
                let config = state.config.read().await.clone();
                
                match prune_storage(&state.storage, &config.storage).await {
                    Ok(result) => {
                        if result.deleted_count > 0 {
                            info!("Pruned {} entries, freed {:.2} MB", result.deleted_count, result.freed_space_mb);
                        }
                    }
                    Err(e) => {
                        error!("Error during storage pruning: {}", e);
                    }
                }
            }
        }
    }
}

async fn process_pending_items(storage: &Arc<Box<dyn StorageEngine>>, sink: &Arc<Box<dyn DeliverySink>>, config: &RelayConfig) -> Result<usize, anyhow::Error> {
    let pending_items = storage.peek_pending(config.worker.batch_size).await?;
    
    if pending_items.is_empty() {
        return Ok(0);
    }
    
    let mut processed_count = 0;
    
    for item in pending_items {
        // Mark as processing to avoid race conditions
        storage.mark_processing(&item.id).await?;
        
        // Attempt delivery
        match sink.deliver(&item.payload, &item.metadata).await {
            Ok(_) => {
                // Success - mark as completed and send callback
                storage.mark_complete(&item.id, config.storage.success_purge).await?;
                send_callback(&item, &config.network.callback_url, "completed", None).await;
                processed_count += 1;
            }
            Err(e) => {
                // Failed - calculate retry delay and update
                let retry_count = item.retry_count + 1;
                
                if retry_count > config.worker.max_retries as i32 {
                    // Max retries exceeded - mark as permanently failed
                    storage.mark_permanent_failure(&item.id).await?;
                    send_callback(&item, &config.network.callback_url, "failed", Some("Max retries exceeded".to_string())).await;
                    warn!(queue_id = %item.id, retry_count = retry_count, "Item exceeded max retries");
                } else {
                    // Calculate exponential backoff
                    let delay_seconds = config.network.retry_backoff_sec * 2_u64.pow(retry_count as u32 - 1);
                    let next_retry_at = Utc::now() + chrono::Duration::seconds(delay_seconds as i64);
                    
                    storage.mark_failed(&item.id, retry_count, next_retry_at).await?;
                    send_callback(&item, &config.network.callback_url, "failed", Some(format!("Retry {} scheduled at {}", retry_count, next_retry_at))).await;
                }
                
                error!(queue_id = %item.id, retry_count = retry_count, error = %e, "Failed to deliver payload");
            }
        }
    }
    
    Ok(processed_count)
}


async fn send_callback(item: &storage::QueueItem, callback_url: &str, status: &str, message: Option<String>) {
    let client = reqwest::Client::new();
    let callback_payload = CallbackPayload {
        queue_id: item.id.clone(),
        status: status.to_string(),
        message,
        processed_at: Utc::now(),
    };
    
    match client
        .post(callback_url)
        .json(&callback_payload)
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                info!("Callback sent successfully for item {}", item.id);
            } else {
                warn!("Failed to send callback for item {}: {}", item.id, response.status());
            }
        }
        Err(e) => {
            error!("Error sending callback for item {}: {}", item.id, e);
        }
    }
}

async fn prune_storage(storage: &Arc<Box<dyn StorageEngine>>, storage_config: &StorageConfig) -> Result<storage::PruneResult, anyhow::Error> {
    storage.prune(
        storage_config.ttl_hours,
        storage_config.max_entries,
        storage_config.max_size_mb,
    ).await
}

async fn config_sync_task(state: AppState) {
    let mut interval = interval(Duration::from_secs(
        state.config.read().await.worker.config_sync_interval_seconds
    ));
    
    loop {
        interval.tick().await;
        
        match fetch_remote_config().await {
            Some(new_config) => {
                let mut current_config = state.config.write().await;
                if new_config.network.remote_url != current_config.network.remote_url {
                    info!("Remote URL updated: {} -> {}", current_config.network.remote_url, new_config.network.remote_url);
                }
                *current_config = new_config;
            }
            None => {
                warn!("Failed to fetch remote config, using existing configuration");
            }
        }
    }
}

async fn fetch_remote_config() -> Option<RelayConfig> {
    // This is a placeholder - in a real implementation, you would fetch from a remote URL
    // For now, we'll just return None to use the default config
    // You could implement something like:
    /*
    let client = reqwest::Client::new();
    match client.get("https://your-config-server.com/relay-config").send().await {
        Ok(response) if response.status().is_success() => {
            match response.json::<RelayConfig>().await {
                Ok(config) => Some(config),
                Err(e) => {
                    error!("Failed to parse remote config: {}", e);
                    None
                }
            }
        }
        Ok(response) => {
            warn!("Remote config returned status: {}", response.status());
            None
        }
        Err(e) => {
            error!("Failed to fetch remote config: {}", e);
            None
        }
    }
    */
    None
}
