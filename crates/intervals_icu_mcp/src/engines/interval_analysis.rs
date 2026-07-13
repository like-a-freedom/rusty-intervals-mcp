//! Interval analysis and work detection logic.
//!
//! This module contains business logic for classifying intervals as work/rest,
//! extracting metrics, and deriving output values. Previously lived in
//! `render/analysis.rs` but belongs in the engine layer.

use serde_json::Value;

/// Classify intervals as work or rest using median-split heuristic.
///
/// Work intervals are those with speed and/or HR above the median.
/// Falls back to counting all intervals if data is insufficient (<3 samples).
pub fn count_work_intervals(intervals: &[Value]) -> usize {
    if intervals.is_empty() {
        return 0;
    }

    let mut speed_data: Vec<(usize, f64)> = Vec::new();
    let mut hr_data: Vec<(usize, f64)> = Vec::new();

    for (i, interval) in intervals.iter().filter_map(|v| v.as_object()).enumerate() {
        if let Some(speed) = interval
            .get("average_speed")
            .and_then(|v| v.as_f64())
            .filter(|&s| s > 0.0)
        {
            speed_data.push((i, speed));
        }
        if let Some(hr) = interval
            .get("average_heartrate")
            .and_then(|v| v.as_f64())
            .filter(|&h| h > 0.0)
        {
            hr_data.push((i, hr));
        }
    }

    if speed_data.len() < 3 && hr_data.len() < 3 {
        return intervals.len();
    }

    let median_speed =
        calculate_median(&mut speed_data.iter().map(|(_, s)| *s).collect::<Vec<_>>());
    let median_hr = calculate_median(&mut hr_data.iter().map(|(_, h)| *h).collect::<Vec<_>>());

    let mut work_count = 0;

    for interval in intervals.iter().filter_map(|v| v.as_object()) {
        let speed = interval.get("average_speed").and_then(|v| v.as_f64());
        let hr = interval.get("average_heartrate").and_then(|v| v.as_f64());

        let is_work = match (speed, hr) {
            (Some(s), Some(h)) => s >= median_speed && h >= median_hr,
            (Some(s), None) => s >= median_speed,
            (None, Some(h)) => h >= median_hr,
            (None, None) => true,
        };

        if is_work {
            work_count += 1;
        }
    }

    work_count.min(intervals.len())
}

/// Calculate the median of a slice of f64 values.
pub fn calculate_median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

/// Extract TSS from a JSON object with fallback field names.
pub fn extract_exact_tss(object: &serde_json::Map<String, Value>) -> Option<f64> {
    [
        "tss",
        "icu_training_load",
        "training_load",
        "icuTrainingLoad",
    ]
    .iter()
    .find_map(|key| {
        object
            .get(*key)
            .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
    })
}

/// Extract a numeric value from a JSON object by key.
pub fn interval_number(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
}

/// Get a numeric value from a JSON map, trying multiple keys.
pub fn numeric_value(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
}

/// Look up stream series data by trying multiple key names.
pub fn stream_series<'a>(streams: Option<&'a Value>, keys: &[&str]) -> Option<&'a Vec<Value>> {
    let object = streams?.as_object()?;
    keys.iter().find_map(|key| object.get(*key)?.as_array())
}

