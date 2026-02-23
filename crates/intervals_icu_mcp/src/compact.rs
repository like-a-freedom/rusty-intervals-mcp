//! Compact and filter utilities for token-efficient responses
//!
//! This module provides generic functions to reduce response size by:
//! - Filtering objects/arrays to only include specified fields
//! - Applying default field sets when no custom fields provided
//! - A `Compact` trait for domain types to implement token-efficient serialization
//! - Generic helpers for common MCP tool patterns (DRY principle)

use serde_json::Value;

/// Helper trait for converting Results to tool-friendly String errors.
///
/// This implements the **DRY** principle by eliminating repetitive
/// `.map_err(|e| e.to_string())` patterns throughout the codebase.
///
/// # Example
/// ```rust,ignore
/// use crate::compact::ToToolError;
///
/// async fn my_tool(&self) -> Result<Json<MyResult>, String> {
///     let result = self.client.some_call().await.to_tool_error()?;
///     Ok(Json(result))
/// }
/// ```
pub trait ToToolError<T> {
    fn to_tool_error(self) -> Result<T, String>;
}

impl<T, E: std::fmt::Display> ToToolError<T> for Result<T, E> {
    fn to_tool_error(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }
}

/// Apply compact mode transformation to a JSON value.
///
/// This implements the **DRY** principle by centralizing the common pattern:
/// ```rust,ignore
/// // Before: repeated in ~40 tool methods
/// let result = if p.compact.unwrap_or(true) {
///     Self::compact_xyz(&v, p.fields.as_deref())
/// } else if let Some(ref fields) = p.fields {
///     Self::filter_array_fields(&v, fields)
/// } else {
///     v
/// };
/// ```
///
/// # Arguments
/// * `value` - The JSON value to transform
/// * `compact` - Whether to apply compact mode (None = default to true)
/// * `fields` - Optional custom fields to filter
/// * `compact_fn` - Function to apply when compact=true
///
/// # Example
/// ```rust,ignore
/// let result = apply_compact_mode(
///     value,
///     p.compact,
///     p.fields,
///     |v, f| domains::gear::compact_gear_list(v, f),
/// );
/// ```
pub fn apply_compact_mode<F>(
    value: Value,
    compact: Option<bool>,
    fields: Option<Vec<String>>,
    compact_fn: F,
) -> Value
where
    F: Fn(&Value, Option<&[String]>) -> Value,
{
    if compact.unwrap_or(true) {
        compact_fn(&value, fields.as_deref())
    } else if let Some(ref fields) = fields {
        filter_array_fields(&value, fields)
    } else {
        value
    }
}

/// Apply compact mode with an additional filter condition.
///
/// Similar to `apply_compact_mode` but supports an additional filter parameter
/// (e.g., sport type filter for sport settings).
///
/// # Arguments
/// * `value` - The JSON value to transform
/// * `compact` - Whether to apply compact mode
/// * `filter_param` - Optional additional filter parameter
/// * `fields` - Optional custom fields
/// * `compact_fn` - Function with signature `(&Value, Option<&[String]>, Option<&[String]>) -> Value`
/// * `filter_fn` - Function for non-compact filtering with same signature
pub fn apply_compact_mode_with_filter<F, G>(
    value: Value,
    compact: Option<bool>,
    filter_param: Option<&[String]>,
    fields: Option<Vec<String>>,
    compact_fn: F,
    filter_fn: G,
) -> Value
where
    F: Fn(&Value, Option<&[String]>, Option<&[String]>) -> Value,
    G: Fn(&Value, Option<&[String]>, Option<&[String]>) -> Value,
{
    if compact.unwrap_or(true) {
        compact_fn(&value, filter_param, fields.as_deref())
    } else if filter_param.is_some() || fields.is_some() {
        filter_fn(&value, filter_param, fields.as_deref())
    } else {
        value
    }
}

