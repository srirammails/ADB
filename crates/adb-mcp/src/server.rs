//! MCP Server implementation
//!
//! Handles JSON-RPC communication for MCP protocol.

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::Duration;

use adb_backends::Adb;
use adb_executor::Executor;
use serde_json::Value;

use crate::protocol::{
    InitializeParams, InitializeResult, JsonRpcRequest, JsonRpcResponse, ServerCapabilities,
    ServerInfo, Tool, ToolCallParams, ToolCallResult, ToolsCapability, ToolsListResult,
};
use crate::tools::get_tools;

/// Convert serde_json::Value to adb_core::Value
fn json_to_core_value(json: &serde_json::Value) -> adb_core::Value {
    match json {
        serde_json::Value::Null => adb_core::Value::Null,
        serde_json::Value::Bool(b) => adb_core::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                adb_core::Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                adb_core::Value::Float(f)
            } else {
                adb_core::Value::Null
            }
        }
        serde_json::Value::String(s) => adb_core::Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            adb_core::Value::Array(arr.iter().map(json_to_core_value).collect())
        }
        serde_json::Value::Object(_) => {
            // Objects are converted to their string representation
            adb_core::Value::String(json.to_string())
        }
    }
}

/// MCP Server for ADB
pub struct McpServer {
    /// The ADB instance
    adb: Arc<Adb>,
    /// The executor
    executor: Executor,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new(adb: Arc<Adb>) -> Self {
        let executor = Executor::new(Arc::clone(&adb));
        Self { adb, executor }
    }

    /// Get a reference to the ADB instance
    pub fn adb(&self) -> &Arc<Adb> {
        &self.adb
    }

    /// Run the MCP server on stdio
    pub async fn run_stdio(&self) -> Result<(), Box<dyn std::error::Error>> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();

