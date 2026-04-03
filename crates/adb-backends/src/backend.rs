//! Backend trait definition
//!
//! All memory type backends implement this trait, providing a uniform
//! interface for storage and retrieval operations.

use async_trait::async_trait;
use std::time::Duration;

use adb_core::{
    AdbError, AdbResult, MemoryRecord, MemoryType, Modifiers, Predicate, Scope, Window,
};

/// Information about a backend's capabilities and latency characteristics
#[derive(Debug, Clone)]
pub struct BackendInfo {
    /// Memory type this backend handles
    pub memory_type: MemoryType,
    /// Expected P50 latency for lookup operations (ms)
    pub lookup_p50_ms: u64,
    /// Expected P99 latency for lookup operations (ms)
    pub lookup_p99_ms: u64,
    /// Expected P50 latency for recall operations (ms)
    pub recall_p50_ms: u64,
    /// Expected P99 latency for recall operations (ms)
    pub recall_p99_ms: u64,
    /// Whether this backend supports SCAN operation
    pub supports_scan: bool,
    /// Whether this backend supports LOAD operation
    pub supports_load: bool,
    /// Whether this backend supports pattern matching
    pub supports_pattern: bool,
    /// Whether this backend supports embedding similarity
    pub supports_embedding: bool,
}

impl BackendInfo {
    /// Create info for Working backend
    pub fn working() -> Self {
        Self {
            memory_type: MemoryType::Working,
            lookup_p50_ms: 0,
            lookup_p99_ms: 1,
            recall_p50_ms: 0,
            recall_p99_ms: 1,
            supports_scan: true,
            supports_load: false,
            supports_pattern: false,
            supports_embedding: false,
        }
    }

    /// Create info for Tools backend
    pub fn tools() -> Self {
        Self {
            memory_type: MemoryType::Tools,
            lookup_p50_ms: 1,
            lookup_p99_ms: 2,
            recall_p50_ms: 1,
            recall_p99_ms: 2,
            supports_scan: false,
            supports_load: true,
            supports_pattern: false,
            supports_embedding: false,
        }
    }

    /// Create info for Procedural backend
    pub fn procedural() -> Self {
        Self {
            memory_type: MemoryType::Procedural,
            lookup_p50_ms: 2,
            lookup_p99_ms: 5,
            recall_p50_ms: 3,
            recall_p99_ms: 8,
            supports_scan: false,
            supports_load: false,
            supports_pattern: true,
            supports_embedding: false,
        }
    }

    /// Create info for Semantic backend
    pub fn semantic() -> Self {
        Self {
            memory_type: MemoryType::Semantic,
            lookup_p50_ms: 5,
            lookup_p99_ms: 15,
            recall_p50_ms: 10,
            recall_p99_ms: 25,
            supports_scan: false,
            supports_load: false,
            supports_pattern: false,
            supports_embedding: true,
        }
    }

    /// Create info for Episodic backend
    pub fn episodic() -> Self {
        Self {
            memory_type: MemoryType::Episodic,
            lookup_p50_ms: 15,
            lookup_p99_ms: 40,
            recall_p50_ms: 20,
            recall_p99_ms: 50,
            supports_scan: false,
            supports_load: false,
            supports_pattern: false,
            supports_embedding: false,
        }
    }
}

/// Backend trait for all memory types
///
/// Each memory type implements this trait to provide storage and retrieval.
/// Not all operations are supported by all backends - unsupported operations
/// return `AdbError::UnsupportedOperation`.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Store a record in this backend
    async fn store(
        &self,
        key: &str,
        data: serde_json::Value,
        scope: Scope,
        namespace: Option<&str>,
        ttl: Option<Duration>,
    ) -> AdbResult<MemoryRecord>;

    /// Lookup records by exact key or predicate
    async fn lookup(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>>;

    /// Recall records by similarity or condition
    async fn recall(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>>;

    /// Delete records matching predicate
    /// Returns the number of records deleted
    async fn forget(&self, predicate: &Predicate) -> AdbResult<u64>;

    /// Update records matching predicate
    /// Returns the number of records updated
    async fn update(
        &self,
        predicate: &Predicate,
        data: serde_json::Value,
    ) -> AdbResult<u64>;

    /// Full scan (Working memory only)
    /// Returns all records, optionally filtered by window
    async fn scan(&self, window: Option<&Window>) -> AdbResult<Vec<MemoryRecord>> {
        Err(AdbError::UnsupportedOperation("scan".to_string()))
    }

    /// Load with ranking (Tools only)
    /// Returns tools sorted by ranking, filtered by predicate
    async fn load(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        Err(AdbError::UnsupportedOperation("load".to_string()))
    }

    /// Get backend information
    fn info(&self) -> BackendInfo;

    /// Get memory type this backend handles
    fn memory_type(&self) -> MemoryType {
        self.info().memory_type
    }

    /// Get record count
    async fn count(&self) -> usize;

    /// Clear all records (use with caution)
    async fn clear(&self) -> AdbResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_info() {
        let info = BackendInfo::working();
        assert_eq!(info.memory_type, MemoryType::Working);
        assert!(info.supports_scan);
        assert!(!info.supports_load);

        let info = BackendInfo::tools();
        assert_eq!(info.memory_type, MemoryType::Tools);
        assert!(!info.supports_scan);
        assert!(info.supports_load);
    }
}
