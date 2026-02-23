use serde_json::Value;

/// Default fields for gear items
const DEFAULT_FIELDS: &[&str] = &["id", "name", "type", "distance", "brand", "model"];

/// Compact gear list to essential fields
pub fn compact_gear_list(value: &Value, fields: Option<&[String]>) -> Value {
    crate::compact::compact_array(value, DEFAULT_FIELDS, fields, None)
}

/// Compact a single gear item to essential fields
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
