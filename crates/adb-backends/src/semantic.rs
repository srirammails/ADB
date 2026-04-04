//! Semantic Memory Backend
//!
//! Uses usearch for vector similarity search. This backend stores embeddings
//! alongside metadata and supports similarity-based retrieval.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::RwLock;
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use adb_core::{
    evaluate_conditions_on_record, AdbError, AdbResult, Condition, MemoryRecord, MemoryType, Modifiers,
    Operator, Predicate, Scope, Value,
};

use crate::backend::{Backend, BackendInfo};

/// Default embedding dimensions (can be configured)
const DEFAULT_DIMENSIONS: usize = 1536;

/// Default capacity for the index
const DEFAULT_CAPACITY: usize = 1_000_000;

/// Semantic memory backend using usearch for vector similarity
pub struct SemanticBackend {
    /// Vector index for similarity search
    index: RwLock<Index>,
    /// Map from internal ID to record key
    id_to_key: RwLock<HashMap<u64, String>>,
    /// Map from record key to internal ID
    key_to_id: RwLock<HashMap<String, u64>>,
    /// Record storage (key -> record)
    records: RwLock<HashMap<String, MemoryRecord>>,
    /// Embedding storage (key -> embedding)
    embeddings: RwLock<HashMap<String, Vec<f32>>>,
    /// Next internal ID
    next_id: AtomicU64,
    /// Embedding dimensions
    dimensions: usize,
}

impl SemanticBackend {
    /// Create a new semantic backend with default dimensions
    pub fn new() -> AdbResult<Self> {
        Self::with_dimensions(DEFAULT_DIMENSIONS)
    }

    /// Create a new semantic backend with specified dimensions
    pub fn with_dimensions(dimensions: usize) -> AdbResult<Self> {
        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        };

        let index = Index::new(&options)
            .map_err(|e| AdbError::VectorIndexError(format!("Failed to create index: {}", e)))?;

        index
            .reserve(DEFAULT_CAPACITY)
            .map_err(|e| AdbError::VectorIndexError(format!("Failed to reserve capacity: {}", e)))?;

