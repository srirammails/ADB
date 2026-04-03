//! Link types for ADB ontology
//!
//! Links are typed edges between memory records that form the dynamic ontology.
//! The LLM defines link types through experience - common types include:
//! - "applied_to" - procedure successfully applied to incident
//! - "triggers" - semantic concept triggers procedural pattern
//! - "learned_from" - procedure grounded in episode
//! - "predicts" - concept predicts failure

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::memory::MemoryType;

/// A typed edge connecting two memory records across any memory types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    /// Unique identifier for this link
    pub id: String,

    /// Source memory type
    pub from_type: MemoryType,

    /// Source record ID
    pub from_id: String,

    /// Target memory type
    pub to_type: MemoryType,

    /// Target record ID
    pub to_id: String,

    /// Link type (LLM-defined, arbitrary string)
    /// Common types: "applied_to", "triggers", "learned_from", "predicts"
    pub link_type: String,

    /// Link weight/strength (0.0 - 1.0)
    /// Updated based on success/failure signals
    pub weight: f32,

    /// When the link was created
    pub created_at: DateTime<Utc>,

    /// When the link was last updated
    pub updated_at: DateTime<Utc>,
}

impl Link {
    /// Create a new link
    pub fn new(
        from_type: MemoryType,
        from_id: impl Into<String>,
        to_type: MemoryType,
        to_id: impl Into<String>,
        link_type: impl Into<String>,
        weight: f32,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from_type,
            from_id: from_id.into(),
            to_type,
            to_id: to_id.into(),
            link_type: link_type.into(),
            weight: weight.clamp(0.0, 1.0),
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a link with specific ID
    pub fn with_id(
        id: impl Into<String>,
        from_type: MemoryType,
        from_id: impl Into<String>,
        to_type: MemoryType,
        to_id: impl Into<String>,
        link_type: impl Into<String>,
        weight: f32,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            from_type,
            from_id: from_id.into(),
            to_type,
            to_id: to_id.into(),
            link_type: link_type.into(),
            weight: weight.clamp(0.0, 1.0),
            created_at: now,
            updated_at: now,
        }
    }

    /// Update the link weight with a success/failure signal
    /// Uses exponential moving average: new = old * decay + signal * (1 - decay)
    pub fn update_weight(&mut self, signal: f32, decay: f32) {
        let decay = decay.clamp(0.0, 1.0);
        let signal = signal.clamp(0.0, 1.0);
        self.weight = (self.weight * decay + signal * (1.0 - decay)).clamp(0.0, 1.0);
        self.updated_at = Utc::now();
    }

    /// Set weight directly
    pub fn set_weight(&mut self, weight: f32) {
        self.weight = weight.clamp(0.0, 1.0);
        self.updated_at = Utc::now();
    }

    /// Check if this link is from a specific record
    pub fn is_from(&self, memory_type: MemoryType, id: &str) -> bool {
        self.from_type == memory_type && self.from_id == id
    }

    /// Check if this link is to a specific record
    pub fn is_to(&self, memory_type: MemoryType, id: &str) -> bool {
        self.to_type == memory_type && self.to_id == id
    }

    /// Check if this link has a specific type
    pub fn has_type(&self, link_type: &str) -> bool {
        self.link_type == link_type
    }

    /// Get the other end of the link given one end
    pub fn other_end(&self, memory_type: MemoryType, id: &str) -> Option<(MemoryType, &str)> {
        if self.is_from(memory_type, id) {
            Some((self.to_type, &self.to_id))
        } else if self.is_to(memory_type, id) {
            Some((self.from_type, &self.from_id))
        } else {
            None
        }
    }
}

/// Predicate for querying links
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinkPredicate {
    /// Filter by source memory type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_type: Option<MemoryType>,

    /// Filter by source record ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_id: Option<String>,

    /// Filter by target memory type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_type: Option<MemoryType>,

    /// Filter by target record ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_id: Option<String>,

    /// Filter by link type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_type: Option<String>,

    /// Filter by minimum weight
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_weight: Option<f32>,

    /// Filter by maximum weight
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_weight: Option<f32>,
}

impl LinkPredicate {
    /// Create a predicate matching links from a specific record
    pub fn from_record(memory_type: MemoryType, id: impl Into<String>) -> Self {
        Self {
            from_type: Some(memory_type),
            from_id: Some(id.into()),
            ..Default::default()
        }
    }

    /// Create a predicate matching links to a specific record
    pub fn to_record(memory_type: MemoryType, id: impl Into<String>) -> Self {
        Self {
            to_type: Some(memory_type),
            to_id: Some(id.into()),
            ..Default::default()
        }
    }

    /// Create a predicate matching links of a specific type
    pub fn of_type(link_type: impl Into<String>) -> Self {
        Self {
            link_type: Some(link_type.into()),
            ..Default::default()
        }
    }

    /// Add link type filter
    pub fn with_type(mut self, link_type: impl Into<String>) -> Self {
        self.link_type = Some(link_type.into());
        self
    }

