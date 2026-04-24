pub mod sqlite;
pub mod rocksdb;
pub mod sled;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QueueStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    PermanentFailure,
}

impl Default for QueueStatus {
    fn default() -> Self {
        QueueStatus::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: String,
    pub payload: String,
    pub status: QueueStatus,
    pub created_at: DateTime<Utc>,
    pub retry_count: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_entries: i64,
    pub pending_count: i64,
    pub processing_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub permanent_failure_count: i64,
    pub storage_size_mb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneResult {
    pub deleted_count: usize,
    pub freed_space_mb: f64,
}

#[async_trait]
pub trait StorageEngine: Send + Sync {
    /// Initialize the storage engine
    async fn initialize(&mut self) -> Result<(), anyhow::Error>;
    
    /// Push a new payload to the queue
    async fn push(&self, payload: String, metadata: Option<HashMap<String, String>>) -> Result<QueueItem, anyhow::Error>;
    
    /// Get pending items up to the specified limit
    async fn peek_pending(&self, limit: usize) -> Result<Vec<QueueItem>, anyhow::Error>;
    
    /// Mark an item as processing
    async fn mark_processing(&self, id: &str) -> Result<(), anyhow::Error>;
    
    /// Mark an item as completed (may delete if success_purge is enabled)
    async fn mark_complete(&self, id: &str, success_purge: bool) -> Result<(), anyhow::Error>;
    
    /// Mark an item as failed with retry information
    async fn mark_failed(&self, id: &str, retry_count: i32, next_retry_at: DateTime<Utc>) -> Result<(), anyhow::Error>;
    
    /// Mark an item as permanently failed (max retries exceeded)
    async fn mark_permanent_failure(&self, id: &str) -> Result<(), anyhow::Error>;
    
    /// Get storage statistics
    async fn get_stats(&self) -> Result<StorageStats, anyhow::Error>;
    
    /// Prune old entries based on TTL and limits
    async fn prune(&self, ttl_hours: u32, max_entries: usize, max_size_mb: u64) -> Result<PruneResult, anyhow::Error>;
    
    /// Get storage engine type
    fn engine_type(&self) -> &'static str;
    
    /// Check if storage is healthy
    async fn health_check(&self) -> Result<bool, anyhow::Error>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageEngineType {
    SQLite,
    RocksDB,
    Sled,
}

impl Default for StorageEngineType {
    fn default() -> Self {
        StorageEngineType::SQLite
    }
}

impl std::fmt::Display for StorageEngineType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageEngineType::SQLite => write!(f, "sqlite"),
            StorageEngineType::RocksDB => write!(f, "rocksdb"),
            StorageEngineType::Sled => write!(f, "sled"),
        }
    }
}

impl std::str::FromStr for StorageEngineType {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sqlite" => Ok(StorageEngineType::SQLite),
            "rocksdb" => Ok(StorageEngineType::RocksDB),
            "sled" => Ok(StorageEngineType::Sled),
            _ => Err(format!("Invalid storage engine: {}", s)),
        }
    }
}

pub fn create_storage_engine(
    engine_type: StorageEngineType,
    config: &StorageConfig,
) -> Result<Box<dyn StorageEngine>, Box<dyn std::error::Error>> {
    match engine_type {
        StorageEngineType::SQLite => {
            let engine = sqlite::SqliteStorage::new(config)?;
            Ok(Box::new(engine))
        }
        StorageEngineType::RocksDB => {
            let engine = rocksdb::RocksDBStorage::new(config)?;
            Ok(Box::new(engine))
        }
        StorageEngineType::Sled => {
            let engine = sled::SledStorage::new(config)?;
            Ok(Box::new(engine))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub engine: StorageEngineType,
    pub path: String,
    pub max_entries: usize,
    pub max_size_mb: u64,
    pub ttl_hours: u32,
    pub success_purge: bool,
    pub batch_prune_interval_sec: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            engine: StorageEngineType::SQLite,
            path: {
                let path = std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./relay_queue.db".to_string());
                if path != ":memory:" && !path.ends_with(".db") {
                    format!("{}.db", path)
                } else {
                    path
                }
            },
            max_entries: std::env::var("STORAGE_MAX_ENTRIES")
                .unwrap_or_else(|_| "50000".to_string())
                .parse()
                .unwrap_or(50000),
            max_size_mb: std::env::var("STORAGE_MAX_SIZE_MB")
                .unwrap_or_else(|_| "500".to_string())
                .parse()
                .unwrap_or(500),
            ttl_hours: std::env::var("STORAGE_TTL_HOURS")
                .unwrap_or_else(|_| "24".to_string())
                .parse()
                .unwrap_or(24),
            success_purge: std::env::var("STORAGE_SUCCESS_PURGE")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            batch_prune_interval_sec: std::env::var("STORAGE_BATCH_PRUNE_INTERVAL_SEC")
                .unwrap_or_else(|_| "600".to_string())
                .parse()
                .unwrap_or(600),
        }
    }
}
