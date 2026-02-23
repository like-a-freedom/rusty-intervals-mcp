use serde_json::{Map, Value};

/// Normalize a date string to YYYY-MM-DD format.
/// 
/// Accepts:
/// - YYYY-MM-DD (returns as-is)
/// - RFC3339 datetime (extracts date)
/// - Naive datetime YYYY-MM-DDTHH:MM:SS (extracts date)
pub fn normalize_date_str(s: &str) -> Option<String> {
    if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
        return Some(s.to_string());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.date_naive().format("%Y-%m-%d").to_string());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(ndt.date().format("%Y-%m-%d").to_string());
    }
    None
}

/// Normalize start_date_local for events: preserve time when provided;
/// if only date is given, set time to 00:00:00.
/// 
/// Accepts:
/// - YYYY-MM-DD -> YYYY-MM-DDT00:00:00
/// - RFC3339 datetime -> YYYY-MM-DDTHH:MM:SS
/// - Naive datetime YYYY-MM-DDTHH:MM:SS -> YYYY-MM-DDTHH:MM:SS
pub fn normalize_event_start(s: &str) -> Option<String> {
    if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
        return Some(format!("{}T00:00:00", s));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_local().format("%Y-%m-%dT%H:%M:%S").to_string());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(ndt.format("%Y-%m-%dT%H:%M:%S").to_string());
    }
    None
}

/// Extract compact activity summary from full details.
pub fn extract_activity_summary(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = [
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
        "icu_intensity",
        "calories",
        "average_speed",
    ];

    let fields_to_extract: Vec<&str> = fields
        .map(|f| f.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| default_fields.to_vec());

    let Some(obj) = value.as_object() else {
        return value.clone();
    };

    let mut result = Map::new();
    for field in fields_to_extract {
        if let Some(val) = obj.get(field) {
            result.insert(field.to_string(), val.clone());
        }
    }

    Value::Object(result)
}

/// Compact an array of activities to essential fields only.
pub fn compact_activities_array(value: &Value, custom_fields: Option<&[String]>) -> Value {
    let default_fields = [
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

    let fields: Vec<&str> = custom_fields
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
            for field in &fields {
                if let Some(val) = obj.get(*field) {
                    result.insert(field.to_string(), val.clone());
                }
            }
            Value::Object(result)
        })
        .collect();

    Value::Array(compacted)
}

/// Filter CSV to limit rows and columns.
pub fn filter_csv(csv: &str, max_rows: usize, columns: Option<&[String]>) -> String {
    let mut lines = csv.lines();
    let Some(header) = lines.next() else {
        return csv.to_string();
    };

    let header_cols: Vec<&str> = header.split(',').collect();

    let col_indices: Vec<usize> = if let Some(cols) = columns {
        header_cols
            .iter()
            .enumerate()
            .filter_map(|(i, h)| {
                if cols.iter().any(|c| c.eq_ignore_ascii_case(h)) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    } else {
        vec![0, 1, 2, 3, 4, 5]
    };

    let mut out = String::new();
    let filtered_header: Vec<&str> = col_indices
        .iter()
        .filter_map(|&i| header_cols.get(i).copied())
        .collect();
    out.push_str(&filtered_header.join(","));
    out.push('\n');

    for line in lines.take(max_rows) {
        let cols: Vec<&str> = line.split(',').collect();
        let filtered: Vec<&str> = col_indices
            .iter()
            .filter_map(|&i| cols.get(i).copied())
            .collect();
        out.push_str(&filtered.join(","));
        out.push('\n');
    }

    out
}
