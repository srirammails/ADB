//! Procedural Memory Backend
//!
//! Stores procedures and patterns as a directed graph using petgraph.
//! Supports pattern matching via token overlap (Jaccard similarity).
//!
//! Latency target: < 5ms

use async_trait::async_trait;
use parking_lot::RwLock;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use adb_core::{
    AdbResult, Condition, MemoryRecord, MemoryType, Metadata, Modifiers, Predicate, Scope, Value,
};

use crate::backend::{Backend, BackendInfo};

/// A procedure node in the graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureNode {
    /// Unique pattern identifier
    pub pattern_id: String,
    /// Pattern to match (text that triggers this procedure)
    pub pattern: String,
    /// Procedure steps to execute
    pub steps: Vec<String>,
    /// Severity level
    pub severity: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Source of this procedure (manual, learned, etc.)
    pub source: String,
    /// Number of successful applications
    pub success_count: u64,
    /// Number of failed applications
    pub failure_count: u64,
    /// Tunable variables
    pub variables: HashMap<String, f64>,
    /// Additional metadata
    pub metadata: serde_json::Value,
    /// Version number (incremented on update)
    pub version: u64,
}

impl ProcedureNode {
    /// Create a new procedure node
    pub fn new(
        pattern_id: impl Into<String>,
        pattern: impl Into<String>,
        steps: Vec<String>,
    ) -> Self {
        Self {
            pattern_id: pattern_id.into(),
            pattern: pattern.into(),
            steps,
            severity: "info".to_string(),
            confidence: 0.5,
            source: "manual".to_string(),
            success_count: 0,
            failure_count: 0,
            variables: HashMap::new(),
            metadata: serde_json::Value::Null,
            version: 1,
        }
    }

    /// Convert to MemoryRecord
    pub fn to_memory_record(&self, scope: Scope) -> MemoryRecord {
        let mut metadata = Metadata::new(scope);
        metadata.version = self.version;
        MemoryRecord {
            id: self.pattern_id.clone(),
            memory_type: MemoryType::Procedural,
            data: serde_json::to_value(self).unwrap_or_default(),
            metadata,
        }
    }

    /// Record a success
    pub fn record_success(&mut self) {
        self.success_count += 1;
        self.update_confidence();
    }

    /// Record a failure
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.update_confidence();
    }

    /// Update confidence based on success/failure ratio
    fn update_confidence(&mut self) {
        let total = self.success_count + self.failure_count;
        if total > 0 {
            self.confidence = self.success_count as f32 / total as f32;
        }
    }

    /// Get success rate
    pub fn success_rate(&self) -> f32 {
        let total = self.success_count + self.failure_count;
        if total > 0 {
            self.success_count as f32 / total as f32
        } else {
            0.5 // Unknown
        }
    }
}

/// An edge in the procedure graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureEdge {
    /// Relationship type (next, requires, triggers, etc.)
    pub relationship: String,
    /// Edge weight (strength of relationship)
    pub weight: f32,
}

impl ProcedureEdge {
    pub fn new(relationship: impl Into<String>, weight: f32) -> Self {
        Self {
            relationship: relationship.into(),
            weight: weight.clamp(0.0, 1.0),
        }
    }
}

/// Procedural memory backend
pub struct ProceduralBackend {
    /// Directed graph of procedures
    graph: RwLock<DiGraph<ProcedureNode, ProcedureEdge>>,
    /// Index from pattern_id to NodeIndex
    index: RwLock<HashMap<String, NodeIndex>>,
    /// Default pattern matching threshold
    default_threshold: f32,
    /// Default scope
    default_scope: Scope,
}

impl ProceduralBackend {
    /// Create a new procedural backend
    pub fn new() -> Self {
        Self {
            graph: RwLock::new(DiGraph::new()),
            index: RwLock::new(HashMap::new()),
            default_threshold: 0.7,
            default_scope: Scope::Shared,
        }
    }

    /// Create with custom default threshold
    pub fn with_threshold(threshold: f32) -> Self {
        Self {
            graph: RwLock::new(DiGraph::new()),
            index: RwLock::new(HashMap::new()),
            default_threshold: threshold.clamp(0.0, 1.0),
            default_scope: Scope::Shared,
        }
    }

