# ADB (Agent Database) - Rust Implementation Architecture

> **Version:** 0.1.0
> **Date:** April 2026
> **Based on:** AQL Specification v0.5
> **Reference Implementation:** Python (aql-ref-db)

---

## 1. Executive Summary

ADB is a **unified in-memory multimodal database** for autonomous AI agents. It implements the **Agent Query Language (AQL)** specification, providing five cognitive memory types within a single process with zero network hops.

### Core Thesis

> Agent learning is the accumulation of two variables:
> 1. **Dynamic ontology** of relationship types between memory records (LINK statements)
> 2. **Tuned execution parameters** within procedural memory (UPDATE statements)
>
> ADB provides the storage and retrieval layer. AQL provides the query interface.

### Why Rust?

| Requirement | Rust Solution |
|-------------|---------------|
| Sub-millisecond latency | Zero-cost abstractions, no GC |
| Memory safety | Ownership model, borrow checker |
| Concurrent access | `tokio` async runtime, `dashmap` |
| Embedding computation | Native SIMD, `usearch` bindings |
| Arrow/Parquet support | `arrow-rs`, `datafusion` |
| Graph operations | `petgraph` (mature Rust graph library) |
| Production deployment | Single static binary, MCP server |

---

## 2. System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         ADB Server                                   │
│  ┌─────────────────────────────────────────────────────────────────┐ │
│  │                      MCP Interface                               │ │
│  │  (JSON-RPC, stdio/http transport, tool registration)            │ │
│  └─────────────────────────────────────────────────────────────────┘ │
│                               │                                      │
│  ┌─────────────────────────────────────────────────────────────────┐ │
│  │                     Query Engine                                 │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │ │
│  │  │    Parser    │──│   Planner    │──│   Executor   │           │ │
│  │  │  (pest/nom)  │  │   (Router)   │  │ (TaskRunner) │           │ │
│  │  └──────────────┘  └──────────────┘  └──────────────┘           │ │
│  └─────────────────────────────────────────────────────────────────┘ │
│                               │                                      │
│  ┌─────────────────────────────────────────────────────────────────┐ │
│  │                    Backend Layer                                 │ │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │ │
│  │  │ Working │ │  Tools  │ │Procedur.│ │Semantic │ │Episodic │   │ │
│  │  │DashMap  │ │ HashMap │ │petgraph │ │ usearch │ │DataFusn │   │ │
│  │  │  <1ms   │ │  <2ms   │ │  <5ms   │ │ <20ms   │ │ <50ms   │   │ │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘   │ │
│  └─────────────────────────────────────────────────────────────────┘ │
│                               │                                      │
│  ┌─────────────────────────────────────────────────────────────────┐ │
│  │                    Link Store (Ontology)                         │ │
│  │  Typed edges connecting records across memory types              │ │
│  │  ┌────────────────────────────────────────────────────────────┐ │ │
│  │  │ (PROCEDURAL, "oom-fix") ──[applied_to:0.95]──> (EPISODIC, "inc-001") │ │
│  │  └────────────────────────────────────────────────────────────┘ │ │
│  └─────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 3. Crate Structure

```
adb/
├── Cargo.toml                    # Workspace root
├── README.md
├── ai/
│   └── ARCHITECTURE.md           # This document
│
├── crates/
│   ├── aql-parser/               # AQL → AST
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── lexer.rs          # Token stream
│   │   │   ├── grammar.pest      # PEG grammar (or nom combinators)
│   │   │   ├── ast.rs            # AST types
│   │   │   └── error.rs          # Parse errors
│   │   └── tests/
│   │
│   ├── aql-planner/              # AST → TaskList
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── router.rs         # Verb + MemType → Backend
│   │   │   ├── estimator.rs      # Latency budget allocation
│   │   │   ├── task.rs           # Task, TaskList types
│   │   │   └── planner.rs        # Main orchestrator
│   │   └── tests/
│   │
│   ├── adb-core/                 # Core types & traits
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── record.rs         # MemoryRecord, Metadata
│   │   │   ├── backend.rs        # Backend trait
│   │   │   ├── link.rs           # Link, LinkStore trait
│   │   │   ├── scope.rs          # Scope, Namespace
│   │   │   ├── error.rs          # AdbError
│   │   │   └── time.rs           # Duration, TTL utilities
│   │   └── tests/
│   │
│   ├── adb-backends/             # Backend implementations
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── working.rs        # DashMap backend
│   │   │   ├── tools.rs          # Tool registry
│   │   │   ├── procedural.rs     # petgraph backend
│   │   │   ├── semantic.rs       # usearch + embeddings
│   │   │   ├── episodic.rs       # DataFusion backend
│   │   │   └── links.rs          # Cross-memory link store
│   │   └── tests/
│   │
│   ├── adb-executor/             # Query execution
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── executor.rs       # TaskExecutor
│   │   │   ├── merger.rs         # REFLECT result assembly
│   │   │   ├── pipeline.rs       # PIPELINE execution
│   │   │   └── timeout.rs        # Budget enforcement
│   │   └── tests/
│   │
│   ├── adb-server/               # ADB instance
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── adb.rs            # Main ADB struct
│   │   │   ├── config.rs         # Configuration
│   │   │   └── builder.rs        # AdbBuilder
│   │   └── tests/
│   │
│   └── adb-mcp/                  # MCP server interface
│       ├── Cargo.toml
│       ├── src/
│       │   ├── lib.rs
│       │   ├── server.rs         # MCP server
│       │   ├── tools.rs          # Tool definitions
│       │   └── transport.rs      # stdio/http
│       └── tests/
│
├── adb-cli/                      # CLI binary
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
│
└── examples/
    ├── simple_agent.rs
    ├── k8s_incident.rs
    └── rtb_bidding.rs
```

---

## 4. Core Types

### 4.1 Memory Record

```rust
// crates/adb-core/src/record.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub memory_type: MemoryType,
    pub data: serde_json::Value,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryType {
    Working,
    Tools,
    Procedural,
    Semantic,
    Episodic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub created_at: DateTime<Utc>,
    pub accessed_at: DateTime<Utc>,
    pub scope: Scope,
    pub namespace: Option<String>,
    pub ttl: Option<Duration>,
    pub version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Private,   // Single agent
    Shared,    // Agent group
    Cluster,   // All agents
}
```

### 4.2 Backend Trait

```rust
// crates/adb-core/src/backend.rs

use async_trait::async_trait;

#[async_trait]
pub trait Backend: Send + Sync {
    /// Store a record
    async fn store(
        &self,
        key: &str,
        data: serde_json::Value,
        scope: Scope,
        namespace: Option<&str>,
        ttl: Option<Duration>,
    ) -> Result<MemoryRecord, AdbError>;

    /// Lookup by exact key or predicate
    async fn lookup(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> Result<Vec<MemoryRecord>, AdbError>;

    /// Recall by similarity or condition
    async fn recall(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> Result<Vec<MemoryRecord>, AdbError>;

    /// Delete records matching predicate
    async fn forget(
        &self,
        predicate: &Predicate,
    ) -> Result<u64, AdbError>;

    /// Update records matching predicate
    async fn update(
        &self,
        predicate: &Predicate,
        data: serde_json::Value,
    ) -> Result<u64, AdbError>;

    /// Full scan (Working memory only)
    async fn scan(
        &self,
        window: Option<&Window>,
    ) -> Result<Vec<MemoryRecord>, AdbError> {
        Err(AdbError::UnsupportedOperation("scan"))
    }

    /// Load with ranking (Tools only)
    async fn load(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> Result<Vec<MemoryRecord>, AdbError> {
        Err(AdbError::UnsupportedOperation("load"))
    }

    /// Memory type this backend handles
    fn memory_type(&self) -> MemoryType;

    /// Expected latency for operations
    fn latency_estimate(&self) -> LatencyEstimate;
}

#[derive(Debug, Clone)]
pub struct LatencyEstimate {
    pub lookup_p50_ms: u64,
    pub lookup_p99_ms: u64,
    pub recall_p50_ms: u64,
    pub recall_p99_ms: u64,
}
```

### 4.3 Link (Ontology Edge)

