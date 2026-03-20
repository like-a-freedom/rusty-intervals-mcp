//! Compact and filter utilities for token-efficient responses
//!
//! This module provides the shared JSON-shaping helpers still used by the
//! intent-driven runtime and domain transformers:
//! - filtering objects/arrays to specific fields,
//! - compacting objects/arrays to default field sets,
//! - compacting serializable domain values,
//! - resolving default-vs-custom field selections in domain modules.

use serde_json::Value;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(serde::Serialize)]
    struct TestItem {
        id: String,
        name: String,
        extra: String,
    }

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
        let arr = json!([
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
    fn test_filter_array_fields_preserves_non_object_items() {
        let arr = json!([
            {"id": "1", "name": "a", "extra": "x"},
            "keep-me"
        ]);
        let fields = vec!["id".to_string()];

        let result = filter_array_fields(&arr, &fields);
        let result_arr = result.as_array().unwrap();

        assert_eq!(result_arr[0], json!({"id": "1"}));
        assert_eq!(result_arr[1], json!("keep-me"));
    }

    #[test]
    fn test_filter_fields_object() {
        let value = json!({"id":"1","name":"x","extra":"ignored"});
        let fields = vec!["id".to_string(), "name".to_string()];

        let result = filter_fields(&value, &fields);

        assert_eq!(result["id"], "1");
        assert_eq!(result["name"], "x");
        assert!(result.get("extra").is_none());
    }

    #[test]
    fn test_filter_fields_non_object_returns_clone() {
        let value = json!([1, 2, 3]);
        let fields = vec!["id".to_string()];

        let result = filter_fields(&value, &fields);

        assert_eq!(result, value);
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

    #[test]
    fn test_compact_item_with_defaults() {
        let item = TestItem {
            id: "1".to_string(),
            name: "Test".to_string(),
            extra: "ignored".to_string(),
        };

        let result = compact_item(&item, &["id", "name"], None);

        assert_eq!(result["id"], "1");
        assert_eq!(result["name"], "Test");
        assert!(result.get("extra").is_none());
    }

    #[test]
    fn test_compact_item_with_custom_fields() {
        let item = TestItem {
            id: "1".to_string(),
            name: "Test".to_string(),
            extra: "included".to_string(),
        };
        let fields = vec!["id".to_string(), "extra".to_string()];

        let result = compact_item(&item, &["id", "name"], Some(&fields));

        assert_eq!(result["id"], "1");
        assert_eq!(result["extra"], "included");
        assert!(result.get("name").is_none());
    }

    #[test]
    fn test_resolve_fields_macro_uses_defaults_and_custom_values() {
        let defaults = &["id", "name"];
        let custom = vec!["id".to_string(), "extra".to_string()];

        let from_defaults: Vec<&str> = resolve_fields!(defaults, None::<&[String]>);
        let from_custom: Vec<&str> = resolve_fields!(defaults, Some(custom.as_slice()));

        assert_eq!(from_defaults, vec!["id", "name"]);
        assert_eq!(from_custom, vec!["id", "extra"]);
    }
}