/// Trait for types that can be compacted to token-efficient JSON representations.
///
/// This trait implements the **Low Coupling** GRASP principle by:
/// - Decoupling domain types from JSON manipulation logic
/// - Providing a consistent interface for compact serialization
/// - Allowing domain modules to own their compaction logic
///
/// # Example
/// ```rust,ignore
/// use intervals_icu_mcp::compact::Compact;
///
/// struct MyDomainType {
///     id: String,
///     name: String,
///     extra: String,
/// }
///
/// impl Compact for MyDomainType {
///     const DEFAULT_FIELDS: &'static [&'static str] = &["id", "name"];
///
///     fn compact_fields(&self) -> Vec<&'static str> {
///         Self::DEFAULT_FIELDS.to_vec()
///     }
/// }
/// ```
pub trait Compact: serde::Serialize {
    /// Default fields to include in compact representation
    const DEFAULT_FIELDS: &'static [&'static str];

    /// Returns the fields to use for compaction.
    /// Can be overridden to provide dynamic field selection.
    fn compact_fields(&self) -> Vec<&'static str> {
        Self::DEFAULT_FIELDS.to_vec()
    }

    /// Compact this value to JSON using default fields
    fn to_compact_json(&self) -> Value
    where
        Self: Sized,
    {
        compact_item(self, Self::DEFAULT_FIELDS, None)
    }

    /// Compact this value to JSON with custom fields
    fn to_compact_json_with_fields(&self, fields: Option<&[String]>) -> Value
    where
        Self: Sized,
    {
        compact_item(self, Self::DEFAULT_FIELDS, fields)
    }

    /// Compact a slice of this type to a JSON array
    fn compact_slice_to_json(
        items: &[Self],
        fields: Option<&[String]>,
        limit: Option<usize>,
    ) -> Value
    where
        Self: Sized,
    {
        compact_items(items, Self::DEFAULT_FIELDS, fields, limit)
    }
}

/// Macro to simplify the common pattern of resolving fields_to_use from optional custom fields.
///
/// Usage: `let fields_to_use = resolve_fields!(default_fields, fields);`
///
/// Where `fields` is `Option<&[String]>` and `default_fields` is `&[&str]`
#[macro_export]
macro_rules! resolve_fields {
    ($defaults:expr, $fields:expr) => {
        $fields
            .map(|f| f.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| $defaults.to_vec())
    };
}

/// Default field sets for different entity types
pub mod defaults {
    /// Fields for activity summaries
    pub const ACTIVITY: &[&str] = &[
        "id",
        "name",
        "start_date_local",
        "type",
        "moving_time",
        "distance",
        "total_elevation_gain",
        "average_watts",
        "average_heartrate",
        "icu_training_load",
    ];

    /// Fields for events/calendar entries
    pub const EVENT: &[&str] = &[
        "id",
        "name",
        "start_date_local",
        "category",
        "type",
        "description",
    ];

    /// Fields for compact event listings
    pub const EVENT_COMPACT: &[&str] = &["id", "start_date_local", "name", "category", "type"];

    /// Fields for gear items
    pub const GEAR: &[&str] = &["id", "name", "type", "distance", "brand", "model"];

    /// Fields for sport settings
    pub const SPORT_SETTINGS: &[&str] = &["type", "ftp", "fthr", "hrZones", "powerZones"];
    pub const WELLNESS: &[&str] = &[
        "id",
        "sleepSecs",
        "stress",
        "restingHR",
        "hrv",
        "weight",
        "fatigue",
        "motivation",
    ];

    /// Fields for fitness summary
    /// Note: API returns fitness (CTL), fatigue (ATL), form (TSB), rampRate
    pub const FITNESS: &[&str] = &["fitness", "fatigue", "form", "rampRate"];

    /// Fields for intervals
    pub const INTERVAL: &[&str] = &[
        "type",
        "start",
        "end",
        "duration",
        "intensity",
        "activity_id",
    ];
}

