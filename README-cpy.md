# RelayRS

A robust Rust daemon for reliable message relay with pluggable storage backends, retry logic, and callback notifications.

## Features

- **HTTP Server**: Axum-based server on `127.0.0.1:8089`
- **Pluggable Storage**: StorageEngine trait abstraction supporting SQLite (implemented), RocksDB, and Sled (placeholders)
- **Persistent Queue**: Configurable message queue with status tracking and metadata support
- **Background Worker**: Tokio task that processes queued messages with configurable batch sizes
- **Exponential Backoff**: Intelligent retry logic with configurable delays and max retries
- **Callback System**: Status notifications to callback URLs
- **Dynamic Configuration**: Remote config synchronization with hierarchical structure
- **Health Monitoring**: Health check and statistics endpoints
- **Retention & Pruning**: Automatic cleanup based on TTL, size limits, and entry counts
- **Success Purging**: Optional immediate deletion of successfully processed items

## Architecture

### Server Layer
- `/ingest` - Accept JSON payloads for queuing
- `/health` - Service health check with queue statistics
- `/stats` - Queue statistics endpoint

### Storage Abstraction Layer
- **StorageEngine trait**: Async interface for pluggable backends
- **SQLite**: Fully implemented with schema auto-initialization
- **RocksDB**: Placeholder implementation (ready for development)
- **Sled**: Placeholder implementation (ready for development)
- Queue items with status tracking: `pending`, `processing`, `completed`, `failed`
- Metadata support for flexible payload enrichment
- Automatic schema initialization

### Worker Layer
- Polls storage every second (configurable)
- Processes items in configurable batches
- Attempts delivery to remote URLs
- Handles failures with exponential backoff
- Sends status callbacks
- Automatic retention and pruning

### Config Sync
- Hierarchical configuration structure (storage, network, worker)
- Fetches remote configuration every 60 seconds (configurable)
- Updates runtime configuration dynamically
- Environment variable support

## Quick Start

1. **Install Dependencies**
   ```bash
   cargo build
   ```

2. **Configure Environment**
   ```bash
   cp .env.example .env
   # Edit .env with your configuration
   ```

3. **Run the Service**
   ```bash
   cargo run
   ```

## API Usage

### Ingest Payload
```bash
curl -X POST http://127.0.0.1:8089/ingest \
  -H "Content-Type: application/json" \
  -d '{
    "data": {
      "message": "Hello World",
      "timestamp": "2024-01-01T00:00:00Z"
    },
    "metadata": {
      "source": "test-client"
    }
  }'
```

### Health Check
```bash
curl http://127.0.0.1:8089/health
```

### Queue Statistics
```bash
curl http://127.0.0.1:8089/stats
```

## Configuration

### Environment Variables

#### Storage Configuration
| Variable | Default | Description |
|----------|---------|-------------|
| `STORAGE_ENGINE` | `sqlite` | Storage backend (sqlite, rocksdb, sled) |
| `STORAGE_PATH` | `./relay_queue.db` | Storage path/file location |
| `STORAGE_MAX_ENTRIES` | `50000` | Maximum queue entries |
| `STORAGE_MAX_SIZE_MB` | `500` | Maximum storage size in MB |
| `STORAGE_TTL_HOURS` | `24` | Retention time-to-live in hours |
| `STORAGE_SUCCESS_PURGE` | `true` | Delete successful items immediately |
| `STORAGE_BATCH_PRUNE_INTERVAL_SEC` | `600` | Pruning interval in seconds |

#### Network Configuration
| Variable | Default | Description |
|----------|---------|-------------|
| `NETWORK_REMOTE_URL` | `http://localhost:3000/webhook` | Target URL for payload delivery |
| `NETWORK_CALLBACK_URL` | `http://localhost:8080/callback` | Callback URL for status notifications |
| `NETWORK_RETRY_BACKOFF_SEC` | `2` | Base delay for exponential backoff |

#### Worker Configuration
| Variable | Default | Description |
|----------|---------|-------------|
| `WORKER_POLL_INTERVAL_SECONDS` | `1` | Worker poll interval |
| `WORKER_CONFIG_SYNC_INTERVAL_SECONDS` | `60` | Config sync interval |
| `WORKER_MAX_RETRIES` | `5` | Maximum retry attempts |
| `WORKER_BATCH_SIZE` | `10` | Items to process per batch |

#### General
| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Logging level |

### Remote Configuration

The service can fetch configuration from a remote endpoint. Implement the `fetch_remote_config()` function in `main.rs` to connect to your config server.

## Database Schema

```sql
CREATE TABLE queue_items (
    id TEXT PRIMARY KEY,
    payload TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at DATETIME
);
```

## Retry Logic

- Exponential backoff: `base_delay * 2^(retry_count - 1)`
- Maximum retries: 5 (configurable)
- Failed items are retried until max retries exceeded
- Permanently failed items are marked as `completed` with failure status

## Callback Format

```json
{
  "queue_id": "uuid-string",
  "status": "completed|failed",
  "message": "Optional error message",
  "processed_at": "2024-01-01T00:00:00Z"
}
```

## Storage Backend Implementation

### Adding a New Storage Backend

1. **Create a new module** in `src/storage/your_backend.rs`
2. **Implement the StorageEngine trait**:
   ```rust
   use async_trait::async_trait;
   use storage::{StorageEngine, QueueItem, StorageStats, PruneResult, StorageConfig};
   
   #[async_trait]
   impl StorageEngine for YourStorage {
       async fn initialize(&mut self) -> Result<(), anyhow::Error> { ... }
       async fn push(&self, payload: String, metadata: Option<HashMap<String, String>>) -> Result<QueueItem, anyhow::Error> { ... }
       async fn peek_pending(&self, limit: usize) -> Result<Vec<QueueItem>, anyhow::Error> { ... }
       async fn mark_processing(&self, id: &str) -> Result<(), anyhow::Error> { ... }
       async fn mark_complete(&self, id: &str, success_purge: bool) -> Result<(), anyhow::Error> { ... }
       async fn mark_failed(&self, id: &str, retry_count: i32, next_retry_at: DateTime<Utc>) -> Result<(), anyhow::Error> { ... }
       async fn get_stats(&self) -> Result<StorageStats, anyhow::Error> { ... }
       async fn prune(&self, ttl_hours: u32, max_entries: usize, max_size_mb: u64) -> Result<PruneResult, anyhow::Error> { ... }
       fn engine_type(&self) -> &'static str { "your_backend" }
       async fn health_check(&self) -> Result<bool, anyhow::Error> { ... }
   }
   ```
3. **Add to mod.rs**: Export your module and update the `create_storage_engine` function
4. **Update StorageEngineType enum**: Add your backend variant

### Current Backend Status

- **SQLite**: ✅ Fully implemented with all features
- **RocksDB**: 🔄 Placeholder implementation ready for development
- **Sled**: 🔄 Placeholder implementation ready for development

## Development

### Building
```bash
cargo build --release
```

### Testing
```bash
cargo test
```

### Running with Custom Config
```bash
STORAGE_ENGINE=sqlite \
NETWORK_REMOTE_URL=https://api.example.com/webhook \
NETWORK_CALLBACK_URL=https://api.example.com/callback \
WORKER_BATCH_SIZE=20 \
cargo run
```

## License

This project is licensed under the MIT License.
