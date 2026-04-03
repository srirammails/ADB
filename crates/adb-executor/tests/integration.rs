//! Integration tests for the full AQL pipeline
//!
//! Tests the complete flow: AQL query string -> parser -> planner -> executor -> results

use std::sync::Arc;

use adb_backends::Adb;
use adb_core::MemoryType;
use adb_executor::Executor;
use serde_json::json;

/// Test helper to create executor with pre-populated data
async fn create_test_executor() -> Executor {
    let adb = Arc::new(Adb::new());

    // Populate working memory
    adb.store(MemoryType::Working, "task-1", json!({"status": "active", "priority": 5}))
        .await
        .unwrap();
    adb.store(MemoryType::Working, "task-2", json!({"status": "pending", "priority": 3}))
        .await
        .unwrap();
    adb.store(MemoryType::Working, "task-3", json!({"status": "completed", "priority": 8}))
        .await
        .unwrap();

    // Populate tools memory
    adb.store(
        MemoryType::Tools,
        "read-file",
        json!({
            "name": "Read File",
            "description": "Reads file contents from disk",
            "category": "file",
            "ranking": 0.9
        }),
    )
    .await
    .unwrap();
    adb.store(
        MemoryType::Tools,
        "write-file",
        json!({
            "name": "Write File",
            "description": "Writes content to a file",
            "category": "file",
            "ranking": 0.85
        }),
    )
    .await
    .unwrap();

    // Populate procedural memory
    adb.store(
        MemoryType::Procedural,
        "oom-fix",
        json!({
            "pattern": "OOMKilled container memory exceeded",
            "steps": ["Check memory limits", "Increase memory allocation", "Restart pod"],
            "severity": "critical"
        }),
    )
    .await
    .unwrap();

    Executor::new(adb)
}

// ============================================================================
// SCAN tests
// ============================================================================

#[tokio::test]
async fn test_scan_all() {
    let executor = create_test_executor().await;

    let result = executor.execute("SCAN FROM WORKING").await.unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    assert_eq!(records.len(), 3);
}

#[tokio::test]
async fn test_scan_with_limit() {
    let executor = create_test_executor().await;

    let result = executor.execute("SCAN FROM WORKING LIMIT 2").await.unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    assert_eq!(records.len(), 2);
}

// ============================================================================
// RECALL tests
// ============================================================================

#[tokio::test]
async fn test_recall_with_filter() {
    let executor = create_test_executor().await;

    let result = executor
        .execute(r#"RECALL FROM WORKING WHERE status = "active""#)
        .await
        .unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "task-1");
}

#[tokio::test]
async fn test_recall_with_comparison() {
    let executor = create_test_executor().await;

    let result = executor
        .execute("RECALL FROM WORKING WHERE priority > 4")
        .await
        .unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    assert_eq!(records.len(), 2); // task-1 (5) and task-3 (8)
}

#[tokio::test]
async fn test_recall_all() {
    let executor = create_test_executor().await;

    // Note: RECALL requires WHERE clause in AQL grammar, so use a condition that matches all
    let result = executor
        .execute("RECALL FROM WORKING WHERE priority >= 0")
        .await
        .unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    assert_eq!(records.len(), 3);
}

// ============================================================================
// LOOKUP tests
// ============================================================================

#[tokio::test]
async fn test_lookup_by_key() {
    let executor = create_test_executor().await;

    let result = executor
        .execute(r#"LOOKUP FROM WORKING KEY id = "task-2""#)
        .await
        .unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "task-2");
    assert_eq!(records[0].get_str("status"), Some("pending"));
}

// ============================================================================
// STORE tests
// ============================================================================

