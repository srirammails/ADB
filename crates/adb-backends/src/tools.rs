//! Tools Memory Backend
//!
//! Stores tool definitions with dynamic ranking based on usage success.
//! Supports LOAD operation for selecting top-ranked relevant tools.
//!
//! Latency target: < 2ms

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use adb_core::{
    evaluate_conditions, AdbResult, Condition, MemoryRecord, MemoryType, Metadata, Modifiers,
    Predicate, Scope, Value, Window,
};

use crate::backend::{Backend, BackendInfo};

/// A tool record with ranking information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecord {
    /// Unique tool identifier
    pub tool_id: String,
    /// Tool name
    pub name: String,
    /// Description of what the tool does
    pub description: String,
    /// JSON Schema for tool parameters
    pub schema: serde_json::Value,
    /// Tool category (e.g., "file", "network", "database")
    pub category: String,
    /// Dynamic ranking score (0.0 - 1.0)
    pub ranking: f32,
    /// Number of times this tool has been called
    pub call_count: u64,
    /// Last time this tool was called
    pub last_called: Option<DateTime<Utc>>,
    /// Relevance scores per task type
    pub relevance_scores: HashMap<String, f32>,
    /// Additional metadata
    pub metadata: serde_json::Value,
}

impl ToolRecord {
    /// Create a new tool record
    pub fn new(
        tool_id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        schema: serde_json::Value,
        category: impl Into<String>,
    ) -> Self {
        Self {
            tool_id: tool_id.into(),
            name: name.into(),
            description: description.into(),
            schema,
            category: category.into(),
            ranking: 0.5, // Default middle ranking
            call_count: 0,
            last_called: None,
            relevance_scores: HashMap::new(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Convert to MemoryRecord
    pub fn to_memory_record(&self, scope: Scope) -> MemoryRecord {
        MemoryRecord {
            id: self.tool_id.clone(),
            memory_type: MemoryType::Tools,
            data: serde_json::to_value(self).unwrap_or_default(),
            metadata: Metadata::new(scope),
        }
    }

    /// Get relevance for a specific task type
    pub fn relevance_for(&self, task_type: &str) -> f32 {
        self.relevance_scores.get(task_type).copied().unwrap_or(0.5)
    }

    /// Update ranking after tool use
    /// ranking = old * decay + signal * (1 - decay)
    pub fn update_ranking(&mut self, success_signal: f32, decay: f32) {
        let decay = decay.clamp(0.0, 1.0);
        let signal = success_signal.clamp(0.0, 1.0);
        self.ranking = (self.ranking * decay + signal * (1.0 - decay)).clamp(0.0, 1.0);
        self.call_count += 1;
        self.last_called = Some(Utc::now());
    }

    /// Set relevance score for a task type
    pub fn set_relevance(&mut self, task_type: impl Into<String>, score: f32) {
        self.relevance_scores.insert(task_type.into(), score.clamp(0.0, 1.0));
    }
}

/// Tools backend implementation
pub struct ToolsBackend {
    /// Tool storage
    tools: RwLock<HashMap<String, ToolRecord>>,
    /// Decay factor for ranking updates (default: 0.9)
    decay_factor: f32,
    /// Default scope
    default_scope: Scope,
}

impl ToolsBackend {
    /// Create a new tools backend
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            decay_factor: 0.9,
            default_scope: Scope::Shared, // Tools are typically shared
        }
    }

    /// Create with custom decay factor
    pub fn with_decay_factor(decay_factor: f32) -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            decay_factor: decay_factor.clamp(0.0, 1.0),
            default_scope: Scope::Shared,
        }
    }

    /// Register a tool
    pub fn register(&self, tool: ToolRecord) {
        let mut tools = self.tools.write();
        tools.insert(tool.tool_id.clone(), tool);
    }

    /// Update ranking for a tool after use
    pub fn update_ranking(&self, tool_id: &str, success_signal: f32) -> bool {
        let mut tools = self.tools.write();
        if let Some(tool) = tools.get_mut(tool_id) {
            tool.update_ranking(success_signal, self.decay_factor);
            true
        } else {
            false
        }
    }

    /// Set relevance score for a tool
    pub fn set_relevance(&self, tool_id: &str, task_type: &str, score: f32) -> bool {
        let mut tools = self.tools.write();
        if let Some(tool) = tools.get_mut(tool_id) {
            tool.set_relevance(task_type, score);
            true
        } else {
            false
        }
    }

    /// Get tool by ID
    pub fn get(&self, tool_id: &str) -> Option<ToolRecord> {
        self.tools.read().get(tool_id).cloned()
    }

    /// Check if a tool matches a predicate
    fn matches_predicate(&self, tool: &ToolRecord, predicate: &Predicate) -> bool {
        match predicate {
            Predicate::All => true,
            Predicate::Key { field, value } => {
                match field.as_str() {
                    "tool_id" | "id" => matches!(value, Value::String(s) if s == &tool.tool_id),
                    "name" => matches!(value, Value::String(s) if s == &tool.name),
                    "category" => matches!(value, Value::String(s) if s == &tool.category),
                    _ => false,
                }
            }
            Predicate::Where { conditions } => {
                let tool_json = serde_json::to_value(tool).unwrap_or_default();
                evaluate_conditions(&tool_json, conditions)
            }
            _ => false,
        }
    }

    /// Check if a tool matches a condition
    fn matches_condition(&self, tool: &ToolRecord, condition: &Condition) -> bool {
        let tool_json = serde_json::to_value(tool).unwrap_or_default();
        condition.matches(&tool_json)
    }
}