```rust
// crates/adb-core/src/link.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub id: String,
    pub from_type: MemoryType,
    pub from_id: String,
    pub to_type: MemoryType,
    pub to_id: String,
    pub link_type: String,       // LLM-defined, arbitrary string
    pub weight: f32,             // 0.0 - 1.0
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait LinkStore: Send + Sync {
    /// Create a link between records
    async fn link(
        &self,
        from_type: MemoryType,
        from_id: &str,
        to_type: MemoryType,
        to_id: &str,
        link_type: &str,
        weight: f32,
    ) -> Result<Link, AdbError>;

    /// Get links from a record
    async fn get_links_from(
        &self,
        from_type: MemoryType,
        from_id: &str,
        link_type: Option<&str>,
    ) -> Result<Vec<Link>, AdbError>;

    /// Get links to a record
    async fn get_links_to(
        &self,
        to_type: MemoryType,
        to_id: &str,
        link_type: Option<&str>,
    ) -> Result<Vec<Link>, AdbError>;

    /// Follow links (traverse)
    async fn follow_links(
        &self,
        from_type: MemoryType,
        from_id: &str,
        link_type: &str,
        depth: u32,
    ) -> Result<Vec<(Link, MemoryRecord)>, AdbError>;

    /// Update link weight
    async fn update_weight(
        &self,
        link_id: &str,
        weight: f32,
    ) -> Result<(), AdbError>;

    /// Delete links matching criteria
    async fn forget_links(
        &self,
        predicate: &LinkPredicate,
    ) -> Result<u64, AdbError>;
}
```

---

## 5. Backend Implementations

### 5.1 Working Memory (DashMap)

**Purpose:** Current task state, active context
**Latency Target:** < 1ms
**Backend:** `dashmap::DashMap<String, MemoryRecord>`

```rust
// crates/adb-backends/src/working.rs

use dashmap::DashMap;
use tokio::time::{interval, Duration};

pub struct WorkingBackend {
    store: DashMap<String, MemoryRecord>,
    ttl_enabled: bool,
}

impl WorkingBackend {
    pub fn new() -> Self {
        Self {
            store: DashMap::new(),
            ttl_enabled: true,
        }
    }

    /// Background task to expire TTL'd entries
    pub fn start_ttl_reaper(&self, check_interval: Duration) {
        let store = self.store.clone();
        tokio::spawn(async move {
            let mut interval = interval(check_interval);
            loop {
                interval.tick().await;
                let now = Utc::now();
                store.retain(|_, record| {
                    match &record.metadata.ttl {
                        Some(ttl) => {
                            let expires_at = record.metadata.created_at + *ttl;
                            now < expires_at
                        }
                        None => true,
                    }
                });
            }
        });
    }
}

#[async_trait]
impl Backend for WorkingBackend {
    async fn scan(&self, window: Option<&Window>) -> Result<Vec<MemoryRecord>, AdbError> {
        let records: Vec<_> = self.store.iter()
            .map(|r| r.value().clone())
            .collect();

        match window {
            Some(Window::Last(n)) => Ok(records.into_iter().rev().take(*n).collect()),
            Some(Window::LastDuration(d)) => {
                let cutoff = Utc::now() - *d;
                Ok(records.into_iter()
                    .filter(|r| r.metadata.created_at > cutoff)
                    .collect())
            }
            Some(Window::TopBy { n, field }) => {
                // Sort by field, take top n
                // Implementation uses serde_json field extraction
                unimplemented!()
            }
            Some(Window::Since { field, value }) => {
                // Everything since condition
                unimplemented!()
            }
            None => Ok(records),
        }
    }

    fn memory_type(&self) -> MemoryType {
        MemoryType::Working
    }

    fn latency_estimate(&self) -> LatencyEstimate {
        LatencyEstimate {
            lookup_p50_ms: 0,
            lookup_p99_ms: 1,
            recall_p50_ms: 0,
            recall_p99_ms: 1,
        }
    }
}
```

### 5.2 Tools Registry

**Purpose:** Tool records with dynamic ranking
**Latency Target:** < 2ms
**Backend:** `HashMap<String, ToolRecord>` with ranking scores

```rust
// crates/adb-backends/src/tools.rs

use std::collections::HashMap;
use parking_lot::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecord {
    pub tool_id: String,
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,   // JSON Schema
    pub category: String,
    pub ranking: f32,                 // 0.0 - 1.0
    pub call_count: u64,
    pub last_called: Option<DateTime<Utc>>,
    pub relevance_scores: HashMap<String, f32>,  // task_type -> score
}

pub struct ToolsBackend {
    tools: RwLock<HashMap<String, ToolRecord>>,
    decay_factor: f32,  // Default: 0.9
}

impl ToolsBackend {
    /// Update ranking after tool use
    /// ranking = old_ranking * decay_factor + success_signal * (1 - decay_factor)
    pub async fn update_ranking(
        &self,
        tool_id: &str,
        success_signal: f32,
    ) -> Result<(), AdbError> {
        let mut tools = self.tools.write();
        if let Some(tool) = tools.get_mut(tool_id) {
            tool.ranking = tool.ranking * self.decay_factor
                         + success_signal * (1.0 - self.decay_factor);
            tool.call_count += 1;
            tool.last_called = Some(Utc::now());
        }
        Ok(())
    }
}

#[async_trait]
impl Backend for ToolsBackend {
    async fn load(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> Result<Vec<MemoryRecord>, AdbError> {
        let tools = self.tools.read();

        // Filter by predicate (task type, category, relevance)
        let mut matches: Vec<_> = tools.values()
            .filter(|t| predicate.matches(t))
            .cloned()
            .collect();

        // Sort by ranking DESC
        matches.sort_by(|a, b| b.ranking.partial_cmp(&a.ranking).unwrap());

        // Apply LIMIT
        if let Some(limit) = modifiers.limit {
            matches.truncate(limit);
        }

        // Convert to MemoryRecord
        Ok(matches.into_iter().map(|t| t.into()).collect())
    }

    fn memory_type(&self) -> MemoryType {
        MemoryType::Tools
    }
}
```

### 5.3 Procedural Memory (petgraph)

**Purpose:** How-to knowledge, runbooks, learned procedures
**Latency Target:** < 5ms
**Backend:** `petgraph::graph::DiGraph`

```rust
// crates/adb-backends/src/procedural.rs

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::dijkstra;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureNode {
    pub pattern_id: String,
    pub pattern: String,           // Pattern to match
    pub steps: Vec<String>,        // Procedure steps
    pub severity: String,
    pub confidence: f32,
    pub source: String,
    pub success_count: u64,
    pub failure_count: u64,
    pub variables: HashMap<String, f32>,  // Tunable parameters
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureEdge {
    pub relationship: String,      // "next", "requires", "triggers"
    pub weight: f32,
}

pub struct ProceduralBackend {
    graph: RwLock<DiGraph<ProcedureNode, ProcedureEdge>>,
    index: RwLock<HashMap<String, NodeIndex>>,  // pattern_id -> NodeIndex
}

impl ProceduralBackend {
    /// Pattern matching using token overlap (Jaccard similarity)
    fn pattern_match(&self, query: &str, pattern: &str, threshold: f32) -> bool {
        let query_tokens: HashSet<_> = query.split_whitespace().collect();
        let pattern_tokens: HashSet<_> = pattern.split_whitespace().collect();

        let intersection = query_tokens.intersection(&pattern_tokens).count();
        let union = query_tokens.union(&pattern_tokens).count();

        if union == 0 {
            return false;
        }

        let similarity = intersection as f32 / union as f32;
        similarity >= threshold
    }

    /// Traverse graph to get dependent procedures
    pub async fn get_dependencies(
        &self,
        pattern_id: &str,
        relationship: &str,
    ) -> Result<Vec<ProcedureNode>, AdbError> {
        let graph = self.graph.read();
        let index = self.index.read();

        if let Some(&node_idx) = index.get(pattern_id) {
            let mut deps = Vec::new();
            for edge in graph.edges(node_idx) {
                if edge.weight().relationship == relationship {
                    let target = edge.target();
                    deps.push(graph[target].clone());
                }
            }
            Ok(deps)
        } else {
            Ok(vec![])
        }
    }
}

#[async_trait]
impl Backend for ProceduralBackend {
    async fn lookup(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> Result<Vec<MemoryRecord>, AdbError> {
        let graph = self.graph.read();

        match predicate {
            Predicate::Pattern { query, threshold } => {
                let matches: Vec<_> = graph.node_weights()
                    .filter(|node| self.pattern_match(query, &node.pattern, *threshold))
                    .cloned()
                    .collect();
                Ok(matches.into_iter().map(|n| n.into()).collect())
            }
            Predicate::Key { field, value } if field == "pattern_id" => {
                let index = self.index.read();
                if let Some(&node_idx) = index.get(value) {
                    Ok(vec![graph[node_idx].clone().into()])
                } else {
                    Ok(vec![])
                }
            }
            _ => Err(AdbError::InvalidPredicate),
        }
    }

    fn memory_type(&self) -> MemoryType {
        MemoryType::Procedural
    }

    fn latency_estimate(&self) -> LatencyEstimate {
        LatencyEstimate {
            lookup_p50_ms: 2,
            lookup_p99_ms: 5,
            recall_p50_ms: 3,
            recall_p99_ms: 8,
        }
    }
}
```

