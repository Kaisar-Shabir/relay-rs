use sqlx::{sqlite::SqlitePool, Row, Sqlite, Pool};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use tracing::{info, error};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QueueItem {
    pub id: String,
    pub payload: String,
    pub status: QueueStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub retry_count: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text")]
pub enum QueueStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl Default for QueueStatus {
    fn default() -> Self {
        QueueStatus::Pending
    }
}

pub struct Database {
    pool: Pool<Sqlite>,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(database_url).await?;
        
        // Initialize database schema directly
        Self::init_schema(&pool).await?;
        
        Ok(Database { pool })
    }

    async fn init_schema(pool: &Pool<Sqlite>) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS queue_items (
                id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                retry_count INTEGER NOT NULL DEFAULT 0,
                next_retry_at DATETIME
            );
            
            CREATE INDEX IF NOT EXISTS idx_queue_status ON queue_items(status);
            CREATE INDEX IF NOT EXISTS idx_queue_next_retry ON queue_items(next_retry_at);
            "#
        )
        .execute(pool)
        .await?;
        
        Ok(())
    }

    pub async fn create_migrations_if_not_exist() -> Result<(), Box<dyn std::error::Error>> {
        // No longer needed - schema is initialized in Database::new
        Ok(())
    }

    pub async fn enqueue(&self, payload: String) -> Result<QueueItem, sqlx::Error> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        
        let _result = sqlx::query(
            r#"
            INSERT INTO queue_items (id, payload, status, created_at, updated_at, retry_count)
            VALUES (?1, ?2, 'pending', ?3, ?4, 0)
            "#
        )
        .bind(&id)
        .bind(&payload)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        
        let queue_item = QueueItem {
            id: id.clone(),
            payload,
            status: QueueStatus::Pending,
            created_at: now,
            updated_at: now,
            retry_count: 0,
            next_retry_at: None,
        };
        
        info!("Enqueued item with ID: {}", id);
        Ok(queue_item)
    }

    pub async fn get_pending_items(&self, limit: i32) -> Result<Vec<QueueItem>, sqlx::Error> {
        let now = Utc::now();
        
        let rows = sqlx::query(
            r#"
            SELECT id, payload, status, created_at, updated_at, retry_count, next_retry_at
            FROM queue_items
            WHERE status = 'pending' 
               OR (status = 'failed' AND next_retry_at <= ?1)
            ORDER BY created_at ASC
            LIMIT ?2
            "#
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        
        let mut items = Vec::new();
        for row in rows {
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "pending" => QueueStatus::Pending,
                "processing" => QueueStatus::Processing,
                "completed" => QueueStatus::Completed,
                "failed" => QueueStatus::Failed,
                _ => QueueStatus::Pending,
            };
            
            items.push(QueueItem {
                id: row.get("id"),
                payload: row.get("payload"),
                status,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                retry_count: row.get("retry_count"),
                next_retry_at: row.get("next_retry_at"),
            });
        }
        
        Ok(items)
    }

    pub async fn mark_as_processing(&self, id: &str) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        
        sqlx::query(
            "UPDATE queue_items SET status = 'processing', updated_at = ?1 WHERE id = ?2"
        )
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    pub async fn mark_as_completed(&self, id: &str) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        
        sqlx::query(
            "UPDATE queue_items SET status = 'completed', updated_at = ?1 WHERE id = ?2"
        )
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        
        info!("Marked item {} as completed", id);
        Ok(())
    }

    pub async fn mark_as_failed(&self, id: &str, retry_count: i32, next_retry_at: DateTime<Utc>) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        
        sqlx::query(
            "UPDATE queue_items SET status = 'failed', retry_count = ?1, next_retry_at = ?2, updated_at = ?3 WHERE id = ?4"
        )
        .bind(retry_count)
        .bind(next_retry_at)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        
        error!("Marked item {} as failed, retry count: {}", id, retry_count);
        Ok(())
    }

    pub async fn get_queue_stats(&self) -> Result<(i64, i64, i64, i64), sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT 
                COUNT(CASE WHEN status = 'pending' THEN 1 END) as pending,
                COUNT(CASE WHEN status = 'processing' THEN 1 END) as processing,
                COUNT(CASE WHEN status = 'completed' THEN 1 END) as completed,
                COUNT(CASE WHEN status = 'failed' THEN 1 END) as failed
            FROM queue_items
            "#
        )
        .fetch_one(&self.pool)
        .await?;
        
        Ok((
            row.get("pending"),
            row.get("processing"),
            row.get("completed"),
            row.get("failed"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_queue_operations() {
        // This would require setting up an in-memory SQLite database for testing
        // For now, we'll leave this as a placeholder
    }
}
