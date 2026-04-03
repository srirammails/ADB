//! ADB MCP Server - Model Context Protocol interface for Agent Database
//!
//! Exposes ADB operations as MCP tools that can be called by Claude Code or other MCP clients.
//!
//! ## Tools Provided
//!
//! - `store_working` - Store data in working memory
//! - `recall_working` - Recall from working memory
//! - `store_episodic` - Store event in episodic memory
//! - `recall_episodic` - Recall events from episodic memory
//! - `scan_procedural` - Find procedures by pattern matching
//! - `get_context` - Get combined context (working + episodic history + related procedures)

pub mod server;
pub mod tools;
pub mod protocol;

pub use server::McpServer;
