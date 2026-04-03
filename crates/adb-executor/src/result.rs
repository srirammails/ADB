//! Query result types

use adb_core::{Link, MemoryRecord};
use serde::{Deserialize, Serialize};

/// Result of a query execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Whether the query succeeded
    pub success: bool,
    /// Result data
    pub data: ResultSet,
    /// Execution metadata
    pub metadata: ResultMetadata,
}

/// The actual result data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResultSet {
    /// Records returned from a query
    Records { records: Vec<MemoryRecord> },
    /// Count of affected records (for mutations)
    Count { count: u64 },
    /// Aggregation results
    Aggregation { values: serde_json::Value },
    /// Links returned from a query
    Links { links: Vec<Link> },
    /// Single record stored
    Stored { record: MemoryRecord },
    /// Pipeline results (multiple result sets)
    Pipeline { steps: Vec<QueryResult> },
    /// Reflect results (multiple sources)
    Reflect {
        sources: Vec<SourceResult>,
        links: Vec<Link>,
    },
    /// Empty result
    Empty,
}

/// Result from a single source in a reflect query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceResult {
    /// Memory type
    pub memory_type: String,
    /// Records
    pub records: Vec<MemoryRecord>,
}

/// Metadata about query execution
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResultMetadata {
    /// Execution time in milliseconds
    pub execution_time_ms: u64,
    /// Number of records scanned
    pub records_scanned: Option<u64>,
    /// Number of records returned
    pub records_returned: Option<u64>,
    /// Pipeline step timings
    pub step_timings: Option<Vec<u64>>,
}

impl QueryResult {
    /// Create a successful records result
    pub fn records(records: Vec<MemoryRecord>, execution_time_ms: u64) -> Self {
        let count = records.len();
        Self {
            success: true,
            data: ResultSet::Records { records },
            metadata: ResultMetadata {
                execution_time_ms,
                records_returned: Some(count as u64),
                ..Default::default()
            },
        }
    }

    /// Create a successful count result
    pub fn count(count: u64, execution_time_ms: u64) -> Self {
        Self {
            success: true,
            data: ResultSet::Count { count },
            metadata: ResultMetadata {
                execution_time_ms,
                ..Default::default()
            },
        }
    }

    /// Create a successful stored result
    pub fn stored(record: MemoryRecord, execution_time_ms: u64) -> Self {
        Self {
            success: true,
            data: ResultSet::Stored { record },
            metadata: ResultMetadata {
                execution_time_ms,
                records_returned: Some(1),
                ..Default::default()
            },
        }
    }

    /// Create a successful aggregation result
    pub fn aggregation(values: serde_json::Value, execution_time_ms: u64) -> Self {
        Self {
            success: true,
            data: ResultSet::Aggregation { values },
            metadata: ResultMetadata {
                execution_time_ms,
                ..Default::default()
            },
        }
    }

    /// Create a pipeline result
    pub fn pipeline(steps: Vec<QueryResult>, execution_time_ms: u64) -> Self {
        let step_timings: Vec<u64> = steps.iter().map(|s| s.metadata.execution_time_ms).collect();
        Self {
            success: steps.iter().all(|s| s.success),
            data: ResultSet::Pipeline { steps },
            metadata: ResultMetadata {
                execution_time_ms,
                step_timings: Some(step_timings),
                ..Default::default()
            },
        }
    }

    /// Create an empty result
    pub fn empty(execution_time_ms: u64) -> Self {
        Self {
            success: true,
            data: ResultSet::Empty,
            metadata: ResultMetadata {
                execution_time_ms,
                ..Default::default()
            },
        }
    }

    /// Get records from the result if present
    pub fn get_records(&self) -> Option<&[MemoryRecord]> {
        match &self.data {
            ResultSet::Records { records } => Some(records),
            _ => None,
        }
    }

    /// Get count from the result if present
    pub fn get_count(&self) -> Option<u64> {
        match &self.data {
            ResultSet::Count { count } => Some(*count),
            _ => None,
        }
    }
}