### 5.4 Semantic Memory (usearch + embeddings)

**Purpose:** Facts, concepts, world knowledge
**Latency Target:** < 20ms
**Backend:** `usearch::Index` for vector similarity

```rust
// crates/adb-backends/src/semantic.rs

use usearch::{Index, IndexOptions, MetricKind, ScalarKind};
use std::collections::HashMap;

const EMBEDDING_DIM: usize = 768;  // sentence-transformers default

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticRecord {
    pub concept_id: String,
    pub concept: String,
    pub knowledge: String,
    pub embedding: Option<Vec<f32>>,  // 768-dim vector
}

pub struct SemanticBackend {
    records: RwLock<HashMap<String, SemanticRecord>>,
    index: RwLock<Index>,
    id_to_key: RwLock<HashMap<u64, String>>,  // usearch ID -> concept_id
    next_id: AtomicU64,
    embedder: Box<dyn Embedder + Send + Sync>,
}

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AdbError>;
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, AdbError>;
}

impl SemanticBackend {
    pub fn new(embedder: Box<dyn Embedder + Send + Sync>) -> Self {
        let options = IndexOptions {
            dimensions: EMBEDDING_DIM,
            metric: MetricKind::Cos,  // Cosine similarity
            quantization: ScalarKind::F32,
            ..Default::default()
        };

        Self {
            records: RwLock::new(HashMap::new()),
            index: RwLock::new(Index::new(&options).unwrap()),
            id_to_key: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(0),
            embedder,
        }
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
    ) -> Result<MemoryRecord, AdbError> {
        let concept = data["concept"].as_str().unwrap_or("");
        let knowledge = data["knowledge"].as_str().unwrap_or("");

        // Generate embedding
        let text = format!("{} {}", concept, knowledge);
        let embedding = self.embedder.embed(&text).await?;

        // Add to vector index
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        {
            let mut index = self.index.write();
            index.add(id, &embedding)?;
        }

        // Store record
        let record = SemanticRecord {
            concept_id: key.to_string(),
            concept: concept.to_string(),
            knowledge: knowledge.to_string(),
            embedding: Some(embedding),
        };

        {
            let mut records = self.records.write();
            records.insert(key.to_string(), record.clone());
        }
        {
            let mut id_map = self.id_to_key.write();
            id_map.insert(id, key.to_string());
        }

        Ok(record.into())
    }

    async fn recall(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> Result<Vec<MemoryRecord>, AdbError> {
        match predicate {
            Predicate::Like { embedding } => {
                let limit = modifiers.limit.unwrap_or(10);
                let min_confidence = modifiers.min_confidence.unwrap_or(0.0);

                let index = self.index.read();
                let results = index.search(embedding, limit)?;

                let records = self.records.read();
                let id_map = self.id_to_key.read();

                let matches: Vec<_> = results.keys.iter()
                    .zip(results.distances.iter())
                    .filter(|(_, dist)| 1.0 - *dist >= min_confidence)
                    .filter_map(|(id, dist)| {
                        id_map.get(id)
                            .and_then(|key| records.get(key))
                            .map(|r| (r.clone(), 1.0 - dist))
                    })
                    .collect();

                Ok(matches.into_iter().map(|(r, _)| r.into()).collect())
            }
            Predicate::Key { field, value } if field == "concept" => {
                let records = self.records.read();
                Ok(records.values()
                    .filter(|r| r.concept == *value)
                    .map(|r| r.clone().into())
                    .collect())
            }
            _ => Err(AdbError::InvalidPredicate),
        }
    }

    fn memory_type(&self) -> MemoryType {
        MemoryType::Semantic
    }

    fn latency_estimate(&self) -> LatencyEstimate {
        LatencyEstimate {
            lookup_p50_ms: 5,
            lookup_p99_ms: 15,
            recall_p50_ms: 10,
            recall_p99_ms: 25,
        }
    }
}
```

### 5.5 Episodic Memory (DataFusion)

**Purpose:** Time-series events, outcomes, history
**Latency Target:** < 50ms
**Backend:** `datafusion` with Arrow record batches

```rust
// crates/adb-backends/src/episodic.rs

use arrow::array::{ArrayRef, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use datafusion::prelude::*;
use datafusion::execution::context::SessionContext;
use parking_lot::RwLock;
use std::sync::Arc;

pub struct EpisodicBackend {
    ctx: SessionContext,
    batches: RwLock<Vec<RecordBatch>>,
    schema: Arc<Schema>,
}

impl EpisodicBackend {
    pub fn new() -> Self {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("timestamp", DataType::Timestamp(TimeUnit::Millisecond, None), false),
            Field::new("data", DataType::Utf8, false),  // JSON string
            Field::new("scope", DataType::Utf8, false),
            Field::new("namespace", DataType::Utf8, true),
            Field::new("memory_type", DataType::Utf8, false),
            Field::new("accessed_at", DataType::Timestamp(TimeUnit::Millisecond, None), false),
        ]));

        Self {
            ctx: SessionContext::new(),
            batches: RwLock::new(Vec::new()),
            schema,
        }
    }

    /// Convert batches to DataFrame for querying
    async fn to_dataframe(&self) -> Result<DataFrame, AdbError> {
        let batches = self.batches.read().clone();
        let provider = MemTable::try_new(self.schema.clone(), vec![batches])?;
        self.ctx.read_table(Arc::new(provider))
    }
}

#[async_trait]
impl Backend for EpisodicBackend {
    async fn recall(
        &self,
        predicate: &Predicate,
        modifiers: &Modifiers,
    ) -> Result<Vec<MemoryRecord>, AdbError> {
        let df = self.to_dataframe().await?;

        // Build filter expression from predicate
        let filtered = match predicate {
            Predicate::Where { conditions } => {
                let mut result = df;
                for cond in conditions {
                    let expr = self.condition_to_expr(cond)?;
                    result = result.filter(expr)?;
                }
                result
            }
            _ => df,
        };

        // Apply ORDER BY
        let ordered = if let Some(order_by) = &modifiers.order_by {
            filtered.sort(vec![col(&order_by.field).sort(
                order_by.ascending,
                true,  // nulls_first
            )])?
        } else {
            // Default: ORDER BY timestamp DESC
            filtered.sort(vec![col("timestamp").sort(false, true)])?
        };

        // Apply LIMIT
        let limited = if let Some(limit) = modifiers.limit {
            ordered.limit(0, Some(limit))?
        } else {
            ordered
        };

        // Apply AGGREGATE if present
        let result = if let Some(agg) = &modifiers.aggregate {
            self.apply_aggregate(limited, agg, &modifiers.having).await?
        } else {
            limited
        };

        // Collect results
        let batches = result.collect().await?;
        self.batches_to_records(batches)
    }

    async fn store(
        &self,
        key: &str,
        data: serde_json::Value,
        scope: Scope,
        namespace: Option<&str>,
        ttl: Option<Duration>,
    ) -> Result<MemoryRecord, AdbError> {
        let now = Utc::now();

        // Create single-row RecordBatch
        let id_array = StringArray::from(vec![key]);
        let ts_array = TimestampMillisecondArray::from(vec![now.timestamp_millis()]);
        let data_array = StringArray::from(vec![serde_json::to_string(&data)?]);
        let scope_array = StringArray::from(vec![scope.as_str()]);
        let ns_array = StringArray::from(vec![namespace]);
        let type_array = StringArray::from(vec!["EPISODIC"]);
        let accessed_array = TimestampMillisecondArray::from(vec![now.timestamp_millis()]);

        let batch = RecordBatch::try_new(
            self.schema.clone(),
            vec![
                Arc::new(id_array),
                Arc::new(ts_array),
                Arc::new(data_array),
                Arc::new(scope_array),
                Arc::new(ns_array),
                Arc::new(type_array),
                Arc::new(accessed_array),
            ],
        )?;

        self.batches.write().push(batch);

        // Return record
        Ok(MemoryRecord {
            id: key.to_string(),
            memory_type: MemoryType::Episodic,
            data,
            metadata: Metadata {
                created_at: now,
                accessed_at: now,
                scope,
                namespace: namespace.map(String::from),
                ttl,
                version: 1,
            },
        })
    }

    fn memory_type(&self) -> MemoryType {
        MemoryType::Episodic
    }

    fn latency_estimate(&self) -> LatencyEstimate {
        LatencyEstimate {
            lookup_p50_ms: 15,
            lookup_p99_ms: 40,
            recall_p50_ms: 20,
            recall_p99_ms: 50,
        }
    }
}
```

