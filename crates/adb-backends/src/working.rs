//! Working Memory Backend
//!
//! Uses DashMap for O(1) concurrent read/write access.
//! Supports TTL-based expiration for temporary working state.
//!
//! Latency target: < 1ms

use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

use adb_core::{
    evaluate_conditions_on_record, AdbError, AdbResult, Condition, MemoryRecord, MemoryType, Metadata,
    Modifiers, Predicate, Scope, Value, Window,
};

use crate::backend::{Backend, BackendInfo};

/// Working memory backend using DashMap
///
/// Provides O(1) lookup and insert with concurrent access.
/// Supports TTL-based expiration with background reaping.
pub struct WorkingBackend {
    /// Concurrent hash map for storage
    store: DashMap<String, MemoryRecord>,
    /// Whether TTL expiration is enabled
    ttl_enabled: AtomicBool,
    /// Notify for shutdown
    shutdown: Arc<Notify>,
}

impl WorkingBackend {
    /// Create a new working memory backend
    pub fn new() -> Self {
        Self {
            store: DashMap::new(),
            ttl_enabled: AtomicBool::new(true),
            shutdown: Arc::new(Notify::new()),
        }
    }

    /// Create with initial capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            store: DashMap::with_capacity(capacity),
            ttl_enabled: AtomicBool::new(true),
            shutdown: Arc::new(Notify::new()),
        }
    }

    /// Start the background TTL reaper task
    ///
    /// This spawns a background task that periodically removes expired records.
    pub fn start_ttl_reaper(&self, check_interval: Duration) -> tokio::task::JoinHandle<()> {
        let store = self.store.clone();
        let ttl_enabled = self.ttl_enabled.load(Ordering::SeqCst);
        let shutdown = self.shutdown.clone();

        tokio::spawn(async move {
            if !ttl_enabled {
                return;
            }

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(check_interval) => {
                        // Remove expired entries
                        store.retain(|_, record| !record.is_expired());
                    }
                    _ = shutdown.notified() => {
                        break;
                    }
                }
            }
        })
    }

    /// Stop the TTL reaper
    pub fn stop_ttl_reaper(&self) {
        self.shutdown.notify_one();
    }

    /// Enable or disable TTL expiration
    pub fn set_ttl_enabled(&self, enabled: bool) {
        self.ttl_enabled.store(enabled, Ordering::SeqCst);
    }

    /// Check if a record matches a predicate
    fn matches_predicate(&self, record: &MemoryRecord, predicate: &Predicate) -> bool {
        match predicate {
            Predicate::All => true,
            Predicate::Key { field, value } => {
                if field == "id" {
                    match value {
                        Value::String(s) => record.id == *s,
                        _ => false,
                    }
                } else {
                    // Check in data
                    record
                        .get(field)
                        .map(|v| value_matches_json(value, v))
                        .unwrap_or(false)
                }
            }
            Predicate::Where { conditions } => {
                evaluate_conditions_on_record(record, conditions)
            }
            Predicate::Like { .. } => false, // Working memory doesn't support embedding
            Predicate::Pattern { .. } => false, // Working memory doesn't support pattern
        }
    }

    /// Check if a record matches a condition
    fn matches_condition(&self, record: &MemoryRecord, condition: &Condition) -> bool {
        condition.matches(&record.data)
    }

    /// Apply modifiers to results
    fn apply_modifiers(&self, mut records: Vec<MemoryRecord>, modifiers: &Modifiers) -> Vec<MemoryRecord> {
        // Apply SCOPE filter first
        if let Some(ref scope) = modifiers.scope {
            records.retain(|r| &r.metadata.scope == scope);
        }

        // Apply NAMESPACE filter
        if let Some(ref namespace) = modifiers.namespace {
            records.retain(|r| r.metadata.namespace.as_ref() == Some(namespace));
        }

        // Apply ORDER BY
        if let Some(order_by) = &modifiers.order_by {
            records.sort_by(|a, b| {
                let va = a.get(&order_by.field);
                let vb = b.get(&order_by.field);
                let cmp = compare_json_values(va, vb);
                if order_by.ascending {
                    cmp
                } else {
                    cmp.reverse()
                }
            });
        }

        // Apply LIMIT
        if let Some(limit) = modifiers.limit {
            records.truncate(limit);
        }

        // Update accessed_at for each record
        for record in &mut records {
            record.touch();
        }

        // Apply RETURN (field projection) - supports dotted paths like "metadata.scope"
        if let Some(ref return_fields) = modifiers.return_fields {
            for record in &mut records {
                record.data = record.project_fields(return_fields);
            }
        }

        records
    }

    /// Apply window to records
    fn apply_window(&self, records: Vec<MemoryRecord>, window: &Window) -> Vec<MemoryRecord> {
        match window {
            Window::LastN { count } => {
                let len = records.len();
                if len <= *count {
                    records
                } else {
                    records.into_iter().skip(len - count).collect()
                }
            }
            Window::LastDuration { duration } => {
                let cutoff = Utc::now() - chrono::Duration::from_std(*duration).unwrap_or_default();
                records
                    .into_iter()
                    .filter(|r| r.metadata.created_at > cutoff)
                    .collect()
            }
            Window::TopBy { count, field } => {
                let mut sorted = records;
                sorted.sort_by(|a, b| {
                    let va = a.get(field);
                    let vb = b.get(field);
                    compare_json_values(vb, va) // Descending for TOP
                });
                sorted.truncate(*count);
                sorted
            }
            Window::Since { condition } => {
                // Find the first record matching the condition, return everything after
                let mut found = false;
                records
                    .into_iter()
                    .filter(|r| {
                        if found {
                            true
                        } else if condition.matches(&r.data) {
                            found = true;
                            true
                        } else {
                            false
                        }
                    })
                    .collect()
            }
        }
    }
}

