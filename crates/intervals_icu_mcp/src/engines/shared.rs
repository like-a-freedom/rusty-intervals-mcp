//! Shared utility functions for engines.
//!
//! These were extracted from multiple engine files to eliminate
//! near-identical copies — see Phase 2 of the audit refactor plan.

use chrono::{NaiveDate, NaiveDateTime};
use serde_json::Value;

/// Parse an activity date string (ISO datetime or date-only) into `NaiveDate`.
///
/// Accepts both `"2024-01-15T14:30:00"` and `"2024-01-15"` formats.
/// When both succeed the datetime variant wins (preserves semantic intent
/// from the Intervals.icu API response).
pub(crate) fn parse_activity_date(value: &str) -> Option<NaiveDate> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|dt| dt.date())
        .or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
}

/// Parse an event date string — identical semantics to [`parse_activity_date`].
///
/// Extracted under its own name so call sites remain readable;
/// the implementation is shared to keep the parsing logic in one place.
pub(crate) fn parse_event_date(value: &str) -> Option<NaiveDate> {
    parse_activity_date(value)
}

/// Parse `icu_zone_times` into a canonical 5-bucket array.
///
/// Maps Z1→bucket 0, Z2→1, Z3→2, Z4→3, Z5/Z6/Z7→4.
/// Duplicates are aggregated; negative or missing seconds are skipped.
/// Returns `None` when the input is not an array or total time is zero.
pub(crate) fn aggregate_five_zone_seconds(zone_times: &Value) -> Option<[f64; 5]> {
    let zones = zone_times.as_array()?;
    let mut totals = [0.0_f64; 5];

    for entry in zones {
        let Some(id) = entry.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(seconds) = entry
            .get("secs")
            .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
            .filter(|seconds| *seconds > 0.0)
        else {
            continue;
        };

        let bucket = match id {
            "Z1" => Some(0),
            "Z2" => Some(1),
            "Z3" => Some(2),
            "Z4" => Some(3),
            "Z5" | "Z6" | "Z7" => Some(4),
            _ => None,
        };
        if let Some(index) = bucket {
            totals[index] += seconds;
        }
    }

    (totals.iter().sum::<f64>() > 0.0).then_some(totals)
}

/// Compute Z1/Z2/Z3 percentage shares from an `icu_zone_times` array.
///
/// Uses the Seiler 3-zone model: Z1+Z2 → easy, Z3 → threshold, Z4+ →高强度.
/// Accepts the raw `"icu_zone_times"` JSON value from an activity detail.
/// Returns `(z1_pct, z2_pct, z3_pct)` as fractions of total zone time, or `None`
/// when the input is not an array or total time is zero.
pub(crate) fn compute_zone_distribution(zone_times: &Value) -> Option<(f64, f64, f64)> {
    let zones = aggregate_five_zone_seconds(zone_times)?;
    let easy = zones[0] + zones[1];
    let threshold = zones[2];
    let high = zones[3] + zones[4];
    let total = easy + threshold + high;

    Some((easy / total, threshold / total, high / total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn five_zone_parser_aggregates_duplicates_and_caps_z6_z7_at_weight_five() {
        let zones = json!([
            {"id": "Z1", "secs": 1200},
            {"id": "Z1", "secs": 600},
            {"id": "Z3", "secs": 300},
            {"id": "Z5", "secs": 60},
            {"id": "Z6", "secs": 120},
            {"id": "Z7", "secs": 180}
        ]);

        assert_eq!(
            aggregate_five_zone_seconds(&zones),
            Some([1800.0, 0.0, 300.0, 0.0, 360.0])
        );
    }

    #[test]
    fn five_zone_parser_ignores_bad_entries_without_discarding_good_data() {
        let zones = json!([
            {"id": "Z2", "secs": 600},
            {"id": "unknown", "secs": 999},
            {"id": "Z3", "secs": -30},
            {"secs": 120},
            "bad"
        ]);

        assert_eq!(
            aggregate_five_zone_seconds(&zones),
            Some([0.0, 600.0, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn five_zone_parser_returns_none_without_positive_recognized_time() {
        assert_eq!(aggregate_five_zone_seconds(&json!([])), None);
        assert_eq!(
            aggregate_five_zone_seconds(&json!([{"id": "Z1", "secs": 0}])),
            None
        );
        assert_eq!(aggregate_five_zone_seconds(&json!({"Z1": 600})), None);
    }

    #[test]
    fn tid_distribution_keeps_existing_seiler_mapping() {
        let zones = json!([
            {"id": "Z1", "secs": 1800},
            {"id": "Z2", "secs": 600},
            {"id": "Z3", "secs": 300},
            {"id": "Z4", "secs": 180},
            {"id": "Z5", "secs": 120}
        ]);

        let (easy, threshold, high) = compute_zone_distribution(&zones).unwrap();
        assert!((easy - 0.8).abs() < 1e-12);
        assert!((threshold - 0.1).abs() < 1e-12);
        assert!((high - 0.1).abs() < 1e-12);
    }
}
