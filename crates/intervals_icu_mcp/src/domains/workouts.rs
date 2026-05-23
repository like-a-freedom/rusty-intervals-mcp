use serde_json::{Map, Value};

/// Compact workout library to essential fields
pub fn compact_workout_library(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = ["id", "name", "description"];
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

/// Compact workouts list (with optional compacting and limit)
pub fn compact_workouts(
    value: &Value,
    compact: bool,
    limit: usize,
    fields: Option<&[String]>,
) -> Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    let default_fields = ["id", "name", "type", "duration", "description"];
    let fields_to_use: Vec<&str> = if compact {
        fields
            .map(|f| f.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| default_fields.to_vec())
    } else {
        return Value::Array(arr.iter().take(limit).cloned().collect());
    };

    let compacted: Vec<Value> = arr
        .iter()
        .take(limit)
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

/// Compact a single folder item to essential fields
pub fn compact_folder_item(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = ["id", "name", "description"];
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
    fn compact_workout_library_filters_fields() {
        let input = json!([
            {"id": "w1", "name": "Interval", "description": "desc", "extra": 1}
        ]);
        let out = compact_workout_library(&input, None);
        assert!(out.is_array());
        let obj = &out.as_array().unwrap()[0];
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("w1"));
        assert!(obj.get("extra").is_none());
    }

    #[test]
    fn compact_workouts_limits_and_filters() {
        let input = json!([
            {"id": "a", "name": "A", "type": "Run", "duration": 30},
            {"id": "b", "name": "B", "type": "Ride", "duration": 60},
            {"id": "c", "name": "C", "type": "Swim", "duration": 20}
        ]);

        let out = compact_workouts(&input, true, 2, None);
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr[0].get("id").is_some());
        assert!(arr[0].get("duration").is_some());
    }

    #[test]
    fn compact_folder_item_filters_fields() {
        let input = json!({"id": "f1", "name": "Base Plan", "description": "desc", "extra": 1});
        let out = compact_folder_item(&input, None);
        assert!(out.is_object());
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("f1"));
        assert_eq!(out.get("name").and_then(|v| v.as_str()), Some("Base Plan"));
        assert!(out.get("description").is_some());
        assert!(out.get("extra").is_none());
    }

    // --- compact_workout_library additional branches ---

    #[test]
    fn compact_workout_library_non_array_returns_as_is() {
        let input = json!("not_an_array");
        let out = compact_workout_library(&input, None);
        assert_eq!(out, json!("not_an_array"));
    }

    #[test]
    fn compact_workout_library_with_custom_fields() {
        let input = json!([
            {"id": "w1", "name": "Hill", "description": "desc", "extra": 1}
        ]);
        let fields = vec!["name".to_string()];
        let out = compact_workout_library(&input, Some(&fields));
        let obj = &out.as_array().unwrap()[0];
        assert_eq!(obj.get("name").and_then(|v| v.as_str()), Some("Hill"));
        assert!(obj.get("id").is_none());
        assert!(obj.get("extra").is_none());
    }

    #[test]
    fn compact_workout_library_skips_non_object_items() {
        let input = json!([
            {"id": "w1", "name": "Valid"},
            "just_a_string",
            42
        ]);
        let out = compact_workout_library(&input, None);
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert!(arr[0].is_object());
        assert_eq!(arr[1], json!("just_a_string"));
        assert_eq!(arr[2], json!(42));
    }

    #[test]
    fn compact_workout_library_missing_field_omitted() {
        let input = json!([
            {"id": "w1", "description": "desc only"}
        ]);
        let out = compact_workout_library(&input, None);
        let obj = &out.as_array().unwrap()[0];
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("w1"));
        assert!(obj.get("name").is_none());
        assert_eq!(
            obj.get("description").and_then(|v| v.as_str()),
            Some("desc only")
        );
    }

    // --- compact_workouts additional branches ---

    #[test]
    fn compact_workouts_non_array_returns_as_is() {
        let input = json!({"not": "an array"});
        let out = compact_workouts(&input, true, 5, None);
        assert_eq!(out, json!({"not": "an array"}));
    }

    #[test]
    fn compact_workouts_no_compact_returns_raw_items() {
        let input = json!([
            {"id": "a", "name": "A", "type": "Run", "duration": 30, "extra": true},
            {"id": "b", "name": "B", "type": "Ride", "duration": 60, "extra": false}
        ]);
        let out = compact_workouts(&input, false, 1, None);
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("extra").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn compact_workouts_compact_with_custom_fields() {
        let input = json!([
            {"id": "a", "name": "A", "type": "Run", "duration": 30, "extra": 1},
            {"id": "b", "name": "B", "type": "Ride", "duration": 60, "extra": 2}
        ]);
        let fields = vec!["name".to_string(), "type".to_string()];
        let out = compact_workouts(&input, true, 5, Some(&fields));
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].get("name").and_then(|v| v.as_str()), Some("A"));
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("Run"));
        assert!(arr[0].get("id").is_none());
        assert!(arr[0].get("extra").is_none());
    }

    #[test]
    fn compact_workouts_skips_non_object_items() {
        let input = json!([
            {"id": "a", "name": "Valid"},
            "raw_string",
            99
        ]);
        let out = compact_workouts(&input, true, 10, None);
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert!(arr[0].is_object());
        assert!(arr[0].get("name").is_some());
        assert_eq!(arr[1], json!("raw_string"));
        assert_eq!(arr[2], json!(99));
    }

    #[test]
    fn compact_workouts_limit_less_than_array_length() {
        let input = json!([
            {"id": "a", "name": "A"},
            {"id": "b", "name": "B"},
            {"id": "c", "name": "C"}
        ]);
        let out = compact_workouts(&input, true, 2, None);
        assert_eq!(out.as_array().unwrap().len(), 2);
    }

    #[test]
    fn compact_workouts_limit_zero_returns_empty() {
        let input = json!([{"id": "a", "name": "A"}]);
        let out = compact_workouts(&input, true, 0, None);
        assert_eq!(out.as_array().unwrap().len(), 0);
    }

    // --- compact_folder_item additional branches ---

    #[test]
    fn compact_folder_item_non_object_returns_as_is() {
        let input = json!([1, 2, 3]);
        let out = compact_folder_item(&input, None);
        assert_eq!(out, json!([1, 2, 3]));
    }

    #[test]
    fn compact_folder_item_with_custom_fields() {
        let input = json!({"id": "f1", "name": "Plan", "description": "desc", "extra": true});
        let fields = vec!["description".to_string()];
        let out = compact_folder_item(&input, Some(&fields));
        assert!(out.is_object());
        assert_eq!(
            out.get("description").and_then(|v| v.as_str()),
            Some("desc")
        );
        assert!(out.get("id").is_none());
        assert!(out.get("extra").is_none());
    }

    #[test]
    fn compact_folder_item_missing_field_omitted() {
        let input = json!({"id": "f1", "extra": 1});
        let out = compact_folder_item(&input, None);
        assert_eq!(out.get("id").and_then(|v| v.as_str()), Some("f1"));
        assert!(out.get("description").is_none());
    }
}