        for line in stdin.lock().lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<JsonRpcRequest>(&line) {
                Ok(request) => {
                    let response = self.handle_request(request).await;
                    let response_json = serde_json::to_string(&response)?;
                    writeln!(stdout, "{}", response_json)?;
                    stdout.flush()?;
                }
                Err(e) => {
                    let error_response = JsonRpcResponse::error(
                        None,
                        -32700, // Parse error
                        format!("Parse error: {}", e),
                    );
                    let response_json = serde_json::to_string(&error_response)?;
                    writeln!(stdout, "{}", response_json)?;
                    stdout.flush()?;
                }
            }
        }

        Ok(())
    }

    /// Handle a JSON-RPC request
    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request.id, request.params).await,
            "initialized" => {
                // Notification, no response needed
                JsonRpcResponse::success(request.id, serde_json::json!({}))
            }
            "tools/list" => self.handle_tools_list(request.id).await,
            "tools/call" => self.handle_tools_call(request.id, request.params).await,
            "shutdown" => JsonRpcResponse::success(request.id, serde_json::json!(null)),
            _ => JsonRpcResponse::error(
                request.id,
                -32601, // Method not found
                format!("Method not found: {}", request.method),
            ),
        }
    }

    async fn handle_initialize(
        &self,
        id: Option<Value>,
        params: Option<Value>,
    ) -> JsonRpcResponse {
        // Parse params if present (for validation)
        if let Some(params) = params {
            if let Err(e) = serde_json::from_value::<InitializeParams>(params) {
                tracing::warn!("Invalid initialize params: {}", e);
            }
        }

        let result = InitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(false),
                }),
            },
            server_info: ServerInfo {
                name: "adb-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
    }

    async fn handle_tools_list(&self, id: Option<Value>) -> JsonRpcResponse {
        let tool_defs = get_tools();
        let tools: Vec<Tool> = tool_defs
            .into_iter()
            .map(|t| Tool {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
            })
            .collect();

        let result = ToolsListResult { tools };
        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
    }

    async fn handle_tools_call(
        &self,
        id: Option<Value>,
        params: Option<Value>,
    ) -> JsonRpcResponse {
        let params = match params {
            Some(p) => match serde_json::from_value::<ToolCallParams>(p) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(
                        id,
                        -32602, // Invalid params
                        format!("Invalid params: {}", e),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(
                    id,
                    -32602,
                    "Missing params for tools/call".to_string(),
                );
            }
        };

        let result = match params.name.as_str() {
            "aql_query" => self.handle_aql_query(params.arguments).await,
            "store_working" => self.handle_store_working(params.arguments).await,
            "recall_working" => self.handle_recall_working(params.arguments).await,
            "recall_episodic" => self.handle_recall_episodic(params.arguments).await,
            "get_context" => self.handle_get_context(params.arguments).await,
            _ => ToolCallResult::error(format!("Unknown tool: {}", params.name)),
        };

        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
    }

    async fn handle_aql_query(&self, args: Option<Value>) -> ToolCallResult {
        let query = match args.and_then(|a| a.get("query").and_then(|q| q.as_str()).map(String::from))
        {
            Some(q) => q,
            None => return ToolCallResult::error("Missing required parameter: query".to_string()),
        };

        match self.executor.execute(&query).await {
            Ok(result) => {
                let json = serde_json::to_value(&result).unwrap_or_default();
                ToolCallResult::json(&json)
            }
            Err(e) => ToolCallResult::error(format!("Query execution failed: {}", e)),
        }
    }

    async fn handle_store_working(&self, args: Option<Value>) -> ToolCallResult {
        let args = match args {
            Some(a) => a,
            None => return ToolCallResult::error("Missing arguments".to_string()),
        };

        let key = match args.get("key").and_then(|k| k.as_str()) {
            Some(k) => k,
            None => return ToolCallResult::error("Missing required parameter: key".to_string()),
        };

        let data = match args.get("data") {
            Some(d) => d.clone(),
            None => return ToolCallResult::error("Missing required parameter: data".to_string()),
        };

        match self
            .adb
            .store(adb_core::MemoryType::Working, key, data)
            .await
        {
            Ok(record) => {
                let json = serde_json::to_value(&record).unwrap_or_default();
                ToolCallResult::json(&json)
            }
            Err(e) => ToolCallResult::error(format!("Store failed: {}", e)),
        }
    }

    async fn handle_recall_working(&self, args: Option<Value>) -> ToolCallResult {
        let predicate = if let Some(args) = args {
            if let Some(key) = args.get("key").and_then(|k| k.as_str()) {
                adb_core::Predicate::key("id", key)
            } else if let (Some(field), Some(value)) = (
                args.get("field").and_then(|f| f.as_str()),
                args.get("value"),
            ) {
                // Convert serde_json::Value to adb_core::Value
                let core_value = json_to_core_value(value);
                adb_core::Predicate::where_eq(field, core_value)
            } else {
                adb_core::Predicate::All
            }
        } else {
            adb_core::Predicate::All
        };

        match self
            .adb
            .recall(adb_core::MemoryType::Working, &predicate)
            .await
        {
            Ok(records) => {
                let json = serde_json::to_value(&records).unwrap_or_default();
                ToolCallResult::json(&json)
            }
            Err(e) => ToolCallResult::error(format!("Recall failed: {}", e)),
        }
    }

    async fn handle_recall_episodic(&self, args: Option<Value>) -> ToolCallResult {
        let window_minutes = args
            .as_ref()
            .and_then(|a| a.get("window_minutes"))
            .and_then(|w| w.as_i64())
            .unwrap_or(60) as i32;

        let limit = args
            .as_ref()
            .and_then(|a| a.get("limit"))
            .and_then(|l| l.as_u64())
            .map(|l| l as usize);

        let mut modifiers = adb_core::Modifiers::default();
        modifiers.window = Some(adb_core::Window::last_duration(Duration::from_secs(
            window_minutes as u64 * 60,
        )));
        if let Some(l) = limit {
            modifiers.limit = Some(l);
        }

        let predicate = if let Some(event_type) = args
            .as_ref()
            .and_then(|a| a.get("event_type"))
            .and_then(|e| e.as_str())
        {
            adb_core::Predicate::where_eq("event_type", event_type)
        } else {
            adb_core::Predicate::All
        };

        match self
            .adb
            .recall_with_modifiers(adb_core::MemoryType::Episodic, &predicate, &modifiers)
            .await
        {
            Ok(records) => {
                let json = serde_json::to_value(&records).unwrap_or_default();
                ToolCallResult::json(&json)
            }
            Err(e) => ToolCallResult::error(format!("Recall failed: {}", e)),
        }
    }

    async fn handle_get_context(&self, args: Option<Value>) -> ToolCallResult {
        let include_working = args
            .as_ref()
            .and_then(|a| a.get("include_working"))
            .and_then(|w| w.as_bool())
            .unwrap_or(true);

        let include_episodic = args
            .as_ref()
            .and_then(|a| a.get("include_episodic"))
            .and_then(|e| e.as_bool())
            .unwrap_or(true);

        let window_minutes = args
            .as_ref()
            .and_then(|a| a.get("window_minutes"))
            .and_then(|w| w.as_i64())
            .unwrap_or(30) as i32;

        let mut context = serde_json::Map::new();

        if include_working {
            match self.adb.scan().await {
                Ok(records) => {
                    context.insert(
                        "working".to_string(),
                        serde_json::to_value(&records).unwrap_or_default(),
                    );
                }
                Err(e) => {
                    context.insert(
                        "working_error".to_string(),
                        serde_json::Value::String(e.to_string()),
                    );
                }
            }
        }

        if include_episodic {
            let mut modifiers = adb_core::Modifiers::default();
            modifiers.window = Some(adb_core::Window::last_duration(Duration::from_secs(
                window_minutes as u64 * 60,
            )));
            modifiers.limit = Some(50); // Reasonable default limit

            match self
                .adb
                .recall_with_modifiers(
                    adb_core::MemoryType::Episodic,
                    &adb_core::Predicate::All,
                    &modifiers,
                )
                .await
            {
                Ok(records) => {
                    context.insert(
                        "episodic".to_string(),
                        serde_json::to_value(&records).unwrap_or_default(),
                    );
                }
                Err(e) => {
                    context.insert(
                        "episodic_error".to_string(),
                        serde_json::Value::String(e.to_string()),
                    );
                }
            }
        }

        ToolCallResult::json(&serde_json::Value::Object(context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_server() -> McpServer {
        McpServer::new(Arc::new(Adb::new()))
    }

    #[tokio::test]
    async fn test_handle_initialize() {
        let server = create_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "test",
                    "version": "1.0"
                }
            })),
        };

        let response = server.handle_request(request).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_handle_tools_list() {
        let server = create_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = server.handle_request(request).await;
        assert!(response.result.is_some());

        let result = response.result.unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        assert!(!tools.is_empty());

        // Check aql_query tool exists
        let has_aql_query = tools.iter().any(|t| t.get("name").unwrap() == "aql_query");
        assert!(has_aql_query);
    }

    #[tokio::test]
    async fn test_aql_query_tool() {
        let server = create_server();

        // First store some data
        server
            .adb
            .store(
                adb_core::MemoryType::Working,
                "test-key",
                json!({"value": "hello"}),
            )
            .await
            .unwrap();

        // Now query via MCP
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "aql_query",
                "arguments": {
                    "query": "SCAN FROM WORKING"
                }
            })),
        };

        let response = server.handle_request(request).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        assert!(!content.is_empty());
    }

    #[tokio::test]
    async fn test_store_working_tool() {
        let server = create_server();

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "store_working",
                "arguments": {
                    "key": "task-1",
                    "data": {
                        "status": "pending",
                        "description": "Test task"
                    }
                }
            })),
        };

        let response = server.handle_request(request).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        // Verify it was stored
        assert_eq!(server.adb.count(adb_core::MemoryType::Working).await, 1);
    }

    #[tokio::test]
    async fn test_recall_working_tool() {
        let server = create_server();

        // Store some data first
        server
            .adb
            .store(
                adb_core::MemoryType::Working,
                "task-1",
                json!({"status": "active"}),
            )
            .await
            .unwrap();

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "recall_working",
                "arguments": {
                    "key": "task-1"
                }
            })),
        };

        let response = server.handle_request(request).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_get_context_tool() {
        let server = create_server();

        // Store some working memory data
        server
            .adb
            .store(
                adb_core::MemoryType::Working,
                "current-task",
                json!({"description": "Processing request"}),
            )
            .await
            .unwrap();

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "get_context",
                "arguments": {
                    "include_working": true,
                    "include_episodic": true
                }
            })),
        };

        let response = server.handle_request(request).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_method_not_found() {
        let server = create_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "unknown/method".to_string(),
            params: None,
        };

        let response = server.handle_request(request).await;
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32601);
    }
}
