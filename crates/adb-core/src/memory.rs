//! Memory types and records for ADB
//!
//! ADB supports five cognitive memory types:
//! - Working: Current task state, active context (< 1ms)
//! - Tools: Tool registry with dynamic rankings (< 2ms)
//! - Procedural: How-to knowledge, runbooks (< 5ms)
//! - Semantic: Facts, concepts, world knowledge (< 20ms)
//! - Episodic: Time-series events, outcomes, history (< 50ms)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::scope::Scope;

/// The five cognitive memory types in ADB
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MemoryType {
    /// Current task state, active context. O(1) lookup/insert.
    Working,
    /// Tool registry with schema, ranking, relevance scores.
    Tools,
    /// How-to knowledge stored as a graph of patterns and procedures.
    Procedural,
    /// Facts and concepts with embedding vectors for similarity search.
    Semantic,
    /// Time-series events optimized for temporal queries.
    Episodic,
}

impl MemoryType {
    /// Get the backend name for this memory type
    pub fn backend_name(&self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Tools => "tools",
            Self::Procedural => "procedural",
            Self::Semantic => "semantic",
            Self::Episodic => "episodic",
        }
    }

    /// Get the expected P50 latency in milliseconds
    pub fn latency_p50_ms(&self) -> u64 {
        match self {
            Self::Working => 0,
            Self::Tools => 1,
            Self::Procedural => 2,
            Self::Semantic => 10,
            Self::Episodic => 20,
        }
    }

    /// Get the expected P99 latency in milliseconds
    pub fn latency_p99_ms(&self) -> u64 {
        match self {
            Self::Working => 1,
            Self::Tools => 2,
            Self::Procedural => 5,
            Self::Semantic => 25,
            Self::Episodic => 50,
        }
    }

    /// Parse from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "WORKING" => Some(Self::Working),
            "TOOLS" => Some(Self::Tools),
            "PROCEDURAL" => Some(Self::Procedural),
            "SEMANTIC" => Some(Self::Semantic),
            "EPISODIC" => Some(Self::Episodic),
            _ => None,
        }
    }

    /// Get all memory types
    pub fn all() -> &'static [Self] {
        &[
            Self::Working,
            Self::Tools,
            Self::Procedural,
            Self::Semantic,
            Self::Episodic,
        ]
    }
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Working => write!(f, "WORKING"),
            Self::Tools => write!(f, "TOOLS"),
            Self::Procedural => write!(f, "PROCEDURAL"),
            Self::Semantic => write!(f, "SEMANTIC"),
            Self::Episodic => write!(f, "EPISODIC"),
        }
    }
}

/// Metadata associated with every memory record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// When the record was created
    pub created_at: DateTime<Utc>,
    /// When the record was last accessed (read)
    pub accessed_at: DateTime<Utc>,
    /// Isolation scope for the record
    pub scope: Scope,
    /// Agent identity namespace
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Time-to-live (optional, for expiring records)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "option_duration_serde")]
    pub ttl: Option<Duration>,
    /// Record version (for optimistic concurrency)
    pub version: u64,
}

impl Metadata {
    /// Create new metadata with current timestamp
    pub fn new(scope: Scope) -> Self {
        let now = Utc::now();
        Self {
            created_at: now,
            accessed_at: now,
            scope,
            namespace: None,
            ttl: None,
            version: 1,
        }
    }

    /// Create metadata with namespace
    pub fn with_namespace(scope: Scope, namespace: impl Into<String>) -> Self {
        let mut meta = Self::new(scope);
        meta.namespace = Some(namespace.into());
        meta
    }

    /// Create metadata with TTL
    pub fn with_ttl(scope: Scope, ttl: Duration) -> Self {
        let mut meta = Self::new(scope);
        meta.ttl = Some(ttl);
        meta
    }

    /// Check if record has expired
    pub fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl {
            let expires_at = self.created_at + chrono::Duration::from_std(ttl).unwrap_or_default();
            Utc::now() > expires_at
        } else {
            false
        }
    }

    /// Update accessed_at to now
    pub fn touch(&mut self) {
        self.accessed_at = Utc::now();
    }

    /// Increment version
    pub fn bump_version(&mut self) {
        self.version += 1;
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self::new(Scope::Private)
    }
}

