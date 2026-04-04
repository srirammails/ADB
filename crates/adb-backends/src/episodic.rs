//! Episodic Memory Backend
//!
//! Time-series storage for events and outcomes. Optimized for temporal queries
//! using DataFusion for analytical operations.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use arrow::array::{ArrayRef, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use chrono::Utc;
use datafusion::prelude::*;
use parking_lot::RwLock;

use adb_core::{
    evaluate_conditions_on_record, AdbError, AdbResult, MemoryRecord, MemoryType, Modifiers, Predicate,
    Scope, Value, Window,
};

use crate::backend::{Backend, BackendInfo};

/// Episodic memory backend using DataFusion for time-series queries
pub struct EpisodicBackend {
    /// Records stored by timestamp for efficient temporal ordering
    /// Key is (timestamp_millis, id) for stable ordering
    records: RwLock<BTreeMap<(i64, String), MemoryRecord>>,
    /// Index from id to timestamp for fast lookups
    id_index: RwLock<std::collections::HashMap<String, i64>>,
    /// DataFusion session context for analytical queries
    ctx: Arc<SessionContext>,
}

impl EpisodicBackend {
    /// Create a new episodic backend
    pub fn new() -> Self {
        Self {
            records: RwLock::new(BTreeMap::new()),
            id_index: RwLock::new(std::collections::HashMap::new()),
            ctx: Arc::new(SessionContext::new()),
        }
    }

    /// Get records within a time window
    pub fn get_window(&self, window: &Window) -> Vec<MemoryRecord> {
        let records = self.records.read();
        let now = Utc::now();

        match window {
            Window::LastN { count } => {
                // Get last N records (most recent first)
                records.values().rev().take(*count).cloned().collect()
            }
            Window::LastDuration { duration } => {
                let cutoff = now - chrono::Duration::from_std(*duration).unwrap_or_default();
                let cutoff_millis = cutoff.timestamp_millis();

                records
                    .range((cutoff_millis, String::new())..)
                    .map(|(_, r)| r.clone())
                    .collect()
            }
            Window::TopBy { count, field } => {
                // Get top N by field value
                let mut sorted: Vec<_> = records.values().cloned().collect();
                sorted.sort_by(|a, b| {
                    let a_val = a.data.get(field);
                    let b_val = b.data.get(field);
                    compare_json_values_desc(a_val, b_val)
                });
                sorted.truncate(*count);
                sorted
            }
            Window::Since { condition } => {
                // Get all records since condition is met
                let mut found_start = false;
                let mut results = Vec::new();

                for (_, record) in records.iter() {
                    if !found_start && condition.matches(&record.data) {
                        found_start = true;
                    }
                    if found_start {
                        results.push(record.clone());
                    }
                }
                results
            }
        }
    }

