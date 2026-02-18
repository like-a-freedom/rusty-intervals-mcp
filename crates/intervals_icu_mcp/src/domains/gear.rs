use serde_json::{Map, Value};

/// Compact gear list to essential fields
pub fn compact_gear_list(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = ["id", "name", "type", "distance", "brand", "model"];
    let fields_to_use: Vec<&str> = fields
        .map(|f| f.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| default_fields.to_vec());

    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    let compacted: Vec<Value> = arr
        .iter()
        .map(|item| {
            let Some(obj) = item.as_object() else {
                return item.clone();
            };
            let mut result = Map::new();
            for field in &fields_to_use {
                if let Some(val) = obj.get(*field) {
                    result.insert(field.to_string(), val.clone());
                }
            }
            Value::Object(result)
        })
        .collect();

    Value::Array(compacted)
}

/// Compact a single gear item to essential fields
pub fn compact_gear_item(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = ["id", "name", "type", "distance", "brand", "model"];
    let fields_to_use: Vec<&str> = fields
        .map(|f| f.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| default_fields.to_vec());

    let Some(obj) = value.as_object() else {
        return value.clone();
    };

    let mut result = Map::new();
    for field in &fields_to_use {
        if let Some(val) = obj.get(*field) {
            result.insert(field.to_string(), val.clone());
        }
    }

    Value::Object(result)
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
