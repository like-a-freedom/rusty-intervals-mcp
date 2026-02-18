use serde_json::Value;

/// Compact fitness summary to essential fields
pub fn compact_fitness_summary(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = [
        "ctl",
        "atl",
        "tsb",
        "ctl_ramp_rate",
        "atl_ramp_rate",
        "date",
    ];
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_fitness_summary_defaults() {
        let input = json!({
            "ctl": 50.0,
            "atl": 30.0,
            "tsb": 20.0,
            "ctl_ramp_rate": 1.2,
            "atl_ramp_rate": 0.8,
            "date": "2026-01-01",
            "extra": 123
        });

        let out = compact_fitness_summary(&input, None);
        assert!(out.get("ctl").is_some());
        assert!(out.get("extra").is_none());
    }

    #[test]
    fn compact_fitness_summary_custom_fields() {
        let input = json!({"ctl": 42.0, "date": "2026-01-02", "foo": "bar"});
        let fields = vec!["date".to_string(), "foo".to_string()];
        let out = compact_fitness_summary(&input, Some(&fields));
        assert_eq!(out.get("date").and_then(|v| v.as_str()), Some("2026-01-02"));
        assert_eq!(out.get("foo").and_then(|v| v.as_str()), Some("bar"));
    }
}
