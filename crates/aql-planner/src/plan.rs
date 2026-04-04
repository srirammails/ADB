//! Execution plan types
//!
//! These types represent the planned execution of AQL queries.

use std::time::Duration;

use adb_core::{MemoryType, Modifiers, Predicate, Scope};

/// A complete execution plan
#[derive(Debug, Clone)]
pub enum ExecutionPlan {
    /// Single operation
    Single(StepPlan),
    /// Pipeline of operations
    Pipeline(PipelinePlan),
    /// Reflect (multi-source query with optional link traversal)
    Reflect(ReflectPlan),
    /// Fan-out operation across ALL memory types
    FanOut(FanOutPlan),
}

/// A pipeline execution plan
#[derive(Debug, Clone)]
pub struct PipelinePlan {
    /// Pipeline name
    pub name: String,
    /// Optional timeout for entire pipeline
    pub timeout: Option<Duration>,
    /// Ordered stages to execute (can be steps or reflects)
    pub stages: Vec<PipelineStage>,
}

/// A stage in a pipeline - can be a single step or a reflect
#[derive(Debug, Clone)]
pub enum PipelineStage {
    /// Single operation step
    Step(StepPlan),
    /// Reflect (multi-source query)
    Reflect(ReflectPlan),
}

/// A single step in an execution plan
#[derive(Debug, Clone)]
pub struct StepPlan {
    /// The operation to perform
    pub operation: Operation,
    /// Memory type to operate on
    pub memory_type: MemoryType,
    /// Predicate for filtering
    pub predicate: Predicate,
    /// Query modifiers
    pub modifiers: Modifiers,
    /// Additional operation-specific data
    pub data: Option<serde_json::Value>,
}

/// Operations that can be executed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Scan all records (working memory)
    Scan,
    /// Recall records (any memory type)
    Recall,
    /// Lookup records (any memory type)
    Lookup,
    /// Load ranked tools (tools memory)
    Load,
    /// Store a record
    Store,
    /// Update records
    Update,
    /// Forget (delete) records
    Forget,
    /// Create a link between records
    Link,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scan => write!(f, "SCAN"),
            Self::Recall => write!(f, "RECALL"),
            Self::Lookup => write!(f, "LOOKUP"),
            Self::Load => write!(f, "LOAD"),
            Self::Store => write!(f, "STORE"),
            Self::Update => write!(f, "UPDATE"),
            Self::Forget => write!(f, "FORGET"),
            Self::Link => write!(f, "LINK"),
        }
    }
}

/// Reflect execution plan (multi-source with link traversal)
#[derive(Debug, Clone)]
pub struct ReflectPlan {
    /// Sources to query
    pub sources: Vec<ReflectSourcePlan>,
    /// Link traversal options
    pub link_options: Option<LinkOptions>,
    /// Optional follow-up operation
    pub then_step: Option<Box<StepPlan>>,
}

/// Fan-out execution plan for ALL memory types
#[derive(Debug, Clone)]
pub struct FanOutPlan {
    /// The operation to perform on all memory types
    pub operation: Operation,
    /// Predicate for filtering
    pub predicate: Predicate,
    /// Query modifiers
    pub modifiers: Modifiers,
    /// Additional operation-specific data (for UPDATE)
    pub data: Option<serde_json::Value>,
}

/// A source in a reflect plan
#[derive(Debug, Clone)]
pub struct ReflectSourcePlan {
    /// Memory type
    pub memory_type: MemoryType,
    /// Predicate
    pub predicate: Predicate,
    /// Modifiers
    pub modifiers: Modifiers,
}

/// Link traversal options for reflect
#[derive(Debug, Clone)]
pub struct LinkOptions {
    /// Include links with results
    pub with_links: bool,
    /// Link type filter
    pub link_type: Option<String>,
    /// Follow links to related records
    pub follow: bool,
    /// Max depth when following
    pub depth: u32,
}

/// Store operation data
#[derive(Debug, Clone)]
pub struct StoreData {
    /// Key for the record
    pub key: String,
    /// Data payload
    pub payload: serde_json::Value,
    /// Scope
    pub scope: Scope,
    /// Namespace
    pub namespace: Option<String>,
    /// TTL
    pub ttl: Option<Duration>,
}

/// Link operation data
#[derive(Debug, Clone)]
pub struct LinkData {
    /// Source memory type
    pub from_type: MemoryType,
    /// Source ID or predicate
    pub from_predicate: Predicate,
    /// Target memory type
    pub to_type: MemoryType,
    /// Target ID or predicate
    pub to_predicate: Predicate,
    /// Link type
    pub link_type: String,
    /// Link weight
    pub weight: f32,
}

