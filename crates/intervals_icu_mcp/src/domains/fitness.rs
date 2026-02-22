use serde_json::Value;
use tracing::{debug, info};

fn normalize_field_name(field: &str) -> String {
    field
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<String>()
        .to_lowercase()
}

fn canonical_field_name(field: &str) -> String {
    match normalize_field_name(field).as_str() {
        "fitness" | "ctl" => "fitness".to_string(),
        "fatigue" | "atl" => "fatigue".to_string(),
        "form" | "tsb" => "form".to_string(),
        "ramprate" | "ramp_rate" | "ctl_ramp_rate" | "atl_ramp_rate" => "rampRate".to_string(),
        "date" => "date".to_string(),
        _ => field.to_string(),
    }
}

fn candidate_keys(field: &str) -> Vec<&str> {
    match field {
        // Canonical output names
        "fitness" => vec!["fitness", "ctl"],
        "fatigue" => vec!["fatigue", "atl"],
        "form" => vec!["form", "tsb"],
        "rampRate" => vec!["rampRate", "ramp_rate"],
        "date" => vec!["date"],
        // Aliases requested explicitly by user fields
        "ctl" => vec!["ctl", "fitness"],
        "atl" => vec!["atl", "fatigue"],
        "tsb" => vec!["tsb", "form"],
        "ramp_rate" => vec!["ramp_rate", "rampRate"],
        _ => vec![field],
    }
}