---

## 6. Query Engine

### 6.1 Parser (pest or nom)

**Choice:** `pest` for readability, `nom` for performance. Recommend `pest` initially.

```pest
// crates/aql-parser/src/grammar.pest

// AQL Grammar v0.5

aql = { SOI ~ statement ~ EOI }

statement = {
    pipeline_stmt
  | reflect_stmt
  | scan_stmt
  | recall_stmt
  | lookup_stmt
  | load_stmt
  | store_stmt
  | update_stmt
  | forget_stmt
  | link_stmt
}

// PIPELINE bid_decision TIMEOUT 80ms
//   LOAD FROM TOOLS WHERE task = "bidding"
//   | LOOKUP FROM SEMANTIC KEY url = {url}
pipeline_stmt = {
    "PIPELINE" ~ identifier ~ timeout_mod?
    ~ pipeline_stage ~ ("|" ~ pipeline_stage)*
}

pipeline_stage = { scan_stmt | recall_stmt | lookup_stmt | load_stmt | reflect_stmt }

// SCAN FROM WORKING WINDOW LAST 10
scan_stmt = {
    "SCAN" ~ "FROM" ~ "WORKING" ~ window_mod? ~ modifiers*
}

// RECALL FROM EPISODIC WHERE pod = "payments" MIN_CONFIDENCE 0.7
recall_stmt = {
    "RECALL" ~ "FROM" ~ memory_type ~ predicate ~ modifiers*
}

// LOOKUP FROM PROCEDURAL PATTERN $log_events THRESHOLD 0.7
lookup_stmt = {
    "LOOKUP" ~ "FROM" ~ memory_type ~ predicate ~ modifiers*
}

// LOAD FROM TOOLS WHERE task = "bidding" ORDER BY ranking DESC LIMIT 3
load_stmt = {
    "LOAD" ~ "FROM" ~ "TOOLS" ~ predicate ~ modifiers*
}

// STORE INTO EPISODIC (incident_id = "inc-001", pod = "payments")
store_stmt = {
    "STORE" ~ "INTO" ~ memory_type ~ payload ~ modifiers*
}

// REFLECT FROM EPISODIC, FROM PROCEDURAL WITH LINKS TYPE "applied_to"
reflect_stmt = {
    "REFLECT" ~ reflect_source ~ ("," ~ reflect_source)*
    ~ with_links_mod? ~ then_clause?
}

reflect_source = { "FROM" ~ memory_type ~ predicate? }

// LINK FROM PROCEDURAL WHERE pattern_id = "oom-fix"
//   TO EPISODIC WHERE incident_id = "inc-001"
//   TYPE "applied_to" WEIGHT 0.95
link_stmt = {
    "LINK" ~ "FROM" ~ memory_type ~ predicate
    ~ "TO" ~ memory_type ~ predicate
    ~ "TYPE" ~ string_literal ~ ("WEIGHT" ~ float)?
}

// Predicates
predicate = {
    where_pred
  | key_pred
  | like_pred
  | pattern_pred
  | all_pred
}

where_pred = { "WHERE" ~ condition ~ ("AND" ~ condition)* }
key_pred = { "KEY" ~ identifier ~ "=" ~ value }
like_pred = { "LIKE" ~ variable }
pattern_pred = { "PATTERN" ~ variable ~ ("THRESHOLD" ~ float)? }
all_pred = { "ALL" }

// Modifiers
modifiers = {
    limit_mod | order_mod | return_mod | timeout_mod
  | min_confidence_mod | scope_mod | namespace_mod | ttl_mod
  | aggregate_mod | having_mod | with_links_mod | follow_links_mod
}

window_mod = {
    "WINDOW" ~ (
        "LAST" ~ integer
      | "LAST" ~ duration
      | "TOP" ~ integer ~ "BY" ~ identifier
      | "SINCE" ~ condition
    )
}

aggregate_mod = {
    "AGGREGATE" ~ agg_func ~ ("," ~ agg_func)*
}

agg_func = {
    ("COUNT" | "AVG" | "SUM" | "MIN" | "MAX") ~ "(" ~ ("*" | identifier) ~ ")" ~ ("AS" ~ identifier)?
}

// Memory types
memory_type = { "WORKING" | "TOOLS" | "PROCEDURAL" | "SEMANTIC" | "EPISODIC" | "ALL" }

// Terminals
identifier = @{ ASCII_ALPHA ~ (ASCII_ALPHANUMERIC | "_")* }
string_literal = @{ "\"" ~ (!"\"" ~ ANY)* ~ "\"" }
float = @{ ASCII_DIGIT+ ~ "." ~ ASCII_DIGIT+ }
integer = @{ ASCII_DIGIT+ }
duration = @{ ASCII_DIGIT+ ~ ("ms" | "s" | "m" | "h" | "d") }
variable = @{ "$" ~ identifier }

WHITESPACE = _{ " " | "\t" | "\n" | "\r" }
```

### 6.2 AST Types

```rust
// crates/aql-parser/src/ast.rs

#[derive(Debug, Clone)]
pub enum Statement {
    Pipeline(PipelineStmt),
    Reflect(ReflectStmt),
    Scan(ScanStmt),
    Recall(RecallStmt),
    Lookup(LookupStmt),
    Load(LoadStmt),
    Store(StoreStmt),
    Update(UpdateStmt),
    Forget(ForgetStmt),
    Link(LinkStmt),
}

#[derive(Debug, Clone)]
pub struct PipelineStmt {
    pub name: String,
    pub timeout: Option<Duration>,
    pub stages: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub struct ReflectStmt {
    pub sources: Vec<ReflectSource>,
    pub with_links: Option<WithLinksMod>,
    pub then_clause: Option<Box<Statement>>,
}

#[derive(Debug, Clone)]
pub struct ReflectSource {
    pub memory_type: MemoryType,
    pub predicate: Option<Predicate>,
}

#[derive(Debug, Clone)]
pub enum Predicate {
    Where(Vec<Condition>),
    Key { field: String, value: Value },
    Like { embedding_var: String },
    Pattern { pattern_var: String, threshold: Option<f32> },
    All,
}

#[derive(Debug, Clone)]
pub struct Modifiers {
    pub limit: Option<usize>,
    pub order_by: Option<OrderBy>,
    pub return_fields: Option<Vec<String>>,
    pub timeout: Option<Duration>,
    pub min_confidence: Option<f32>,
    pub scope: Option<Scope>,
    pub namespace: Option<String>,
    pub ttl: Option<Duration>,
    pub aggregate: Option<Vec<AggFunc>>,
    pub having: Option<Vec<Condition>>,
    pub with_links: Option<WithLinksMod>,
    pub follow_links: Option<FollowLinksMod>,
    pub window: Option<Window>,
}

#[derive(Debug, Clone)]
pub enum Window {
    Last(usize),
    LastDuration(Duration),
    TopBy { n: usize, field: String },
    Since { field: String, value: Value },
}

#[derive(Debug, Clone)]
pub struct LinkStmt {
    pub from_type: MemoryType,
    pub from_predicate: Predicate,
    pub to_type: MemoryType,
    pub to_predicate: Predicate,
    pub link_type: String,
    pub weight: Option<f32>,
}
```

