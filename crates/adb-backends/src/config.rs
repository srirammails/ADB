//! ADB Configuration
//!
//! Configuration options for the Agent Database.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use adb_core::Scope;

/// Configuration for the ADB instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdbConfig {
    /// Default scope for operations
    pub default_scope: Scope,
    /// Default namespace (agent identity)
    pub default_namespace: Option<String>,
    /// Maximum concurrent queries
    pub max_concurrent_queries: usize,
    /// Default query timeout
    #[serde(with = "duration_millis")]
    pub default_timeout: Duration,
    /// Working memory configuration
    pub working: WorkingConfig,
    /// Tools configuration
    pub tools: ToolsConfig,
    /// Procedural configuration
    pub procedural: ProceduralConfig,
}

/// Working memory configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingConfig {
    /// Enable TTL expiration
    pub ttl_enabled: bool,
    /// TTL check interval
    #[serde(with = "duration_millis")]
    pub ttl_check_interval: Duration,
    /// Maximum entries (0 = unlimited)
    pub max_entries: usize,
}

/// Tools configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    /// Decay factor for ranking updates (0.0 - 1.0)
    pub decay_factor: f32,
}

/// Procedural memory configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralConfig {
    /// Default pattern matching threshold
    pub default_threshold: f32,
}

impl Default for AdbConfig {
    fn default() -> Self {
        Self {
            default_scope: Scope::Private,
            default_namespace: None,
            max_concurrent_queries: 100,
            default_timeout: Duration::from_millis(100),
            working: WorkingConfig::default(),
            tools: ToolsConfig::default(),
            procedural: ProceduralConfig::default(),
        }
    }
}

impl Default for WorkingConfig {
    fn default() -> Self {
        Self {
            ttl_enabled: true,
            ttl_check_interval: Duration::from_secs(1),
            max_entries: 10000,
        }
    }
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self { decay_factor: 0.9 }
    }
}

impl Default for ProceduralConfig {
    fn default() -> Self {
        Self {
            default_threshold: 0.7,
        }
    }
}

impl AdbConfig {
    /// Create a new builder
    pub fn builder() -> AdbConfigBuilder {
        AdbConfigBuilder::default()
    }
}

/// Builder for AdbConfig
#[derive(Debug, Default)]
pub struct AdbConfigBuilder {
    config: AdbConfig,
}

impl AdbConfigBuilder {
    /// Set default scope
    pub fn default_scope(mut self, scope: Scope) -> Self {
        self.config.default_scope = scope;
        self
    }

    /// Set default namespace
    pub fn default_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.config.default_namespace = Some(namespace.into());
        self
    }

    /// Set default timeout
    pub fn default_timeout(mut self, timeout: Duration) -> Self {
        self.config.default_timeout = timeout;
        self
    }

    /// Enable or disable TTL
    pub fn ttl_enabled(mut self, enabled: bool) -> Self {
        self.config.working.ttl_enabled = enabled;
        self
    }

    /// Set TTL check interval
    pub fn ttl_check_interval(mut self, interval: Duration) -> Self {
        self.config.working.ttl_check_interval = interval;
        self
    }

    /// Set tools decay factor
    pub fn tools_decay_factor(mut self, factor: f32) -> Self {
        self.config.tools.decay_factor = factor.clamp(0.0, 1.0);
        self
    }

    /// Set procedural pattern threshold
    pub fn procedural_threshold(mut self, threshold: f32) -> Self {
        self.config.procedural.default_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Build the configuration
    pub fn build(self) -> AdbConfig {
        self.config
    }
}

// Serde helper for Duration
mod duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (value.as_millis() as u64).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis: u64 = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AdbConfig::default();
        assert_eq!(config.default_scope, Scope::Private);
        assert!(config.working.ttl_enabled);
        assert!((config.tools.decay_factor - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_builder() {
        let config = AdbConfig::builder()
            .default_scope(Scope::Shared)
            .default_namespace("agent-k8s")
            .ttl_enabled(false)
            .build();

        assert_eq!(config.default_scope, Scope::Shared);
        assert_eq!(config.default_namespace, Some("agent-k8s".to_string()));
        assert!(!config.working.ttl_enabled);
    }

    #[test]
    fn test_serialization() {
        let config = AdbConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AdbConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.default_scope, config.default_scope);
    }
}
