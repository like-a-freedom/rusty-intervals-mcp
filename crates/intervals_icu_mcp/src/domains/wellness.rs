//! Domain module for wellness data management.
//!
//! This module handles wellness data transformation and summarization.
//! It uses the `crate::compact` utilities for token-efficient JSON responses.
//!
//! # GRASP Principles
//! - **Information Expert**: Wellness summarization logic is encapsulated here
//! - **Low Coupling**: Uses centralized compact utilities

use serde_json::Value;

/// Default fields for wellness entries in compact responses.
///
/// This constant is used by the shared compaction helpers for wellness payloads.
pub const DEFAULT_FIELDS: &[&str] = &[
    "id",
    "sleepSecs",
    "stress",
    "restingHR",
    "hrv",
    "weight",
    "fatigue",
    "motivation",
    "mood",
    "readiness",
];

/// Normalize a date string to YYYY-MM-DD format.
///
/// Accepts either YYYY-MM-DD or ISO 8601 datetimes.
/// This follows the **Information Expert** principle by keeping
/// date normalization logic in the domain module.
pub fn normalize_date(date_str: &str) -> Option<String> {
    crate::intents::utils::normalize_date_str(date_str)
}

pub fn transform_wellness(value: &Value, summary_only: bool, fields: Option<&[String]>) -> Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    if summary_only {
        let mut sleep_total: f64 = 0.0;
        let mut stress_total: f64 = 0.0;
        let mut hr_total: f64 = 0.0;
        let mut hrv_sum: f64 = 0.0;
        let mut sleep_count: usize = 0;
        let mut stress_count: usize = 0;
        let mut hr_count: usize = 0;
        let mut hrv_occurrences: usize = 0;

        for item in arr {
            if let Some(obj) = item.as_object() {
                if let Some(v) = obj.get("sleepSecs").and_then(|v| v.as_f64()) {
                    sleep_total += v / 3600.0;
                    sleep_count += 1;
                }
                if let Some(v) = obj.get("stress").and_then(|v| v.as_f64()) {
                    stress_total += v;
                    stress_count += 1;
                }
                if let Some(v) = obj.get("restingHR").and_then(|v| v.as_f64()) {
                    hr_total += v;
                    hr_count += 1;
                }
                if let Some(v) = obj.get("hrv").and_then(|v| v.as_f64()) {
                    hrv_sum += v;
                    hrv_occurrences += 1;
                }
            }
        }

        return serde_json::json!({
            "days": arr.len(),
            "avg_sleep_hours": if sleep_count > 0 { (sleep_total / sleep_count as f64 * 10.0).round() / 10.0 } else { 0.0 },
            "avg_stress": if stress_count > 0 { (stress_total / stress_count as f64 * 10.0).round() / 10.0 } else { 0.0 },
            "avg_resting_hr": if hr_count > 0 { (hr_total / hr_count as f64).round() } else { 0.0 },
            "avg_hrv": if hrv_occurrences > 0 { (hrv_sum / hrv_occurrences as f64).round() } else { 0.0 }
        });
    }

    if let Some(field_list) = fields {
        let fields_to_use = if field_list.is_empty() {
            DEFAULT_FIELDS.to_vec()
        } else {
            field_list.iter().map(|s| s.as_str()).collect()
        };

        return crate::compact::compact_array(value, &fields_to_use, None, None);
    }

    value.clone()
}

