//! Main ADB Instance
//!
//! The `Adb` struct provides the unified interface for all agent memory operations.

use std::sync::Arc;
use std::time::Duration;

use adb_core::{
    AdbError, AdbResult, Link, LinkPredicate, MemoryRecord, MemoryType, Modifiers, Predicate,
    Scope,
};

use crate::config::AdbConfig;
use crate::{
    Backend, EpisodicBackend, LinkStore, LinkStoreOps, ProceduralBackend, SemanticBackend,
    ToolsBackend, WorkingBackend,
};

/// The main Agent Database instance
///
/// Provides unified access to all five memory types and the link store.
pub struct Adb {
    /// Configuration
    config: AdbConfig,
    /// Working memory backend
    working: Arc<WorkingBackend>,
    /// Tools registry backend
    tools: Arc<ToolsBackend>,
    /// Procedural memory backend
    procedural: Arc<ProceduralBackend>,
    /// Semantic memory backend
    semantic: Arc<SemanticBackend>,
    /// Episodic memory backend
    episodic: Arc<EpisodicBackend>,
    /// Link store for cross-memory ontology
    links: Arc<LinkStore>,
}

impl Adb {
    /// Create a new ADB instance with default configuration
    pub fn new() -> Self {
        Self::with_config(AdbConfig::default())
    }

