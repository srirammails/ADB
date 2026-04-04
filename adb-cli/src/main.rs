//! ADB CLI - Command line interface for Agent Database
//!
//! Runs the ADB MCP server for integration with Claude Code and other MCP clients.

use std::sync::Arc;

use adb_backends::Adb;
use adb_executor::Executor;
use adb_mcp::server::McpServer;
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "adb")]
#[command(about = "Agent Database - Memory system for AI agents", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the MCP server on stdio (default)
    Serve,
    /// Run the HTTP server
    ServeHttp {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },
    /// Execute an AQL query
    Query {
        /// The AQL query to execute
        query: String,
    },
}

#[derive(Clone)]
struct AppState {
    executor: Arc<Executor>,
}

#[derive(Deserialize)]
struct QueryRequest {
    query: String,
}

#[derive(Serialize)]
struct QueryResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize tracing - for MCP serve mode, write to stderr without colors
    let is_serve = matches!(cli.command, None | Some(Commands::Serve));

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    if is_serve { "warn".into() } else { "adb=info".into() }
                }),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(!is_serve)  // Disable ANSI colors for MCP
                .with_writer(std::io::stderr)  // Write to stderr, not stdout
        )
        .init();

    match cli.command.unwrap_or(Commands::Serve) {
        Commands::Serve => {
            eprintln!("Starting ADB MCP server on stdio");
            let adb = Arc::new(Adb::new());
            let server = McpServer::new(adb);
            server.run_stdio().await?;
        }
        Commands::ServeHttp { port } => {
            eprintln!("Starting ADB HTTP server on port {}", port);
            let adb = Arc::new(Adb::new());
            let executor = Arc::new(Executor::new(adb));
            let state = AppState { executor };

            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any);

            let app = Router::new()
                .route("/health", get(health_check))
                .route("/query", post(execute_query))
                .layer(cors)
                .with_state(state);

            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
            eprintln!("ADB HTTP server listening on http://0.0.0.0:{}", port);
            axum::serve(listener, app).await?;
        }
        Commands::Query { query } => {
            let adb = Arc::new(Adb::new());
            let executor = Executor::new(adb);
            match executor.execute(&query).await {
                Ok(result) => {
                    let json = serde_json::to_string_pretty(&result)?;
                    println!("{}", json);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}

async fn execute_query(
    State(state): State<AppState>,
    Json(payload): Json<QueryRequest>,
) -> (StatusCode, Json<QueryResponse>) {
    match state.executor.execute(&payload.query).await {
        Ok(result) => {
            let json_result = serde_json::to_value(&result).unwrap_or_default();
            (
                StatusCode::OK,
                Json(QueryResponse {
                    success: true,
                    result: Some(json_result),
                    error: None,
                }),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(QueryResponse {
                success: false,
                result: None,
                error: Some(e.to_string()),
            }),
        ),
    }
}