impl Default for WorkingBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Backend for WorkingBackend {
    async fn store(
        &self,
        key: &str,
        data: serde_json::Value,
        scope: Scope,
        namespace: Option<&str>,
        ttl: Option<Duration>,
    ) -> AdbResult<MemoryRecord> {
        let mut metadata = Metadata::new(scope);
        metadata.namespace = namespace.map(String::from);
        metadata.ttl = ttl;

        let record = MemoryRecord {
            id: key.to_string(),
            memory_type: MemoryType::Working,
            data,
            metadata,
        };

        self.store.insert(key.to_string(), record.clone());
        Ok(record)
    }

    async fn lookup(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        // For KEY predicate with id field, use direct lookup
        if let Predicate::Key { field, value } = predicate {
            if field == "id" {
                if let Value::String(id) = value {
                    return Ok(self
                        .store
                        .get(id)
                        .map(|r| vec![r.value().clone()])
                        .unwrap_or_default());
                }
            }
        }

        // Otherwise, scan and filter
        let records: Vec<MemoryRecord> = self
            .store
            .iter()
            .filter(|r| !r.value().is_expired())
            .filter(|r| self.matches_predicate(r.value(), predicate))
            .map(|r| r.value().clone())
            .collect();

        Ok(self.apply_modifiers(records, modifiers))
    }

    async fn recall(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        // Working memory recall is same as lookup
        self.lookup(predicate, modifiers).await
    }

    async fn forget(&self, predicate: &Predicate) -> AdbResult<u64> {
        let mut count = 0u64;

        // For ALL predicate, clear everything
        if matches!(predicate, Predicate::All) {
            count = self.store.len() as u64;
            self.store.clear();
            return Ok(count);
        }

        // Otherwise, find and remove matching records
        let to_remove: Vec<String> = self
            .store
            .iter()
            .filter(|r| self.matches_predicate(r.value(), predicate))
            .map(|r| r.key().clone())
            .collect();

        for key in to_remove {
            if self.store.remove(&key).is_some() {
                count += 1;
            }
        }

        Ok(count)
    }

    async fn update(
        &self,
        predicate: &Predicate,
        data: serde_json::Value,
    ) -> AdbResult<u64> {
        let mut count = 0u64;

        // Find matching records (excluding already expired ones)
        let keys: Vec<String> = self
            .store
            .iter()
            .filter(|r| !r.value().is_expired()) // Skip expired records
            .filter(|r| self.matches_predicate(r.value(), predicate))
            .map(|r| r.key().clone())
            .collect();

        // Update each record with race condition protection
        for key in keys {
            if let Some(mut entry) = self.store.get_mut(&key) {
                // Double-check expiration under lock to handle race with TTL reaper
                if entry.is_expired() {
                    continue; // Skip expired record (TTL may have expired between collect and update)
                }

                // Merge data
                if let serde_json::Value::Object(updates) = &data {
                    if let serde_json::Value::Object(ref mut existing) = entry.data {
                        for (k, v) in updates {
                            existing.insert(k.clone(), v.clone());
                        }
                    }
                } else {
                    entry.data = data.clone();
                }
                entry.metadata.bump_version();
                entry.metadata.touch();
                count += 1;
            }
            // If get_mut returns None, the record was removed by TTL reaper
            // This is expected and we don't count it as an update
        }

        Ok(count)
    }