#[tokio::test]
async fn test_store_new_record() {
    let executor = create_test_executor().await;

    let result = executor
        .execute(r#"STORE INTO WORKING (key = "new-task", status = "created", priority = 1)"#)
        .await
        .unwrap();

    assert!(result.success);

    // Verify it was stored
    let scan_result = executor.execute("SCAN FROM WORKING").await.unwrap();
    let records = scan_result.get_records().unwrap();
    assert_eq!(records.len(), 4); // 3 original + 1 new
}

// ============================================================================
// FORGET tests
// ============================================================================

#[tokio::test]
async fn test_forget_with_filter() {
    let executor = create_test_executor().await;

    let result = executor
        .execute(r#"FORGET FROM WORKING WHERE status = "completed""#)
        .await
        .unwrap();

    assert!(result.success);
    let count = result.get_count().unwrap();
    assert_eq!(count, 1);

    // Verify it was deleted
    let scan_result = executor.execute("SCAN FROM WORKING").await.unwrap();
    let records = scan_result.get_records().unwrap();
    assert_eq!(records.len(), 2);
}

// ============================================================================
// LOAD (tools) tests
// ============================================================================

#[tokio::test]
async fn test_load_tools() {
    let executor = create_test_executor().await;

    // LOAD requires WHERE clause - use condition that matches all
    let result = executor
        .execute(r#"LOAD FROM TOOLS WHERE category = "file" LIMIT 2"#)
        .await
        .unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    assert_eq!(records.len(), 2);
    // Should be sorted by ranking (highest first)
    assert_eq!(records[0].id, "read-file");
}

// ============================================================================
// PIPELINE tests
// ============================================================================

#[tokio::test]
async fn test_pipeline_execution() {
    let executor = create_test_executor().await;

    // Pipeline with proper syntax - LOAD requires WHERE clause
    let result = executor
        .execute(r#"PIPELINE test_pipe SCAN FROM WORKING LIMIT 1 | LOAD FROM TOOLS WHERE category = "file" LIMIT 1"#)
        .await
        .unwrap();

    assert!(result.success);

    // Check that we got pipeline results
    if let adb_executor::ResultSet::Pipeline { steps } = result.data {
        assert_eq!(steps.len(), 2);
        assert!(steps[0].success);
        assert!(steps[1].success);
    } else {
        panic!("Expected pipeline result set");
    }
}

// ============================================================================
// Cross-memory type tests
// ============================================================================

#[tokio::test]
async fn test_procedural_recall() {
    let executor = create_test_executor().await;

    let result = executor
        .execute(r#"RECALL FROM PROCEDURAL WHERE severity = "critical""#)
        .await
        .unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "oom-fix");
}

// ============================================================================
// Error handling tests
// ============================================================================

#[tokio::test]
async fn test_parse_error() {
    let executor = create_test_executor().await;

    let result = executor.execute("INVALID QUERY SYNTAX").await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_empty_result() {
    let executor = create_test_executor().await;

    let result = executor
        .execute(r#"RECALL FROM WORKING WHERE status = "nonexistent""#)
        .await
        .unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    assert_eq!(records.len(), 0);
}

// ============================================================================
// MCP Server integration tests
// ============================================================================

#[tokio::test]
async fn test_mcp_aql_query() {
    use adb_mcp::server::McpServer;
    use adb_mcp::protocol::JsonRpcRequest;

    let adb = Arc::new(Adb::new());
    adb.store(MemoryType::Working, "test-key", json!({"value": 42}))
        .await
        .unwrap();

    let server = McpServer::new(adb);

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
}

#[tokio::test]
async fn test_mcp_full_workflow() {
    use adb_mcp::server::McpServer;
    use adb_mcp::protocol::JsonRpcRequest;

    let adb = Arc::new(Adb::new());
    let server = McpServer::new(adb);

    // 1. Initialize
    let init_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "initialize".to_string(),
        params: Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1.0"}
        })),
    };
    let response = server.handle_request(init_request).await;
    assert!(response.result.is_some());

    // 2. List tools
    let list_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(2)),
        method: "tools/list".to_string(),
        params: None,
    };
    let response = server.handle_request(list_request).await;
    let result = response.result.unwrap();
    let tools = result.get("tools").unwrap().as_array().unwrap();
    assert!(tools.len() >= 5); // At least our 5 core tools

    // 3. Store data via tool
    let store_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(3)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "store_working",
            "arguments": {
                "key": "workflow-task",
                "data": {"status": "running", "step": 1}
            }
        })),
    };
    let response = server.handle_request(store_request).await;
    assert!(response.result.is_some());

    // 4. Query data via AQL
    let query_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(4)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "aql_query",
            "arguments": {
                "query": r#"RECALL FROM WORKING WHERE status = "running""#
            }
        })),
    };
    let response = server.handle_request(query_request).await;
    assert!(response.result.is_some());
    assert!(response.error.is_none());

    // 5. Get context
    let context_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(5)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "get_context",
            "arguments": {
                "include_working": true,
                "include_episodic": false
            }
        })),
    };
    let response = server.handle_request(context_request).await;
    assert!(response.result.is_some());
}