/// Compact a JSON object to only include specified fields
///
/// # Arguments
/// * `value` - The JSON value to compact
/// * `default_fields` - Default fields to include if no custom fields specified
/// * `fields` - Optional custom fields to override defaults
pub fn compact_object(value: &Value, default_fields: &[&str], fields: Option<&[String]>) -> Value {
    let fields_to_use: Vec<&str> = fields
        .map(|f| f.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| default_fields.to_vec());

    let Some(obj) = value.as_object() else {
        return value.clone();
    };

    let mut result = serde_json::Map::new();
    for field in &fields_to_use {
        if let Some(val) = obj.get(*field) {
            result.insert(field.to_string(), val.clone());
        }
    }

    Value::Object(result)
}

/// Compact an array of JSON objects
///
/// # Arguments
/// * `value` - The JSON array to compact
/// * `default_fields` - Default fields to include
/// * `fields` - Optional custom fields
/// * `limit` - Optional maximum number of items to return
pub fn compact_array(
    value: &Value,
    default_fields: &[&str],
    fields: Option<&[String]>,
    limit: Option<usize>,
) -> Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    let iter = arr.iter();
    let iter: Box<dyn Iterator<Item = &Value>> = if let Some(l) = limit {
        Box::new(iter.take(l))
    } else {
        Box::new(iter)
    };

    let compacted: Vec<Value> = iter
        .map(|item| compact_object(item, default_fields, fields))
        .collect();

    Value::Array(compacted)
}

/// Filter an array to only include specified fields per object
///
/// Similar to compact_array but without applying default field sets
pub fn filter_array_fields(value: &Value, fields: &[String]) -> Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    let fields_ref: Vec<&str> = fields.iter().map(|s| s.as_str()).collect();

    let filtered: Vec<Value> = arr
        .iter()
        .map(|item| {
            let Some(obj) = item.as_object() else {
                return item.clone();
            };

            let mut result = serde_json::Map::new();
            for field in &fields_ref {
                if let Some(val) = obj.get(*field) {
                    result.insert(field.to_string(), val.clone());
                }
            }
            Value::Object(result)
        })
        .collect();

    Value::Array(filtered)
}

/// Filter a JSON object to only include specified fields
pub fn filter_fields(value: &Value, fields: &[String]) -> Value {
    let Some(obj) = value.as_object() else {
        return value.clone();
    };

    let mut result = serde_json::Map::new();
    for field in fields {
        if let Some(val) = obj.get(field) {
            result.insert(field.clone(), val.clone());
        }
    }

    Value::Object(result)
}

/// Generic compact function that handles both objects and arrays
///
/// # Arguments
/// * `value` - The JSON value (object or array)
/// * `compact` - Whether to apply compact mode
/// * `default_fields` - Fields to use when compact=true
/// * `fields` - Optional custom fields
/// * `limit` - Optional limit for arrays
pub fn compact(
    value: &Value,
    compact: bool,
    default_fields: &[&str],
    fields: Option<&[String]>,
    limit: Option<usize>,
) -> Value {
    if !compact {
        // Not compact mode - just apply field filtering if specified
        return if let Some(f) = fields {
            if value.is_array() {
                filter_array_fields(value, f)
            } else {
                compact_object(value, &[], Some(f))
            }
        } else {
            value.clone()
        };
    }

    // Compact mode
    if value.is_array() {
        compact_array(value, default_fields, fields, limit)
    } else {
        compact_object(value, default_fields, fields)
    }
}

/// Compact a single serializable item
pub fn compact_single<T: serde::Serialize>(
    item: &T,
    default_fields: &[&str],
    fields: Option<&[String]>,
) -> Value {
    let value = serde_json::to_value(item).unwrap_or_default();
    compact_object(&value, default_fields, fields)
}