    /// Create a new ADB instance with custom configuration
    pub fn with_config(config: AdbConfig) -> Self {
        let working = Arc::new(WorkingBackend::new());

        // Start TTL reaper if enabled
        if config.working.ttl_enabled {
            working.start_ttl_reaper(config.working.ttl_check_interval);
        }

        Self {
            working,
            tools: Arc::new(ToolsBackend::with_decay_factor(config.tools.decay_factor)),
            procedural: Arc::new(ProceduralBackend::with_threshold(
                config.procedural.default_threshold,
            )),
            semantic: Arc::new(SemanticBackend::new().expect("Failed to create semantic backend")),
            episodic: Arc::new(EpisodicBackend::new()),
            links: Arc::new(LinkStore::new()),
            config,
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &AdbConfig {
        &self.config
    }

    // =========================================================================
    // Store Operations
    // =========================================================================

    /// Store a record in the specified memory type
    pub async fn store(
        &self,
        memory_type: MemoryType,
        key: &str,
        data: serde_json::Value,
    ) -> AdbResult<MemoryRecord> {
        self.store_with_options(memory_type, key, data, self.config.default_scope, None, None)
            .await
    }

    /// Store a record with options
    pub async fn store_with_options(
        &self,
        memory_type: MemoryType,
        key: &str,
        data: serde_json::Value,
        scope: Scope,
        namespace: Option<&str>,
        ttl: Option<Duration>,
    ) -> AdbResult<MemoryRecord> {
        match memory_type {
            MemoryType::Working => {
                self.working.store(key, data, scope, namespace, ttl).await
            }
            MemoryType::Tools => self.tools.store(key, data, scope, namespace, ttl).await,
            MemoryType::Procedural => {
                self.procedural.store(key, data, scope, namespace, ttl).await
            }
            MemoryType::Semantic => {
                self.semantic.store(key, data, scope, namespace, ttl).await
            }
            MemoryType::Episodic => {
                self.episodic.store(key, data, scope, namespace, ttl).await
            }
        }
    }

    // =========================================================================
    // Lookup Operations
    // =========================================================================

    /// Lookup records from the specified memory type
    pub async fn lookup(
        &self,
        memory_type: MemoryType,
        predicate: &Predicate,
    ) -> AdbResult<Vec<MemoryRecord>> {
        self.lookup_with_modifiers(memory_type, predicate, &Modifiers::default())
            .await
    }

    /// Lookup with modifiers
    pub async fn lookup_with_modifiers(
        &self,
        memory_type: MemoryType,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        match memory_type {
            MemoryType::Working => self.working.lookup(predicate, modifiers).await,
            MemoryType::Tools => self.tools.lookup(predicate, modifiers).await,
            MemoryType::Procedural => self.procedural.lookup(predicate, modifiers).await,
            MemoryType::Semantic => self.semantic.lookup(predicate, modifiers).await,
            MemoryType::Episodic => self.episodic.lookup(predicate, modifiers).await,
        }
    }

    // =========================================================================
    // Recall Operations
    // =========================================================================

    /// Recall records from the specified memory type
    pub async fn recall(
        &self,
        memory_type: MemoryType,
        predicate: &Predicate,
    ) -> AdbResult<Vec<MemoryRecord>> {
        self.recall_with_modifiers(memory_type, predicate, &Modifiers::default())
            .await
    }

    /// Recall with modifiers
    pub async fn recall_with_modifiers(
        &self,
        memory_type: MemoryType,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> AdbResult<Vec<MemoryRecord>> {
        match memory_type {
            MemoryType::Working => self.working.recall(predicate, modifiers).await,
            MemoryType::Tools => self.tools.recall(predicate, modifiers).await,
            MemoryType::Procedural => self.procedural.recall(predicate, modifiers).await,
            MemoryType::Semantic => self.semantic.recall(predicate, modifiers).await,
            MemoryType::Episodic => self.episodic.recall(predicate, modifiers).await,
        }
    }

    // =========================================================================
    // Scan Operations (Working only)
    // =========================================================================

    /// Scan all records from working memory
    pub async fn scan(&self) -> AdbResult<Vec<MemoryRecord>> {
        self.working.scan(None).await
    }

    /// Scan with window
    pub async fn scan_window(
        &self,
        window: &adb_core::Window,
    ) -> AdbResult<Vec<MemoryRecord>> {
        self.working.scan(Some(window)).await
    }

    // =========================================================================
    // Load Operations (Tools only)
    // =========================================================================

    /// Load top-ranked tools matching predicate
    pub async fn load(
        &self,
        predicate: &Predicate,
        limit: usize,
    ) -> AdbResult<Vec<MemoryRecord>> {
        self.tools
            .load(predicate, &Modifiers::with_limit(limit))
            .await
    }

    // =========================================================================
    // Forget Operations
    // =========================================================================

    /// Forget records matching predicate from the specified memory type
    pub async fn forget(
        &self,
        memory_type: MemoryType,
        predicate: &Predicate,
    ) -> AdbResult<u64> {
        match memory_type {
            MemoryType::Working => self.working.forget(predicate).await,
            MemoryType::Tools => self.tools.forget(predicate).await,
            MemoryType::Procedural => self.procedural.forget(predicate).await,
            MemoryType::Semantic => self.semantic.forget(predicate).await,
            MemoryType::Episodic => self.episodic.forget(predicate).await,
        }
    }

    // =========================================================================
    // Update Operations
    // =========================================================================

    /// Update records matching predicate in the specified memory type
    pub async fn update(
        &self,
        memory_type: MemoryType,
        predicate: &Predicate,
        data: serde_json::Value,
    ) -> AdbResult<u64> {
        match memory_type {
            MemoryType::Working => self.working.update(predicate, data).await,
            MemoryType::Tools => self.tools.update(predicate, data).await,
            MemoryType::Procedural => self.procedural.update(predicate, data).await,
            MemoryType::Semantic => self.semantic.update(predicate, data).await,
            MemoryType::Episodic => self.episodic.update(predicate, data).await,
        }
    }

    // =========================================================================
    // Link Operations (Ontology)
    // =========================================================================

    /// Create a link between two records
    pub async fn link(
        &self,
        from_type: MemoryType,
        from_id: &str,
        to_type: MemoryType,
        to_id: &str,
        link_type: &str,
        weight: f32,
    ) -> AdbResult<Link> {
        self.links
            .link(from_type, from_id, to_type, to_id, link_type, weight)
            .await
    }

    /// Get links from a record
    pub async fn get_links_from(
        &self,
        from_type: MemoryType,
        from_id: &str,
        link_type: Option<&str>,
    ) -> AdbResult<Vec<Link>> {
        self.links.get_links_from(from_type, from_id, link_type).await
    }

    /// Get links to a record
    pub async fn get_links_to(
        &self,
        to_type: MemoryType,
        to_id: &str,
        link_type: Option<&str>,
    ) -> AdbResult<Vec<Link>> {
        self.links.get_links_to(to_type, to_id, link_type).await
    }

    /// Get all links matching predicate
    pub async fn get_links(&self, predicate: &LinkPredicate) -> AdbResult<Vec<Link>> {
        self.links.get_links(predicate).await
    }

    /// Update link weight
    pub async fn update_link_weight(&self, link_id: &str, weight: f32) -> AdbResult<()> {
        self.links.update_weight(link_id, weight).await
    }

    /// Update link weight with success/failure signal
    pub async fn update_link_with_signal(
        &self,
        link_id: &str,
        signal: f32,
        decay: f32,
    ) -> AdbResult<()> {
        self.links
            .update_weight_with_signal(link_id, signal, decay)
            .await
    }

    /// Forget links matching predicate
    pub async fn forget_links(&self, predicate: &LinkPredicate) -> AdbResult<u64> {
        self.links.forget_links(predicate).await
    }

    // =========================================================================
    // Utility Operations
    // =========================================================================

    /// Get record count for a memory type
    pub async fn count(&self, memory_type: MemoryType) -> usize {
        match memory_type {
            MemoryType::Working => self.working.count().await,
            MemoryType::Tools => self.tools.count().await,
            MemoryType::Procedural => self.procedural.count().await,
            MemoryType::Semantic => self.semantic.count().await,
            MemoryType::Episodic => self.episodic.count().await,
        }
    }

    /// Get link count
    pub async fn link_count(&self) -> usize {
        self.links.count().await
    }

    /// Clear all records from a memory type
    pub async fn clear(&self, memory_type: MemoryType) -> AdbResult<()> {
        match memory_type {
            MemoryType::Working => self.working.clear().await,
            MemoryType::Tools => self.tools.clear().await,
            MemoryType::Procedural => self.procedural.clear().await,
            MemoryType::Semantic => self.semantic.clear().await,
            MemoryType::Episodic => self.episodic.clear().await,
        }
    }

    /// Clear all memories and links
    pub async fn clear_all(&self) -> AdbResult<()> {
        self.working.clear().await?;
        self.tools.clear().await?;
        self.procedural.clear().await?;
        self.semantic.clear().await?;
        self.episodic.clear().await?;
        self.links.clear().await?;
        Ok(())
    }

    // =========================================================================
    // Direct Backend Access (for advanced use)
    // =========================================================================

    /// Get working memory backend
    pub fn working(&self) -> &Arc<WorkingBackend> {
        &self.working
    }

    /// Get tools backend
    pub fn tools(&self) -> &Arc<ToolsBackend> {
        &self.tools
    }

    /// Get procedural backend
    pub fn procedural(&self) -> &Arc<ProceduralBackend> {
        &self.procedural
    }

    /// Get semantic backend
    pub fn semantic(&self) -> &Arc<SemanticBackend> {
        &self.semantic
    }

    /// Get episodic backend
    pub fn episodic(&self) -> &Arc<EpisodicBackend> {
        &self.episodic
    }

    /// Get link store
    pub fn link_store(&self) -> &Arc<LinkStore> {
        &self.links
    }
}

impl Default for Adb {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_create_adb() {
        let adb = Adb::new();
        assert_eq!(adb.count(MemoryType::Working).await, 0);
        assert_eq!(adb.count(MemoryType::Tools).await, 0);
        assert_eq!(adb.count(MemoryType::Procedural).await, 0);
    }

    #[tokio::test]
    async fn test_store_and_lookup_working() {
        let adb = Adb::new();

        adb.store(
            MemoryType::Working,
            "task-1",
            json!({"status": "pending", "priority": 5}),
        )
        .await
        .unwrap();

        let results = adb
            .lookup(MemoryType::Working, &Predicate::key("id", "task-1"))
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].get_str("status"), Some("pending"));
    }

    #[tokio::test]
    async fn test_store_and_load_tools() {
        let adb = Adb::new();

        adb.store(
            MemoryType::Tools,
            "read-file",
            json!({
                "name": "Read File",
                "description": "Reads file contents",
                "category": "file",
                "ranking": 0.8
            }),
        )
        .await
        .unwrap();

        adb.store(
            MemoryType::Tools,
            "write-file",
            json!({
                "name": "Write File",
                "description": "Writes file contents",
                "category": "file",
                "ranking": 0.6
            }),
        )
        .await
        .unwrap();

        let results = adb.load(&Predicate::all(), 2).await.unwrap();

        assert_eq!(results.len(), 2);
        // Should be sorted by ranking
        assert_eq!(results[0].id, "read-file");
    }

    #[tokio::test]
    async fn test_store_and_lookup_procedural() {
        let adb = Adb::new();

        adb.store(
            MemoryType::Procedural,
            "oom-fix",
            json!({
                "pattern": "OOMKilled container memory",
                "steps": ["Check limits", "Increase memory", "Restart"],
                "severity": "critical"
            }),
        )
        .await
        .unwrap();

        let results = adb
            .lookup(MemoryType::Procedural, &Predicate::key("id", "oom-fix"))
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_scan_working() {
        let adb = Adb::new();

        for i in 0..5 {
            adb.store(
                MemoryType::Working,
                &format!("item-{}", i),
                json!({"index": i}),
            )
            .await
            .unwrap();
        }

        let results = adb.scan().await.unwrap();
        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn test_link_operations() {
        let adb = Adb::new();

        // Create procedure and episode
        adb.store(
            MemoryType::Procedural,
            "oom-fix",
            json!({"pattern": "OOM", "steps": ["fix"]}),
        )
        .await
        .unwrap();

        // Create link
        let link = adb
            .link(
                MemoryType::Procedural,
                "oom-fix",
                MemoryType::Episodic,
                "inc-001",
                "applied_to",
                0.95,
            )
            .await
            .unwrap();

        assert_eq!(link.link_type, "applied_to");
        assert!((link.weight - 0.95).abs() < 0.01);

        // Get links
        let links = adb
            .get_links_from(MemoryType::Procedural, "oom-fix", None)
            .await
            .unwrap();

        assert_eq!(links.len(), 1);

        // Update link weight
        adb.update_link_with_signal(&link.id, 1.0, 0.9)
            .await
            .unwrap();

        let links = adb
            .get_links_from(MemoryType::Procedural, "oom-fix", None)
            .await
            .unwrap();

        // Should have increased slightly
        assert!(links[0].weight > 0.95);
    }

    #[tokio::test]
    async fn test_forget() {
        let adb = Adb::new();

        adb.store(MemoryType::Working, "temp-1", json!({"temp": true}))
            .await
            .unwrap();
        adb.store(MemoryType::Working, "temp-2", json!({"temp": true}))
            .await
            .unwrap();
        adb.store(MemoryType::Working, "keep", json!({"temp": false}))
            .await
            .unwrap();

        let count = adb
            .forget(MemoryType::Working, &Predicate::where_eq("temp", true))
            .await
            .unwrap();

        assert_eq!(count, 2);
        assert_eq!(adb.count(MemoryType::Working).await, 1);
    }

    #[tokio::test]
    async fn test_update() {
        let adb = Adb::new();

        adb.store(
            MemoryType::Working,
            "task-1",
            json!({"status": "pending"}),
        )
        .await
        .unwrap();

        adb.update(
            MemoryType::Working,
            &Predicate::key("id", "task-1"),
            json!({"status": "completed"}),
        )
        .await
        .unwrap();

        let results = adb
            .lookup(MemoryType::Working, &Predicate::key("id", "task-1"))
            .await
            .unwrap();

        assert_eq!(results[0].get_str("status"), Some("completed"));
    }

    #[tokio::test]
    async fn test_clear() {
        let adb = Adb::new();

        for i in 0..10 {
            adb.store(
                MemoryType::Working,
                &format!("item-{}", i),
                json!({"i": i}),
            )
            .await
            .unwrap();
        }

        assert_eq!(adb.count(MemoryType::Working).await, 10);

        adb.clear(MemoryType::Working).await.unwrap();

        assert_eq!(adb.count(MemoryType::Working).await, 0);
    }

    #[tokio::test]
    async fn test_config_builder() {
        let config = AdbConfig::builder()
            .default_scope(Scope::Shared)
            .default_namespace("agent-k8s")
            .default_timeout(std::time::Duration::from_millis(200))
            .ttl_enabled(false)
            .build();

        let adb = Adb::with_config(config);

        assert_eq!(adb.config().default_scope, Scope::Shared);
        assert_eq!(
            adb.config().default_namespace,
            Some("agent-k8s".to_string())
        );
    }
}
