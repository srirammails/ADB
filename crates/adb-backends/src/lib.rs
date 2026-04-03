//! ADB Backends - Memory type implementations
//!
//! This crate provides backend implementations for all five ADB memory types:
//! - Working: DashMap-based concurrent key-value store
//! - Tools: HashMap-based tool registry with ranking
//! - Procedural: petgraph-based directed graph for procedures
//! - Semantic: usearch-based vector similarity search
//! - Episodic: DataFusion-based time-series storage
//!
//! Also provides the main `Adb` struct that coordinates all backends.

pub mod adb;
pub mod backend;
pub mod config;
pub mod episodic;
pub mod links;
pub mod procedural;
pub mod semantic;
pub mod tools;
pub mod working;

// Re-exports
pub use adb::Adb;
pub use backend::{Backend, BackendInfo};
pub use config::AdbConfig;
pub use episodic::EpisodicBackend;
pub use links::{LinkStore, LinkStoreOps};
pub use procedural::ProceduralBackend;
pub use semantic::SemanticBackend;
pub use tools::ToolsBackend;
pub use working::WorkingBackend;
