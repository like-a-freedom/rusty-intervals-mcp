use serde_json::Value;

/// Default fields for sport settings in compact responses.
pub const DEFAULT_FIELDS: &[&str] = &["type", "ftp", "fthr", "hrZones", "powerZones"];

/// Compact sport settings to essential fields
pub fn compact_sport_settings(
    value: &Value,
    sports_filter: Option<&[String]>,
    fields: Option<&[String]>,
) -> Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    let filtered: Vec<Value> = arr
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;

            if let Some(filter) = sports_filter {
                let sport_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if !filter.iter().any(|s| s.eq_ignore_ascii_case(sport_type)) {
                    return None;
                }
            }

            Some(crate::compact::compact_object(item, DEFAULT_FIELDS, fields))
        })
        .collect();

    Value::Array(filtered)
}

/// Filter sport settings by sport type and/or fields (without compacting)
pub fn filter_sport_settings(
    value: &Value,
    sports_filter: Option<&[String]>,
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

            // Apply field filtering if specified
            if let Some(field_list) = fields {
                Some(crate::compact::compact_object(item, &[], Some(field_list)))
            } else {
                Some(item.clone())
            }
        })
        .collect();

    Value::Array(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_sport_settings_filters_and_compacts() {
        let input = json!([
            {"type": "Run", "ftp": 250, "powerZones": [1,2]},
            {"type": "Ride", "ftp": 280, "powerZones": [3,4]}
        ]);

        // no filters -> default compact fields
        let out = compact_sport_settings(&input, None, None);
        assert_eq!(out.as_array().unwrap().len(), 2);
        assert!(out[0].get("ftp").is_some());
        assert!(out[0].get("powerZones").is_some());

        // filter sports
        let filter = vec!["ride".to_string()];
        let out2 = compact_sport_settings(&input, Some(&filter), None);
        let arr = out2.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(
            arr[0].get("type").and_then(|v| v.as_str()).unwrap_or(""),
            "Ride"
        );
    }

    #[test]
    fn filter_sport_settings_field_selection() {
        let input = json!([
            {"type": "Run", "ftp": 250, "fthr": 180},
            {"type": "Ride", "ftp": 280, "fthr": 190}
        ]);

        let fields = vec!["type".to_string()];
        let out = filter_sport_settings(&input, None, Some(&fields));
        let arr = out.as_array().unwrap();
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("Run"));
        assert!(arr[0].get("ftp").is_none());
    }

    // --- compact_sport_settings: remaining branch coverage ---

    #[test]
    fn compact_non_array_returns_clone() {
        let input = json!({"type": "Run", "ftp": 250});
        let result = compact_sport_settings(&input, None, None);
        assert_eq!(result, input);
    }

    #[test]
    fn compact_empty_array() {
        let result = compact_sport_settings(&json!([]), None, None);
        assert_eq!(result, json!([]));
    }

    #[test]
    fn compact_skips_non_object_items() {
        let input = json!([
            {"type": "Run", "ftp": 250},
            "not an object",
            42
        ]);
        let result = compact_sport_settings(&input, None, None);
        assert_eq!(result.as_array().unwrap().len(), 1);
        assert_eq!(result[0].get("type").and_then(|v| v.as_str()), Some("Run"));
    }

    #[test]
    fn compact_with_custom_fields() {
        let input = json!([
            {"type": "Run", "ftp": 250, "fthr": 180, "hrZones": [1]},
            {"type": "Ride", "ftp": 280, "fthr": 190, "hrZones": [2]}
        ]);
        let fields = vec!["type".to_string()];
        let result = compact_sport_settings(&input, None, Some(&fields));
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("Run"));
        assert!(arr[0].get("ftp").is_none());
        assert!(arr[1].get("fthr").is_none());
    }

    #[test]
    fn compact_filter_no_match_returns_empty() {
        let input = json!([
            {"type": "Run", "ftp": 250},
            {"type": "Ride", "ftp": 280}
        ]);
        let filter = vec!["Swim".to_string()];
        let result = compact_sport_settings(&input, Some(&filter), None);
        assert!(result.as_array().unwrap().is_empty());
    }

    #[test]
    fn compact_filter_missing_type_field() {
        let input = json!([
            {"ftp": 250},
            {"type": "Ride", "ftp": 280}
        ]);
        let filter = vec!["Ride".to_string()];
        let result = compact_sport_settings(&input, Some(&filter), None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("Ride"));
    }

    #[test]
    fn compact_multiple_filter_values() {
        let input = json!([
            {"type": "Run", "ftp": 250},
            {"type": "Ride", "ftp": 280},
            {"type": "Swim", "ftp": 200}
        ]);
        let filter = vec!["Run".to_string(), "Swim".to_string()];
        let result = compact_sport_settings(&input, Some(&filter), None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("Run"));
        assert_eq!(arr[1].get("type").and_then(|v| v.as_str()), Some("Swim"));
    }

    // --- filter_sport_settings: remaining branch coverage ---

    #[test]
    fn filter_non_array_returns_clone() {
        let input = json!({"type": "Run"});
        let result = filter_sport_settings(&input, None, None);
        assert_eq!(result, input);
    }

    #[test]
    fn filter_empty_array() {
        let result = filter_sport_settings(&json!([]), None, None);
        assert_eq!(result, json!([]));
    }

    #[test]
    fn filter_skips_non_object_items() {
        let input = json!([
            {"type": "Run", "ftp": 250},
            "not an object"
        ]);
        let result = filter_sport_settings(&input, None, None);
        assert_eq!(result.as_array().unwrap().len(), 1);
        assert_eq!(result[0].get("type").and_then(|v| v.as_str()), Some("Run"));
    }

    #[test]
    fn filter_with_sport_filter() {
        let input = json!([
            {"type": "Run", "ftp": 250},
            {"type": "Ride", "ftp": 280}
        ]);
        let filter = vec!["Ride".to_string()];
        let result = filter_sport_settings(&input, Some(&filter), None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("Ride"));
    }

    #[test]
    fn filter_sport_filter_no_match_returns_empty() {
        let input = json!([
            {"type": "Run", "ftp": 250},
            {"type": "Ride", "ftp": 280}
        ]);
        let filter = vec!["Swim".to_string()];
        let result = filter_sport_settings(&input, Some(&filter), None);
        assert!(result.as_array().unwrap().is_empty());
    }

    #[test]
    fn filter_no_fields_returns_full_item() {
        let input = json!([
            {"type": "Run", "ftp": 250, "fthr": 180}
        ]);
        let result = filter_sport_settings(&input, None, None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0], json!({"type": "Run", "ftp": 250, "fthr": 180}));
    }

    #[test]
    fn filter_with_sport_filter_and_fields() {
        let input = json!([
            {"type": "Run", "ftp": 250, "fthr": 180},
            {"type": "Ride", "ftp": 280, "fthr": 190}
        ]);
        let filter = vec!["Run".to_string()];
        let fields = vec!["ftp".to_string()];
        let result = filter_sport_settings(&input, Some(&filter), Some(&fields));
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert!(arr[0].get("type").is_none());
        assert_eq!(arr[0].get("ftp").and_then(|v| v.as_i64()), Some(250));
    }

    #[test]
    fn filter_missing_type_field() {
        let input = json!([
            {"ftp": 250},
            {"type": "Ride", "ftp": 280}
        ]);
        let filter = vec!["Ride".to_string()];
        let result = filter_sport_settings(&input, Some(&filter), None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("Ride"));
    }
}
