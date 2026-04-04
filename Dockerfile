# Build stage
FROM rust:latest AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    cmake \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy source code
COPY . .

# Build release binary
RUN cargo build --release

# Runtime stage - use rust base for library compatibility
FROM rust:latest

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/adb /usr/local/bin/adb

# Expose MCP port (if using HTTP transport)
EXPOSE 3000

# Default command - run HTTP server
CMD ["adb", "serve-http", "--port", "3000"]
