//! Executor error types

use adb_core::AdbError;
use aql_planner::PlanError;
use thiserror::Error;

/// Result type for executor operations
pub type ExecutorResult<T> = Result<T, ExecutorError>;

/// Errors that can occur during query execution
#[derive(Debug, Error)]
pub enum ExecutorError {
    /// Planning error
    #[error("Planning error: {0}")]
    Planning(#[from] PlanError),

    /// Backend error
    #[error("Backend error: {0}")]
    Backend(#[from] AdbError),

    /// Parse error
    #[error("Parse error: {0}")]
    Parse(String),

    /// Timeout
    #[error("Query timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Missing data
    #[error("Missing required data: {0}")]
    MissingData(String),

    /// Pipeline error at step
    #[error("Pipeline failed at step {step}: {message}")]
    PipelineError { step: usize, message: String },

    /// Unsupported operation
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),
}