/// Compact fitness summary to essential fields.
///
/// The Intervals.icu API returns fitness metrics from the athlete-summary endpoint as:
/// - Array of SummaryWithCats objects (most recent first)
/// - Each object contains: fitness (CTL), fatigue (ATL), form (TSB), rampRate
///
/// This function extracts the most recent entry and returns the essential fields.
pub fn compact_fitness_summary(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = ["fitness", "fatigue", "form", "rampRate"];

    // Handle empty fields array - treat as None to use defaults
    let mut fields_to_use: Vec<(String, String)> = match fields {
        Some([]) => {
            info!("compact_fitness_summary: empty fields array, using defaults");
            default_fields
                .iter()
                .map(|s| (s.to_string(), s.to_string()))
                .collect()
        }
        Some(f) => {
            info!("compact_fitness_summary: using provided fields: {:?}", f);
            f.iter()
                .map(|s| (s.to_string(), canonical_field_name(s)))
                .collect()
        }
        None => {
            info!("compact_fitness_summary: no fields specified, using defaults");
            default_fields
                .iter()
                .map(|s| (s.to_string(), s.to_string()))
                .collect()
        }
    };

    if fields_to_use.is_empty() {
        fields_to_use = default_fields
            .iter()
            .map(|s| (s.to_string(), s.to_string()))
            .collect();
    }

    let log_fields: Vec<String> = fields_to_use
        .iter()
        .map(|(out, canon)| format!("{}=>{}", out, canon))
        .collect();

    info!(
        "compact_fitness_summary: input is_array={}, fields_to_use={:?}",
        value.is_array(),
        log_fields
    );

    // API returns array of SummaryWithCats. Prefer first element that actually
    // contains at least one requested fitness field (or alias); fallback to first.
    let obj = if let Some(arr) = value.as_array() {
        info!(
            "compact_fitness_summary: extracting element from array of {}",
            arr.len()
        );
        arr.iter()
            .filter_map(|v| v.as_object())
            .find(|obj| {
                fields_to_use.iter().any(|(_, canonical)| {
                    candidate_keys(canonical)
                        .iter()
                        .any(|candidate| obj.contains_key(*candidate))
                })
            })
            .or_else(|| arr.iter().find_map(|v| v.as_object()))
    } else {
        info!("compact_fitness_summary: input is not an array, trying as object");
        value.as_object()
    };

    let Some(obj) = obj else {
        info!("compact_fitness_summary: failed to extract object, returning clone");
        return value.clone();
    };

    info!(
        "compact_fitness_summary: extracted object with keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    let mut result = serde_json::Map::new();
    for (out_field, canonical_field) in &fields_to_use {
        let mut found = None;
        for candidate in candidate_keys(canonical_field) {
            if let Some(val) = obj.get(candidate) {
                info!(
                    "compact_fitness_summary: found field '{}' via key '{}' = {:?}",
                    out_field, candidate, val
                );
                found = Some(val.clone());
                break;
            }
        }

        if let Some(val) = found {
            result.insert(out_field.to_string(), val);
        } else {
            debug!(
                "compact_fitness_summary: field '{}' (canonical '{}') not found in object",
                out_field, canonical_field
            );
        }
    }

    // If custom fields produced no values, fallback to default fitness set
    if result.is_empty() && fields.is_some() {
        info!("compact_fitness_summary: no requested fields found, falling back to defaults");
        for field in default_fields {
            for candidate in candidate_keys(field) {
                if let Some(val) = obj.get(candidate) {
                    result.insert(field.to_string(), val.clone());
                    break;
                }
            }
        }
    }

    let result_len = result.len();
    let out = Value::Object(result);
    info!(
        "compact_fitness_summary: output created with {} keys",
        result_len
    );
    out
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

    #[test]
    fn compact_fitness_summary_array_extracts_first() {
        // API returns array of SummaryWithCats (most recent first)
        let input = json!([
            {
                "fitness": 55.0,
                "fatigue": 35.0,
                "form": 20.0,
                "rampRate": 1.5,
                "date": "2025-02-22"
            },
            {
                "fitness": 50.0,
                "fatigue": 30.0,
                "form": 20.0,
                "rampRate": 1.2,
                "date": "2025-02-21"
            }
        ]);

        let out = compact_fitness_summary(&input, None);
        // Should extract from first (most recent) entry
        assert_eq!(out.get("fitness").and_then(|v| v.as_f64()), Some(55.0));
        assert_eq!(out.get("fatigue").and_then(|v| v.as_f64()), Some(35.0));
        assert_eq!(out.get("form").and_then(|v| v.as_f64()), Some(20.0));
        assert_eq!(out.get("rampRate").and_then(|v| v.as_f64()), Some(1.5));
        assert!(out.get("date").is_none());
    }

    #[test]
    fn compact_fitness_summary_empty_array() {
        let input = json!([]);
        let out = compact_fitness_summary(&input, None);
        // Empty array should return clone of input
        assert!(out.is_array());
        assert!(out.as_array().unwrap().is_empty());
    }

    #[test]
    fn compact_fitness_summary_empty_fields_array() {
        // When fields is Some([]), should use defaults
        let input = json!([{
            "fitness": 55.0,
            "fatigue": 35.0,
            "form": 20.0,
            "rampRate": 1.5
        }]);
        let empty_fields: Vec<String> = vec![];
        let out = compact_fitness_summary(&input, Some(&empty_fields));

        // Should use default fields
        assert_eq!(out.get("fitness").and_then(|v| v.as_f64()), Some(55.0));
        assert_eq!(out.get("fatigue").and_then(|v| v.as_f64()), Some(35.0));
        assert_eq!(out.get("form").and_then(|v| v.as_f64()), Some(20.0));
        assert_eq!(out.get("rampRate").and_then(|v| v.as_f64()), Some(1.5));
    }

    #[test]
    fn compact_fitness_summary_supports_alias_keys() {
        // Real-world payloads can contain ctl/atl/tsb aliases
        let input = json!([{
            "ctl": 61.0,
            "atl": 74.0,
            "tsb": -13.0,
            "ramp_rate": 2.1
        }]);

        let out = compact_fitness_summary(&input, None);
        assert_eq!(out.get("fitness").and_then(|v| v.as_f64()), Some(61.0));
        assert_eq!(out.get("fatigue").and_then(|v| v.as_f64()), Some(74.0));
        assert_eq!(out.get("form").and_then(|v| v.as_f64()), Some(-13.0));
        assert_eq!(out.get("rampRate").and_then(|v| v.as_f64()), Some(2.1));
    }

    #[test]
    fn compact_fitness_summary_skips_first_item_without_metrics() {
        let input = json!([
            {
                "date": "2026-02-22",
                "count": 3,
                "distance": 12345
            },
            {
                "fitness": 49.0,
                "fatigue": 52.0,
                "form": -3.0,
                "rampRate": 0.8
            }
        ]);

        let out = compact_fitness_summary(&input, None);
        assert_eq!(out.get("fitness").and_then(|v| v.as_f64()), Some(49.0));
        assert_eq!(out.get("fatigue").and_then(|v| v.as_f64()), Some(52.0));
        assert_eq!(out.get("form").and_then(|v| v.as_f64()), Some(-3.0));
        assert_eq!(out.get("rampRate").and_then(|v| v.as_f64()), Some(0.8));
    }

    #[test]
    fn compact_fitness_summary_user_alias_fields_with_ramp_and_date() {
        let input = json!([
            {
                "count": 5,
                "date": "2026-02-16",
                "fitness": 45.98,
                "fatigue": 65.36,
                "form": -19.38,
                "rampRate": 0.87
            }
        ]);

        let fields = vec![
            "ctl".to_string(),
            "atl".to_string(),
            "tsb".to_string(),
            "ctl_ramp_rate".to_string(),
            "atl_ramp_rate".to_string(),
            "date".to_string(),
        ];

        let out = compact_fitness_summary(&input, Some(&fields));
        assert_eq!(out.get("ctl").and_then(|v| v.as_f64()), Some(45.98));
        assert_eq!(out.get("atl").and_then(|v| v.as_f64()), Some(65.36));
        assert_eq!(out.get("tsb").and_then(|v| v.as_f64()), Some(-19.38));
        assert_eq!(
            out.get("ctl_ramp_rate").and_then(|v| v.as_f64()),
            Some(0.87)
        );
        assert_eq!(
            out.get("atl_ramp_rate").and_then(|v| v.as_f64()),
            Some(0.87)
        );
        assert_eq!(out.get("date").and_then(|v| v.as_str()), Some("2026-02-16"));
    }
}