/// Compact a serializable item to a JSON object, applying default or custom fields.
///
/// This is a convenience wrapper that handles the common pattern of:
/// 1. Serializing an item to JSON
/// 2. Filtering to default fields or custom fields
///
/// # Arguments
/// * `item` - Item to serialize and compact
/// * `default_fields` - Default fields to use if no custom fields provided
/// * `fields` - Optional custom fields (overrides defaults)
pub fn compact_item<T: serde::Serialize>(
    item: &T,
    default_fields: &[&str],
    fields: Option<&[String]>,
) -> Value {
    let value = serde_json::to_value(item).unwrap_or_default();
    compact_object(&value, default_fields, fields)
}

/// Compact an array of serializable items with optional limiting.
///
/// # Arguments
/// * `items` - Slice of items to compact
/// * `default_fields` - Default fields to use
/// * `fields` - Optional custom fields
/// * `limit` - Optional maximum number of items to return
pub fn compact_items<T: serde::Serialize>(
    items: &[T],
    default_fields: &[&str],
    fields: Option<&[String]>,
    limit: Option<usize>,
) -> Value {
    let iter: Box<dyn Iterator<Item = &T>> = if let Some(l) = limit {
        Box::new(items.iter().take(l))
    } else {
        Box::new(items.iter())
    };

    let compacted: Vec<Value> = iter
        .map(|item| compact_item(item, default_fields, fields))
        .collect();

    Value::Array(compacted)
}

