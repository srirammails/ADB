//! MCP Tool definitions
//!
//! Defines the tools exposed via MCP for ADB operations.

use serde::{Deserialize, Serialize};

/// Tool definition for MCP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Input schema (JSON Schema)
    pub input_schema: serde_json::Value,
}

/// Get all available tools
pub fn get_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "aql_query".to_string(),
            description: "Execute an AQL query against the Agent Database".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The AQL query to execute"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "store_working".to_string(),
            description: "Store data in working memory".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The key for the record"
                    },
                    "data": {
                        "type": "object",
                        "description": "The data to store"
                    }
                },
                "required": ["key", "data"]
            }),
        },
        ToolDef {
            name: "recall_working".to_string(),
            description: "Recall data from working memory".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Optional key filter"
                    },
                    "field": {
                        "type": "string",
                        "description": "Optional field to filter on"
                    },
                    "value": {
                        "description": "Optional value to match"
                    }
                }
            }),
        },
        ToolDef {
            name: "recall_episodic".to_string(),
            description: "Recall events from episodic memory with time window".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "window_minutes": {
                        "type": "integer",
                        "description": "Time window in minutes (default 60)"
                    },
                    "event_type": {
                        "type": "string",
                        "description": "Optional event type filter"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of events to return"
                    }
                }
            }),
        },
        ToolDef {
            name: "get_context".to_string(),
            description: "Get combined context from working memory and recent episodic history".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "include_working": {
                        "type": "boolean",
                        "description": "Include working memory (default true)"
                    },
                    "include_episodic": {
                        "type": "boolean",
                        "description": "Include episodic history (default true)"
                    },
                    "window_minutes": {
                        "type": "integer",
                        "description": "Episodic time window in minutes (default 30)"
                    }
                }
            }),
        },
    ]
}