    /// Add minimum weight filter
    pub fn with_min_weight(mut self, min_weight: f32) -> Self {
        self.min_weight = Some(min_weight);
        self
    }

    /// Check if a link matches this predicate
    pub fn matches(&self, link: &Link) -> bool {
        if let Some(ft) = &self.from_type {
            if link.from_type != *ft {
                return false;
            }
        }
        if let Some(fi) = &self.from_id {
            if link.from_id != *fi {
                return false;
            }
        }
        if let Some(tt) = &self.to_type {
            if link.to_type != *tt {
                return false;
            }
        }
        if let Some(ti) = &self.to_id {
            if link.to_id != *ti {
                return false;
            }
        }
        if let Some(lt) = &self.link_type {
            if link.link_type != *lt {
                return false;
            }
        }
        if let Some(min) = self.min_weight {
            if link.weight < min {
                return false;
            }
        }
        if let Some(max) = self.max_weight {
            if link.weight > max {
                return false;
            }
        }
        true
    }
}

/// Common link types used by agents
pub mod link_types {
    /// Procedure successfully applied to incident
    pub const APPLIED_TO: &str = "applied_to";
    /// Semantic concept triggers procedural pattern
    pub const TRIGGERS: &str = "triggers";
    /// Procedure evidence-grounded in episode
    pub const LEARNED_FROM: &str = "learned_from";
    /// Concept predicts failure
    pub const PREDICTS: &str = "predicts";
    /// Pattern filters noise alerts
    pub const NOISE_FILTER_FOR: &str = "noise_filter_for";
    /// Newer procedure replaces older one
    pub const SUPERSEDES: &str = "supersedes";
    /// Content weakens procedure effectiveness
    pub const WEAKENED_BY: &str = "weakened_by";
    /// Requires another procedure first
    pub const REQUIRES: &str = "requires";
    /// Next step in sequence
    pub const NEXT: &str = "next";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_creation() {
        let link = Link::new(
            MemoryType::Procedural,
            "oom-fix",
            MemoryType::Episodic,
            "inc-001",
            "applied_to",
            0.95,
        );

        assert_eq!(link.from_type, MemoryType::Procedural);
        assert_eq!(link.from_id, "oom-fix");
        assert_eq!(link.to_type, MemoryType::Episodic);
        assert_eq!(link.to_id, "inc-001");
        assert_eq!(link.link_type, "applied_to");
        assert!((link.weight - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_weight_update() {
        let mut link = Link::new(
            MemoryType::Procedural,
            "test",
            MemoryType::Episodic,
            "test",
            "applied_to",
            0.5,
        );

        // Success signal should increase weight
        link.update_weight(1.0, 0.9);
        assert!(link.weight > 0.5);

        // Failure signal should decrease weight
        link.update_weight(0.0, 0.9);
        assert!(link.weight < 0.6);
    }

    #[test]
    fn test_link_predicate_matching() {
        let link = Link::new(
            MemoryType::Procedural,
            "oom-fix",
            MemoryType::Episodic,
            "inc-001",
            "applied_to",
            0.95,
        );

        // Match by from record
        let pred = LinkPredicate::from_record(MemoryType::Procedural, "oom-fix");
        assert!(pred.matches(&link));

        // Match by to record
        let pred = LinkPredicate::to_record(MemoryType::Episodic, "inc-001");
        assert!(pred.matches(&link));

        // Match by type
        let pred = LinkPredicate::of_type("applied_to");
        assert!(pred.matches(&link));

        // Match with min weight
        let pred = LinkPredicate::of_type("applied_to").with_min_weight(0.9);
        assert!(pred.matches(&link));

        let pred = LinkPredicate::of_type("applied_to").with_min_weight(0.99);
        assert!(!pred.matches(&link));
    }

    #[test]
    fn test_other_end() {
        let link = Link::new(
            MemoryType::Procedural,
            "oom-fix",
            MemoryType::Episodic,
            "inc-001",
            "applied_to",
            0.95,
        );

        // From the from end
        let other = link.other_end(MemoryType::Procedural, "oom-fix");
        assert_eq!(other, Some((MemoryType::Episodic, "inc-001")));

        // From the to end
        let other = link.other_end(MemoryType::Episodic, "inc-001");
        assert_eq!(other, Some((MemoryType::Procedural, "oom-fix")));

        // From neither end
        let other = link.other_end(MemoryType::Working, "other");
        assert_eq!(other, None);
    }

    #[test]
    fn test_link_serialization() {
        let link = Link::new(
            MemoryType::Semantic,
            "concept-1",
            MemoryType::Procedural,
            "pattern-1",
            "triggers",
            0.8,
        );

        let json = serde_json::to_string(&link).unwrap();
        let parsed: Link = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.from_type, link.from_type);
        assert_eq!(parsed.to_id, link.to_id);
        assert_eq!(parsed.link_type, link.link_type);
    }
}
