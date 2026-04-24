use super::{StorageEngine, QueueItem, QueueStatus, StorageStats, PruneResult, StorageConfig};
use async_trait::async_trait;
use chrono::{DateTime, Utc, Duration};
use sqlx::{Pool, Sqlite, Row};
use std::collections::HashMap;
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct SqliteStorage {
    pool: Option<Pool<Sqlite>>,
    config: StorageConfig,
}

impl SqliteStorage {
    pub fn new(config: &StorageConfig) -> Result<Self, Box<dyn std::error::Error>> {
        // Create a placeholder pool - will be initialized in initialize()
        Ok(Self {
            pool: None,
            config: config.clone(),
        })
    }
    
    async fn init_schema(&self) -> Result<(), anyhow::Error> {
        // Enable WAL mode for better concurrency during high-load ingestions
        sqlx::query("PRAGMA journal_mode = WAL;")
            .execute(self.get_pool())
            .await?;
        
        // Create the queue table with proper schema
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS queue_items (
                id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                retry_count INTEGER NOT NULL DEFAULT 0,
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                next_retry_at DATETIME,
                metadata TEXT
            );
            
            CREATE INDEX IF NOT EXISTS idx_queue_status ON queue_items(status);
            CREATE INDEX IF NOT EXISTS idx_queue_next_retry ON queue_items(next_retry_at);
            CREATE INDEX IF NOT EXISTS idx_queue_created_at ON queue_items(created_at);
            "#
        )
        .execute(self.get_pool())
        .await?;
        
        Ok(())
    }
    
    fn serialize_metadata(metadata: &Option<HashMap<String, String>>) -> Option<String> {
        metadata.as_ref().map(|m| serde_json::to_string(m).ok()).flatten()
    }
    
    fn deserialize_metadata(metadata_str: &Option<String>) -> Option<HashMap<String, String>> {
        metadata_str.as_ref().map(|s| serde_json::from_str(s).ok()).flatten()
    }
    
    fn get_pool(&self) -> &Pool<Sqlite> {
        self.pool.as_ref().expect("Storage not initialized")
    }
}

#[async_trait]
impl StorageEngine for SqliteStorage {
    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        // Create parent directories if they don't exist (only for file-based databases)
        if self.config.path != ":memory:" {
            if let Some(parent) = std::path::Path::new(&self.config.path).parent() {
                std::fs::create_dir_all(parent)?;
            }
        }
        
        let database_url = if self.config.path == ":memory:" {
            "sqlite::memory:".to_string()
        } else {
            format!("sqlite:{}", self.config.path)
        };
        
