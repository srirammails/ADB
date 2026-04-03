//! Time utilities for ADB
//!
//! Handles duration parsing and TTL management.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Time-to-live specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ttl(#[serde(with = "duration_serde")] Duration);

impl Ttl {
    /// Create a new TTL from duration
    pub fn new(duration: Duration) -> Self {
        Self(duration)
    }

    /// Create TTL from seconds
    pub fn from_secs(secs: u64) -> Self {
        Self(Duration::from_secs(secs))
    }

    /// Create TTL from milliseconds
    pub fn from_millis(millis: u64) -> Self {
        Self(Duration::from_millis(millis))
    }

    /// Create TTL from minutes
    pub fn from_mins(mins: u64) -> Self {
        Self(Duration::from_secs(mins * 60))
    }

    /// Create TTL from hours
    pub fn from_hours(hours: u64) -> Self {
        Self(Duration::from_secs(hours * 3600))
    }

    /// Create TTL from days
    pub fn from_days(days: u64) -> Self {
        Self(Duration::from_secs(days * 86400))
    }

    /// Get the underlying duration
    pub fn as_duration(&self) -> Duration {
        self.0
    }

    /// Get total milliseconds
    pub fn as_millis(&self) -> u128 {
        self.0.as_millis()
    }

    /// Get total seconds
    pub fn as_secs(&self) -> u64 {
        self.0.as_secs()
    }

    /// Parse a duration string like "30s", "5m", "1h", "7d"
    pub fn parse(s: &str) -> Option<Self> {
        parse_duration(s).map(Self)
    }
}

impl From<Duration> for Ttl {
    fn from(d: Duration) -> Self {
        Self(d)
    }
}

impl From<Ttl> for Duration {
    fn from(ttl: Ttl) -> Self {
        ttl.0
    }
}

impl std::fmt::Display for Ttl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let secs = self.0.as_secs();
        if secs >= 86400 && secs % 86400 == 0 {
            write!(f, "{}d", secs / 86400)
        } else if secs >= 3600 && secs % 3600 == 0 {
            write!(f, "{}h", secs / 3600)
        } else if secs >= 60 && secs % 60 == 0 {
            write!(f, "{}m", secs / 60)
        } else if secs > 0 {
            write!(f, "{}s", secs)
        } else {
            write!(f, "{}ms", self.0.as_millis())
        }
    }
}

/// Parse a duration string like "30s", "5m", "1h", "7d", "100ms"
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Find where the numeric part ends
    let num_end = s
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit() && *c != '.')
        .map(|(i, _)| i)
        .unwrap_or(s.len());

    if num_end == 0 {
        return None;
    }

    let num_str = &s[..num_end];
    let unit = s[num_end..].trim();

    // Try parsing as float for sub-second precision
    let value: f64 = num_str.parse().ok()?;

    let duration = match unit.to_lowercase().as_str() {
        "ms" | "millis" | "milliseconds" => Duration::from_secs_f64(value / 1000.0),
        "s" | "sec" | "secs" | "second" | "seconds" | "" => Duration::from_secs_f64(value),
        "m" | "min" | "mins" | "minute" | "minutes" => Duration::from_secs_f64(value * 60.0),
        "h" | "hr" | "hrs" | "hour" | "hours" => Duration::from_secs_f64(value * 3600.0),
        "d" | "day" | "days" => Duration::from_secs_f64(value * 86400.0),
        _ => return None,
    };

    Some(duration)
}

/// Format a duration as a human-readable string
pub fn format_duration(d: Duration) -> String {
    Ttl(d).to_string()
}

// Custom serde for Duration as milliseconds
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_millis().serialize(serializer)
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
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("7d"), Some(Duration::from_secs(604800)));
        assert_eq!(parse_duration("100ms"), Some(Duration::from_millis(100)));
        assert_eq!(parse_duration("1.5s"), Some(Duration::from_millis(1500)));

        // Edge cases
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration("5x"), None);
    }

    #[test]
    fn test_ttl_display() {
        assert_eq!(Ttl::from_secs(30).to_string(), "30s");
        assert_eq!(Ttl::from_mins(5).to_string(), "5m");
        assert_eq!(Ttl::from_hours(2).to_string(), "2h");
        assert_eq!(Ttl::from_days(1).to_string(), "1d");
        assert_eq!(Ttl::from_millis(500).to_string(), "500ms");
    }

    #[test]
    fn test_ttl_serialization() {
        let ttl = Ttl::from_secs(60);
        let json = serde_json::to_string(&ttl).unwrap();
        assert_eq!(json, "60000"); // Serialized as milliseconds

        let parsed: Ttl = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_secs(), 60);
    }

    #[test]
    fn test_ttl_conversions() {
        let ttl = Ttl::from_mins(5);
        let duration: Duration = ttl.into();
        assert_eq!(duration, Duration::from_secs(300));

        let ttl2: Ttl = duration.into();
        assert_eq!(ttl2.as_secs(), 300);
    }
}
