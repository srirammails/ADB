//! ADB Core - Core types and traits for Agent Database
//!
//! This crate provides the foundational types used throughout ADB:
//! - Memory types and records
//! - Backend trait definitions
//! - Link/ontology types
//! - Error types
//! - Scope and namespace handling

pub mod error;
pub mod link;
pub mod memory;
pub mod predicate;
pub mod scope;
pub mod time;

// Re-exports for convenience
pub use error::{AdbError, AdbResult};
pub use link::{Link, LinkPredicate};
pub use memory::{MemoryRecord, MemoryType, Metadata};
pub use predicate::{evaluate_conditions, evaluate_conditions_on_record, AggregateFunc, AggregateFuncType, Condition, FollowLinks, LogicalOp, Modifiers, Operator, OrderBy, Predicate, Value, Window, WithLinks};
pub use scope::{Namespace, Scope};
pub use time::Ttl;