/// Apply sport type filter to an array of sport settings
pub fn filter_by_sport_type(
    value: &Value,
    sports_filter: Option<&[String]>,
    default_fields: &[&str],
    fields: Option<&[String]>,
) -> Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    let filtered: Vec<Value> = arr
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;

            // Apply sport type filter if specified
            if let Some(filter) = sports_filter {
                let sport_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if !filter.iter().any(|s| s.eq_ignore_ascii_case(sport_type)) {
                    return None;
                }
            }

            // Apply field filtering
            Some(compact_object(item, default_fields, fields))
        })
        .collect();

    Value::Array(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_object_with_defaults() {
        let obj = serde_json::json!({
            "id": "123",
            "name": "Test",
            "extra": "field"
        });
        let result = compact_object(&obj, &["id", "name"], None);
        assert_eq!(result["id"], "123");
        assert_eq!(result["name"], "Test");
        assert!(result.get("extra").is_none());
    }

    #[test]
    fn test_compact_object_with_custom_fields() {
        let obj = serde_json::json!({
            "id": "123",
            "name": "Test",
            "extra": "field"
        });
        let fields = vec!["id".to_string()];
        let result = compact_object(&obj, &["id", "name"], Some(&fields));
        assert_eq!(result["id"], "123");
        assert!(result.get("name").is_none());
    }

    #[test]
    fn test_compact_array_with_limit() {
        let arr = serde_json::json!([
            {"id": "1", "name": "a"},
            {"id": "2", "name": "b"},
            {"id": "3", "name": "c"}
        ]);
        let result = compact_array(&arr, &["id"], None, Some(2));
        let result_arr = result.as_array().unwrap();
        assert_eq!(result_arr.len(), 2);
        assert_eq!(result_arr[0]["id"], "1");
    }

    #[test]
    fn test_filter_array_fields() {
        let arr = serde_json::json!([
            {"id": "1", "name": "a", "extra": "x"},
            {"id": "2", "name": "b", "extra": "y"}
        ]);
        let fields = vec!["id".to_string(), "name".to_string()];
        let result = filter_array_fields(&arr, &fields);
        let result_arr = result.as_array().unwrap();
        assert_eq!(result_arr.len(), 2);
        assert!(result_arr[0].get("extra").is_none());
    }

    #[test]
    fn test_filter_fields_object() {
        let value = serde_json::json!({"id":"1","name":"x","extra":"ignored"});
        let fields = vec!["id".to_string(), "name".to_string()];

        let result = filter_fields(&value, &fields);

        assert_eq!(result["id"], "1");
        assert_eq!(result["name"], "x");
        assert!(result.get("extra").is_none());
    }

    #[test]
    fn test_compact_non_object_returns_clone() {
        let val = serde_json::json!("string value");
        let result = compact_object(&val, &["id"], None);
        assert_eq!(result, val);
    }

    #[test]
    fn test_compact_non_array_returns_clone() {
        let val = serde_json::json!({"id": "123"});
        let result = compact_array(&val, &["id"], None, None);
        assert_eq!(result, val);
    }

    // Tests for Compact trait
    #[derive(serde::Serialize)]
    struct TestCompactType {
        id: String,
        name: String,
        extra: String,
    }

    impl Compact for TestCompactType {
        const DEFAULT_FIELDS: &'static [&'static str] = &["id", "name"];
    }

    #[test]
    fn test_compact_trait_default_fields() {
        let item = TestCompactType {
            id: "1".to_string(),
            name: "Test".to_string(),
            extra: "Should be excluded".to_string(),
        };
        let result = item.to_compact_json();
        assert_eq!(result["id"], "1");
        assert_eq!(result["name"], "Test");
        assert!(result.get("extra").is_none());
    }

    #[test]
    fn test_compact_trait_custom_fields() {
        let item = TestCompactType {
            id: "1".to_string(),
            name: "Test".to_string(),
            extra: "Should be included".to_string(),
        };
        let fields = vec!["id".to_string(), "extra".to_string()];
        let result = item.to_compact_json_with_fields(Some(&fields));
        assert_eq!(result["id"], "1");
        assert!(result.get("name").is_none());
        assert_eq!(result["extra"], "Should be included");
    }

    #[test]
    fn test_compact_trait_slice() {
        let items = vec![
            TestCompactType {
                id: "1".to_string(),
                name: "First".to_string(),
                extra: "x".to_string(),
            },
            TestCompactType {
                id: "2".to_string(),
                name: "Second".to_string(),
                extra: "y".to_string(),
            },
        ];
        let result = TestCompactType::compact_slice_to_json(&items, None, Some(1));
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "1");
        assert!(arr[0].get("extra").is_none());
    }

    // Tests for ToToolError trait
    #[test]
    fn test_to_tool_error_success() {
        let result: Result<i32, std::convert::Infallible> = Ok(42);
        assert_eq!(result.to_tool_error(), Ok(42));
    }

    #[test]
    fn test_to_tool_error_error() {
        let result: Result<i32, &'static str> = Err("test error");
        assert_eq!(result.to_tool_error(), Err("test error".to_string()));
    }

    // Tests for apply_compact_mode
    #[test]
    fn test_apply_compact_mode_compact_true() {
        let value = serde_json::json!({"id": "1", "name": "Test", "extra": "x"});
        let result = apply_compact_mode(value, Some(true), None, |v, _f| {
            crate::compact::compact_object(v, &["id"], None)
        });
        assert_eq!(result["id"], "1");
        assert!(result.get("name").is_none());
    }

    #[test]
    fn test_apply_compact_mode_compact_false_no_fields() {
        let value = serde_json::json!({"id": "1", "name": "Test"});
        let result = apply_compact_mode(
            value.clone(),
            Some(false),
            None,
            |_v, _f| serde_json::json!({"compact": true}),
        );
        assert_eq!(result, value);
    }

    #[test]
    fn test_apply_compact_mode_with_fields() {
        let value = serde_json::json!([{"id": "1", "name": "Test", "extra": "x"}]);
        let fields = vec!["id".to_string()];
        let result = apply_compact_mode(value, Some(false), Some(fields), |_v, _f| {
            serde_json::json!({})
        });
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["id"], "1");
        assert!(arr[0].get("name").is_none());
    }

    #[test]
    fn test_apply_compact_mode_default_compact() {
        let value = serde_json::json!({"id": "1", "name": "Test", "extra": "x"});
        let result = apply_compact_mode(
            value,
            None, // Should default to true
            None,
            |v, _f| crate::compact::compact_object(v, &["id"], None),
        );
        assert_eq!(result["id"], "1");
        assert!(result.get("name").is_none());
    }
}
