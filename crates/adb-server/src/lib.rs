//! ADB Server - Main Agent Database Instance
//!
//! Provides the main `Adb` struct that orchestrates all backends
//! and provides a unified interface for agent memory operations.
//!
//! Note: The core Adb and AdbConfig types are now in adb-backends.
//! This crate re-exports them for convenience and will contain
//! HTTP/gRPC server implementations.

// Re-export from adb-backends for backward compatibility
pub use adb_backends::{Adb, AdbConfig};