        self.pool = Some(sqlx::SqlitePool::connect(&database_url).await?);
        self.init_schema().await
    }
    
    async fn push(&self, payload: String, metadata: Option<HashMap<String, String>>) -> Result<QueueItem, anyhow::Error> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let metadata_str = Self::serialize_metadata(&metadata);
        
        sqlx::query(
            "INSERT INTO queue_items (id, payload, status, created_at, retry_count, metadata) VALUES (?1, ?2, 'pending', ?3, 0, ?4)"
        )
        .bind(&id)
        .bind(&payload)
        .bind(now)
        .bind(metadata_str)
        .execute(self.get_pool())
        .await?;
        
        let queue_item = QueueItem {
            id: id.clone(),
            payload,
            status: QueueStatus::Pending,
            created_at: now,
            retry_count: 0,
            next_retry_at: None,
            metadata,
        };
        
        info!("Enqueued item with ID: {}", id);
        Ok(queue_item)
    }
    
    async fn peek_pending(&self, limit: usize) -> Result<Vec<QueueItem>, anyhow::Error> {
        let now = Utc::now();
        
        let rows = sqlx::query(
            r#"
            SELECT id, payload, status, created_at, retry_count, next_retry_at, metadata
            FROM queue_items
            WHERE status = 'pending' 
               OR (status = 'failed' AND next_retry_at <= ?1)
            ORDER BY created_at ASC
            LIMIT ?2
            "#
        )
        .bind(now)
        .bind(limit as i32)
        .fetch_all(self.get_pool())
        .await?;
        
        let mut items = Vec::new();
        for row in rows {
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "pending" => QueueStatus::Pending,
                "processing" => QueueStatus::Processing,
                "completed" => QueueStatus::Completed,
                "failed" => QueueStatus::Failed,
                "permanent_failure" => QueueStatus::PermanentFailure,
                _ => QueueStatus::Pending,
            };
            
            let metadata_str: Option<String> = row.get("metadata");
            let metadata = Self::deserialize_metadata(&metadata_str);
            
            items.push(QueueItem {
                id: row.get("id"),
                payload: row.get("payload"),
                status,
                created_at: row.get("created_at"),
                retry_count: row.get("retry_count"),
                next_retry_at: row.get("next_retry_at"),
                metadata,
            });
        }
        
        Ok(items)
    }
    
    async fn mark_processing(&self, id: &str) -> Result<(), anyhow::Error> {
        sqlx::query("UPDATE queue_items SET status = 'processing' WHERE id = ?1")
            .bind(id)
            .execute(self.get_pool())
            .await?;
        
        Ok(())
    }
    
    async fn mark_complete(&self, id: &str, success_purge: bool) -> Result<(), anyhow::Error> {
        if success_purge {
            // Delete the record entirely
            sqlx::query("DELETE FROM queue_items WHERE id = ?1")
                .bind(id)
                .execute(self.get_pool())
                .await?;
            
            info!("Deleted completed item: {}", id);
        } else {
            // Mark as completed
            sqlx::query("UPDATE queue_items SET status = 'completed' WHERE id = ?1")
                .bind(id)
                .execute(self.get_pool())
                .await?;
            
            info!("Marked item {} as completed", id);
        }
        
        Ok(())
    }
    
    async fn mark_failed(&self, id: &str, retry_count: i32, next_retry_at: DateTime<Utc>) -> Result<(), anyhow::Error> {
        sqlx::query(
            "UPDATE queue_items SET status = 'failed', retry_count = ?1, next_retry_at = ?2 WHERE id = ?3"
        )
        .bind(retry_count)
        .bind(next_retry_at)
        .bind(id)
        .execute(self.get_pool())
        .await?;
        
        error!("Marked item {} as failed, retry count: {}", id, retry_count);
        Ok(())
    }
    
    async fn mark_permanent_failure(&self, id: &str) -> Result<(), anyhow::Error> {
        sqlx::query("UPDATE queue_items SET status = 'permanent_failure' WHERE id = ?1")
            .bind(id)
            .execute(self.get_pool())
            .await?;
        
        error!("Marked item {} as permanently failed", id);
        Ok(())
    }
    
    async fn get_stats(&self) -> Result<StorageStats, anyhow::Error> {
        let row = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as total,
                COUNT(CASE WHEN status = 'pending' THEN 1 END) as pending,
                COUNT(CASE WHEN status = 'processing' THEN 1 END) as processing,
                COUNT(CASE WHEN status = 'completed' THEN 1 END) as completed,
                COUNT(CASE WHEN status = 'failed' THEN 1 END) as failed,
                COUNT(CASE WHEN status = 'permanent_failure' THEN 1 END) as permanent_failure
            FROM queue_items
            "#
        )
        .fetch_one(self.get_pool())
        .await?;
        
        // Get database size - try actual file size first, fallback to SQLite page calculation
        let storage_size_mb = if self.config.path != ":memory:" {
            // Try to get actual file size for higher accuracy
            match std::fs::metadata(&self.config.path) {
                Ok(metadata) => metadata.len() as f64 / (1024.0 * 1024.0),
                Err(_) => {
                    // Fallback to SQLite page calculation if file metadata fails
                    let size_result = sqlx::query("SELECT page_count * page_size as size FROM pragma_page_count(), pragma_page_size()")
                        .fetch_one(self.get_pool())
                        .await;
                    
                    match size_result {
                        Ok(row) => {
                            let size_bytes: i64 = row.get("size");
                            size_bytes as f64 / (1024.0 * 1024.0)
                        }
                        Err(_) => 0.0, // Final fallback
                    }
                }
            }
        } else {
            // For in-memory databases, use SQLite page calculation
            let size_result = sqlx::query("SELECT page_count * page_size as size FROM pragma_page_count(), pragma_page_size()")
                .fetch_one(self.get_pool())
                .await;
            
            match size_result {
                Ok(row) => {
                    let size_bytes: i64 = row.get("size");
                    size_bytes as f64 / (1024.0 * 1024.0)
                }
                Err(_) => 0.0,
            }
        };
        
        Ok(StorageStats {
            total_entries: row.get("total"),
            pending_count: row.get("pending"),
            processing_count: row.get("processing"),
            completed_count: row.get("completed"),
            failed_count: row.get("failed"),
            permanent_failure_count: row.get("permanent_failure"),
            storage_size_mb,
        })
    }
    
    async fn prune(&self, ttl_hours: u32, max_entries: usize, _max_size_mb: u64) -> Result<PruneResult, anyhow::Error> {
        let mut deleted_count = 0;
        let cutoff_time = Utc::now() - Duration::hours(ttl_hours as i64);
        
        // First, prune old completed entries
        let result = sqlx::query(
            "DELETE FROM queue_items WHERE status = 'completed' AND created_at < ?1"
        )
        .bind(cutoff_time)
        .execute(self.get_pool())
        .await?;
        
        deleted_count += result.rows_affected() as usize;
        
        // Second, fix zombie processing items (items stuck in 'processing' for > 1 hour)
        let zombie_cutoff = Utc::now() - Duration::hours(1);
        let zombie_result = sqlx::query(
            "UPDATE queue_items SET status = 'pending' WHERE status = 'processing' AND created_at < ?1"
        )
        .bind(zombie_cutoff)
        .execute(self.get_pool())
        .await?;
        
        if zombie_result.rows_affected() > 0 {
            warn!("Recovered {} zombie processing items", zombie_result.rows_affected());
        }
        
        // Check if we still exceed limits
        let stats = self.get_stats().await?;
        
        // If we exceed max entries, prune oldest entries (excluding pending/processing)
        if stats.total_entries as usize > max_entries {
            let excess = stats.total_entries as usize - max_entries;
            let result = sqlx::query(
                "DELETE FROM queue_items WHERE id IN (SELECT id FROM queue_items WHERE status IN ('completed', 'failed') ORDER BY created_at ASC LIMIT ?1)"
            )
            .bind(excess as i32)
            .execute(self.get_pool())
            .await?;
            
            deleted_count += result.rows_affected() as usize;
        }
        
        // Get updated size
        let updated_stats = self.get_stats().await?;
        let freed_space_mb = stats.storage_size_mb - updated_stats.storage_size_mb;
        
        if deleted_count > 0 {
            info!("Pruned {} entries, freed {:.2} MB", deleted_count, freed_space_mb);
        }
        
        Ok(PruneResult {
            deleted_count,
            freed_space_mb,
        })
    }
    
    fn engine_type(&self) -> &'static str {
        "sqlite"
    }
    
    async fn health_check(&self) -> Result<bool, anyhow::Error> {
        // Simple connectivity test
        sqlx::query("SELECT 1")
            .fetch_one(self.get_pool())
            .await
            .map(|_| true)
            .map_err(|e| anyhow::anyhow!(e))
    }
}