impl Default for ToolsBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Backend for ToolsBackend {
    async fn store(
        &self,
        key: &str,
        data: serde_json::Value,
        _scope: Scope,
        _namespace: Option<&str>,
        _ttl: Option<Duration>,
    ) -> AdbResult<MemoryRecord> {
        // Use tool_id from data if provided, otherwise use key
        let tool_id = data["tool_id"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| key.to_string());

        // Parse tool from data
        let name = data["name"].as_str().unwrap_or(&tool_id).to_string();
        let description = data["description"].as_str().unwrap_or("").to_string();
        let schema = data.get("schema").cloned().unwrap_or_default();
        let category = data["category"].as_str().unwrap_or("general").to_string();

        let mut tool = ToolRecord::new(&tool_id, name, description, schema, category);

        // Set initial ranking if provided
        if let Some(ranking) = data["ranking"].as_f64() {
            tool.ranking = ranking as f32;
        }

        // Set relevance scores if provided
        if let Some(relevance) = data.get("relevance_scores") {
            if let Some(obj) = relevance.as_object() {
                for (task, score) in obj {
                    if let Some(s) = score.as_f64() {
                        tool.set_relevance(task, s as f32);
                    }
                }
            }
        }

        // Store additional/custom fields in metadata (task, ad_format, etc.)
        let mut meta = data.get("metadata").cloned().unwrap_or(serde_json::json!({}));
        if let serde_json::Value::Object(ref mut meta_obj) = meta {
            // Preserve any custom fields that aren't part of the core ToolRecord
            let known_fields = ["tool_id", "name", "description", "schema", "category",
                              "ranking", "relevance_scores", "metadata", "key"];
            if let serde_json::Value::Object(ref data_obj) = data {
                for (k, v) in data_obj {
                    if !known_fields.contains(&k.as_str()) {
                        meta_obj.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        tool.metadata = meta;

        let record = tool.to_memory_record(self.default_scope);
        self.register(tool);

        Ok(record)
    }

    async fn lookup(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        let tools = self.tools.read();

        let mut results: Vec<MemoryRecord> = tools
            .values()
            .filter(|t| self.matches_predicate(t, predicate))
            .map(|t| t.to_memory_record(self.default_scope))
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
        // Recall is same as lookup for tools
        self.lookup(predicate, modifiers).await
    }

    async fn load(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        let tools = self.tools.read();

        // Filter by predicate
        let mut matches: Vec<&ToolRecord> = tools
            .values()
            .filter(|t| self.matches_predicate(t, predicate))
            .collect();

        // Extract task type from predicate for relevance scoring
        let task_type = if let Predicate::Where { conditions } = predicate {
            conditions
                .iter()
                .find_map(|c| {
                    if let Condition::Simple { field, value, .. } = c {
                        if field == "task" || field == "task_type" {
                            if let Value::String(s) = value {
                                return Some(s.as_str());
                            }
                        }
                    }
                    None
                })
        } else {
            None
        };

        // Sort by ranking (descending) with relevance adjustment
        matches.sort_by(|a, b| {
            let score_a = if let Some(tt) = task_type {
                a.ranking * 0.7 + a.relevance_for(tt) * 0.3
            } else {
                a.ranking
            };
            let score_b = if let Some(tt) = task_type {
                b.ranking * 0.7 + b.relevance_for(tt) * 0.3
            } else {
                b.ranking
            };
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply limit
        let limit = modifiers.limit.unwrap_or(10);
        let results: Vec<MemoryRecord> = matches
            .into_iter()
            .take(limit)
            .map(|t| t.to_memory_record(self.default_scope))
            .collect();

        Ok(results)
    }

    async fn forget(&self, predicate: &Predicate) -> AdbResult<u64> {
        let mut tools = self.tools.write();
        let initial_count = tools.len();

        // Find tools to remove
        let to_remove: Vec<String> = tools
            .values()
            .filter(|t| self.matches_predicate(t, predicate))
            .map(|t| t.tool_id.clone())
            .collect();

        for id in &to_remove {
            tools.remove(id);
        }

        Ok((initial_count - tools.len()) as u64)
    }

    async fn update(
        &self,
        predicate: &Predicate,
        data: serde_json::Value,
    ) -> AdbResult<u64> {
        let mut tools = self.tools.write();
        let mut count = 0u64;

        // Find matching tools
        let ids: Vec<String> = tools
            .values()
            .filter(|t| self.matches_predicate(t, predicate))
            .map(|t| t.tool_id.clone())
            .collect();

        for id in ids {
            if let Some(tool) = tools.get_mut(&id) {
                // Update fields from data
                if let Some(name) = data["name"].as_str() {
                    tool.name = name.to_string();
                }
                if let Some(desc) = data["description"].as_str() {
                    tool.description = desc.to_string();
                }
                if let Some(cat) = data["category"].as_str() {
                    tool.category = cat.to_string();
                }
                if let Some(ranking) = data["ranking"].as_f64() {
                    tool.ranking = ranking as f32;
                }
                if let Some(schema) = data.get("schema") {
                    tool.schema = schema.clone();
                }
                count += 1;
            }
        }

        Ok(count)
    }

    fn info(&self) -> BackendInfo {
        BackendInfo::tools()
    }

    async fn count(&self) -> usize {
        self.tools.read().len()
    }

    async fn clear(&self) -> AdbResult<()> {
        self.tools.write().clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_tool(id: &str, ranking: f32) -> ToolRecord {
        let mut tool = ToolRecord::new(
            id,
            format!("Tool {}", id),
            format!("Description for {}", id),
            json!({"type": "object"}),
            "test",
        );
        tool.ranking = ranking;
        tool
    }

    #[tokio::test]
    async fn test_store_and_lookup() {
        let backend = ToolsBackend::new();

        let record = backend
            .store(
                "read-file",
                json!({
                    "name": "Read File",
                    "description": "Reads contents of a file",
                    "schema": {"type": "object", "properties": {"path": {"type": "string"}}},
                    "category": "file"
                }),
                Scope::Shared,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(record.id, "read-file");

        let results = backend
            .lookup(&Predicate::key("id", "read-file"), &Modifiers::default())
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_load_ranked() {
        let backend = ToolsBackend::new();

        // Register tools with different rankings
        backend.register(create_test_tool("tool-a", 0.3));
        backend.register(create_test_tool("tool-b", 0.9));
        backend.register(create_test_tool("tool-c", 0.6));

        let results = backend
            .load(&Predicate::all(), &Modifiers::with_limit(3))
            .await
            .unwrap();

        // Should be sorted by ranking descending
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].id, "tool-b"); // 0.9
        assert_eq!(results[1].id, "tool-c"); // 0.6
        assert_eq!(results[2].id, "tool-a"); // 0.3
    }

    #[tokio::test]
    async fn test_load_with_limit() {
        let backend = ToolsBackend::new();

        for i in 0..10 {
            backend.register(create_test_tool(&format!("tool-{}", i), i as f32 / 10.0));
        }

        let results = backend
            .load(&Predicate::all(), &Modifiers::with_limit(3))
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_update_ranking() {
        let backend = ToolsBackend::new();
        backend.register(create_test_tool("test-tool", 0.5));

        // Success signal
        backend.update_ranking("test-tool", 1.0);

        let tool = backend.get("test-tool").unwrap();
        // 0.5 * 0.9 + 1.0 * 0.1 = 0.55
        assert!((tool.ranking - 0.55).abs() < 0.001);
        assert_eq!(tool.call_count, 1);
    }

    #[tokio::test]
    async fn test_relevance_scoring() {
        let backend = ToolsBackend::new();

        let mut tool_a = create_test_tool("tool-a", 0.8);
        tool_a.set_relevance("bidding", 0.3);

        let mut tool_b = create_test_tool("tool-b", 0.6);
        tool_b.set_relevance("bidding", 0.9);

        backend.register(tool_a);
        backend.register(tool_b);

        // Load all tools - they should be ranked by base ranking
        // since no task filter is applied
        let results = backend
            .load(
                &Predicate::all(),
                &Modifiers::with_limit(2),
            )
            .await
            .unwrap();

        // Without task filter, tool-a should rank higher (0.8 > 0.6)
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "tool-a"); // 0.8 ranking
        assert_eq!(results[1].id, "tool-b"); // 0.6 ranking
    }

    #[tokio::test]
    async fn test_filter_by_category() {
        let backend = ToolsBackend::new();

        backend
            .store(
                "file-read",
                json!({"name": "Read", "category": "file", "description": ""}),
                Scope::Shared,
                None,
                None,
            )
            .await
            .unwrap();

        backend
            .store(
                "http-get",
                json!({"name": "GET", "category": "network", "description": ""}),
                Scope::Shared,
                None,
                None,
            )
            .await
            .unwrap();

        let results = backend
            .load(
                &Predicate::where_eq("category", "file"),
                &Modifiers::default(),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "file-read");
    }

    #[tokio::test]
    async fn test_forget_tools() {
        let backend = ToolsBackend::new();

        backend.register(create_test_tool("tool-1", 0.5));
        backend.register(create_test_tool("tool-2", 0.5));
        backend.register(create_test_tool("tool-3", 0.5));

        let count = backend
            .forget(&Predicate::key("id", "tool-2"))
            .await
            .unwrap();

        assert_eq!(count, 1);
        assert_eq!(backend.count().await, 2);
        assert!(backend.get("tool-2").is_none());
    }
}
