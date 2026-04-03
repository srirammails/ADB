//! Scope and namespace handling for ADB
//!
//! Scopes control visibility of records:
//! - Private: Only visible to the owning agent
//! - Shared: Visible to a group of agents
//! - Cluster: Visible to all agents

use serde::{Deserialize, Serialize};

/// Isolation scope for memory records
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Only visible to the owning agent
    #[default]
    Private,
    /// Visible to a group of agents (same namespace)
    Shared,
    /// Visible to all agents in the cluster
    Cluster,
}

impl Scope {
    /// Parse from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "private" => Some(Self::Private),
            "shared" => Some(Self::Shared),
            "cluster" => Some(Self::Cluster),
            _ => None,
        }
    }

    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Shared => "shared",
            Self::Cluster => "cluster",
        }
    }

    /// Check if this scope allows access from another scope
    pub fn allows_access_from(&self, other: &Scope) -> bool {
        match (self, other) {
            // Cluster records are visible to everyone
            (Scope::Cluster, _) => true,
            // Shared records are visible to shared and cluster
            (Scope::Shared, Scope::Shared | Scope::Cluster) => true,
            // Private records only visible to same scope
            (Scope::Private, Scope::Private) => true,
            _ => false,
        }
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Agent namespace for multi-agent isolation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Namespace(String);

impl Namespace {
    /// Create a new namespace
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Get the namespace string
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Check if this namespace matches another (considering wildcards)
    pub fn matches(&self, other: &Self) -> bool {
        // Exact match
        if self.0 == other.0 {
            return true;
        }

        // Wildcard matching: "agent.*" matches "agent.foo"
        if self.0.ends_with(".*") {
            let prefix = &self.0[..self.0.len() - 2];
            return other.0.starts_with(prefix) && other.0[prefix.len()..].starts_with('.');
        }

        false
    }
}

impl Default for Namespace {
    fn default() -> Self {
        Self("default".to_string())
    }
}

impl std::fmt::Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for Namespace {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Namespace {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_parsing() {
        assert_eq!(Scope::from_str("private"), Some(Scope::Private));
        assert_eq!(Scope::from_str("SHARED"), Some(Scope::Shared));
        assert_eq!(Scope::from_str("Cluster"), Some(Scope::Cluster));
        assert_eq!(Scope::from_str("invalid"), None);
    }

    #[test]
    fn test_scope_access() {
        // Cluster allows everyone
        assert!(Scope::Cluster.allows_access_from(&Scope::Private));
        assert!(Scope::Cluster.allows_access_from(&Scope::Shared));
        assert!(Scope::Cluster.allows_access_from(&Scope::Cluster));

        // Shared allows shared and cluster
        assert!(!Scope::Shared.allows_access_from(&Scope::Private));
        assert!(Scope::Shared.allows_access_from(&Scope::Shared));
        assert!(Scope::Shared.allows_access_from(&Scope::Cluster));

        // Private only allows private
        assert!(Scope::Private.allows_access_from(&Scope::Private));
        assert!(!Scope::Private.allows_access_from(&Scope::Shared));
        assert!(!Scope::Private.allows_access_from(&Scope::Cluster));
    }

    #[test]
    fn test_namespace_matching() {
        let ns1 = Namespace::new("agent.k8s");
        let ns2 = Namespace::new("agent.k8s");
        let ns3 = Namespace::new("agent.rtb");
        let wildcard = Namespace::new("agent.*");

        assert!(ns1.matches(&ns2));
        assert!(!ns1.matches(&ns3));
        assert!(wildcard.matches(&ns1));
        assert!(wildcard.matches(&ns3));
    }

    #[test]
    fn test_scope_serialization() {
        let scope = Scope::Shared;
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(json, "\"shared\"");

        let parsed: Scope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Scope::Shared);
    }
}
