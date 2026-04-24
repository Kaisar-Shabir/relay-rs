# RelayRS

**A high-performance, resilient edge buffer sidecar for self-healing infrastructure.**

---

## 1. Architecture

RelayRS implements the **Transactional Outbox** pattern, sitting between your service hot path and analysis infrastructure (Kafka, HTTP endpoints, etc.).

### 1.1 The 5-Step Flow

1. **Intercept** - Service middleware captures requests and sends them to `POST /ingest`
2. **Commit** - Payload is written to local SQLite WAL-mode database with ACID guarantees
3. **Acknowledge** - HTTP 202 Accepted returned only after disk write confirmation
4. **Relay** - Background worker polls local DB and dispatches to pluggable Sink
5. **Self-Heal** - Exponential backoff with automatic retry tracking when Sinks are unavailable

---

## 2. Key Features

- **Transactional Durability** - Zero data loss with disk persistence before acknowledgment
- **Pluggable Sink Architecture**
  - `HttpSink` - Standard webhooks and API calls
  - `KafkaSink` - High-throughput distributed message streaming  
  - `StdoutSink` - Local development and debugging
- **Backpressure Management** - Automatic `507 Insufficient Storage` when queues hit capacity
- **Zombie Recovery** - Automatic detection and recovery of stuck "processing" items
- **Structured Logging** - Native JSON logging for `jq`, ELK, and Datadog integration

---

## 3. Build, Test, & Run

### 3.1 Build

```bash
# Build release with all features
cargo build --release --all-features
```

### 3.2 Test

```bash
# Run full test suite
cargo test --all-features
```

### 3.3 Run

```bash
# Run with environment variables
RUST_LOG=info STORAGE_ENGINE=sqlite SINK_TYPE=http cargo run --bin relay-rs

# Run with JSON logging and jq filtering
RUST_LOG=info cargo run --bin relay-rs | jq
```

---

## 4. Configuration

Create a `.env` file in the root directory:

| Variable | Default | Description |
|----------|---------|-------------|
| `STORAGE_ENGINE` | `sqlite` | Storage engine: `sqlite`, `sled`, `rocksdb` |
| `STORAGE_PATH` | `./relay_queue.db` | Path to local database file |
| `STORAGE_MAX_ENTRIES` | `50000` | Max queue depth before backpressure |
| `STORAGE_MAX_SIZE_MB` | `500` | Maximum storage size in MB |
| `STORAGE_TTL_HOURS` | `24` | Time-to-live for entries in hours |
| `SINK_TYPE` | `http` | Sink type: `stdout`, `http`, `kafka` |
| `MAX_RETRIES` | `5` | Max retry attempts before permanent failure |

---

## 5. API Reference

### 5.1 POST /ingest

Main entry point for intercepted requests.

**Request Payload:**
```json
{
  "data": { 
    "method": "GET", 
    "url": "http://api.internal/v1/user",
    "body": "{...}" 
  },
  "metadata": { 
    "service_name": "checkout-svc",
    "region": "us-east-1"
  }
}
```

**Responses:**
- `202 Accepted` - Successfully queued
- `507 Insufficient Storage` - Queue capacity exceeded

### 5.2 GET /health

Returns system health, storage engine status, and disk usage.

**Response:**
```json
{
  "status": "healthy",
  "timestamp": "2024-01-01T00:00:00Z",
  "storage_engine": "sqlite",
  "storage_stats": {
    "total_entries": 1000,
    "pending_count": 50,
    "processing_count": 5,
    "storage_size_mb": 12.5
  }
}
```

### 5.3 GET /stats

Real-time queue metrics.

**Response:**
```json
{
  "total_entries": 1000,
  "pending_count": 50,
  "processing_count": 5,
  "completed_count": 900,
  "failed_count": 40,
  "permanent_failure_count": 5,
  "storage_size_mb": 12.5
}
```

### 5.4 GET /metrics

Prometheus-compatible metrics endpoint.

---

## 6. Security & Privacy

**Security-at-the-Edge Philosophy:**

- **Local Scrubbing** - Regex-based PII/Secret scrubbing before disk persistence
- **Encrypted Storage** - Filesystem-level encryption support for buffered data
- **Zero-Leakage** - Internal metadata stripped before Sink delivery unless configured

---

## 7. Roadmap

- [ ] **Prometheus Integration** - Native `/metrics` endpoint for Grafana observability
- [ ] **Batch Dispatching** - Optimize Kafka throughput with multi-message batches
- [ ] **Dynamic Config** - Hot-reload Sinks and Storage settings without restart
- [ ] **TUI Dashboard** - Terminal-based UI for real-time queue monitoring

---

*Developed for high-scale, self-healing infrastructure.*

