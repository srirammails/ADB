//! Predicates, conditions, and modifiers for AQL queries
//!
//! This module defines the query building blocks:
//! - Predicates: WHERE, KEY, LIKE, PATTERN, ALL
//! - Conditions: field = value, field > value, etc.
//! - Modifiers: LIMIT, ORDER BY, RETURN, TIMEOUT, etc.
//! - Window: LAST N, LAST duration, TOP N BY field, SINCE condition

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::scope::Scope;

/// A query predicate specifying what records to match
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Predicate {
    /// WHERE condition AND condition ...
    Where { conditions: Vec<Condition> },
    /// KEY field = value (exact lookup)
    Key { field: String, value: Value },
    /// LIKE $embedding (similarity search)
    Like { embedding_var: String },
    /// PATTERN $var THRESHOLD 0.7
    Pattern {
        pattern_var: String,
        threshold: Option<f32>,
    },
    /// ALL (match everything)
    All,
}

impl Predicate {
    /// Create a WHERE predicate with a single condition
    pub fn where_eq(field: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::Where {
            conditions: vec![Condition::new(field, Operator::Eq, value)],
        }
    }

    /// Create a KEY predicate
    pub fn key(field: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::Key {
            field: field.into(),
            value: value.into(),
        }
    }

    /// Create a LIKE predicate
    pub fn like(embedding_var: impl Into<String>) -> Self {
        Self::Like {
            embedding_var: embedding_var.into(),
        }
    }

    /// Create a PATTERN predicate
    pub fn pattern(pattern_var: impl Into<String>, threshold: Option<f32>) -> Self {
        Self::Pattern {
            pattern_var: pattern_var.into(),
            threshold,
        }
    }

    /// Create an ALL predicate
    pub fn all() -> Self {
        Self::All
    }

    /// Check if this predicate requires embedding
    pub fn requires_embedding(&self) -> bool {
        matches!(self, Self::Like { .. })
    }

    /// Check if this predicate requires pattern matching
    pub fn requires_pattern_match(&self) -> bool {
        matches!(self, Self::Pattern { .. })
    }
}

impl Default for Predicate {
    fn default() -> Self {
        Self::All
    }
}

/// A single condition in a WHERE clause
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    /// Field name to compare
    pub field: String,
    /// Comparison operator
    pub operator: Operator,
    /// Value to compare against
    pub value: Value,
}

impl Condition {
    /// Create a new condition
    pub fn new(field: impl Into<String>, operator: Operator, value: impl Into<Value>) -> Self {
        Self {
            field: field.into(),
            operator,
            value: value.into(),
        }
    }

    /// Create an equality condition
    pub fn eq(field: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::new(field, Operator::Eq, value)
    }

    /// Create a not-equal condition
    pub fn ne(field: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::new(field, Operator::Ne, value)
    }

    /// Create a greater-than condition
    pub fn gt(field: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::new(field, Operator::Gt, value)
    }

    /// Create a less-than condition
    pub fn lt(field: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::new(field, Operator::Lt, value)
    }

    /// Evaluate this condition against a JSON value
    pub fn matches(&self, data: &serde_json::Value) -> bool {
        let field_value = data.get(&self.field);
        match field_value {
            Some(v) => self.operator.compare(v, &self.value),
            None => false,
        }
    }
}

/// Comparison operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    /// Equal (=)
    Eq,
    /// Not equal (!=, <>)
    Ne,
    /// Greater than (>)
    Gt,
    /// Greater than or equal (>=)
    Gte,
    /// Less than (<)
    Lt,
    /// Less than or equal (<=)
    Lte,
    /// Contains (for strings/arrays)
    Contains,
    /// Starts with (for strings)
    StartsWith,
    /// Ends with (for strings)
    EndsWith,
    /// In list
    In,
}

impl Operator {
    /// Compare a JSON value against a target value
    pub fn compare(&self, json_value: &serde_json::Value, target: &Value) -> bool {
        match self {
            Self::Eq => value_equals(json_value, target),
            Self::Ne => !value_equals(json_value, target),
            Self::Gt => value_compare(json_value, target).map_or(false, |c| c > 0),
            Self::Gte => value_compare(json_value, target).map_or(false, |c| c >= 0),
            Self::Lt => value_compare(json_value, target).map_or(false, |c| c < 0),
            Self::Lte => value_compare(json_value, target).map_or(false, |c| c <= 0),
            Self::Contains => match (json_value, target) {
                (serde_json::Value::String(s), Value::String(t)) => s.contains(t.as_str()),
                (serde_json::Value::Array(arr), _) => {
                    arr.iter().any(|v| value_equals(v, target))
                }
                _ => false,
            },
            Self::StartsWith => match (json_value, target) {
                (serde_json::Value::String(s), Value::String(t)) => s.starts_with(t.as_str()),
                _ => false,
            },
            Self::EndsWith => match (json_value, target) {
                (serde_json::Value::String(s), Value::String(t)) => s.ends_with(t.as_str()),
                _ => false,
            },
            Self::In => match target {
                Value::Array(arr) => arr.iter().any(|v| value_equals(json_value, v)),
                _ => false,
            },
        }
    }

