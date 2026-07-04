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

/// Compute Z1/Z2/Z3 percentage shares from an `icu_zone_times` array.
///
/// Uses the Seiler 3-zone model: Z1+Z2 → easy, Z3 → threshold, Z4+ →高强度.
/// Accepts the raw `"icu_zone_times"` JSON value from an activity detail.
/// Returns `(z1_pct, z2_pct, z3_pct)` as fractions of total zone time, or `None`
/// when the input is not an array or total time is zero.
pub(crate) fn compute_zone_distribution(zone_times: &Value) -> Option<(f64, f64, f64)> {
    let zones = zone_times.as_array()?;

    let mut z1 = 0.0_f64;
    let mut z2 = 0.0_f64;
    let mut z3 = 0.0_f64;

    for entry in zones {
        let id = entry.get("id")?.as_str()?;
        let secs = entry
            .get("secs")
            .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
            .unwrap_or(0.0);
        match id {
            "Z1" | "Z2" => z1 += secs,
            "Z3" => z2 += secs,
            "Z4" | "Z5" | "Z6" | "Z7" => z3 += secs,
            _ => {}
        }
    }

    let total = z1 + z2 + z3;
    if total <= f64::EPSILON {
        return None;
    }

    Some((z1 / total, z2 / total, z3 / total))
}
