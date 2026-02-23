//! Domain module for gear management.
//!
//! This module handles gear-related data transformation and compaction.
//! It uses the `crate::compact` utilities for token-efficient JSON responses.
//!
//! # GRASP Principles
//! - **Information Expert**: Gear compaction logic is here, not in handlers
//! - **Low Coupling**: Uses centralized compact utilities

use serde_json::Value;

/// Default fields for gear items in compact responses.
///
/// This constant is used by both the compaction functions and can be
/// referenced by implementations of the `Compact` trait for gear-related types.
pub const DEFAULT_FIELDS: &[&str] = &["id", "name", "type", "distance", "brand", "model"];

/// Compact gear list to essential fields.
///
/// Uses the **Low Coupling** principle by delegating to centralized compact utilities.
pub fn compact_gear_list(value: &Value, fields: Option<&[String]>) -> Value {
    crate::compact::compact_array(value, DEFAULT_FIELDS, fields, None)
}

/// Compact a single gear item to essential fields.
pub fn compact_gear_item(value: &Value, fields: Option<&[String]>) -> Value {
    crate::compact::compact_object(value, DEFAULT_FIELDS, fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_gear_list_filters_fields() {
        let input = json!([
            {"id": "g1", "name": "Shoe", "distance": 1200, "brand": "A"},
            {"id": "g2", "name": "Bike", "distance": 34000, "brand": "B"}
        ]);

        let out = compact_gear_list(&input, None);
        assert!(out.is_array());
        let arr = out.as_array().unwrap();
        assert_eq!(arr[0].get("id").and_then(|v| v.as_str()), Some("g1"));
        assert!(arr[0].get("distance").is_some());
        assert!(arr[0].get("unknown_field").is_none());
    }

    #[test]
    fn compact_gear_item_respects_custom_fields() {
        let input =
            json!({"id": "g1", "name": "Shoe", "distance": 1200, "brand": "A", "model": "X"});
        let fields = vec!["id".to_string(), "model".to_string()];
        let out = compact_gear_item(&input, Some(&fields));
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("g1"));
        assert_eq!(out.get("model").and_then(|v| v.as_str()), Some("X"));
        assert!(out.get("distance").is_none());
    }
}