/// Average a sub-slice of numeric values.
pub fn average_stream_slice(values: &[Value], start_index: usize, end_index: usize) -> Option<f64> {
    if start_index >= end_index || start_index >= values.len() {
        return None;
    }

    let upper_bound = end_index.min(values.len());
    let numeric = values[start_index..upper_bound]
        .iter()
        .filter_map(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
        .collect::<Vec<_>>();

    if numeric.is_empty() {
        None
    } else {
        Some(numeric.iter().sum::<f64>() / numeric.len() as f64)
    }
}

/// Average an entire numeric stream by trying multiple key names.
pub fn average_numeric_stream_value(streams: Option<&Value>, keys: &[&str]) -> Option<f64> {
    let values = stream_series(streams, keys)?;
    let numeric = values
        .iter()
        .filter_map(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
        .collect::<Vec<_>>();

    if numeric.is_empty() {
        None
    } else {
        Some(numeric.iter().sum::<f64>() / numeric.len() as f64)
    }
}

/// Format speed (m/s) to pace string (min:sec /km).
pub fn format_pace_from_speed(speed_mps: f64) -> Option<String> {
    if speed_mps <= 0.0 {
        return None;
    }

    let seconds_per_km = (1000.0 / speed_mps).round() as i64;
    Some(format!(
        "{}:{:02} /km",
        seconds_per_km / 60,
        seconds_per_km % 60
    ))
}

/// Format pace per km from seconds and distance.
pub fn format_pace_per_km(seconds: i64, distance_m: f64) -> Option<String> {
    if seconds <= 0 || distance_m <= 0.0 {
        return None;
    }

    let total_seconds = (seconds as f64 / (distance_m / 1000.0)).round() as i64;
    Some(format!(
        "{}:{:02} /km",
        total_seconds / 60,
        total_seconds % 60
    ))
}

/// Determine whether intervals are power-based or pace-based.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntervalOutputKind {
    Power,
    Pace,
}

/// A derived interval output value.
pub enum IntervalOutputValue {
    Power(f64),
    Pace(f64),
}

impl IntervalOutputValue {
    pub fn kind(&self) -> IntervalOutputKind {
        match self {
            Self::Power(_) => IntervalOutputKind::Power,
            Self::Pace(_) => IntervalOutputKind::Pace,
        }
    }

    pub fn format(&self) -> String {
        match self {
            Self::Power(value) => format!("{value:.0} W"),
            Self::Pace(speed_mps) => {
                format_pace_from_speed(*speed_mps).unwrap_or_else(|| "n/a".to_string())
            }
        }
    }
}

/// Determine whether a set of intervals is power-based or pace-based.
///
/// Checks interval payloads first (non-null `average_watts`), then falls back to
/// streams presence of `watts`/`power` arrays — matching the fallback strategy in
/// `derive_interval_output`.
pub fn preferred_interval_output_kind(
    intervals: &[Value],
    streams: Option<&Value>,
) -> IntervalOutputKind {
    let power_keys = [
        "average_watts",
        "average_watts_alt",
        "average_watts_alt_acc",
        "weighted_average_watts",
    ];
    let has_power_in_intervals = intervals.iter().filter_map(|v| v.as_object()).any(|obj| {
        power_keys
            .iter()
            .any(|k| obj.get(*k).and_then(|v| v.as_f64()).is_some())
    });

    if has_power_in_intervals {
        return IntervalOutputKind::Power;
    }

    let has_power_in_streams = stream_series(streams, &["watts", "power"]).is_some();
    if has_power_in_streams {
        return IntervalOutputKind::Power;
    }

    IntervalOutputKind::Pace
}

/// Derive the best available output value for an interval (power or pace).
pub fn derive_interval_output(
    interval: &serde_json::Map<String, Value>,
    streams: Option<&Value>,
    output_kind: IntervalOutputKind,
) -> Option<IntervalOutputValue> {
    match output_kind {
        IntervalOutputKind::Power => {
            let power = [
                "average_watts",
                "average_watts_alt",
                "average_watts_alt_acc",
                "weighted_average_watts",
            ]
            .iter()
            .find_map(|key| interval_number(interval, key))
            .or_else(|| {
                let start_index = interval.get("start_index").and_then(Value::as_u64)? as usize;
                let end_index = interval.get("end_index").and_then(Value::as_u64)? as usize;
                let watts_stream = stream_series(streams, &["watts", "power"])?;
                average_stream_slice(watts_stream, start_index, end_index)
            });
            power.map(IntervalOutputValue::Power)
        }
        IntervalOutputKind::Pace => {
            let speed = numeric_value(interval, "average_speed")
                .filter(|speed| *speed > 0.0)
                .or_else(|| {
                    let start_index = interval.get("start_index").and_then(Value::as_u64)? as usize;
                    let end_index = interval.get("end_index").and_then(Value::as_u64)? as usize;
                    let speed_stream = stream_series(streams, &["velocity_smooth", "pace"])?;
                    average_stream_slice(speed_stream, start_index, end_index)
                        .filter(|speed| *speed > 0.0)
                });
            speed.map(IntervalOutputValue::Pace)
        }
    }
}

/// Determine the quality output label from workout detail and streams.
pub fn quality_output_finding(
    workout_detail: Option<&Value>,
    streams: Option<&Value>,
) -> Option<String> {
    let detail = workout_detail.and_then(Value::as_object);

    if let Some(power) = detail.and_then(|obj| numeric_value(obj, "average_watts")) {
        return Some(format!("Average power tracked at {:.0} W.", power));
    }

    if let Some(speed) = detail
        .and_then(|obj| numeric_value(obj, "average_speed"))
        .or_else(|| average_numeric_stream_value(streams, &["velocity_smooth", "pace"]))
    {
        if let Some(pace) = format_pace_from_speed(speed) {
            return Some(format!("Average pace held at {pace}."));
        }

        return Some(format!("Average speed tracked at {:.1} km/h.", speed * 3.6));
    }

    None
}

/// Check if a workout ID is a planned workout (event).
pub fn is_planned_workout_id(id: &str) -> bool {
    id.starts_with("event:")
}

use crate::domains::interval_detection::TimeRange;
use crate::domains::interval_segment::{SegmentRole, SegmentWindow};

/// Convert upstream interval objects with safe index boundaries into
/// `SegmentWindow` values.
///
/// An upstream interval is included only when:
/// - `start_index` and `end_index` are valid non-negative integers
/// - `start < end`
/// - both indexes exist in `time_s`
/// - the type is `WORK` (or missing/unknown), or `RECOVERY`
pub fn upstream_segment_windows(intervals: &[Value], time_s: &[f64]) -> Vec<SegmentWindow> {
    intervals
        .iter()
        .filter_map(|interval| {
            let obj = interval.as_object()?;
            let start_idx = obj.get("start_index")?.as_u64()? as usize;
            let end_idx = obj.get("end_index")?.as_u64()? as usize;

            if start_idx >= end_idx {
                return None;
            }
            if end_idx >= time_s.len() {
                return None;
            }

            let start = time_s[start_idx];
            let end = time_s[end_idx];
            if !start.is_finite() || !end.is_finite() {
                return None;
            }

            let role = match obj.get("type").and_then(Value::as_str) {
                Some("RECOVERY") => SegmentRole::Recovery,
                _ => SegmentRole::Work,
            };

            Some(SegmentWindow {
                role,
                range: TimeRange { start, end },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn count_work_intervals_empty() {
        assert_eq!(count_work_intervals(&[]), 0);
    }

    #[test]
    fn count_work_intervals_few_samples() {
        let intervals = vec![json!({"average_speed": 5.0}), json!({"average_speed": 6.0})];
        assert_eq!(count_work_intervals(&intervals), 2);
    }

    #[test]
    fn count_work_intervals_with_data() {
        let intervals = vec![
            json!({"average_speed": 3.0, "average_heartrate": 120.0}),
            json!({"average_speed": 4.0, "average_heartrate": 130.0}),
            json!({"average_speed": 5.0, "average_heartrate": 140.0}),
            json!({"average_speed": 6.0, "average_heartrate": 150.0}),
            json!({"average_speed": 7.0, "average_heartrate": 160.0}),
            json!({"average_speed": 8.0, "average_heartrate": 170.0}),
        ];
        let work = count_work_intervals(&intervals);
        assert!(work > 0 && work < intervals.len());
    }

    #[test]
    fn calculate_median_odd() {
        let mut values = vec![1.0, 3.0, 5.0];
        assert_eq!(calculate_median(&mut values), 3.0);
    }

    #[test]
    fn calculate_median_even() {
        let mut values = vec![1.0, 2.0, 3.0, 4.0];
        assert_eq!(calculate_median(&mut values), 2.5);
    }

    #[test]
    fn calculate_median_empty() {
        let mut values = vec![];
        assert_eq!(calculate_median(&mut values), 0.0);
    }

    #[test]
    fn extract_exact_tss_priority() {
        let mut obj = serde_json::Map::new();
        obj.insert("tss".into(), json!(100.0));
        obj.insert("icu_training_load".into(), json!(200.0));
        assert_eq!(extract_exact_tss(&obj), Some(100.0));
    }

    #[test]
    fn extract_exact_tss_fallback() {
        let mut obj = serde_json::Map::new();
        obj.insert("icu_training_load".into(), json!(150.0));
        assert_eq!(extract_exact_tss(&obj), Some(150.0));
    }

    #[test]
    fn interval_number_f64() {
        let mut obj = serde_json::Map::new();
        obj.insert("average_speed".into(), json!(5.5));
        assert_eq!(interval_number(&obj, "average_speed"), Some(5.5));
    }

    #[test]
    fn interval_number_i64() {
        let mut obj = serde_json::Map::new();
        obj.insert("moving_time".into(), json!(120));
        assert_eq!(interval_number(&obj, "moving_time"), Some(120.0));
    }

    #[test]
    fn stream_series_found() {
        let streams = json!({"velocity_smooth": [1.0, 2.0, 3.0]});
        let result = stream_series(Some(&streams), &["velocity_smooth"]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn stream_series_not_found() {
        let streams = json!({"heart_rate": [70, 80]});
        let result = stream_series(Some(&streams), &["velocity_smooth"]);
        assert!(result.is_none());
    }

    #[test]
    fn average_stream_slice_basic() {
        let values = vec![json!(1.0), json!(2.0), json!(3.0), json!(4.0)];
        assert_eq!(average_stream_slice(&values, 1, 3), Some(2.5));
    }

    #[test]
    fn average_stream_slice_out_of_bounds() {
        let values = vec![json!(1.0)];
        assert_eq!(average_stream_slice(&values, 5, 10), None);
    }

    #[test]
    fn average_numeric_stream_value_basic() {
        let streams = json!({"watts": [100, 150, 200]});
        assert_eq!(
            average_numeric_stream_value(Some(&streams), &["watts"]),
            Some(150.0)
        );
    }

    #[test]
    fn format_pace_from_speed_zero() {
        assert_eq!(format_pace_from_speed(0.0), None);
    }

    #[test]
    fn format_pace_from_speed_negative() {
        assert_eq!(format_pace_from_speed(-1.0), None);
    }

    #[test]
    fn format_pace_from_speed_5_mps() {
        // 5 m/s = 200s/km = 3:20 /km
        assert_eq!(format_pace_from_speed(5.0), Some("3:20 /km".to_string()));
    }

    #[test]
    fn format_pace_per_km_zero() {
        assert_eq!(format_pace_per_km(0, 1000.0), None);
    }

    #[test]
    fn format_pace_per_km_basic() {
        // 300 seconds over 5000 meters = 60 seconds per km = 1:00 /km
        assert_eq!(
            format_pace_per_km(300, 5000.0),
            Some("1:00 /km".to_string())
        );
    }

    #[test]
    fn interval_output_value_power() {
        let val = IntervalOutputValue::Power(250.0);
        assert_eq!(val.kind(), IntervalOutputKind::Power);
        assert_eq!(val.format(), "250 W");
    }

    #[test]
    fn interval_output_value_pace() {
        let val = IntervalOutputValue::Pace(5.0);
        assert_eq!(val.kind(), IntervalOutputKind::Pace);
        assert_eq!(val.format(), "3:20 /km");
    }

    #[test]
    fn preferred_interval_output_kind_power() {
        let intervals = vec![
            json!({"average_watts": 200.0}),
            json!({"average_watts": 250.0}),
        ];
        assert_eq!(
            preferred_interval_output_kind(&intervals, None),
            IntervalOutputKind::Power
        );
    }

    #[test]
    fn preferred_interval_output_kind_pace() {
        let intervals = vec![json!({"average_speed": 5.0}), json!({"average_speed": 6.0})];
        assert_eq!(
            preferred_interval_output_kind(&intervals, None),
            IntervalOutputKind::Pace
        );
    }

    #[test]
    fn derive_interval_output_power() {
        let mut interval = serde_json::Map::new();
        interval.insert("average_watts".into(), json!(250.0));
        let result = derive_interval_output(&interval, None, IntervalOutputKind::Power);
        assert!(matches!(result, Some(IntervalOutputValue::Power(250.0))));
    }

    #[test]
    fn derive_interval_output_pace() {
        let mut interval = serde_json::Map::new();
        interval.insert("average_speed".into(), json!(5.0));
        let result = derive_interval_output(&interval, None, IntervalOutputKind::Pace);
        assert!(matches!(result, Some(IntervalOutputValue::Pace(5.0))));
    }

    #[test]
    fn quality_output_finding_power() {
        let detail = json!({"average_watts": 250.0});
        let result = quality_output_finding(Some(&detail), None);
        assert_eq!(result, Some("Average power tracked at 250 W.".to_string()));
    }

    #[test]
    fn quality_output_finding_pace() {
        let detail = json!({"average_speed": 5.0});
        let result = quality_output_finding(Some(&detail), None);
        assert_eq!(result, Some("Average pace held at 3:20 /km.".to_string()));
    }

    #[test]
    fn quality_output_finding_none() {
        let result = quality_output_finding(None, None);
        assert!(result.is_none());
    }

    #[test]
    fn is_planned_workout_id_true() {
        assert!(is_planned_workout_id("event:12345"));
    }

    #[test]
    fn is_planned_workout_id_false() {
        assert!(!is_planned_workout_id("activity:12345"));
    }

    // ── upstream_segment_windows ───────────────────────────────────────

    #[test]
    fn upstream_indexes_become_half_open_time_ranges() {
        let intervals = vec![json!({
            "type": "WORK",
            "start_index": 2,
            "end_index": 5
        })];
        let windows = upstream_segment_windows(
            &intervals,
            &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        );
        assert_eq!(
            windows,
            vec![SegmentWindow {
                role: SegmentRole::Work,
                range: TimeRange { start: 2.0, end: 5.0 },
            }]
        );
    }

    #[test]
    fn upstream_interval_without_safe_end_boundary_is_skipped() {
        let intervals = vec![json!({"start_index": 2, "end_index": 6})];
        assert!(upstream_segment_windows(&intervals, &[0.0, 1.0, 2.0]).is_empty());
    }

    #[test]
    fn upstream_recovery_type_is_mapped_to_recovery_role() {
        let intervals = vec![json!({
            "type": "RECOVERY",
            "start_index": 0,
            "end_index": 3
        })];
        let windows = upstream_segment_windows(&intervals, &[0.0, 1.0, 2.0, 3.0]);
        assert_eq!(windows[0].role, SegmentRole::Recovery);
    }
}