// ============================================================================
// FOLLOW LINKS tests
// ============================================================================

#[tokio::test]
async fn test_follow_links() {
    let adb = Arc::new(Adb::new());

    // Store source record in PROCEDURAL
    adb.store(
        MemoryType::Procedural,
        "oom-fix",
        json!({
            "pattern": "OOMKilled",
            "steps": ["Check memory", "Increase limit"]
        }),
    ).await.unwrap();

    // Store target record in EPISODIC
    adb.store(
        MemoryType::Episodic,
        "inc-001",
        json!({
            "incident_type": "OOM",
            "pod": "api-server"
        }),
    ).await.unwrap();

    // Create link from procedural to episodic
    adb.link(
        MemoryType::Procedural,
        "oom-fix",
        MemoryType::Episodic,
        "inc-001",
        "applied_to",
        0.95,
    ).await.unwrap();

    // Execute FOLLOW LINKS query
    let executor = Executor::new(Arc::clone(&adb));
    let result = executor.execute(
        r#"RECALL FROM PROCEDURAL WHERE pattern = "OOMKilled" FOLLOW LINKS TYPE "applied_to""#
    ).await.unwrap();

    assert!(result.success);
    let records = result.get_records().unwrap();
    // FOLLOW LINKS should return the target records (inc-001), not the source (oom-fix)
    assert_eq!(records.len(), 1, "Expected 1 target record from FOLLOW LINKS");
    assert_eq!(records[0].id, "inc-001");
    assert_eq!(records[0].data["pod"], "api-server");
}

// ============================================================================
// HAVING with alias tests
// ============================================================================

#[tokio::test]
async fn test_having_with_alias() {
    let adb = Arc::new(Adb::new());

    // Store some records
    adb.store(
        MemoryType::Episodic,
        "e1",
        json!({"strategy": "tech_news", "ctr": 0.05}),
    ).await.unwrap();
    adb.store(
        MemoryType::Episodic,
        "e2",
        json!({"strategy": "tech_news", "ctr": 0.03}),
    ).await.unwrap();
    adb.store(
        MemoryType::Episodic,
        "e3",
        json!({"strategy": "sports", "ctr": 0.08}),
    ).await.unwrap();

    let executor = Executor::new(Arc::clone(&adb));

    // Test HAVING with alias - should return the count
    let result = executor.execute(
        r#"RECALL FROM EPISODIC WHERE strategy = "tech_news" AGGREGATE COUNT(*) AS total HAVING total > 0"#
    ).await.unwrap();

    assert!(result.success, "Query should succeed");
    // Should have the aggregate result with the aliased key
    if let adb_executor::ResultSet::Aggregation { values } = &result.data {
        assert!(values.is_object(), "Result should be an object");
        let obj = values.as_object().unwrap();
        assert!(obj.contains_key("total"), "Result should have 'total' key");
        assert_eq!(obj["total"], 2, "Count should be 2");
    } else {
        panic!("Expected Aggregation result");
    }
}