/// A memory record stored in ADB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    /// Unique identifier for this record
    pub id: String,
    /// Which memory type this record belongs to
    pub memory_type: MemoryType,
    /// The actual data payload (JSON)
    pub data: serde_json::Value,
    /// Record metadata
    pub metadata: Metadata,
}

impl MemoryRecord {
    /// Create a new memory record
    pub fn new(
        id: impl Into<String>,
        memory_type: MemoryType,
        data: serde_json::Value,
        scope: Scope,
    ) -> Self {
        Self {
            id: id.into(),
            memory_type,
            data,
            metadata: Metadata::new(scope),
        }
    }

    /// Create a new record with all options
    pub fn with_options(
        id: impl Into<String>,
        memory_type: MemoryType,
        data: serde_json::Value,
        scope: Scope,
        namespace: Option<String>,
        ttl: Option<Duration>,
    ) -> Self {
        let mut record = Self::new(id, memory_type, data, scope);
        record.metadata.namespace = namespace;
        record.metadata.ttl = ttl;
        record
    }

    /// Check if this record has expired
    pub fn is_expired(&self) -> bool {
        self.metadata.is_expired()
    }

    /// Get a field from the data payload
    pub fn get(&self, field: &str) -> Option<&serde_json::Value> {
        self.data.get(field)
    }

    /// Get a string field from the data payload
    pub fn get_str(&self, field: &str) -> Option<&str> {
        self.data.get(field).and_then(|v| v.as_str())
    }

    /// Get an i64 field from the data payload
    pub fn get_i64(&self, field: &str) -> Option<i64> {
        self.data.get(field).and_then(|v| v.as_i64())
    }

    /// Get an f64 field from the data payload
    pub fn get_f64(&self, field: &str) -> Option<f64> {
        self.data.get(field).and_then(|v| v.as_f64())
    }

    /// Update accessed_at timestamp
    pub fn touch(&mut self) {
        self.metadata.touch();
    }
}

// Custom serde for Option<Duration>
mod option_duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => d.as_millis().serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<u64> = Option::deserialize(deserializer)?;
        Ok(opt.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_memory_type_parsing() {
        assert_eq!(MemoryType::from_str("WORKING"), Some(MemoryType::Working));
        assert_eq!(MemoryType::from_str("working"), Some(MemoryType::Working));
        assert_eq!(MemoryType::from_str("Episodic"), Some(MemoryType::Episodic));
        assert_eq!(MemoryType::from_str("invalid"), None);
    }

    #[test]
    fn test_memory_type_display() {
        assert_eq!(MemoryType::Working.to_string(), "WORKING");
        assert_eq!(MemoryType::Semantic.to_string(), "SEMANTIC");
    }

    #[test]
    fn test_memory_record_creation() {
        let record = MemoryRecord::new(
            "test-1",
            MemoryType::Working,
            json!({"key": "value", "count": 42}),
            Scope::Private,
        );

        assert_eq!(record.id, "test-1");
        assert_eq!(record.memory_type, MemoryType::Working);
        assert_eq!(record.get_str("key"), Some("value"));
        assert_eq!(record.get_i64("count"), Some(42));
        assert!(!record.is_expired());
    }

    #[test]
    fn test_metadata_ttl() {
        let meta = Metadata::with_ttl(Scope::Private, Duration::from_millis(1));

        // Sleep briefly and check expiration
        std::thread::sleep(Duration::from_millis(10));
        assert!(meta.is_expired());
    }

    #[test]
    fn test_metadata_touch() {
        let mut meta = Metadata::new(Scope::Private);
        let original_accessed = meta.accessed_at;

        std::thread::sleep(Duration::from_millis(1));
        meta.touch();

        assert!(meta.accessed_at > original_accessed);
    }

    #[test]
    fn test_record_serialization() {
        let record = MemoryRecord::new(
            "test-1",
            MemoryType::Episodic,
            json!({"event": "login"}),
            Scope::Shared,
        );

        let json_str = serde_json::to_string(&record).unwrap();
        let parsed: MemoryRecord = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed.id, record.id);
        assert_eq!(parsed.memory_type, record.memory_type);
    }
}