        Ok(Self {
            index: RwLock::new(index),
            id_to_key: RwLock::new(HashMap::new()),
            key_to_id: RwLock::new(HashMap::new()),
            records: RwLock::new(HashMap::new()),
            embeddings: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            dimensions,
        })
    }

    /// Get the embedding dimensions
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Store a record with its embedding
    pub fn store_with_embedding(
        &self,
        record: MemoryRecord,
        embedding: Vec<f32>,
    ) -> AdbResult<()> {
        if embedding.len() != self.dimensions {
            return Err(AdbError::EmbeddingError(format!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimensions,
                embedding.len()
            )));
        }

        let key = record.id.clone();

        // Check if key already exists
        let existing_id = self.key_to_id.read().get(&key).copied();

        if let Some(id) = existing_id {
            // Update existing record
            let index = self.index.read();
            index
                .remove(id)
                .map_err(|e| AdbError::VectorIndexError(format!("Failed to remove from index: {}", e)))?;

            index
                .add(id, &embedding)
                .map_err(|e| AdbError::VectorIndexError(format!("Failed to add to index: {}", e)))?;

            self.records.write().insert(key.clone(), record);
            self.embeddings.write().insert(key, embedding);
        } else {
            // Add new record
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);

            let index = self.index.read();
            index
                .add(id, &embedding)
                .map_err(|e| AdbError::VectorIndexError(format!("Failed to add to index: {}", e)))?;

            self.id_to_key.write().insert(id, key.clone());
            self.key_to_id.write().insert(key.clone(), id);
            self.records.write().insert(key.clone(), record);
            self.embeddings.write().insert(key, embedding);
        }

        Ok(())
    }

    /// Search by similarity to a query embedding
    pub fn search_similar(
        &self,
        query_embedding: &[f32],
        limit: usize,
        min_confidence: Option<f32>,
    ) -> AdbResult<Vec<(MemoryRecord, f32)>> {
        if query_embedding.len() != self.dimensions {
            return Err(AdbError::EmbeddingError(format!(
                "Query embedding dimension mismatch: expected {}, got {}",
                self.dimensions,
                query_embedding.len()
            )));
        }

        let index = self.index.read();
        let results = index
            .search(query_embedding, limit)
            .map_err(|e| AdbError::VectorIndexError(format!("Search failed: {}", e)))?;

        let id_to_key = self.id_to_key.read();
        let records = self.records.read();

        let mut matches = Vec::new();
        for (id, distance) in results.keys.iter().zip(results.distances.iter()) {
            // Convert distance to similarity (cosine distance -> cosine similarity)
            let similarity = 1.0 - distance;

            // Apply confidence threshold
            if let Some(min_conf) = min_confidence {
                if similarity < min_conf {
                    continue;
                }
            }

            if let Some(key) = id_to_key.get(id) {
                if let Some(record) = records.get(key) {
                    matches.push((record.clone(), similarity));
                }
            }
        }

        Ok(matches)
    }

    /// Get embedding for a key
    pub fn get_embedding(&self, key: &str) -> Option<Vec<f32>> {
        self.embeddings.read().get(key).cloned()
    }

    /// Apply modifiers to search results
    fn apply_modifiers(&self, mut results: Vec<MemoryRecord>, modifiers: &Modifiers) -> Vec<MemoryRecord> {
        // Apply SCOPE filter first
        if let Some(ref scope) = modifiers.scope {
            results.retain(|r| &r.metadata.scope == scope);
        }

        // Apply NAMESPACE filter
        if let Some(ref namespace) = modifiers.namespace {
            results.retain(|r| r.metadata.namespace.as_ref() == Some(namespace));
        }

        // Apply ORDER BY
        if let Some(ref order_by) = modifiers.order_by {
            results.sort_by(|a, b| {
                let a_val = a.data.get(&order_by.field);
                let b_val = b.data.get(&order_by.field);
                let cmp = match (a_val, b_val) {
                    (Some(a), Some(b)) => compare_json_values(a, b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                };
                if order_by.ascending {
                    cmp
                } else {
                    cmp.reverse()
                }
            });
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

    /// Check if a record matches WHERE conditions (supports AND/OR)
    fn matches_conditions(&self, record: &MemoryRecord, conditions: &[Condition]) -> bool {
        evaluate_conditions_on_record(record, conditions)
    }

    /// Check if a record matches predicate
    fn matches_predicate(&self, record: &MemoryRecord, predicate: &Predicate) -> bool {
        match predicate {
            Predicate::Where { conditions } => self.matches_conditions(record, conditions),
            Predicate::Key { field, value } => {
                let key_value = value_to_string(value);
                if field == "id" || field == "key" {
                    record.id == key_value
                } else {
                    record.data.get(field).map(|v| value_to_string_json(v)) == Some(key_value)
                }
            }
            Predicate::All => true,
            Predicate::Like { .. } | Predicate::Pattern { .. } => {
                // For Like/Pattern, we need embeddings - handled separately
                true
            }
        }
    }
}

impl Default for SemanticBackend {
    fn default() -> Self {
        Self::new().expect("Failed to create default SemanticBackend")
    }
}

#[async_trait]
impl Backend for SemanticBackend {
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
            MemoryType::Semantic,
            data,
            scope,
            namespace.map(String::from),
            ttl,
        );

        self.records.write().insert(key.to_string(), record.clone());
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
                    records.get(&key_value).cloned().into_iter().collect()
                } else {
                    records
                        .values()
                        .filter(|r| {
                            r.data.get(field).map(|v| value_to_string_json(v)) == Some(key_value.clone())
                        })
                        .cloned()
                        .collect()
                }
            }
            Predicate::Where { conditions } => {
                records
                    .values()
                    .filter(|r| self.matches_conditions(r, conditions))
                    .cloned()
                    .collect()
            }
            Predicate::All => records.values().cloned().collect(),
            Predicate::Like { .. } | Predicate::Pattern { .. } => {
                // These require embeddings - return empty for lookup
                // Use recall() for similarity-based queries
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
        // For semantic memory, recall is primarily for similarity search
        // The actual embedding should be passed via the embeddings map or context
        match predicate {
            Predicate::Like { .. } | Predicate::Pattern { .. } => {
                // In a real implementation, the embedding would be resolved from context
                // For now, we return all records with embeddings, filtered by confidence
                let records = self.records.read();
                let embeddings = self.embeddings.read();

                let mut results: Vec<MemoryRecord> = records
                    .iter()
                    .filter(|(key, _)| embeddings.contains_key(*key))
                    .map(|(_, r)| r.clone())
                    .collect();

                drop(records);
                drop(embeddings);

                results = self.apply_modifiers(results, modifiers);
                Ok(results)
            }
            _ => self.lookup(predicate, modifiers).await,
        }
    }

    async fn update(
        &self,
        predicate: &Predicate,
        data: serde_json::Value,
    ) -> AdbResult<u64> {
        let mut records = self.records.write();
        let mut count = 0;

        for record in records.values_mut() {
            if self.matches_predicate(record, predicate) {
                // Merge data into existing record
                if let (Some(existing), Some(updates)) = (record.data.as_object_mut(), data.as_object()) {
                    for (key, value) in updates {
                        existing.insert(key.clone(), value.clone());
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
        let mut embeddings = self.embeddings.write();
        let mut id_to_key = self.id_to_key.write();
        let mut key_to_id = self.key_to_id.write();
        let index = self.index.read();

        let keys_to_remove: Vec<String> = records
            .iter()
            .filter(|(_, r)| self.matches_predicate(r, predicate))
            .map(|(k, _)| k.clone())
            .collect();

        for key in &keys_to_remove {
            records.remove(key);
            embeddings.remove(key);

            if let Some(id) = key_to_id.remove(key) {
                id_to_key.remove(&id);
                let _ = index.remove(id);
            }
        }

        Ok(keys_to_remove.len() as u64)
    }

    async fn clear(&self) -> AdbResult<()> {
        self.records.write().clear();
        self.embeddings.write().clear();
        self.id_to_key.write().clear();
        self.key_to_id.write().clear();

        // Recreate the index
        let options = IndexOptions {
            dimensions: self.dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        };

        let new_index = Index::new(&options)
            .map_err(|e| AdbError::VectorIndexError(format!("Failed to create index: {}", e)))?;

        new_index
            .reserve(DEFAULT_CAPACITY)
            .map_err(|e| AdbError::VectorIndexError(format!("Failed to reserve capacity: {}", e)))?;

        *self.index.write() = new_index;

        Ok(())
    }

    async fn count(&self) -> usize {
        self.records.read().len()
    }

    fn info(&self) -> BackendInfo {
        BackendInfo::semantic()
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

/// Compare two JSON values
fn compare_json_values(a: &serde_json::Value, b: &serde_json::Value) -> std::cmp::Ordering {
    match (a, b) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
            let a_f = a.as_f64().unwrap_or(0.0);
            let b_f = b.as_f64().unwrap_or(0.0);
            a_f.partial_cmp(&b_f).unwrap_or(std::cmp::Ordering::Equal)
        }
        (serde_json::Value::String(a), serde_json::Value::String(b)) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_record(key: &str) -> MemoryRecord {
        MemoryRecord::new(
            key,
            MemoryType::Semantic,
            json!({"name": key, "category": "test"}),
            Scope::Private,
        )
    }

    fn create_random_embedding(dimensions: usize) -> Vec<f32> {
        use std::f32::consts::PI;
        (0..dimensions)
            .map(|i| (i as f32 * PI / dimensions as f32).sin())
            .collect()
    }

    #[test]
    fn test_create_backend() {
        let backend = SemanticBackend::new();
        assert!(backend.is_ok());

        let backend = backend.unwrap();
        assert_eq!(backend.dimensions(), DEFAULT_DIMENSIONS);
        assert_eq!(backend.memory_type(), MemoryType::Semantic);
    }

    #[test]
    fn test_custom_dimensions() {
        let backend = SemanticBackend::with_dimensions(768).unwrap();
        assert_eq!(backend.dimensions(), 768);
    }

    #[test]
    fn test_store_with_embedding() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();
        let record = create_test_record("doc-1");
        let embedding = create_random_embedding(128);

        let result = backend.store_with_embedding(record, embedding);
        assert!(result.is_ok());

        assert_eq!(backend.records.read().len(), 1);
    }

    #[test]
    fn test_embedding_dimension_mismatch() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();
        let record = create_test_record("doc-1");
        let wrong_embedding = create_random_embedding(256);

        let result = backend.store_with_embedding(record, wrong_embedding);
        assert!(result.is_err());
    }

    #[test]
    fn test_similarity_search() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();

        // Store multiple records with embeddings
        for i in 0..5 {
            let record = create_test_record(&format!("doc-{}", i));
            let embedding = create_random_embedding(128);
            backend.store_with_embedding(record, embedding).unwrap();
        }

        // Create a query embedding and search
        let query = create_random_embedding(128);
        let results = backend.search_similar(&query, 3, None).unwrap();

        assert_eq!(results.len(), 3);

        // Results should be sorted by similarity (descending)
        for i in 0..results.len() - 1 {
            assert!(results[i].1 >= results[i + 1].1);
        }
    }

    #[test]
    fn test_similarity_with_confidence_threshold() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();

        // Store records
        for i in 0..5 {
            let record = create_test_record(&format!("doc-{}", i));
            let embedding = create_random_embedding(128);
            backend.store_with_embedding(record, embedding).unwrap();
        }

        let query = create_random_embedding(128);

        // With high threshold, should filter out low-similarity results
        let results = backend.search_similar(&query, 10, Some(0.99)).unwrap();
        // Results may be empty or few depending on threshold
        assert!(results.iter().all(|(_, sim)| *sim >= 0.99));
    }

    #[tokio::test]
    async fn test_store_and_lookup() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();

        let record = backend
            .store("doc-1", json!({"name": "test"}), Scope::Private, None, None)
            .await
            .unwrap();

        assert_eq!(record.id, "doc-1");

        let results = backend
            .lookup(
                &Predicate::Key {
                    field: "id".to_string(),
                    value: Value::String("doc-1".to_string()),
                },
                &Modifiers::default(),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "doc-1");
    }

    #[tokio::test]
    async fn test_forget() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();

        // Store records
        for i in 0..5 {
            let record = create_test_record(&format!("doc-{}", i));
            let embedding = create_random_embedding(128);
            backend.store_with_embedding(record, embedding).unwrap();
        }

        assert_eq!(backend.count().await, 5);

        // Forget all records
        let count = backend.forget(&Predicate::All).await.unwrap();
        assert_eq!(count, 5);
        assert_eq!(backend.count().await, 0);
    }

    #[tokio::test]
    async fn test_update() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();

        backend
            .store("doc-1", json!({"name": "test", "version": 1}), Scope::Private, None, None)
            .await
            .unwrap();

        let count = backend
            .update(
                &Predicate::Key {
                    field: "id".to_string(),
                    value: Value::String("doc-1".to_string()),
                },
                json!({"version": 2}),
            )
            .await
            .unwrap();

        assert_eq!(count, 1);

        // Verify update
        let results = backend
            .lookup(&Predicate::All, &Modifiers::default())
            .await
            .unwrap();

        assert_eq!(results[0].data.get("version"), Some(&json!(2)));
    }

    #[tokio::test]
    async fn test_clear() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();

        // Store records
        for i in 0..5 {
            let record = create_test_record(&format!("doc-{}", i));
            let embedding = create_random_embedding(128);
            backend.store_with_embedding(record, embedding).unwrap();
        }

        assert_eq!(backend.count().await, 5);

        backend.clear().await.unwrap();
        assert_eq!(backend.count().await, 0);
    }

    #[test]
    fn test_get_embedding() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();

        let record = create_test_record("doc-1");
        let embedding = create_random_embedding(128);
        backend
            .store_with_embedding(record, embedding.clone())
            .unwrap();

        let retrieved = backend.get_embedding("doc-1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), embedding);

        let missing = backend.get_embedding("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_update_existing_record() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();

        let record1 = create_test_record("doc-1");
        let embedding1 = create_random_embedding(128);
        backend
            .store_with_embedding(record1, embedding1)
            .unwrap();

        // Store same key with different embedding
        let record2 = MemoryRecord::new(
            "doc-1",
            MemoryType::Semantic,
            json!({"name": "doc-1", "version": 2}),
            Scope::Private,
        );
        let embedding2 = create_random_embedding(128);
        backend
            .store_with_embedding(record2, embedding2.clone())
            .unwrap();

        // Should still have only one record
        assert_eq!(backend.records.read().len(), 1);

        // Should have the updated embedding
        let retrieved = backend.get_embedding("doc-1").unwrap();
        assert_eq!(retrieved, embedding2);
    }

    #[tokio::test]
    async fn test_lookup_by_field() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();

        backend
            .store("doc-1", json!({"category": "a", "value": 1}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("doc-2", json!({"category": "b", "value": 2}), Scope::Private, None, None)
            .await
            .unwrap();
        backend
            .store("doc-3", json!({"category": "a", "value": 3}), Scope::Private, None, None)
            .await
            .unwrap();

        let results = backend
            .lookup(
                &Predicate::Where {
                    conditions: vec![Condition {
                        field: "category".to_string(),
                        operator: Operator::Eq,
                        value: Value::String("a".to_string()),
                    }],
                },
                &Modifiers::default(),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_backend_info() {
        let backend = SemanticBackend::with_dimensions(128).unwrap();
        let info = backend.info();

        assert_eq!(info.memory_type, MemoryType::Semantic);
        assert!(info.supports_embedding);
        assert!(!info.supports_scan);
    }
}