### 6.3 Planner

```rust
// crates/aql-planner/src/planner.rs

use crate::router::Router;
use crate::estimator::LatencyEstimator;
use crate::task::{Task, TaskList, TaskType};

pub struct Planner {
    router: Router,
    estimator: LatencyEstimator,
}

impl Planner {
    pub fn plan(&self, stmt: &Statement) -> Result<TaskList, PlanError> {
        match stmt {
            Statement::Pipeline(p) => self.plan_pipeline(p),
            Statement::Reflect(r) => self.plan_reflect(r),
            Statement::Scan(s) => self.plan_simple(s, TaskType::Scan),
            Statement::Recall(r) => self.plan_simple(r, TaskType::Recall),
            Statement::Lookup(l) => self.plan_simple(l, TaskType::Lookup),
            Statement::Load(l) => self.plan_simple(l, TaskType::Load),
            Statement::Store(s) => self.plan_simple(s, TaskType::Store),
            Statement::Update(u) => self.plan_simple(u, TaskType::Update),
            Statement::Forget(f) => self.plan_simple(f, TaskType::Forget),
            Statement::Link(l) => self.plan_link(l),
        }
    }

    fn plan_pipeline(&self, pipeline: &PipelineStmt) -> Result<TaskList, PlanError> {
        let total_budget_ms = pipeline.timeout
            .map(|d| d.as_millis() as u64)
            .unwrap_or(100);

        // Estimate latency for each stage
        let stage_estimates: Vec<_> = pipeline.stages.iter()
            .map(|s| self.estimator.estimate(s))
            .collect();

        let total_estimated: u64 = stage_estimates.iter().sum();

        // Allocate budget proportionally
        let mut tasks = Vec::new();
        let mut prev_task_id = None;

        for (i, stage) in pipeline.stages.iter().enumerate() {
            let stage_budget = if total_estimated > 0 {
                (stage_estimates[i] as f64 / total_estimated as f64 * total_budget_ms as f64) as u64
            } else {
                total_budget_ms / pipeline.stages.len() as u64
            };

            let task = Task {
                id: format!("{}_{}", pipeline.name, i),
                task_type: self.router.route(stage),
                backend: self.router.backend(stage),
                predicate: self.extract_predicate(stage),
                payload: self.extract_payload(stage),
                modifiers: self.extract_modifiers(stage),
                depends_on: prev_task_id.iter().cloned().collect(),
                budget_ms: stage_budget,
                scope: self.extract_scope(stage),
                namespace: self.extract_namespace(stage),
            };

            prev_task_id = Some(task.id.clone());
            tasks.push(task);
        }

        Ok(TaskList { tasks })
    }

    fn plan_reflect(&self, reflect: &ReflectStmt) -> Result<TaskList, PlanError> {
        // REFLECT generates parallel tasks for each source
        let mut tasks: Vec<Task> = reflect.sources.iter()
            .enumerate()
            .map(|(i, source)| Task {
                id: format!("reflect_source_{}", i),
                task_type: TaskType::Recall,
                backend: source.memory_type.backend_name(),
                predicate: source.predicate.clone(),
                payload: None,
                modifiers: Default::default(),
                depends_on: vec![],
                budget_ms: 20,  // Per-source budget
                scope: Scope::Private,
                namespace: None,
            })
            .collect();

        // Add merge task that depends on all sources
        let source_ids: Vec<_> = tasks.iter().map(|t| t.id.clone()).collect();
        tasks.push(Task {
            id: "reflect_merge".to_string(),
            task_type: TaskType::Merge,
            backend: "merger".to_string(),
            predicate: None,
            payload: None,
            modifiers: Default::default(),
            depends_on: source_ids,
            budget_ms: 5,
            scope: Scope::Private,
            namespace: None,
        });

        // Add THEN clause if present
        if let Some(then_stmt) = &reflect.then_clause {
            let then_tasks = self.plan(then_stmt)?;
            let merge_id = "reflect_merge".to_string();
            for mut task in then_tasks.tasks {
                task.depends_on.push(merge_id.clone());
                tasks.push(task);
            }
        }

        Ok(TaskList { tasks })
    }
}
```

### 6.4 Router

```rust
// crates/aql-planner/src/router.rs

use std::collections::HashMap;

/// Routes (Verb, MemoryType) -> Backend
pub struct Router {
    routes: HashMap<(TaskType, MemoryType), &'static str>,
}

impl Router {
    pub fn new() -> Self {
        let mut routes = HashMap::new();

        // SCAN only valid on WORKING
        routes.insert((TaskType::Scan, MemoryType::Working), "working");

        // LOAD only valid on TOOLS
        routes.insert((TaskType::Load, MemoryType::Tools), "tools");

        // LOOKUP routes
        routes.insert((TaskType::Lookup, MemoryType::Procedural), "procedural");
        routes.insert((TaskType::Lookup, MemoryType::Semantic), "semantic");
        routes.insert((TaskType::Lookup, MemoryType::Tools), "tools");

        // RECALL routes
        routes.insert((TaskType::Recall, MemoryType::Episodic), "episodic");
        routes.insert((TaskType::Recall, MemoryType::Semantic), "semantic");

        // STORE/UPDATE/FORGET valid on specific types
        for mem_type in [
            MemoryType::Working,
            MemoryType::Tools,
            MemoryType::Procedural,
            MemoryType::Semantic,
            MemoryType::Episodic,
        ] {
            routes.insert((TaskType::Store, mem_type), mem_type.backend_name());
            routes.insert((TaskType::Update, mem_type), mem_type.backend_name());
            routes.insert((TaskType::Forget, mem_type), mem_type.backend_name());
        }

        Self { routes }
    }

    pub fn route(&self, stmt: &Statement) -> TaskType {
        match stmt {
            Statement::Scan(_) => TaskType::Scan,
            Statement::Recall(_) => TaskType::Recall,
            Statement::Lookup(_) => TaskType::Lookup,
            Statement::Load(_) => TaskType::Load,
            Statement::Store(_) => TaskType::Store,
            Statement::Update(_) => TaskType::Update,
            Statement::Forget(_) => TaskType::Forget,
            Statement::Link(_) => TaskType::Link,
            Statement::Reflect(_) => TaskType::Reflect,
            Statement::Pipeline(_) => TaskType::Pipeline,
        }
    }

    pub fn backend(&self, stmt: &Statement) -> String {
        let task_type = self.route(stmt);
        let mem_type = stmt.memory_type();

        self.routes
            .get(&(task_type, mem_type))
            .map(|s| s.to_string())
            .unwrap_or_else(|| panic!("No route for {:?} on {:?}", task_type, mem_type))
    }

    pub fn validate(&self, stmt: &Statement) -> Result<(), ValidationError> {
        match stmt {
            Statement::Scan(s) if s.memory_type != MemoryType::Working => {
                Err(ValidationError::InvalidMemoryType {
                    verb: "SCAN",
                    memory_type: s.memory_type,
                    allowed: vec![MemoryType::Working],
                })
            }
            Statement::Load(l) if l.memory_type != MemoryType::Tools => {
                Err(ValidationError::InvalidMemoryType {
                    verb: "LOAD",
                    memory_type: l.memory_type,
                    allowed: vec![MemoryType::Tools],
                })
            }
            Statement::Store(s) if s.memory_type == MemoryType::All => {
                Err(ValidationError::InvalidMemoryType {
                    verb: "STORE",
                    memory_type: MemoryType::All,
                    allowed: vec![
                        MemoryType::Working,
                        MemoryType::Tools,
                        MemoryType::Procedural,
                        MemoryType::Semantic,
                        MemoryType::Episodic,
                    ],
                })
            }
            Statement::Forget(f) if f.predicate.is_none() => {
                Err(ValidationError::MissingPredicate {
                    verb: "FORGET",
                    message: "FORGET requires WHERE clause",
                })
            }
            _ => Ok(()),
        }
    }
}
```

### 6.5 Executor

