# ADB - Agent Database

A memory system for AI agents with the AQL (Agent Query Language) query interface.

## Memory Types

- **EPISODIC** - Event-based memories with timestamps and context
- **SEMANTIC** - Factual knowledge with embeddings for similarity search
- **PROCEDURAL** - Step-by-step procedures and patterns
- **WORKING** - Short-term scratchpad for active tasks
- **TOOLS** - Tool definitions and metadata

## Quick Start

```bash
# Build
cargo build --release

# Run MCP server (for Claude Desktop)
./target/release/adb serve

# Run HTTP server
./target/release/adb serve --http --port 6444
```

## AQL Examples

```sql
-- Store a memory
STORE INTO EPISODIC { "event": "user_login", "user": "alice" }

-- Recall with filters
RECALL FROM SEMANTIC WHERE topic = "rust" LIMIT 10

-- Query all memory types
RECALL FROM ALL WHERE importance > 0.5

-- Aggregate
RECALL FROM EPISODIC WHERE type = "metric" AGGREGATE SUM(value)

-- Link traversal
REFLECT LINKS FROM "memory_123" FOLLOW LINKS
```

## Docker

```bash
# Build image
docker build -t adb:latest .

# Run MCP server
docker run -i --rm adb:latest
```

## Project Structure

```
crates/
  adb-core/       # Core types: Memory, Link, Predicate
  adb-backends/   # Memory backend implementations
  adb-executor/   # Query execution engine
  aql-parser/     # AQL grammar and parser
  aql-planner/    # Query planning
  adb-mcp/        # MCP protocol server
  adb-server/     # HTTP server (optional)
adb-cli/          # CLI binary
```

## License

MIT
