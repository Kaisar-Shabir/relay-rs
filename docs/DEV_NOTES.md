# Development Notes & Code Discrepancies

## 1. Identified Discrepancies

### 1.1 Kafka Implementation Status

**Expected:** Fully implemented KafkaSink with producer configuration  
**Actual:** KafkaSink exists but may require additional Kafka client dependencies

**Files Affected:**
- `src/sinks/kafka.rs` - Implementation exists
- `Cargo.toml` - Kafka dependencies commented out (lines 29-30)

**Resolution Required:**
```toml
# Uncomment and add proper Kafka dependencies
kafka = "0.9"  # or appropriate version
# Additional tokio features may be needed
```

### 1.2 Storage Engine Implementation Status

**Expected:** Multiple storage engines (SQLite, RocksDB, Sled) fully implemented  
**Actual:** Only SQLite is fully implemented

**Files Affected:**
- `src/storage/rocksdb.rs` - Placeholder implementation
- `src/storage/sled.rs` - Placeholder implementation
- `Cargo.toml` - RocksDB and Sled dependencies commented out

**Resolution Required:**
```toml
# Uncomment storage dependencies
rocksdb = "0.21"
sled = "0.34"
```

### 1.3 Configuration Environment Variables

**Expected:** Consistent environment variable naming  
**Actual:** Some inconsistencies between documentation and implementation

**Discrepancies:**
- Documentation uses `MAX_ENTRIES` but code uses `STORAGE_MAX_ENTRIES`
- Documentation uses `STORAGE_PATH` but code default is `./relay_queue.db`

**Actual Environment Variables:**
```rust
STORAGE_ENGINE     // ✅ Matches
STORAGE_PATH       // ✅ Matches  
STORAGE_MAX_ENTRIES // ❌ Doc shows MAX_ENTRIES
STORAGE_MAX_SIZE_MB // ✅ Matches
STORAGE_TTL_HOURS   // ✅ Matches
SINK_TYPE          // ✅ Matches
// MAX_RETRIES is hardcoded in config struct, not from env
```

---

## 2. Implementation Gaps

### 2.1 Dynamic Configuration

**Expected:** Remote configuration fetching implemented  
**Actual:** Placeholder implementation with TODO comments

**File:** `src/main.rs` (lines 445-472)
```rust
async fn fetch_remote_config() -> Option<RelayConfig> {
    // This is a placeholder - in a real implementation, you would fetch from a remote URL
    // For now, we'll just return None to use the default config
    None
}
```

### 2.2 PII Scrubbing

**Expected:** Regex-based PII scrubbing before persistence  
**Actual:** Not implemented - mentioned in documentation but missing from code

**Suggested Implementation Location:** `src/main.rs` in `ingest_endpoint` function

### 2.3 Callback Mechanism

**Expected:** Configurable callback URLs for delivery status  
**Actual:** Hardcoded callback URL in configuration

**File:** `src/main.rs` (line 83)
```rust
callback_url: "http://localhost:8080/callback".to_string(),
```

---

## 3. Testing Coverage

### 3.1 Unit Tests

**Current Status:** No unit tests found in the codebase

**Missing Tests:**
- Storage engine operations (push, peek, mark_* operations)
- Retry logic and exponential backoff
- Zombie recovery mechanism
- Configuration parsing and validation
- Sink delivery mechanisms

### 3.2 Integration Tests

**Current Status:** No integration tests found

**Missing Tests:**
- End-to-end HTTP API flow
- Multi-threaded concurrent access
- Database schema migrations
- Error handling and recovery scenarios

---

## 4. Performance Optimizations

### 4.1 SQLite Configuration

**Current Implementation:** Basic WAL mode enabled
**Potential Optimizations:**
```sql
PRAGMA synchronous = NORMAL;  -- Balance between safety and performance
PRAGMA cache_size = -20000;   -- 20MB cache
PRAGMA temp_store = memory;   -- Temporary tables in memory
PRAGMA mmap_size = 268435456; -- 256MB memory-mapped I/O
```

### 4.2 Batch Processing

**Current Implementation:** Sequential item processing
**Potential Optimization:** Batch delivery to sinks (especially Kafka)

---

## 5. Security Considerations

### 5.1 Input Validation

**Current Implementation:** Basic JSON parsing
**Missing Validations:**
- Payload size limits
- Metadata field validation
- Rate limiting per client
- Authentication/authorization

### 5.2 Data Encryption

**Current Implementation:** Plaintext storage
**Missing Features:**
- At-rest encryption for sensitive data
- TLS for HTTP endpoints
- API key authentication

---

## 6. Operational Readiness

### 6.1 Health Checks

**Current Implementation:** Basic connectivity test
**Enhancements Needed:**
- Disk space monitoring
- Queue depth thresholds
- Sink connectivity testing
- Memory usage tracking

### 6.2 Metrics Enhancement

**Current Implementation:** Basic Prometheus metrics
**Missing Metrics:**
- Request latency distribution
- Error rates by type
- Retry attempt histogram
- Processing throughput

### 6.3 Logging Improvements

**Current Implementation:** Structured JSON logging
**Enhancements Needed:**
- Correlation IDs across requests
- Sensitive data redaction
- Log level configuration
- Log rotation configuration

---

## 7. Deployment Considerations

### 7.1 Containerization

**Missing Files:**
- `Dockerfile` for container builds
- `docker-compose.yml` for local development
- Kubernetes deployment manifests

### 7.2 Configuration Management

**Missing Features:**
- Configuration validation on startup
- Environment-specific configuration files
- Secret management integration

---

## 8. Next Steps Priority

### High Priority
1. Fix environment variable naming consistency
2. Implement basic unit tests for core functionality
3. Complete Kafka dependency configuration
4. Add input validation and rate limiting

### Medium Priority
1. Implement PII scrubbing mechanism
2. Add comprehensive integration tests
3. Enhance health checks and metrics
4. Create Docker deployment files

### Low Priority
1. Implement RocksDB and Sled storage engines
2. Add remote configuration fetching
3. Implement advanced SQLite optimizations
4. Add authentication/authorization

---

*This document serves as a living record of development priorities and implementation gaps. Regular updates should be made as features are implemented and discrepancies are resolved.*