    /// Parse operator from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "=" | "==" => Some(Self::Eq),
            "!=" | "<>" => Some(Self::Ne),
            ">" => Some(Self::Gt),
            ">=" => Some(Self::Gte),
            "<" => Some(Self::Lt),
            "<=" => Some(Self::Lte),
            "CONTAINS" | "contains" => Some(Self::Contains),
            "STARTS_WITH" | "starts_with" => Some(Self::StartsWith),
            "ENDS_WITH" | "ends_with" => Some(Self::EndsWith),
            "IN" | "in" => Some(Self::In),
            _ => None,
        }
    }
}

/// A typed value for conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    /// Variable reference (for pipeline inter-stage binding)
    Variable(String),
}

impl Value {
    /// Convert to JSON value
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(b) => serde_json::Value::Bool(*b),
            Self::Int(i) => serde_json::Value::Number((*i).into()),
            Self::Float(f) => {
                serde_json::Number::from_f64(*f).map_or(serde_json::Value::Null, |n| {
                    serde_json::Value::Number(n)
                })
            }
            Self::String(s) => serde_json::Value::String(s.clone()),
            Self::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| v.to_json()).collect())
            }
            Self::Variable(v) => serde_json::Value::String(format!("${{{}}}", v)),
        }
    }

    /// Check if this value is a variable reference
    pub fn is_variable(&self) -> bool {
        matches!(self, Self::Variable(_))
    }

    /// Get variable name if this is a variable
    pub fn as_variable(&self) -> Option<&str> {
        match self {
            Self::Variable(v) => Some(v.as_str()),
            _ => None,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}

impl From<i32> for Value {
    fn from(i: i32) -> Self {
        Self::Int(i as i64)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

/// Helper to compare JSON value with our Value type
fn value_equals(json: &serde_json::Value, value: &Value) -> bool {
    match (json, value) {
        (serde_json::Value::Null, Value::Null) => true,
        (serde_json::Value::Bool(a), Value::Bool(b)) => a == b,
        (serde_json::Value::Number(n), Value::Int(i)) => n.as_i64() == Some(*i),
        (serde_json::Value::Number(n), Value::Float(f)) => n.as_f64() == Some(*f),
        (serde_json::Value::String(a), Value::String(b)) => a == b,
        // Variables should be bound before comparison - if we reach here, no match
        (_, Value::Variable(_)) => false,
        _ => false,
    }
}

/// Helper to compare JSON values (returns ordering)
fn value_compare(json: &serde_json::Value, value: &Value) -> Option<i32> {
    match (json, value) {
        (serde_json::Value::Number(n), Value::Int(i)) => {
            // Try integer comparison first
            if let Some(jv) = n.as_i64() {
                Some(jv.cmp(i) as i32)
            } else {
                // JSON is float, coerce int target to float for comparison
                n.as_f64().and_then(|jv| jv.partial_cmp(&(*i as f64)).map(|c| c as i32))
            }
        }
        (serde_json::Value::Number(n), Value::Float(f)) => {
            // Try float comparison, works for both float and int JSON values
            let jv = n.as_f64().or_else(|| n.as_i64().map(|i| i as f64));
            jv.and_then(|jv| jv.partial_cmp(f).map(|c| c as i32))
        }
        (serde_json::Value::String(a), Value::String(b)) => Some(a.cmp(b) as i32),
        _ => None,
    }
}

/// Query modifiers
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Modifiers {
    /// Maximum number of results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Sort order
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_by: Option<OrderBy>,
    /// Fields to return
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_fields: Option<Vec<String>>,
    /// Query timeout
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(with = "option_duration_millis")]
    pub timeout: Option<Duration>,
    /// Minimum confidence/similarity threshold
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,
    /// Isolation scope
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    /// Agent namespace
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Time-to-live for stored records
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(with = "option_duration_millis")]
    pub ttl: Option<Duration>,
    /// Aggregate functions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<Vec<AggregateFunc>>,
    /// HAVING clause (filter after aggregation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub having: Option<Vec<Condition>>,
    /// Window for SCAN
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<Window>,
    /// Include links with results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_links: Option<WithLinks>,
    /// Follow links to other memory types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_links: Option<FollowLinks>,
}

impl Modifiers {
    /// Create modifiers with limit
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit: Some(limit),
            ..Default::default()
        }
    }

    /// Add order by
    pub fn order_by(mut self, field: impl Into<String>, ascending: bool) -> Self {
        self.order_by = Some(OrderBy {
            field: field.into(),
            ascending,
        });
        self
    }

    /// Add timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Add min confidence
    pub fn min_confidence(mut self, confidence: f32) -> Self {
        self.min_confidence = Some(confidence);
        self
    }
}

