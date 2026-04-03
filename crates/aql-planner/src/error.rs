//! Planner error types

use thiserror::Error;

/// Result type for planner operations
pub type PlanResult<T> = Result<T, PlanError>;

/// Errors that can occur during query planning
#[derive(Debug, Error)]
pub enum PlanError {
    /// Unsupported operation for memory type
    #[error("Unsupported operation '{op}' for memory type '{memory_type}'")]
    UnsupportedOperation { op: String, memory_type: String },

    /// Invalid memory type
    #[error("Invalid memory type: {0}")]
    InvalidMemoryType(String),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid predicate for operation
    #[error("Invalid predicate '{predicate}' for operation '{op}'")]
    InvalidPredicate { predicate: String, op: String },

    /// Pipeline contains no stages
    #[error("Pipeline must contain at least one stage")]
    EmptyPipeline,

    /// Variable not bound
    #[error("Variable '${0}' is not bound")]
    UnboundVariable(String),

    /// Type mismatch
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },
}