```rust
// crates/adb-executor/src/executor.rs

use tokio::time::{timeout, Duration};
use futures::future::join_all;

pub struct TaskExecutor {
    backends: HashMap<String, Arc<dyn Backend>>,
    link_store: Arc<dyn LinkStore>,
    merger: ResultMerger,
}

impl TaskExecutor {
    pub async fn execute(&self, task_list: TaskList) -> Result<ExecutionResult, AdbError> {
        // Build dependency graph
        let mut completed: HashMap<String, Vec<MemoryRecord>> = HashMap::new();
        let mut pending: Vec<Task> = task_list.tasks;

        while !pending.is_empty() {
            // Find tasks with satisfied dependencies
            let (ready, not_ready): (Vec<_>, Vec<_>) = pending
                .into_iter()
                .partition(|t| t.depends_on.iter().all(|d| completed.contains_key(d)));

            if ready.is_empty() && !not_ready.is_empty() {
                return Err(AdbError::CyclicDependency);
            }

            // Execute ready tasks in parallel
            let futures: Vec<_> = ready.iter()
                .map(|task| self.execute_task(task, &completed))
                .collect();

            let results = join_all(futures).await;

            for (task, result) in ready.into_iter().zip(results) {
                match result {
                    Ok(records) => {
                        completed.insert(task.id, records);
                    }
                    Err(e) => {
                        // Log error but continue (partial results)
                        tracing::warn!("Task {} failed: {:?}", task.id, e);
                        completed.insert(task.id, vec![]);
                    }
                }
            }

            pending = not_ready;
        }

        // Return final results (from last task or merge task)
        let final_results = completed
            .into_iter()
            .max_by_key(|(id, _)| task_list.tasks.iter().position(|t| &t.id == id))
            .map(|(_, records)| records)
            .unwrap_or_default();

        Ok(ExecutionResult { records: final_results })
    }

    async fn execute_task(
        &self,
        task: &Task,
        completed: &HashMap<String, Vec<MemoryRecord>>,
    ) -> Result<Vec<MemoryRecord>, AdbError> {
        let backend = self.backends.get(&task.backend)
            .ok_or(AdbError::UnknownBackend(task.backend.clone()))?;

        // Apply timeout
        let budget = Duration::from_millis(task.budget_ms);

        timeout(budget, async {
            match task.task_type {
                TaskType::Scan => {
                    backend.scan(task.modifiers.window.as_ref()).await
                }
                TaskType::Recall => {
                    backend.recall(
                        task.predicate.as_ref().ok_or(AdbError::MissingPredicate)?,
                        &task.modifiers,
                    ).await
                }
                TaskType::Lookup => {
                    backend.lookup(
                        task.predicate.as_ref().ok_or(AdbError::MissingPredicate)?,
                        &task.modifiers,
                    ).await
                }
                TaskType::Load => {
                    backend.load(
                        task.predicate.as_ref().ok_or(AdbError::MissingPredicate)?,
                        &task.modifiers,
                    ).await
                }
                TaskType::Store => {
                    let payload = task.payload.as_ref().ok_or(AdbError::MissingPayload)?;
                    let record = backend.store(
                        &uuid::Uuid::new_v4().to_string(),
                        payload.clone(),
                        task.scope,
                        task.namespace.as_deref(),
                        task.modifiers.ttl,
                    ).await?;
                    Ok(vec![record])
                }
                TaskType::Update => {
                    let count = backend.update(
                        task.predicate.as_ref().ok_or(AdbError::MissingPredicate)?,
                        task.payload.clone().ok_or(AdbError::MissingPayload)?,
                    ).await?;
                    Ok(vec![])  // Return empty, count logged
                }
                TaskType::Forget => {
                    let count = backend.forget(
                        task.predicate.as_ref().ok_or(AdbError::MissingPredicate)?,
                    ).await?;
                    Ok(vec![])
                }
                TaskType::Link => {
                    // Handled separately by LinkStore
                    Err(AdbError::InvalidTaskType)
                }
                TaskType::Merge => {
                    // Collect results from dependencies
                    let dep_results: Vec<_> = task.depends_on.iter()
                        .filter_map(|id| completed.get(id))
                        .flatten()
                        .cloned()
                        .collect();
                    Ok(self.merger.merge(dep_results, &task.modifiers))
                }
                TaskType::Reflect => {
                    // Handled by Merge task
                    Err(AdbError::InvalidTaskType)
                }
                TaskType::Pipeline => {
                    // Should be decomposed by planner
                    Err(AdbError::InvalidTaskType)
                }
            }
        }).await.map_err(|_| AdbError::Timeout(task.budget_ms))?
    }
}
```

---

## 7. MCP Server Interface

```rust
// crates/adb-mcp/src/server.rs

use mcp_sdk::{Server, Tool, ToolResult};
use serde_json::json;

pub struct AdbMcpServer {
    adb: Arc<Adb>,
}

impl AdbMcpServer {
    pub fn tools(&self) -> Vec<Tool> {
        vec![
            Tool {
                name: "aql_query".to_string(),
                description: "Execute an AQL query against agent memory".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "AQL query string"
                        }
                    },
                    "required": ["query"]
                }),
            },
            Tool {
                name: "store_memory".to_string(),
                description: "Store a memory record".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "memory_type": {
                            "type": "string",
                            "enum": ["WORKING", "TOOLS", "PROCEDURAL", "SEMANTIC", "EPISODIC"]
                        },
                        "data": {
                            "type": "object",
                            "description": "Record data"
                        },
                        "ttl_seconds": {
                            "type": "integer",
                            "description": "Optional TTL in seconds"
                        }
                    },
                    "required": ["memory_type", "data"]
                }),
            },
            Tool {
                name: "recall_similar".to_string(),
                description: "Recall memories similar to a query".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "memory_type": {
                            "type": "string",
                            "enum": ["SEMANTIC", "EPISODIC", "ALL"]
                        },
                        "query": {
                            "type": "string",
                            "description": "Text to find similar memories for"
                        },
                        "limit": {
                            "type": "integer",
                            "default": 5
                        },
                        "min_confidence": {
                            "type": "number",
                            "default": 0.7
                        }
                    },
                    "required": ["memory_type", "query"]
                }),
            },
            Tool {
                name: "link_memories".to_string(),
                description: "Create a typed link between memory records".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "from_type": { "type": "string" },
                        "from_id": { "type": "string" },
                        "to_type": { "type": "string" },
                        "to_id": { "type": "string" },
                        "link_type": {
                            "type": "string",
                            "description": "Relationship type (e.g., 'applied_to', 'triggers')"
                        },
                        "weight": {
                            "type": "number",
                            "default": 1.0
                        }
                    },
                    "required": ["from_type", "from_id", "to_type", "to_id", "link_type"]
                }),
            },
        ]
    }

    pub async fn handle_tool(&self, name: &str, args: serde_json::Value) -> ToolResult {
        match name {
            "aql_query" => {
                let query = args["query"].as_str().unwrap();
                match self.adb.execute(query).await {
                    Ok(result) => ToolResult::success(json!(result)),
                    Err(e) => ToolResult::error(format!("{:?}", e)),
                }
            }
            "store_memory" => {
                let mem_type = args["memory_type"].as_str().unwrap();
                let data = args["data"].clone();
                let ttl = args.get("ttl_seconds")
                    .and_then(|v| v.as_u64())
                    .map(Duration::from_secs);

                let query = format!(
                    "STORE INTO {} ({}) {}",
                    mem_type,
                    self.format_payload(&data),
                    ttl.map(|t| format!("TTL {}s", t.as_secs())).unwrap_or_default()
                );

                match self.adb.execute(&query).await {
                    Ok(result) => ToolResult::success(json!(result)),
                    Err(e) => ToolResult::error(format!("{:?}", e)),
                }
            }
            _ => ToolResult::error("Unknown tool"),
        }
    }
}
```

---

## 8. Embedding Strategy

### 8.1 Embedder Trait

```rust
// crates/adb-backends/src/semantic.rs

#[async_trait]
pub trait Embedder: Send + Sync {
    /// Generate embedding for text
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AdbError>;

    /// Batch embedding (more efficient)
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, AdbError>;

    /// Embedding dimensions
    fn dimensions(&self) -> usize;
}
```

### 8.2 Embedding Options

