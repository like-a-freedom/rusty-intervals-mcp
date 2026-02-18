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
}
