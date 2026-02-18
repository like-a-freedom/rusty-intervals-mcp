use serde_json::{Map, Value};

/// Compact sport settings to essential fields
pub fn compact_sport_settings(
    value: &Value,
    sports_filter: Option<&[String]>,
    fields: Option<&[String]>,
) -> Value {
    let default_fields = ["type", "ftp", "fthr", "hrZones", "powerZones"];
    let fields_to_use: Vec<&str> = fields
        .map(|f| f.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| default_fields.to_vec());

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
            let mut result = Map::new();
            for field in &fields_to_use {
                if let Some(val) = obj.get(*field) {
                    result.insert(field.to_string(), val.clone());
                }
            }
            Some(Value::Object(result))
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
                let mut result = Map::new();
                for field in field_list {
                    if let Some(val) = obj.get(field) {
                        result.insert(field.clone(), val.clone());
                    }
                }
                Some(Value::Object(result))
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
}