| Option | Latency | Quality | Deployment |
|--------|---------|---------|------------|
| **Local ONNX** | ~5ms | Good | In-process, no network |
| **Ollama** | ~20ms | Good | Local server |
| **OpenAI** | ~100ms | Excellent | Network required |
| **Claude** | ~150ms | Excellent | Network required |

**Recommended:** Local ONNX with `ort` crate for production (<20ms latency target).

```rust
// crates/adb-backends/src/embedders/onnx.rs

use ort::{Environment, Session, Value};

pub struct OnnxEmbedder {
    session: Session,
    tokenizer: Tokenizer,
    dimensions: usize,
}

impl OnnxEmbedder {
    pub fn new(model_path: &str) -> Result<Self, AdbError> {
        let env = Environment::builder()
            .with_name("adb_embedder")
            .build()?;

        let session = Session::builder(&env)?
            .with_optimization_level(ort::GraphOptimizationLevel::Level3)?
            .with_model_from_file(model_path)?;

        Ok(Self {
            session,
            tokenizer: Tokenizer::from_pretrained("sentence-transformers/all-MiniLM-L6-v2")?,
            dimensions: 384,  // MiniLM
        })
    }
}

#[async_trait]
impl Embedder for OnnxEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AdbError> {
        let encoding = self.tokenizer.encode(text, true)?;
        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let attention_mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&x| x as i64).collect();

        let outputs = self.session.run(vec![
            Value::from_array(self.session.allocator(), &[input_ids])?,
            Value::from_array(self.session.allocator(), &[attention_mask])?,
        ])?;

        let embeddings: Vec<f32> = outputs[0].try_extract()?.view().iter().cloned().collect();
        Ok(embeddings)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}
```

---

## 9. Configuration

```rust
// crates/adb-server/src/config.rs

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdbConfig {
    /// Server configuration
    pub server: ServerConfig,

    /// Backend configurations
    pub backends: BackendsConfig,

    /// Embedding configuration
    pub embedding: EmbeddingConfig,

    /// MCP server configuration
    pub mcp: McpConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Default scope for operations
    pub default_scope: Scope,

    /// Default namespace (agent identity)
    pub default_namespace: Option<String>,

    /// Maximum concurrent queries
    pub max_concurrent_queries: usize,

    /// Default query timeout
    #[serde(with = "humantime_serde")]
    pub default_timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendsConfig {
    pub working: WorkingConfig,
    pub tools: ToolsConfig,
    pub procedural: ProceduralConfig,
    pub semantic: SemanticConfig,
    pub episodic: EpisodicConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingConfig {
    /// Enable TTL expiration
    pub ttl_enabled: bool,

    /// TTL check interval
    #[serde(with = "humantime_serde")]
    pub ttl_check_interval: Duration,

    /// Maximum entries (0 = unlimited)
    pub max_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConfig {
    /// Embedding model path (ONNX) or provider
    pub embedding_model: String,

    /// Embedding dimensions
    pub embedding_dimensions: usize,

    /// Index type for vector search
    pub index_type: IndexType,

    /// Maximum vectors
    pub max_vectors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexType {
    Flat,      // Exact search (small datasets)
    Hnsw,      // Approximate (large datasets)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicConfig {
    /// Maximum records before archival
    pub max_records: usize,

    /// Archive to Parquet when exceeded
    pub archive_enabled: bool,

    /// Archive path
    pub archive_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Transport type
    pub transport: McpTransport,

    /// Server name
    pub name: String,

    /// Server version
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum McpTransport {
    Stdio,
    Http { host: String, port: u16 },
}

impl Default for AdbConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                default_scope: Scope::Private,
                default_namespace: None,
                max_concurrent_queries: 100,
                default_timeout: Duration::from_millis(100),
            },
            backends: BackendsConfig {
                working: WorkingConfig {
                    ttl_enabled: true,
                    ttl_check_interval: Duration::from_secs(1),
                    max_entries: 10000,
                },
                tools: ToolsConfig {
                    decay_factor: 0.9,
                },
                procedural: ProceduralConfig {
                    default_threshold: 0.7,
                },
                semantic: SemanticConfig {
                    embedding_model: "all-MiniLM-L6-v2".to_string(),
                    embedding_dimensions: 384,
                    index_type: IndexType::Hnsw,
                    max_vectors: 100000,
                },
                episodic: EpisodicConfig {
                    max_records: 1000000,
                    archive_enabled: false,
                    archive_path: None,
                },
            },
            embedding: EmbeddingConfig {
                provider: EmbeddingProvider::Onnx,
                model_path: "models/all-MiniLM-L6-v2.onnx".to_string(),
            },
            mcp: McpConfig {
                transport: McpTransport::Stdio,
                name: "adb".to_string(),
                version: "0.1.0".to_string(),
            },
        }
    }
}
```

---

## 10. Build & Dependencies

### 10.1 Workspace Cargo.toml

```toml
# Cargo.toml (workspace root)

[workspace]
resolver = "2"
members = [
    "crates/aql-parser",
    "crates/aql-planner",
    "crates/adb-core",
    "crates/adb-backends",
    "crates/adb-executor",
    "crates/adb-server",
    "crates/adb-mcp",
    "adb-cli",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
repository = "https://github.com/your-org/adb"

[workspace.dependencies]
# Async runtime
tokio = { version = "1.36", features = ["full"] }
async-trait = "0.1"
futures = "0.3"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Time
chrono = { version = "0.4", features = ["serde"] }
humantime-serde = "1.1"

# Concurrent data structures
dashmap = "5.5"
parking_lot = "0.12"

# Graph
petgraph = "0.6"

# Vector search
usearch = "2.8"

# Arrow/DataFusion (analytics)
arrow = "51"
datafusion = "37"
parquet = "51"

# Parsing
pest = "2.7"
pest_derive = "2.7"

# Error handling
thiserror = "1.0"
anyhow = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# UUID
uuid = { version = "1.7", features = ["v4", "serde"] }

# CLI
clap = { version = "4.5", features = ["derive"] }

# Testing
criterion = "0.5"
proptest = "1.4"
```

### 10.2 adb-backends Cargo.toml

```toml
# crates/adb-backends/Cargo.toml

[package]
name = "adb-backends"
version.workspace = true
edition.workspace = true

[dependencies]
adb-core = { path = "../adb-core" }

tokio.workspace = true
async-trait.workspace = true
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
thiserror.workspace = true
tracing.workspace = true
uuid.workspace = true

# Working memory
dashmap.workspace = true

# Procedural graph
petgraph.workspace = true

# Semantic vectors
usearch.workspace = true

# Episodic analytics
arrow.workspace = true
datafusion.workspace = true
parquet.workspace = true
parking_lot.workspace = true

# Embeddings (optional features)
ort = { version = "2.0", optional = true }
tokenizers = { version = "0.15", optional = true }

[features]
default = ["onnx-embeddings"]
onnx-embeddings = ["ort", "tokenizers"]
```

---

## 11. Testing Strategy

### 11.1 Unit Tests

```rust
// crates/adb-backends/src/working.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_store_and_lookup() {
        let backend = WorkingBackend::new();

        let record = backend.store(
            "test-1",
            json!({"key": "value"}),
            Scope::Private,
            None,
            None,
        ).await.unwrap();

        assert_eq!(record.id, "test-1");

        let results = backend.lookup(
            &Predicate::Key { field: "id".to_string(), value: "test-1".into() },
            &Modifiers::default(),
        ).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data["key"], "value");
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        let backend = WorkingBackend::new();
        backend.start_ttl_reaper(Duration::from_millis(10));

        backend.store(
            "expires",
            json!({"temp": true}),
            Scope::Private,
            None,
            Some(Duration::from_millis(50)),
        ).await.unwrap();

        // Should exist immediately
        let results = backend.scan(None).await.unwrap();
        assert_eq!(results.len(), 1);

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should be gone
        let results = backend.scan(None).await.unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_window_last_n() {
        let backend = WorkingBackend::new();

        for i in 0..10 {
            backend.store(
                &format!("item-{}", i),
                json!({"index": i}),
                Scope::Private,
                None,
                None,
            ).await.unwrap();
        }

        let results = backend.scan(Some(&Window::Last(3))).await.unwrap();
        assert_eq!(results.len(), 3);
    }
}
```

