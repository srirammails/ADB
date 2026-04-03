//! Error types for ADB operations

use thiserror::Error;

/// Result type alias for ADB operations
pub type AdbResult<T> = Result<T, AdbError>;

/// Errors that can occur during ADB operations
#[derive(Error, Debug)]
pub enum AdbError {
    // Parse errors
    #[error("Parse error: {message} at position {position}")]
    ParseError { message: String, position: usize },

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    // Validation errors
    #[error("Invalid memory type '{memory_type}' for verb '{verb}'. Allowed: {allowed:?}")]
    InvalidMemoryType {
        verb: String,
        memory_type: String,
        allowed: Vec<String>,
    },

    #[error("Missing predicate: {verb} requires a WHERE clause")]
    MissingPredicate { verb: String },

    #[error("Missing payload: {verb} requires data")]
    MissingPayload { verb: String },

    #[error("Invalid predicate for {memory_type}: {message}")]
    InvalidPredicate {
        memory_type: String,
        message: String,
    },

    // Execution errors
    #[error("Operation timed out after {budget_ms}ms")]
    Timeout { budget_ms: u64 },

    #[error("Unknown backend: {0}")]
    UnknownBackend(String),

    #[error("Operation '{0}' not supported on this backend")]
    UnsupportedOperation(String),

    #[error("Record not found: {memory_type}/{id}")]
    NotFound { memory_type: String, id: String },

    #[error("Cyclic dependency detected in task graph")]
    CyclicDependency,

    // Backend-specific errors
    #[error("Embedding error: {0}")]
    EmbeddingError(String),

    #[error("Vector index error: {0}")]
    VectorIndexError(String),

    #[error("Graph error: {0}")]
    GraphError(String),

    #[error("Query execution error: {0}")]
    QueryError(String),

    // Serialization errors
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    // Link errors
    #[error("Link error: {0}")]
    LinkError(String),

    #[error("Invalid link: source or target not found")]
    InvalidLink,

    // Configuration errors
    #[error("Configuration error: {0}")]
    ConfigError(String),

    // Generic error wrapper
    #[error("Internal error: {0}")]
    Internal(String),
}

impl AdbError {
    /// Create a parse error with position information
    pub fn parse(message: impl Into<String>, position: usize) -> Self {
        Self::ParseError {
            message: message.into(),
            position,
        }
    }

    /// Create an invalid memory type error
    pub fn invalid_memory_type(
        verb: impl Into<String>,
        memory_type: impl Into<String>,
        allowed: Vec<&str>,
    ) -> Self {
        Self::InvalidMemoryType {
            verb: verb.into(),
            memory_type: memory_type.into(),
            allowed: allowed.into_iter().map(String::from).collect(),
        }
    }

    /// Create a not found error
    pub fn not_found(memory_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self::NotFound {
            memory_type: memory_type.into(),
            id: id.into(),
        }
    }

    /// Check if this is a timeout error
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    /// Check if this is a not found error
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AdbError::Timeout { budget_ms: 100 };
        assert_eq!(err.to_string(), "Operation timed out after 100ms");

        let err = AdbError::not_found("EPISODIC", "inc-001");
        assert_eq!(err.to_string(), "Record not found: EPISODIC/inc-001");
    }

    #[test]
    fn test_error_checks() {
        let timeout = AdbError::Timeout { budget_ms: 50 };
        assert!(timeout.is_timeout());
        assert!(!timeout.is_not_found());

        let not_found = AdbError::not_found("WORKING", "key-1");
        assert!(!not_found.is_timeout());
        assert!(not_found.is_not_found());
    }
}