    /// Create Arrow schema for episodic records
    fn create_schema() -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new(
                "timestamp",
                DataType::Timestamp(TimeUnit::Millisecond, None),
                false,
            ),
            Field::new("data", DataType::Utf8, false),
        ])
    }

    /// Convert records to Arrow RecordBatch for DataFusion queries
    fn records_to_batch(&self, records: &[MemoryRecord]) -> AdbResult<RecordBatch> {
        let schema = Arc::new(Self::create_schema());

        let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
        let timestamps: Vec<i64> = records
            .iter()
            .map(|r| r.metadata.created_at.timestamp_millis())
            .collect();
        let data: Vec<String> = records
            .iter()
            .map(|r| serde_json::to_string(&r.data).unwrap_or_default())
            .collect();

        let id_array: ArrayRef = Arc::new(StringArray::from(ids));
        let timestamp_array: ArrayRef = Arc::new(TimestampMillisecondArray::from(timestamps));
        let data_array: ArrayRef = Arc::new(StringArray::from(
            data.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        ));

        RecordBatch::try_new(schema, vec![id_array, timestamp_array, data_array])
            .map_err(|e| AdbError::QueryError(format!("Failed to create batch: {}", e)))
    }

    /// Execute aggregation query using DataFusion
    pub async fn aggregate(
        &self,
        records: Vec<MemoryRecord>,
        agg_funcs: &[adb_core::AggregateFunc],
    ) -> AdbResult<serde_json::Value> {
        // Build aggregation results manually for simplicity
        let mut results = serde_json::Map::new();

        for agg in agg_funcs {
            let alias = agg
                .alias
                .clone()
                .unwrap_or_else(|| format!("{:?}", agg.func).to_lowercase());

            let value = match agg.func {
                adb_core::AggregateFuncType::Count => {
                    serde_json::Value::Number(records.len().into())
                }
                adb_core::AggregateFuncType::Sum => {
                    if let Some(ref field) = agg.field {
                        let values: Vec<f64> = records
                            .iter()
                            .filter_map(|r| extract_field_value(r, field))
                            .filter_map(|v| json_to_f64(v))
                            .collect();

                        if values.is_empty() {
                            // No values found - return 0 for SUM (SQL semantics)
                            serde_json::Value::Number(serde_json::Number::from(0))
                        } else {
                            let sum: f64 = values.iter().sum();
                            // Normalize -0.0 to 0.0
                            let sum = if sum == 0.0 { 0.0 } else { sum };
                            serde_json::Number::from_f64(sum)
                                .map(serde_json::Value::Number)
                                .unwrap_or(serde_json::Value::Number(serde_json::Number::from(0)))
                        }
                    } else {
                        serde_json::Value::Null
                    }
                }
                adb_core::AggregateFuncType::Avg => {
                    if let Some(ref field) = agg.field {
                        let values: Vec<f64> = records
                            .iter()
                            .filter_map(|r| extract_field_value(r, field))
                            .filter_map(|v| json_to_f64(v))
                            .collect();

                        if values.is_empty() {
                            serde_json::Value::Null
                        } else {
                            let avg = values.iter().sum::<f64>() / values.len() as f64;
                            serde_json::Number::from_f64(avg)
                                .map(serde_json::Value::Number)
                                .unwrap_or(serde_json::Value::Null)
                        }
                    } else {
                        serde_json::Value::Null
                    }
                }
                adb_core::AggregateFuncType::Min => {
                    if let Some(ref field) = agg.field {
                        let values: Vec<f64> = records
                            .iter()
                            .filter_map(|r| extract_field_value(r, field))
                            .filter_map(|v| json_to_f64(v))
                            .collect();

                        if values.is_empty() {
                            serde_json::Value::Null
                        } else {
                            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
                            serde_json::Number::from_f64(min)
                                .map(serde_json::Value::Number)
                                .unwrap_or(serde_json::Value::Null)
                        }
                    } else {
                        serde_json::Value::Null
                    }
                }
                adb_core::AggregateFuncType::Max => {
                    if let Some(ref field) = agg.field {
                        let values: Vec<f64> = records
                            .iter()
                            .filter_map(|r| extract_field_value(r, field))
                            .filter_map(|v| json_to_f64(v))
                            .collect();

                        if values.is_empty() {
                            serde_json::Value::Null
                        } else {
                            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                            serde_json::Number::from_f64(max)
                                .map(serde_json::Value::Number)
                                .unwrap_or(serde_json::Value::Null)
                        }
                    } else {
                        serde_json::Value::Null
                    }
                }
            };

            results.insert(alias, value);
        }

        Ok(serde_json::Value::Object(results))
    }

    /// Apply modifiers to results
    fn apply_modifiers(
        &self,
        mut results: Vec<MemoryRecord>,
        modifiers: &Modifiers,
    ) -> Vec<MemoryRecord> {
        // Apply SCOPE filter first
        if let Some(ref scope) = modifiers.scope {
            results.retain(|r| &r.metadata.scope == scope);
        }

        // Apply NAMESPACE filter
        if let Some(ref namespace) = modifiers.namespace {
            results.retain(|r| r.metadata.namespace.as_ref() == Some(namespace));
        }

        // Apply window if present
        if let Some(ref window) = modifiers.window {
            results = self.filter_by_window(&results, window);
        }

        // Apply ORDER BY
        if let Some(ref order_by) = modifiers.order_by {
            results.sort_by(|a, b| {
                let a_val = a.data.get(&order_by.field);
                let b_val = b.data.get(&order_by.field);
                let cmp = compare_json_values(a_val, b_val);
                if order_by.ascending {
                    cmp
                } else {
                    cmp.reverse()
                }
            });
        } else {
            // Default: sort by timestamp descending (most recent first)
            results.sort_by(|a, b| b.metadata.created_at.cmp(&a.metadata.created_at));
        }

        // Apply LIMIT
        if let Some(limit) = modifiers.limit {
            results.truncate(limit);
        }

        // Apply RETURN (field projection) - supports dotted paths like "metadata.scope"
        if let Some(ref return_fields) = modifiers.return_fields {
            for record in &mut results {
                record.data = record.project_fields(return_fields);
            }
        }

        results
    }

    /// Filter records by window
    fn filter_by_window(&self, records: &[MemoryRecord], window: &Window) -> Vec<MemoryRecord> {
        let now = Utc::now();

        match window {
            Window::LastN { count } => {
                let mut sorted: Vec<_> = records.to_vec();
                sorted.sort_by(|a, b| b.metadata.created_at.cmp(&a.metadata.created_at));
                sorted.truncate(*count);
                sorted
            }
            Window::LastDuration { duration } => {
                let cutoff = now - chrono::Duration::from_std(*duration).unwrap_or_default();
                records
                    .iter()
                    .filter(|r| r.metadata.created_at >= cutoff)
                    .cloned()
                    .collect()
            }
            Window::TopBy { count, field } => {
                let mut sorted: Vec<_> = records.to_vec();
                sorted.sort_by(|a, b| {
                    let a_val = a.data.get(field);
                    let b_val = b.data.get(field);
                    compare_json_values_desc(b_val, a_val)
                });
                sorted.truncate(*count);
                sorted
            }
            Window::Since { condition } => {
                let mut found_start = false;
                let mut results = Vec::new();

                // Sort by timestamp first
                let mut sorted: Vec<_> = records.to_vec();
                sorted.sort_by(|a, b| a.metadata.created_at.cmp(&b.metadata.created_at));

                for record in sorted {
                    if !found_start && condition.matches(&record.data) {
                        found_start = true;
                    }
                    if found_start {
                        results.push(record);
                    }
                }
                results
            }
        }
    }

    /// Check if a record matches predicate
    fn matches_predicate(&self, record: &MemoryRecord, predicate: &Predicate) -> bool {
        match predicate {
            Predicate::Where { conditions } => evaluate_conditions_on_record(record, conditions),
            Predicate::Key { field, value } => {
                let key_value = value_to_string(value);
                if field == "id" || field == "key" {
                    record.id == key_value
                } else {
                    record
                        .data
                        .get(field)
                        .map(|v| value_to_string_json(v))
                        == Some(key_value)
                }
            }
            Predicate::All => true,
            Predicate::Like { .. } | Predicate::Pattern { .. } => {
                // Not supported for episodic memory
                false
            }
        }
    }

    /// Get timestamp for a record
    fn get_timestamp(record: &MemoryRecord) -> i64 {
        record.metadata.created_at.timestamp_millis()
    }
}