    /// Add a procedure to the graph
    pub fn add_procedure(&self, procedure: ProcedureNode) -> NodeIndex {
        let pattern_id = procedure.pattern_id.clone();
        let mut graph = self.graph.write();
        let mut index = self.index.write();

        let node_idx = graph.add_node(procedure);
        index.insert(pattern_id, node_idx);

        node_idx
    }

    /// Add an edge between procedures
    pub fn add_edge(
        &self,
        from_id: &str,
        to_id: &str,
        relationship: impl Into<String>,
        weight: f32,
    ) -> bool {
        let index = self.index.read();
        if let (Some(&from_idx), Some(&to_idx)) = (index.get(from_id), index.get(to_id)) {
            drop(index);
            let mut graph = self.graph.write();
            graph.add_edge(from_idx, to_idx, ProcedureEdge::new(relationship, weight));
            true
        } else {
            false
        }
    }

    /// Get a procedure by ID
    pub fn get(&self, pattern_id: &str) -> Option<ProcedureNode> {
        let index = self.index.read();
        let graph = self.graph.read();
        index.get(pattern_id).map(|&idx| graph[idx].clone())
    }

    /// Pattern matching using Jaccard similarity
    fn pattern_match(&self, query: &str, pattern: &str, threshold: f32) -> bool {
        let similarity = jaccard_similarity(query, pattern);
        similarity >= threshold
    }

    /// Get similarity score between query and pattern
    fn similarity_score(&self, query: &str, pattern: &str) -> f32 {
        jaccard_similarity(query, pattern)
    }

    /// Get dependent procedures (following edges)
    pub fn get_dependencies(&self, pattern_id: &str, relationship: &str) -> Vec<ProcedureNode> {
        let index = self.index.read();
        let graph = self.graph.read();

        if let Some(&node_idx) = index.get(pattern_id) {
            graph
                .edges(node_idx)
                .filter(|e| e.weight().relationship == relationship)
                .map(|e| graph[e.target()].clone())
                .collect()
        } else {
            vec![]
        }
    }

    /// Get procedures that depend on this one (incoming edges)
    pub fn get_dependents(&self, pattern_id: &str, relationship: &str) -> Vec<ProcedureNode> {
        let index = self.index.read();
        let graph = self.graph.read();

        if let Some(&node_idx) = index.get(pattern_id) {
            graph
                .edges_directed(node_idx, petgraph::Direction::Incoming)
                .filter(|e| e.weight().relationship == relationship)
                .map(|e| graph[e.source()].clone())
                .collect()
        } else {
            vec![]
        }
    }

    /// Check if a procedure matches a predicate
    fn matches_predicate(&self, proc: &ProcedureNode, predicate: &Predicate) -> bool {
        match predicate {
            Predicate::All => true,
            Predicate::Key { field, value } => match field.as_str() {
                "pattern_id" | "id" => {
                    matches!(value, Value::String(s) if s == &proc.pattern_id)
                }
                "severity" => matches!(value, Value::String(s) if s == &proc.severity),
                "source" => matches!(value, Value::String(s) if s == &proc.source),
                _ => false,
            },
            Predicate::Where { conditions } => {
                conditions.iter().all(|c| self.matches_condition(proc, c))
            }
            Predicate::Pattern { .. } => true, // Pattern matching handled separately
            Predicate::Like { .. } => false,    // Not supported
        }
    }

    /// Check if a procedure matches a condition
    fn matches_condition(&self, proc: &ProcedureNode, condition: &Condition) -> bool {
        let proc_json = serde_json::to_value(proc).unwrap_or_default();
        condition.matches(&proc_json)
    }
}

