//! ADB CLI - Command line interface for Agent Database
//!
//! Runs the ADB MCP server for integration with Claude Code and other MCP clients.

use std::sync::Arc;

use adb_backends::Adb;
use adb_mcp::server::McpServer;
use clap::{Parser, Subcommand};
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
    /// Run the MCP server (default)
    Serve,
    /// Execute an AQL query
    Query {
        /// The AQL query to execute
        query: String,
    },
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
        Commands::Query { query } => {
            let adb = Arc::new(Adb::new());
            let executor = adb_executor::Executor::new(adb);
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