impl Default for EpisodicBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Backend for EpisodicBackend {
    async fn store(
        &self,
        key: &str,
        data: serde_json::Value,
        scope: Scope,
        namespace: Option<&str>,
        ttl: Option<Duration>,
    ) -> AdbResult<MemoryRecord> {
        let record = MemoryRecord::with_options(
            key,
            MemoryType::Episodic,
            data,
            scope,
            namespace.map(String::from),
            ttl,
        );

        let timestamp = Self::get_timestamp(&record);

        // Remove existing record with same id if present
        if let Some(old_ts) = self.id_index.read().get(key).copied() {
            self.records.write().remove(&(old_ts, key.to_string()));
        }

        self.records
            .write()
            .insert((timestamp, key.to_string()), record.clone());
        self.id_index
            .write()
            .insert(key.to_string(), timestamp);

        Ok(record)
    }

    async fn lookup(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        let records = self.records.read();

        let mut results: Vec<MemoryRecord> = match predicate {
            Predicate::Key { field, value } => {
                let key_value = value_to_string(value);
                if field == "id" || field == "key" {
                    // Fast path: lookup by id using index
                    if let Some(ts) = self.id_index.read().get(&key_value).copied() {
                        records
                            .get(&(ts, key_value))
                            .cloned()
                            .into_iter()
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    records
                        .values()
                        .filter(|r| {
                            r.data.get(field).map(|v| value_to_string_json(v))
                                == Some(key_value.clone())
                        })
                        .cloned()
                        .collect()
                }
            }
            Predicate::Where { conditions } => records
                .values()
                .filter(|r| evaluate_conditions_on_record(r, conditions))
                .cloned()
                .collect(),
            Predicate::All => records.values().cloned().collect(),
            Predicate::Like { .. } | Predicate::Pattern { .. } => {
                // Not supported for episodic memory
                Vec::new()
            }
        };

        drop(records);
        results = self.apply_modifiers(results, modifiers);
        Ok(results)
    }

    async fn recall(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        // For episodic memory, recall works the same as lookup
        // but with emphasis on temporal ordering
        self.lookup(predicate, modifiers).await
    }

    async fn update(&self, predicate: &Predicate, data: serde_json::Value) -> AdbResult<u64> {
        let mut records = self.records.write();
        let mut count = 0;

        // Collect keys to update (can't modify while iterating)
        let keys_to_update: Vec<(i64, String)> = records
            .iter()
            .filter(|(_, r)| self.matches_predicate(r, predicate))
            .map(|(k, _)| k.clone())
            .collect();

        for key in keys_to_update {
            if let Some(record) = records.get_mut(&key) {
                // Merge data into existing record
                if let (Some(existing), Some(updates)) =
                    (record.data.as_object_mut(), data.as_object())
                {
                    for (k, v) in updates {
                        existing.insert(k.clone(), v.clone());
                    }
                } else {
                    record.data = data.clone();
                }
                record.metadata.touch();
                record.metadata.bump_version();
                count += 1;
            }
        }

        Ok(count)
    }

    async fn forget(&self, predicate: &Predicate) -> AdbResult<u64> {
        let mut records = self.records.write();
        let mut id_index = self.id_index.write();

        let keys_to_remove: Vec<(i64, String)> = records
            .iter()
            .filter(|(_, r)| self.matches_predicate(r, predicate))
            .map(|(k, _)| k.clone())
            .collect();

        for (ts, id) in &keys_to_remove {
            records.remove(&(*ts, id.clone()));
            id_index.remove(id);
        }

        Ok(keys_to_remove.len() as u64)
    }

    async fn clear(&self) -> AdbResult<()> {
        self.records.write().clear();
        self.id_index.write().clear();
        Ok(())
    }

    async fn count(&self) -> usize {
        self.records.read().len()
    }

    fn info(&self) -> BackendInfo {
        BackendInfo::episodic()
    }
}

/// Convert Value to String for comparisons
fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(arr) => format!("{:?}", arr),
        Value::Variable(v) => format!("${{{}}}", v),
    }
}

