//! Utility functions for date/time normalization and other common operations.

use std::fmt::Display;

/// Builder for constructing query parameter vectors.
///
/// This utility implements the **DRY** principle by eliminating repetitive
/// patterns for building `Vec<(&str, &str)>` query parameters throughout
/// the codebase.
///
/// # Example
/// ```rust,ignore
/// use crate::utils::QueryBuilder;
///
/// let query = QueryBuilder::new()
///     .add("oldest", &oldest.to_string())
///     .add("newest", &today.to_string())
///     .add_opt("limit", limit.as_ref())
///     .build();
/// ```
#[derive(Debug, Default)]
pub struct QueryBuilder<'a> {
    params: Vec<(&'a str, String)>,
}

impl<'a> QueryBuilder<'a> {
    /// Create a new empty query builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a required parameter.
    ///
    /// # Arguments
    /// * `key` - Parameter name
    /// * `value` - Parameter value (anything that implements `Display`)
    pub fn add(mut self, key: &'a str, value: impl Display) -> Self {
        self.params.push((key, value.to_string()));
        self
    }

    /// Add an optional parameter (only if `Some`).
    ///
    /// # Arguments
    /// * `key` - Parameter name
    /// * `opt` - Optional value
    pub fn add_opt(mut self, key: &'a str, opt: Option<impl Display>) -> Self {
        if let Some(value) = opt {
            self.params.push((key, value.to_string()));
        }
        self
    }

    /// Add multiple parameters at once.
    pub fn extend<I>(mut self, params: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, String)>,
    {
        self.params.extend(params);
        self
    }

    /// Build the query parameter vector with owned strings.
    ///
    /// Returns a `Vec<(&'a str, String)>` which owns the values.
    pub fn build_owned(self) -> Vec<(&'a str, String)> {
        self.params
    }
}

/// Normalize start_date_local for events: preserve time when provided;
/// if only date is given, set time to 00:00:00.
///
/// Accepts:
/// - YYYY-MM-DD -> YYYY-MM-DDT00:00:00
/// - RFC3339 datetime -> YYYY-MM-DDTHH:MM:SS
/// - Naive datetime YYYY-MM-DDTHH:MM:SS -> YYYY-MM-DDTHH:MM:SS
pub fn normalize_event_start(s: &str) -> Option<String> {
    if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
        return Some(format!("{}T00:00:00", s));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_local().format("%Y-%m-%dT%H:%M:%S").to_string());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(ndt.format("%Y-%m-%dT%H:%M:%S").to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_builder_empty() {
        let query = QueryBuilder::new().build_owned();
        assert!(query.is_empty());
    }

    #[test]
    fn query_builder_add_required() {
        let query = QueryBuilder::new()
            .add("key", "value")
            .add("num", 42)
            .build_owned();
        assert_eq!(query.len(), 2);
        assert_eq!(query[0], ("key", "value".to_string()));
        assert_eq!(query[1], ("num", "42".to_string()));
    }

    #[test]
    fn query_builder_add_optional_some() {
        let query = QueryBuilder::new()
            .add("required", "value")
            .add_opt("optional", Some(123))
            .build_owned();
        assert_eq!(query.len(), 2);
        assert_eq!(query[1], ("optional", "123".to_string()));
    }

    #[test]
    fn query_builder_add_optional_none() {
        let query = QueryBuilder::new()
            .add("required", "value")
            .add_opt("optional", None::<i32>)
            .build_owned();
        assert_eq!(query.len(), 1);
        assert_eq!(query[0], ("required", "value".to_string()));
    }

    #[test]
    fn query_builder_extend() {
        let extra = vec![("extra1", "val1".to_string())];
        let query = QueryBuilder::new()
            .add("base", "value")
            .extend(extra)
            .build_owned();
        assert_eq!(query.len(), 2);
    }

    #[test]
    fn normalize_event_start_accepts_date_only() {
        let result = normalize_event_start("2025-12-15");
        assert_eq!(result.unwrap(), "2025-12-15T00:00:00");
    }

    #[test]
    fn normalize_event_start_preserves_datetime() {
        let result = normalize_event_start("2025-12-15T10:30:00");
        assert_eq!(result.unwrap(), "2025-12-15T10:30:00");
    }

    #[test]
    fn normalize_event_start_preserves_rfc3339() {
        let result = normalize_event_start("2025-12-15T10:30:00Z");
        assert_eq!(result.unwrap(), "2025-12-15T10:30:00");
    }

    #[test]
    fn normalize_event_start_rejects_invalid() {
        let result = normalize_event_start("not-a-date");
        assert!(result.is_none());
    }
}