### 11.2 Integration Tests

```rust
// tests/integration/k8s_incident.rs

use adb_server::Adb;

#[tokio::test]
async fn test_k8s_incident_workflow() {
    let adb = Adb::new(AdbConfig::default()).await.unwrap();

    // 1. Store initial alert in working memory
    adb.execute(r#"
        STORE INTO WORKING (
            alert_id = "alert-001",
            pod = "payments-api",
            severity = "critical",
            message = "OOMKilled"
        ) TTL 5m
    "#).await.unwrap();

    // 2. Lookup matching procedure
    let procedures = adb.execute(r#"
        LOOKUP FROM PROCEDURAL PATTERN "OOMKilled" THRESHOLD 0.7
    "#).await.unwrap();

    assert!(!procedures.records.is_empty());

    // 3. Recall similar past incidents
    let history = adb.execute(r#"
        RECALL FROM EPISODIC WHERE pod = "payments-api"
        ORDER BY timestamp DESC LIMIT 5
    "#).await.unwrap();

    // 4. Create link after successful resolution
    adb.execute(r#"
        LINK FROM PROCEDURAL WHERE pattern_id = "oom-fix"
        TO EPISODIC WHERE incident_id = "inc-001"
        TYPE "applied_to" WEIGHT 0.95
    "#).await.unwrap();

    // 5. Verify link was created
    let links = adb.link_store().get_links_from(
        MemoryType::Procedural,
        "oom-fix",
        Some("applied_to"),
    ).await.unwrap();

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].weight, 0.95);
}
```

### 11.3 Benchmarks

```rust
// benches/backends.rs

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use adb_backends::{WorkingBackend, SemanticBackend, EpisodicBackend};

fn working_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let backend = WorkingBackend::new();

    // Pre-populate
    rt.block_on(async {
        for i in 0..10000 {
            backend.store(
                &format!("key-{}", i),
                json!({"index": i, "data": "x".repeat(100)}),
                Scope::Private,
                None,
                None,
            ).await.unwrap();
        }
    });

    c.bench_function("working_lookup", |b| {
        b.to_async(&rt).iter(|| async {
            backend.lookup(
                &Predicate::Key { field: "id".into(), value: "key-5000".into() },
                &Modifiers::default(),
            ).await.unwrap()
        })
    });

    c.bench_function("working_scan_all", |b| {
        b.to_async(&rt).iter(|| async {
            backend.scan(None).await.unwrap()
        })
    });
}

fn semantic_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let embedder = Box::new(MockEmbedder::new(768));
    let backend = SemanticBackend::new(embedder);

    // Pre-populate with 10k vectors
    rt.block_on(async {
        for i in 0..10000 {
            backend.store(
                &format!("concept-{}", i),
                json!({
                    "concept": format!("concept_{}", i),
                    "knowledge": format!("Knowledge about topic {}", i)
                }),
                Scope::Private,
                None,
                None,
            ).await.unwrap();
        }
    });

    let query_embedding: Vec<f32> = (0..768).map(|i| (i as f32).sin()).collect();

    c.bench_function("semantic_recall_top10", |b| {
        b.to_async(&rt).iter(|| async {
            backend.recall(
                &Predicate::Like { embedding: query_embedding.clone() },
                &Modifiers { limit: Some(10), ..Default::default() },
            ).await.unwrap()
        })
    });
}

criterion_group!(benches, working_benchmark, semantic_benchmark);
criterion_main!(benches);
```

---

## 12. Implementation Roadmap

### Phase 1: Core (Weeks 1-2)
- [ ] `adb-core`: Types, traits, errors
- [ ] `aql-parser`: Grammar, lexer, AST
- [ ] `adb-backends/working`: DashMap backend
- [ ] Basic CLI with REPL

### Phase 2: Backends (Weeks 3-4)
- [ ] `adb-backends/tools`: Tool registry with ranking
- [ ] `adb-backends/procedural`: petgraph backend
- [ ] `adb-backends/semantic`: usearch + ONNX embeddings
- [ ] `adb-backends/episodic`: DataFusion backend

### Phase 3: Query Engine (Weeks 5-6)
- [ ] `aql-planner`: Router, estimator, task generation
- [ ] `adb-executor`: Task execution, timeout, dependencies
- [ ] `adb-executor/merger`: REFLECT result assembly
- [ ] Pipeline support with budget allocation

### Phase 4: Links & Ontology (Week 7)
- [ ] `adb-backends/links`: Link store implementation
- [ ] LINK statement execution
- [ ] FOLLOW LINKS traversal
- [ ] WITH LINKS modifier

### Phase 5: Integration (Week 8)
- [ ] `adb-server`: Main ADB struct, builder
- [ ] `adb-mcp`: MCP server with tools
- [ ] Integration tests (K8s, RTB scenarios)
- [ ] Performance benchmarks

### Phase 6: Polish (Week 9-10)
- [ ] Documentation
- [ ] Error messages
- [ ] Logging/tracing
- [ ] Configuration validation
- [ ] Release builds

---

## 13. Design Decisions & Rationale

### 13.1 Why In-Memory Only (Initially)?

- **Latency:** Sub-millisecond for working memory is non-negotiable
- **Simplicity:** No WAL, no durability concerns initially
- **Agent use case:** Most agent memory is session-scoped
- **Future:** Parquet archival for episodic, RocksDB for persistence

### 13.2 Why pest over nom?

- **Readability:** Grammar file is self-documenting
- **Maintainability:** Grammar changes don't require Rust expertise
- **Performance:** Adequate for query parsing (not the bottleneck)
- **Trade-off:** Slightly slower than handwritten nom, but faster development

### 13.3 Why DataFusion for Episodic?

- **SQL-like filtering:** WHERE, ORDER BY, AGGREGATE all work
- **Arrow native:** Zero-copy with other Arrow tools
- **Parquet support:** Future archival path
- **Community:** Active, well-maintained

### 13.4 Why usearch over faiss?

- **Pure Rust bindings:** No Python dependency
- **Cross-platform:** Works on macOS/Linux/Windows
- **Performance:** Comparable to FAISS for HNSW
- **Simplicity:** Single-file index, easy deployment

### 13.5 Why Separate Link Store?

- **Cross-memory:** Links span memory types (Procedural → Episodic)
- **Query efficiency:** Dedicated index for link traversal
- **Ontology growth:** Links are the primary learning mechanism
- **Isolation:** Link operations don't affect backend latency

---

## 14. Success Criteria

### Performance Targets

| Backend | Operation | P50 | P99 |
|---------|-----------|-----|-----|
| Working | lookup | < 0.1ms | < 1ms |
| Working | scan(1000) | < 1ms | < 5ms |
| Tools | load(10) | < 0.5ms | < 2ms |
| Procedural | lookup | < 2ms | < 5ms |
| Procedural | pattern match | < 3ms | < 8ms |
| Semantic | recall(10) | < 10ms | < 25ms |
| Episodic | recall(100) | < 20ms | < 50ms |
| Pipeline | full(80ms budget) | < 80ms | < 100ms |

### Correctness

- All AQL v0.5 spec examples parse and execute correctly
- Python reference implementation parity (same results)
- Routing validation catches all invalid queries
- TTL expiration accurate to 100ms

### Usability

- Single binary deployment
- MCP server works with Claude Code
- REPL with syntax highlighting
- Clear error messages with query position

---

## 15. References

- **AQL Specification v0.5:** `/Users/sriram.reddy/Dev/AQL/spec/AQL_SPEC_v0.5.md`
- **Python Implementation Guide:** `/Users/sriram.reddy/Dev/AQL/.ai/AQL_python_fullstack_impl.md`
- **Python Reference DB:** `/Users/sriram.reddy/Dev/AQL/aql-ref-db/`
- **Rust Dependencies:**
  - [dashmap](https://crates.io/crates/dashmap)
  - [petgraph](https://crates.io/crates/petgraph)
  - [usearch](https://crates.io/crates/usearch)
  - [datafusion](https://crates.io/crates/datafusion)
  - [pest](https://crates.io/crates/pest)
  - [tokio](https://crates.io/crates/tokio)

---

*Architecture document generated based on AQL Specification v0.5 and Python reference implementation analysis.*