/// Convert serde_json::Value to String
fn value_to_string_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        _ => value.to_string(),
    }
}

/// Compare two optional JSON values (ascending)
fn compare_json_values(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
) -> std::cmp::Ordering {
    match (a, b) {
        (Some(serde_json::Value::Number(a)), Some(serde_json::Value::Number(b))) => {
            let a_f = a.as_f64().unwrap_or(0.0);
            let b_f = b.as_f64().unwrap_or(0.0);
            a_f.partial_cmp(&b_f).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Some(serde_json::Value::String(a)), Some(serde_json::Value::String(b))) => a.cmp(b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

/// Compare two optional JSON values (descending)
fn compare_json_values_desc(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
) -> std::cmp::Ordering {
    compare_json_values(a, b).reverse()
}

/// Convert a JSON value to f64, handling both integer and float types
fn json_to_f64(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => {
            // Try as f64 first (for floats), then as i64 (for integers)
            n.as_f64().or_else(|| n.as_i64().map(|i| i as f64))
        }
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// Extract a field value from a MemoryRecord, checking both data and top-level fields
fn extract_field_value<'a>(record: &'a MemoryRecord, field: &str) -> Option<&'a serde_json::Value> {
    // First try to get from record.data directly
    if let Some(value) = record.data.get(field) {
        return Some(value);
    }

    // If data is an object, also check if the field exists
    if let Some(obj) = record.data.as_object() {
        if let Some(value) = obj.get(field) {
            return Some(value);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use adb_core::Condition;
    use serde_json::json;
    use std::time::Duration as StdDuration;

    fn create_event(id: &str, event_type: &str, value: i64) -> MemoryRecord {
        MemoryRecord::new(
            id,
            MemoryType::Episodic,
            json!({
                "event_type": event_type,
                "value": value,
                "pod": "payments"
            }),
            Scope::Private,
        )
    }

    #[test]
    fn test_create_backend() {
        let backend = EpisodicBackend::new();
        assert_eq!(backend.memory_type(), MemoryType::Episodic);
    }

    #[tokio::test]
    async fn test_store_and_lookup() {
        let backend = EpisodicBackend::new();

        let record = backend
            .store(
                "event-1",
                json!({"event_type": "login", "user": "alice"}),
                Scope::Private,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(record.id, "event-1");
        assert_eq!(backend.count().await, 1);

        let results = backend
            .lookup(
                &Predicate::Key {
                    field: "id".to_string(),
                    value: Value::String("event-1".to_string()),
                },
                &Modifiers::default(),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data.get("event_type"), Some(&json!("login")));
    }

    #[tokio::test]
    async fn test_temporal_ordering() {
        let backend = EpisodicBackend::new();

        // Store events with small delays to ensure different timestamps
        for i in 0..5 {
            backend
                .store(
                    &format!("event-{}", i),
                    json!({"seq": i}),
                    Scope::Private,
                    None,
                    None,
                )
                .await
                .unwrap();
            tokio::time::sleep(StdDuration::from_millis(10)).await;
        }

        // Default ordering should be most recent first
        let results = backend
            .lookup(&Predicate::All, &Modifiers::default())
            .await
            .unwrap();

        assert_eq!(results.len(), 5);
        // Most recent first
        assert_eq!(results[0].data.get("seq"), Some(&json!(4)));
        assert_eq!(results[4].data.get("seq"), Some(&json!(0)));
    }

    #[tokio::test]
    async fn test_window_last_n() {
        let backend = EpisodicBackend::new();

        for i in 0..10 {
            backend
                .store(
                    &format!("event-{}", i),
                    json!({"seq": i}),
                    Scope::Private,
                    None,
                    None,
                )
                .await
                .unwrap();
            tokio::time::sleep(StdDuration::from_millis(5)).await;
        }

        let results = backend.get_window(&Window::LastN { count: 3 });
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_window_top_by() {
        let backend = EpisodicBackend::new();

        backend
            .store("e1", json!({"score": 10}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("e2", json!({"score": 50}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("e3", json!({"score": 30}), Scope::Private, None, None)
            .await
            .unwrap();

        let results = backend.get_window(&Window::TopBy {
            count: 2,
            field: "score".to_string(),
        });

        assert_eq!(results.len(), 2);
        // Should have highest scores first
        assert_eq!(results[0].data.get("score"), Some(&json!(50)));
        assert_eq!(results[1].data.get("score"), Some(&json!(30)));
    }

    #[tokio::test]
    async fn test_forget() {
        let backend = EpisodicBackend::new();

        for i in 0..5 {
            backend
                .store(
                    &format!("event-{}", i),
                    json!({"type": if i % 2 == 0 { "a" } else { "b" }}),
                    Scope::Private,
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        assert_eq!(backend.count().await, 5);

        // Forget type "a" events
        let count = backend
            .forget(&Predicate::Where {
                conditions: vec![Condition::eq("type", "a")],
            })
            .await
            .unwrap();

        assert_eq!(count, 3); // events 0, 2, 4
        assert_eq!(backend.count().await, 2);
    }

    #[tokio::test]
    async fn test_update() {
        let backend = EpisodicBackend::new();

        backend
            .store(
                "event-1",
                json!({"status": "pending", "value": 100}),
                Scope::Private,
                None,
                None,
            )
            .await
            .unwrap();

        let count = backend
            .update(
                &Predicate::Key {
                    field: "id".to_string(),
                    value: Value::String("event-1".to_string()),
                },
                json!({"status": "completed"}),
            )
            .await
            .unwrap();

        assert_eq!(count, 1);

        let results = backend
            .lookup(&Predicate::All, &Modifiers::default())
            .await
            .unwrap();

        assert_eq!(results[0].data.get("status"), Some(&json!("completed")));
        // Original value should be preserved
        assert_eq!(results[0].data.get("value"), Some(&json!(100)));
    }

    #[tokio::test]
    async fn test_aggregation() {
        let backend = EpisodicBackend::new();

        for i in 0..5 {
            backend
                .store(
                    &format!("event-{}", i),
                    json!({"value": (i + 1) * 10}),
                    Scope::Private,
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        let records = backend
            .lookup(&Predicate::All, &Modifiers::default())
            .await
            .unwrap();

        let agg_funcs = vec![
            adb_core::AggregateFunc {
                func: adb_core::AggregateFuncType::Count,
                field: None,
                alias: Some("total".to_string()),
            },
            adb_core::AggregateFunc {
                func: adb_core::AggregateFuncType::Sum,
                field: Some("value".to_string()),
                alias: Some("sum_value".to_string()),
            },
            adb_core::AggregateFunc {
                func: adb_core::AggregateFuncType::Avg,
                field: Some("value".to_string()),
                alias: Some("avg_value".to_string()),
            },
        ];

        let result = backend.aggregate(records, &agg_funcs).await.unwrap();

        assert_eq!(result.get("total"), Some(&json!(5)));
        assert_eq!(result.get("sum_value"), Some(&json!(150.0))); // 10+20+30+40+50
        assert_eq!(result.get("avg_value"), Some(&json!(30.0)));
    }

    #[tokio::test]
    async fn test_clear() {
        let backend = EpisodicBackend::new();

        for i in 0..5 {
            backend
                .store(
                    &format!("event-{}", i),
                    json!({"seq": i}),
                    Scope::Private,
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        assert_eq!(backend.count().await, 5);

        backend.clear().await.unwrap();
        assert_eq!(backend.count().await, 0);
    }

    #[tokio::test]
    async fn test_lookup_by_field() {
        let backend = EpisodicBackend::new();

        backend
            .store(
                "e1",
                json!({"pod": "payments", "severity": 3}),
                Scope::Private,
                None,
                None,
            )
            .await
            .unwrap();
        backend
            .store(
                "e2",
                json!({"pod": "auth", "severity": 5}),
                Scope::Private,
                None,
                None,
            )
            .await
            .unwrap();
        backend
            .store(
                "e3",
                json!({"pod": "payments", "severity": 7}),
                Scope::Private,
                None,
                None,
            )
            .await
            .unwrap();

        let results = backend
            .lookup(
                &Predicate::Where {
                    conditions: vec![Condition::eq("pod", "payments")],
                },
                &Modifiers::default(),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_order_by() {
        let backend = EpisodicBackend::new();

        backend
            .store("e1", json!({"score": 30}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("e2", json!({"score": 10}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("e3", json!({"score": 50}), Scope::Private, None, None)
            .await
            .unwrap();

        let results = backend
            .lookup(
                &Predicate::All,
                &Modifiers::default().order_by("score", false), // descending
            )
            .await
            .unwrap();

        assert_eq!(results[0].data.get("score"), Some(&json!(50)));
        assert_eq!(results[1].data.get("score"), Some(&json!(30)));
        assert_eq!(results[2].data.get("score"), Some(&json!(10)));
    }

    #[test]
    fn test_backend_info() {
        let backend = EpisodicBackend::new();
        let info = backend.info();

        assert_eq!(info.memory_type, MemoryType::Episodic);
        assert!(!info.supports_embedding);
        assert!(!info.supports_scan);
    }

    #[tokio::test]
    async fn test_update_existing_record() {
        let backend = EpisodicBackend::new();

        // Store initial record
        backend
            .store("event-1", json!({"v": 1}), Scope::Private, None, None)
            .await
            .unwrap();

        // Store with same id should update
        backend
            .store("event-1", json!({"v": 2}), Scope::Private, None, None)
            .await
            .unwrap();

        // Should still have only one record
        assert_eq!(backend.count().await, 1);

        let results = backend
            .lookup(&Predicate::All, &Modifiers::default())
            .await
            .unwrap();

        assert_eq!(results[0].data.get("v"), Some(&json!(2)));
    }
}
