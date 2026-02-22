use serde_json::Value;

/// Compact fitness summary to essential fields.
///
/// The Intervals.icu API returns fitness metrics as:
/// - `fitness` (CTL - Chronic Training Load)
/// - `fatigue` (ATL - Acute Training Load)
/// - `form` (TSB - Training Stress Balance = fitness - fatigue)
/// - `rampRate` (rate of change of fitness)
pub fn compact_fitness_summary(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = ["fitness", "fatigue", "form", "rampRate"];
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
        // API returns fitness/fatigue/form/rampRate format
        let input = json!({
            "fitness": 50.0,
            "fatigue": 30.0,
            "form": 20.0,
            "rampRate": 1.2,
            "weight": 70.0,
            "extra": 123
        });

        let out = compact_fitness_summary(&input, None);
        assert_eq!(out.get("fitness").and_then(|v| v.as_f64()), Some(50.0));
        assert_eq!(out.get("fatigue").and_then(|v| v.as_f64()), Some(30.0));
        assert_eq!(out.get("form").and_then(|v| v.as_f64()), Some(20.0));
        assert_eq!(out.get("rampRate").and_then(|v| v.as_f64()), Some(1.2));
        assert!(out.get("weight").is_none());
        assert!(out.get("extra").is_none());
    }

    #[test]
    fn compact_fitness_summary_custom_fields() {
        let input = json!({"fitness": 42.0, "fatigue": 10.0, "form": 32.0, "foo": "bar"});
        let fields = vec!["fitness".to_string(), "foo".to_string()];
        let out = compact_fitness_summary(&input, Some(&fields));
        assert_eq!(out.get("fitness").and_then(|v| v.as_f64()), Some(42.0));
        assert_eq!(out.get("foo").and_then(|v| v.as_str()), Some("bar"));
        assert!(out.get("fatigue").is_none());
        assert!(out.get("form").is_none());
    }

    #[test]
    fn compact_fitness_summary_empty_input() {
        let input = json!({});
        let out = compact_fitness_summary(&input, None);
        assert!(out.as_object().unwrap().is_empty());
    }

    #[test]
    fn compact_fitness_summary_non_object_returns_clone() {
        let input = json!("string value");
        let out = compact_fitness_summary(&input, None);
        assert_eq!(out, input);
    }
}