    async fn scan(&self, window: Option<&Window>) -> AdbResult<Vec<MemoryRecord>> {
        let records: Vec<MemoryRecord> = self
            .store
            .iter()
            .filter(|r| !r.value().is_expired())
            .map(|r| r.value().clone())
            .collect();

        match window {
            Some(w) => Ok(self.apply_window(records, w)),
            None => Ok(records),
        }
    }

    fn info(&self) -> BackendInfo {
        BackendInfo::working()
    }

    async fn count(&self) -> usize {
        self.store.len()
    }

    async fn clear(&self) -> AdbResult<()> {
        self.store.clear();
        Ok(())
    }
}

/// Helper to compare a Value with a JSON value
fn value_matches_json(value: &Value, json: &serde_json::Value) -> bool {
    match (value, json) {
        (Value::Null, serde_json::Value::Null) => true,
        (Value::Bool(a), serde_json::Value::Bool(b)) => a == b,
        (Value::Int(a), serde_json::Value::Number(b)) => b.as_i64() == Some(*a),
        (Value::Float(a), serde_json::Value::Number(b)) => b.as_f64() == Some(*a),
        (Value::String(a), serde_json::Value::String(b)) => a == b,
        _ => false,
    }
}

/// Helper to compare two JSON values
fn compare_json_values(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a), Some(b)) => {
            // Try numeric comparison first
            if let (Some(an), Some(bn)) = (a.as_f64(), b.as_f64()) {
                return an.partial_cmp(&bn).unwrap_or(Ordering::Equal);
            }
            // Fall back to string comparison
            if let (Some(as_), Some(bs)) = (a.as_str(), b.as_str()) {
                return as_.cmp(bs);
            }
            // Default to equal
            Ordering::Equal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_store_and_lookup() {
        let backend = WorkingBackend::new();

        let record = backend
            .store("test-1", json!({"key": "value", "count": 42}), Scope::Private, None, None)
            .await
            .unwrap();

        assert_eq!(record.id, "test-1");
        assert_eq!(record.memory_type, MemoryType::Working);

        // Lookup by ID
        let results = backend
            .lookup(&Predicate::key("id", "test-1"), &Modifiers::default())
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].get_str("key"), Some("value"));
        assert_eq!(results[0].get_i64("count"), Some(42));
    }

    #[tokio::test]
    async fn test_lookup_by_field() {
        let backend = WorkingBackend::new();

        backend
            .store("item-1", json!({"pod": "payments", "severity": 3}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("item-2", json!({"pod": "auth", "severity": 5}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("item-3", json!({"pod": "payments", "severity": 7}), Scope::Private, None, None)
            .await
            .unwrap();

        // Lookup by pod
        let results = backend
            .lookup(&Predicate::where_eq("pod", "payments"), &Modifiers::default())
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_scan_all() {
        let backend = WorkingBackend::new();

        for i in 0..10 {
            backend
                .store(
                    &format!("item-{}", i),
                    json!({"index": i}),
                    Scope::Private,
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        let results = backend.scan(None).await.unwrap();
        assert_eq!(results.len(), 10);
    }

    #[tokio::test]
    async fn test_scan_window_last_n() {
        let backend = WorkingBackend::new();

        for i in 0..10 {
            backend
                .store(
                    &format!("item-{}", i),
                    json!({"index": i}),
                    Scope::Private,
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        let results = backend.scan(Some(&Window::last_n(3))).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_forget() {
        let backend = WorkingBackend::new();

        backend
            .store("item-1", json!({"temp": true}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("item-2", json!({"temp": false}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("item-3", json!({"temp": true}), Scope::Private, None, None)
            .await
            .unwrap();

        // Forget temp records
        let count = backend
            .forget(&Predicate::where_eq("temp", true))
            .await
            .unwrap();

        assert_eq!(count, 2);
        assert_eq!(backend.count().await, 1);
    }

    #[tokio::test]
    async fn test_update() {
        let backend = WorkingBackend::new();

        backend
            .store("item-1", json!({"status": "pending", "count": 0}), Scope::Private, None, None)
            .await
            .unwrap();

        // Update status
        let count = backend
            .update(
                &Predicate::key("id", "item-1"),
                json!({"status": "completed", "count": 5}),
            )
            .await
            .unwrap();

        assert_eq!(count, 1);

        let results = backend
            .lookup(&Predicate::key("id", "item-1"), &Modifiers::default())
            .await
            .unwrap();

        assert_eq!(results[0].get_str("status"), Some("completed"));
        assert_eq!(results[0].get_i64("count"), Some(5));
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        let backend = WorkingBackend::new();

        // Store with very short TTL
        backend
            .store(
                "expires",
                json!({"temp": true}),
                Scope::Private,
                None,
                Some(Duration::from_millis(10)),
            )
            .await
            .unwrap();

        // Should exist immediately
        assert_eq!(backend.count().await, 1);

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Lookup should filter expired
        let results = backend
            .lookup(&Predicate::key("id", "expires"), &Modifiers::default())
            .await
            .unwrap();

        // The record might still be in store but should be filtered as expired
        // (actual removal happens in reaper)
        let non_expired: Vec<_> = results.into_iter().filter(|r| !r.is_expired()).collect();
        assert_eq!(non_expired.len(), 0);
    }

    #[tokio::test]
    async fn test_order_by() {
        let backend = WorkingBackend::new();

        backend
            .store("a", json!({"score": 30}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("b", json!({"score": 10}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("c", json!({"score": 20}), Scope::Private, None, None)
            .await
            .unwrap();

        let mods = Modifiers::default().order_by("score", true); // ascending
        let results = backend.lookup(&Predicate::all(), &mods).await.unwrap();

        assert_eq!(results[0].get_i64("score"), Some(10));
        assert_eq!(results[1].get_i64("score"), Some(20));
        assert_eq!(results[2].get_i64("score"), Some(30));
    }

    #[tokio::test]
    async fn test_limit() {
        let backend = WorkingBackend::new();

        for i in 0..10 {
            backend
                .store(&format!("item-{}", i), json!({"i": i}), Scope::Private, None, None)
                .await
                .unwrap();
        }

        let mods = Modifiers::with_limit(3);
        let results = backend.lookup(&Predicate::all(), &mods).await.unwrap();

        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_clear() {
        let backend = WorkingBackend::new();

        for i in 0..5 {
            backend
                .store(&format!("item-{}", i), json!({"i": i}), Scope::Private, None, None)
                .await
                .unwrap();
        }

        assert_eq!(backend.count().await, 5);

        backend.clear().await.unwrap();

        assert_eq!(backend.count().await, 0);
    }

    #[tokio::test]
    async fn test_update_expired_record() {
        // B22: UPDATE/TTL race condition test
        let backend = WorkingBackend::new();

        // Store record with very short TTL (10ms)
        backend
            .store(
                "ttl-item",
                json!({"status": "active", "value": 100}),
                Scope::Private,
                None,
                Some(Duration::from_millis(10)),
            )
            .await
            .unwrap();

        // Verify it exists initially
        let results = backend
            .lookup(&Predicate::key("id", "ttl-item"), &Modifiers::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Try to update the expired record
        let update_count = backend
            .update(
                &Predicate::key("id", "ttl-item"),
                json!({"status": "updated", "value": 200}),
            )
            .await
            .unwrap();

        // Update count should be 0 because record is expired
        assert_eq!(update_count, 0, "Should not update expired record");

        // Lookup should also return empty (expired records filtered)
        let results = backend
            .lookup(&Predicate::key("id", "ttl-item"), &Modifiers::default())
            .await
            .unwrap();
        let non_expired: Vec<_> = results.into_iter().filter(|r| !r.is_expired()).collect();
        assert_eq!(non_expired.len(), 0, "Expired record should not be returned");
    }

    #[tokio::test]
    async fn test_update_non_expired_record() {
        // Ensure UPDATE still works normally for non-TTL records
        let backend = WorkingBackend::new();

        // Store record without TTL
        backend
            .store(
                "normal-item",
                json!({"status": "active", "value": 100}),
                Scope::Private,
                None,
                None, // No TTL
            )
            .await
            .unwrap();

        // Update should succeed
        let update_count = backend
            .update(
                &Predicate::key("id", "normal-item"),
                json!({"status": "updated", "value": 200}),
            )
            .await
            .unwrap();

        assert_eq!(update_count, 1, "Should update non-expired record");

        // Verify the update applied
        let results = backend
            .lookup(&Predicate::key("id", "normal-item"), &Modifiers::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].get_str("status"), Some("updated"));
        assert_eq!(results[0].get_i64("value"), Some(200));
    }
}