/// Sort order specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBy {
    /// Field to sort by
    pub field: String,
    /// True for ascending, false for descending
    pub ascending: bool,
}

impl OrderBy {
    /// Create ascending order
    pub fn asc(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            ascending: true,
        }
    }

    /// Create descending order
    pub fn desc(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            ascending: false,
        }
    }
}

/// Aggregate function specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateFunc {
    /// Function type
    pub func: AggregateFuncType,
    /// Field to aggregate (None for COUNT(*))
    pub field: Option<String>,
    /// Alias for result
    pub alias: Option<String>,
}

/// Aggregate function types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AggregateFuncType {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

/// Window specification for SCAN
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Window {
    /// Last N items
    LastN { count: usize },
    /// Last duration
    LastDuration {
        #[serde(with = "duration_millis")]
        duration: Duration,
    },
    /// Top N by field
    TopBy {
        count: usize,
        field: String,
    },
    /// Since condition
    Since { condition: Condition },
}

impl Window {
    /// Create WINDOW LAST N
    pub fn last_n(count: usize) -> Self {
        Self::LastN { count }
    }

    /// Create WINDOW LAST duration
    pub fn last_duration(duration: Duration) -> Self {
        Self::LastDuration { duration }
    }

    /// Create WINDOW TOP N BY field
    pub fn top_by(count: usize, field: impl Into<String>) -> Self {
        Self::TopBy {
            count,
            field: field.into(),
        }
    }

    /// Create WINDOW SINCE condition
    pub fn since(condition: Condition) -> Self {
        Self::Since { condition }
    }
}

/// WITH LINKS modifier
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WithLinks {
    /// Include all links
    All,
    /// Include only links of specific type
    Type { link_type: String },
}

/// FOLLOW LINKS modifier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowLinks {
    /// Link type to follow
    pub link_type: String,
    /// Maximum depth (default 1)
    pub depth: Option<u32>,
}

// Serde helpers for Duration
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

mod option_duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => (d.as_millis() as u64).serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<u64> = Option::deserialize(deserializer)?;
        Ok(opt.map(Duration::from_millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_condition_matching() {
        let data = json!({"name": "test", "count": 42, "active": true});

        assert!(Condition::eq("name", "test").matches(&data));
        assert!(!Condition::eq("name", "other").matches(&data));
        assert!(Condition::gt("count", 40).matches(&data));
        assert!(Condition::lt("count", 50).matches(&data));
        assert!(Condition::eq("active", true).matches(&data));
    }

    #[test]
    fn test_operator_contains() {
        let data = json!({"tags": ["rust", "adb"], "name": "hello world"});

        let cond = Condition::new("name", Operator::Contains, "world");
        assert!(cond.matches(&data));

        let cond = Condition::new("tags", Operator::Contains, "rust");
        assert!(cond.matches(&data));
    }

    #[test]
    fn test_predicate_where() {
        let pred = Predicate::Where {
            conditions: vec![
                Condition::eq("pod", "payments"),
                Condition::gt("severity", 5),
            ],
        };

        assert!(!pred.requires_embedding());
        assert!(!pred.requires_pattern_match());
    }

    #[test]
    fn test_modifiers_builder() {
        let mods = Modifiers::with_limit(10)
            .order_by("timestamp", false)
            .min_confidence(0.7);

        assert_eq!(mods.limit, Some(10));
        assert_eq!(mods.order_by.as_ref().unwrap().ascending, false);
        assert_eq!(mods.min_confidence, Some(0.7));
    }

    #[test]
    fn test_window_variants() {
        let w1 = Window::last_n(10);
        let w2 = Window::last_duration(Duration::from_secs(60));
        let w3 = Window::top_by(5, "score");

        // Serialize and check
        let json = serde_json::to_string(&w1).unwrap();
        assert!(json.contains("last_n"));

        let json = serde_json::to_string(&w2).unwrap();
        assert!(json.contains("last_duration"));

        let json = serde_json::to_string(&w3).unwrap();
        assert!(json.contains("top_by"));
    }
}