pub fn compact_wellness_entry(value: &Value, fields: Option<&[String]>) -> Value {
    crate::compact::compact_object(value, DEFAULT_FIELDS, fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_wellness_summary_returns_aggregates() {
        let input = serde_json::json!([
            {"sleepSecs": 28800, "stress": 20, "restingHR": 50, "hrv": 45},
            {"sleepSecs": 25200, "stress": 30, "restingHR": 55, "hrv": 40}
        ]);

        let out = transform_wellness(&input, true, None);
        assert_eq!(out["days"], 2);
        assert_eq!(out["avg_sleep_hours"], 7.5);
    }

    #[test]
    fn transform_wellness_summary_returns_zero_when_no_matching_fields() {
        let input = serde_json::json!([
            {"foo": "bar"},
            {"baz": 123}
        ]);
        let out = transform_wellness(&input, true, None);
        assert_eq!(out["days"], 2);
        assert_eq!(out["avg_sleep_hours"], 0.0);
        assert_eq!(out["avg_stress"], 0.0);
        assert_eq!(out["avg_resting_hr"], 0.0);
        assert_eq!(out["avg_hrv"], 0.0);
    }

    #[test]
    fn transform_wellness_passes_through_non_array() {
        let input = serde_json::json!({"foo": "bar"});
        let out = transform_wellness(&input, true, None);
        assert_eq!(out, input);
        let out2 = transform_wellness(&input, false, None);
        assert_eq!(out2, input);
    }

    #[test]
    fn transform_wellness_compact_with_custom_fields() {
        let input = serde_json::json!([
            {"id": "w1", "sleepSecs": 28800, "stress": 20, "restingHR": 50, "hrv": 45}
        ]);
        let out = transform_wellness(&input, false, Some(&["id".into(), "sleepSecs".into()]));
        assert_eq!(out.as_array().unwrap()[0]["id"], "w1");
        assert_eq!(out.as_array().unwrap()[0]["sleepSecs"], 28800);
        assert!(out.as_array().unwrap()[0].get("stress").is_none());
    }

    #[test]
    fn transform_wellness_compact_uses_defaults_when_fields_empty() {
        let input = serde_json::json!([
            {"id": "w1", "sleepSecs": 28800, "stress": 20, "fatigue": 5}
        ]);
        let out = transform_wellness(&input, false, Some(&[]));
        let arr = out.as_array().unwrap();
        assert_eq!(arr[0]["id"], "w1");
        assert_eq!(arr[0]["sleepSecs"], 28800);
        assert_eq!(arr[0]["stress"], 20);
        assert_eq!(arr[0]["fatigue"], 5);
    }

    #[test]
    fn transform_wellness_summary_with_single_entry() {
        let input = serde_json::json!([{"sleepSecs": 36000, "restingHR": 48, "hrv": 72}]);
        let out = transform_wellness(&input, true, None);
        assert_eq!(out["days"], 1);
        assert_eq!(out["avg_sleep_hours"], 10.0);
        assert_eq!(out["avg_resting_hr"], 48.0);
        assert_eq!(out["avg_hrv"], 72.0);
    }

    #[test]
    fn compact_wellness_entry_filters_fields() {
        let input = serde_json::json!({"id":"w1","sleepSecs":28800,"extra":"x"});
        let out = compact_wellness_entry(&input, Some(&["id".into()]));
        assert_eq!(out["id"], "w1");
        assert!(out.get("sleepSecs").is_none());
    }

    #[test]
    fn compact_wellness_entry_uses_default_fields() {
        let input = serde_json::json!({
            "id": "w1",
            "sleepSecs": 28800,
            "stress": 20,
            "restingHR": 50,
            "hrv": 45,
            "fatigue": 3,
            "mood": 7,
            "readiness": 8.0,
            "extra": "ignored"
        });
        let out = compact_wellness_entry(&input, None);
        assert_eq!(out["id"], "w1");
        assert_eq!(out["sleepSecs"], 28800);
        assert_eq!(out["stress"], 20);
        assert_eq!(out["restingHR"], 50);
        assert_eq!(out["hrv"], 45);
        assert_eq!(out["fatigue"], 3);
        assert_eq!(out["mood"], 7);
        assert_eq!(out["readiness"], 8.0);
        assert!(out.get("extra").is_none());
        assert!(out.get("weight").is_none());
        assert!(out.get("motivation").is_none());
    }

    #[test]
    fn compact_wellness_entry_with_custom_field_list() {
        let input = serde_json::json!({
            "id": "w1",
            "sleepSecs": 28800,
            "stress": 20,
            "restingHR": 50
        });
        let out = compact_wellness_entry(&input, Some(&["id".into(), "hrv".into()]));
        assert_eq!(out["id"], "w1");
        assert!(out.get("sleepSecs").is_none());
        assert!(out.get("stress").is_none());
        assert!(out.get("restingHR").is_none());
        assert!(out.get("hrv").is_none());
    }

    #[test]
    fn normalize_date_parses_iso8601() {
        let result = normalize_date("2026-03-23T10:30:00");
        assert_eq!(result, Some("2026-03-23".to_string()));
    }

    #[test]
    fn normalize_date_parses_plain_date() {
        let result = normalize_date("2026-03-23");
        assert_eq!(result, Some("2026-03-23".to_string()));
    }
}