impl StepPlan {
    /// Create a new scan plan
    pub fn scan(modifiers: Modifiers) -> Self {
        Self {
            operation: Operation::Scan,
            memory_type: MemoryType::Working,
            predicate: Predicate::All,
            modifiers,
            data: None,
        }
    }

    /// Create a new recall plan
    pub fn recall(memory_type: MemoryType, predicate: Predicate, modifiers: Modifiers) -> Self {
        Self {
            operation: Operation::Recall,
            memory_type,
            predicate,
            modifiers,
            data: None,
        }
    }

    /// Create a new lookup plan
    pub fn lookup(memory_type: MemoryType, predicate: Predicate, modifiers: Modifiers) -> Self {
        Self {
            operation: Operation::Lookup,
            memory_type,
            predicate,
            modifiers,
            data: None,
        }
    }

    /// Create a new load plan (tools only)
    pub fn load(predicate: Predicate, modifiers: Modifiers) -> Self {
        Self {
            operation: Operation::Load,
            memory_type: MemoryType::Tools,
            predicate,
            modifiers,
            data: None,
        }
    }

    /// Create a new store plan
    pub fn store(memory_type: MemoryType, data: StoreData, modifiers: Modifiers) -> Self {
        Self {
            operation: Operation::Store,
            memory_type,
            predicate: Predicate::All,
            modifiers,
            data: Some(serde_json::to_value(&data).unwrap_or_default()),
        }
    }

    /// Create a new update plan
    pub fn update(
        memory_type: MemoryType,
        predicate: Predicate,
        payload: serde_json::Value,
        modifiers: Modifiers,
    ) -> Self {
        Self {
            operation: Operation::Update,
            memory_type,
            predicate,
            modifiers,
            data: Some(payload),
        }
    }

    /// Create a new forget plan
    pub fn forget(memory_type: MemoryType, predicate: Predicate, modifiers: Modifiers) -> Self {
        Self {
            operation: Operation::Forget,
            memory_type,
            predicate,
            modifiers,
            data: None,
        }
    }

    /// Create a new link plan
    pub fn link(link_data: LinkData) -> Self {
        Self {
            operation: Operation::Link,
            memory_type: link_data.from_type,
            predicate: link_data.from_predicate.clone(),
            modifiers: Modifiers::default(),
            data: Some(serde_json::to_value(&link_data).unwrap_or_default()),
        }
    }
}

// Serialization support for StoreData and LinkData
impl serde::Serialize for StoreData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("StoreData", 5)?;
        state.serialize_field("key", &self.key)?;
        state.serialize_field("payload", &self.payload)?;
        state.serialize_field("scope", &format!("{:?}", self.scope))?;
        state.serialize_field("namespace", &self.namespace)?;
        state.serialize_field("ttl_ms", &self.ttl.map(|d| d.as_millis() as u64))?;
        state.end()
    }
}

impl serde::Serialize for LinkData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("LinkData", 6)?;
        state.serialize_field("from_type", &format!("{:?}", self.from_type))?;
        // Serialize from_predicate conditions as structured data
        state.serialize_field("from_conditions", &predicate_to_conditions(&self.from_predicate))?;
        state.serialize_field("to_type", &format!("{:?}", self.to_type))?;
        // Serialize to_predicate conditions as structured data
        state.serialize_field("to_conditions", &predicate_to_conditions(&self.to_predicate))?;
        state.serialize_field("link_type", &self.link_type)?;
        state.serialize_field("weight", &self.weight)?;
        state.end()
    }
}

/// Convert a predicate to a serializable conditions array
fn predicate_to_conditions(pred: &Predicate) -> Vec<serde_json::Value> {
    match pred {
        Predicate::Where { conditions } => {
            conditions.iter().map(|c| {
                serde_json::json!({
                    "field": c.field,
                    "operator": format!("{:?}", c.operator),
                    "value": condition_value_to_json(&c.value)
                })
            }).collect()
        }
        Predicate::Key { field, value } => {
            vec![serde_json::json!({
                "field": field,
                "operator": "Eq",
                "value": condition_value_to_json(value)
            })]
        }
        _ => vec![]
    }
}

/// Convert a condition value to JSON
fn condition_value_to_json(value: &adb_core::Value) -> serde_json::Value {
    match value {
        adb_core::Value::Null => serde_json::Value::Null,
        adb_core::Value::Bool(b) => serde_json::Value::Bool(*b),
        adb_core::Value::Int(i) => serde_json::json!(i),
        adb_core::Value::Float(f) => serde_json::json!(f),
        adb_core::Value::String(s) => serde_json::Value::String(s.clone()),
        adb_core::Value::Variable(v) => serde_json::Value::String(format!("${}", v)),
        adb_core::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(condition_value_to_json).collect())
        }
    }
}
