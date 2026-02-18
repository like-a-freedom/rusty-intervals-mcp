//! Compact and filter utilities for token-efficient responses
//!
//! This module provides generic functions to reduce response size by:
//! - Filtering objects/arrays to only include specified fields
//! - Applying default field sets when no custom fields provided

use serde_json::Value;

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
    pub const FITNESS: &[&str] = &[
        "ctl",
        "atl",
        "tsb",
        "ctl_ramp_rate",
        "atl_ramp_rate",
        "date",
    ];

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
}