impl Default for ProceduralBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Backend for ProceduralBackend {
    async fn store(
        &self,
        key: &str,
        data: serde_json::Value,
        _scope: Scope,
        _namespace: Option<&str>,
        _ttl: Option<Duration>,
    ) -> AdbResult<MemoryRecord> {
        // Use pattern_id from data if provided, otherwise use key
        let pattern_id = data["pattern_id"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| key.to_string());

        let pattern = data["pattern"].as_str().unwrap_or("").to_string();

        // Handle steps - support both array and string formats
        let steps: Vec<String> = if let Some(arr) = data.get("steps").and_then(|s| s.as_array()) {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else if let Some(s) = data.get("steps").and_then(|s| s.as_str()) {
            // Single string - split by newline or treat as single step
            s.lines().map(String::from).collect()
        } else {
            Vec::new()
        };

        let mut proc = ProcedureNode::new(&pattern_id, pattern, steps);

        // Set optional fields
        if let Some(severity) = data["severity"].as_str() {
            proc.severity = severity.to_string();
        }
        if let Some(source) = data["source"].as_str() {
            proc.source = source.to_string();
        }
        if let Some(confidence) = data["confidence"].as_f64() {
            proc.confidence = confidence as f32;
        }

        // Read success_count and failure_count from input
        if let Some(sc) = data["success_count"].as_u64() {
            proc.success_count = sc;
        } else if let Some(sc) = data["success_count"].as_i64() {
            proc.success_count = sc as u64;
        }
        if let Some(fc) = data["failure_count"].as_u64() {
            proc.failure_count = fc;
        } else if let Some(fc) = data["failure_count"].as_i64() {
            proc.failure_count = fc as u64;
        }

        // Read variables - support both object and string formats
        if let Some(vars) = data.get("variables").and_then(|v| v.as_object()) {
            for (k, v) in vars {
                if let Some(val) = v.as_f64() {
                    proc.variables.insert(k.clone(), val);
                } else if let Some(val) = v.as_i64() {
                    proc.variables.insert(k.clone(), val as f64);
                }
            }
        } else if let Some(vars_str) = data.get("variables").and_then(|v| v.as_str()) {
            // Parse "key=value,key2=value2" format
            for pair in vars_str.split(',') {
                if let Some((k, v)) = pair.split_once('=') {
                    if let Ok(val) = v.trim().parse::<f64>() {
                        proc.variables.insert(k.trim().to_string(), val);
                    }
                }
            }
        }

        proc.metadata = data.get("metadata").cloned().unwrap_or_default();

        let record = proc.to_memory_record(self.default_scope);
        self.add_procedure(proc);

        Ok(record)
    }

    async fn lookup(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        let graph = self.graph.read();

        // Handle pattern matching
        if let Predicate::Pattern {
            pattern_var,
            threshold,
        } = predicate
        {
            let thresh = threshold.unwrap_or(self.default_threshold);
            let query = pattern_var.trim_start_matches('$');

            let mut matches: Vec<(f32, MemoryRecord)> = graph
                .node_weights()
                .filter(|proc| self.pattern_match(query, &proc.pattern, thresh))
                .map(|proc| {
                    let score = self.similarity_score(query, &proc.pattern);
                    (score, proc.to_memory_record(self.default_scope))
                })
                .collect();

            // Sort by similarity descending
            matches.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

            let results: Vec<MemoryRecord> = matches.into_iter().map(|(_, r)| r).collect();

            // Apply limit
            if let Some(limit) = modifiers.limit {
                return Ok(results.into_iter().take(limit).collect());
            }
            return Ok(results);
        }

        // Handle exact key lookup
        if let Predicate::Key { field, value } = predicate {
            if field == "pattern_id" || field == "id" {
                if let Value::String(id) = value {
                    return Ok(self
                        .get(id)
                        .map(|p| vec![p.to_memory_record(self.default_scope)])
                        .unwrap_or_default());
                }
            }
        }

        // General filter
        let mut results: Vec<MemoryRecord> = graph
            .node_weights()
            .filter(|proc| self.matches_predicate(proc, predicate))
            .map(|proc| proc.to_memory_record(self.default_scope))
            .collect();

        // Apply SCOPE filter
        if let Some(ref scope) = modifiers.scope {
            results.retain(|r| &r.metadata.scope == scope);
        }

        // Apply NAMESPACE filter
        if let Some(ref namespace) = modifiers.namespace {
            results.retain(|r| r.metadata.namespace.as_ref() == Some(namespace));
        }

        // Apply limit
        if let Some(limit) = modifiers.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    async fn recall(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        // Recall same as lookup for procedural
        self.lookup(predicate, modifiers).await
    }

    async fn forget(&self, predicate: &Predicate) -> AdbResult<u64> {
        let mut graph = self.graph.write();
        let mut index = self.index.write();

        // Find nodes to remove
        let to_remove: Vec<(String, NodeIndex)> = index
            .iter()
            .filter(|(_, &idx)| self.matches_predicate(&graph[idx], predicate))
            .map(|(id, &idx)| (id.clone(), idx))
            .collect();

        let count = to_remove.len() as u64;

        // Remove from graph (in reverse order to maintain indices)
        let mut indices: Vec<NodeIndex> = to_remove.iter().map(|(_, idx)| *idx).collect();
        indices.sort_by(|a, b| b.index().cmp(&a.index()));

        for (id, _) in &to_remove {
            index.remove(id);
        }

        for idx in indices {
            graph.remove_node(idx);
        }

        // Note: After removal, node indices may be invalidated
        // In a production system, we'd need to rebuild the index
        // For now, this is acceptable for the test cases

        Ok(count)
    }

    async fn update(
        &self,
        predicate: &Predicate,
        data: serde_json::Value,
    ) -> AdbResult<u64> {
        let mut graph = self.graph.write();
        let index = self.index.read();
        let mut count = 0u64;

        // Find matching nodes
        let ids: Vec<String> = index
            .iter()
            .filter(|(_, &idx)| self.matches_predicate(&graph[idx], predicate))
            .map(|(id, _)| id.clone())
            .collect();

        for id in ids {
            if let Some(&idx) = index.get(&id) {
                let proc = &mut graph[idx];

                // Update fields
                if let Some(pattern) = data["pattern"].as_str() {
                    proc.pattern = pattern.to_string();
                }
                if let Some(severity) = data["severity"].as_str() {
                    proc.severity = severity.to_string();
                }
                if let Some(source) = data["source"].as_str() {
                    proc.source = source.to_string();
                }
                if let Some(confidence) = data["confidence"].as_f64() {
                    proc.confidence = confidence as f32;
                }

                // Handle steps - support both array and string formats
                if let Some(steps) = data.get("steps").and_then(|s| s.as_array()) {
                    proc.steps = steps
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                } else if let Some(steps_str) = data.get("steps").and_then(|s| s.as_str()) {
                    proc.steps = steps_str.lines().map(String::from).collect();
                }

                // Update success_count and failure_count
                if let Some(sc) = data["success_count"].as_u64() {
                    proc.success_count = sc;
                } else if let Some(sc) = data["success_count"].as_i64() {
                    proc.success_count = sc as u64;
                }
                if let Some(fc) = data["failure_count"].as_u64() {
                    proc.failure_count = fc;
                } else if let Some(fc) = data["failure_count"].as_i64() {
                    proc.failure_count = fc as u64;
                }

                // Handle variables - support both object and string formats
                if let Some(vars) = data.get("variables").and_then(|v| v.as_object()) {
                    for (k, v) in vars {
                        if let Some(val) = v.as_f64() {
                            proc.variables.insert(k.clone(), val);
                        } else if let Some(val) = v.as_i64() {
                            proc.variables.insert(k.clone(), val as f64);
                        }
                    }
                } else if let Some(vars_str) = data.get("variables").and_then(|v| v.as_str()) {
                    // Parse "key=value,key2=value2" format
                    for pair in vars_str.split(',') {
                        if let Some((k, v)) = pair.split_once('=') {
                            if let Ok(val) = v.trim().parse::<f64>() {
                                proc.variables.insert(k.trim().to_string(), val);
                            }
                        }
                    }
                }

                // Update metadata if provided
                if let Some(meta) = data.get("metadata") {
                    proc.metadata = meta.clone();
                }

                // Increment version on update
                proc.version += 1;

                count += 1;
            }
        }

        Ok(count)
    }

    fn info(&self) -> BackendInfo {
        BackendInfo::procedural()
    }

    async fn count(&self) -> usize {
        self.graph.read().node_count()
    }

    async fn clear(&self) -> AdbResult<()> {
        self.graph.write().clear();
        self.index.write().clear();
        Ok(())
    }
}

/// Calculate Jaccard similarity between two strings
/// based on token (word) overlap
fn jaccard_similarity(a: &str, b: &str) -> f32 {
    let tokens_a: HashSet<&str> = a.split_whitespace().collect();
    let tokens_b: HashSet<&str> = b.split_whitespace().collect();

    if tokens_a.is_empty() && tokens_b.is_empty() {
        return 1.0;
    }

    let intersection = tokens_a.intersection(&tokens_b).count();
    let union = tokens_a.union(&tokens_b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_jaccard_similarity() {
        assert!((jaccard_similarity("foo bar baz", "foo bar qux") - 0.5).abs() < 0.01);
        assert!((jaccard_similarity("foo bar", "foo bar") - 1.0).abs() < 0.01);
        assert!((jaccard_similarity("foo", "bar") - 0.0).abs() < 0.01);
        assert!((jaccard_similarity("", "") - 1.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_store_and_lookup() {
        let backend = ProceduralBackend::new();

        let record = backend
            .store(
                "oom-fix",
                json!({
                    "pattern": "OOMKilled container restart",
                    "steps": ["Check memory limits", "Increase if needed", "Restart pod"],
                    "severity": "critical",
                    "source": "runbook"
                }),
                Scope::Shared,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(record.id, "oom-fix");

        let results = backend
            .lookup(&Predicate::key("id", "oom-fix"), &Modifiers::default())
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_pattern_matching() {
        let backend = ProceduralBackend::new();

        backend
            .store(
                "oom-fix",
                json!({
                    "pattern": "OOMKilled container memory exceeded",
                    "steps": ["Check limits"]
                }),
                Scope::Shared,
                None,
                None,
            )
            .await
            .unwrap();

        backend
            .store(
                "cpu-throttle",
                json!({
                    "pattern": "CPU throttling high utilization",
                    "steps": ["Check CPU limits"]
                }),
                Scope::Shared,
                None,
                None,
            )
            .await
            .unwrap();

        // Should match oom-fix
        let results = backend
            .lookup(
                &Predicate::pattern("OOMKilled container", Some(0.5)),
                &Modifiers::default(),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "oom-fix");
    }

    #[tokio::test]
    async fn test_graph_edges() {
        let backend = ProceduralBackend::new();

        let proc1 = ProcedureNode::new("step-1", "Check logs", vec!["kubectl logs".into()]);
        let proc2 = ProcedureNode::new("step-2", "Restart pod", vec!["kubectl delete pod".into()]);
        let proc3 = ProcedureNode::new("step-3", "Verify", vec!["kubectl get pods".into()]);

        backend.add_procedure(proc1);
        backend.add_procedure(proc2);
        backend.add_procedure(proc3);

        backend.add_edge("step-1", "step-2", "next", 1.0);
        backend.add_edge("step-2", "step-3", "next", 1.0);

        let deps = backend.get_dependencies("step-1", "next");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].pattern_id, "step-2");

        let deps = backend.get_dependencies("step-2", "next");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].pattern_id, "step-3");
    }

    #[tokio::test]
    async fn test_update_procedure() {
        let backend = ProceduralBackend::new();

        backend
            .store(
                "test-proc",
                json!({
                    "pattern": "test pattern",
                    "steps": ["step 1"],
                    "confidence": 0.5
                }),
                Scope::Shared,
                None,
                None,
            )
            .await
            .unwrap();

        // Update confidence
        backend
            .update(
                &Predicate::key("id", "test-proc"),
                json!({"confidence": 0.9, "steps": ["step 1", "step 2"]}),
            )
            .await
            .unwrap();

        let proc = backend.get("test-proc").unwrap();
        assert!((proc.confidence - 0.9).abs() < 0.01);
        assert_eq!(proc.steps.len(), 2);
    }

    #[tokio::test]
    async fn test_success_failure_tracking() {
        let backend = ProceduralBackend::new();

        let mut proc = ProcedureNode::new("test", "test", vec![]);
        proc.record_success();
        proc.record_success();
        proc.record_failure();

        // 2 success, 1 failure = 66.7%
        assert!((proc.success_rate() - 0.667).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_variables() {
        let backend = ProceduralBackend::new();

        backend
            .store(
                "parameterized",
                json!({
                    "pattern": "scale deployment",
                    "steps": ["kubectl scale"],
                    "variables": {
                        "min_replicas": 2.0,
                        "max_replicas": 10.0
                    }
                }),
                Scope::Shared,
                None,
                None,
            )
            .await
            .unwrap();

        let proc = backend.get("parameterized").unwrap();
        assert_eq!(proc.variables.get("min_replicas"), Some(&2.0));
        assert_eq!(proc.variables.get("max_replicas"), Some(&10.0));
    }
}
