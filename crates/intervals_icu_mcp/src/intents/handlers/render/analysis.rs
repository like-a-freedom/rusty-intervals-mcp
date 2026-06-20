use chrono::NaiveDate;
use serde_json::Value;

use crate::domains::coach::{
    DecouplingMetrics, EspeDerivedMetrics, EspePowerAnchors, FitnessMetrics, HeatMetrics,
    NdliMetrics, WdrMetrics,
};
use crate::intents::ContentBlock;

pub(crate) fn build_load_management_text(
    metrics: Option<&crate::domains::coach::LoadManagementMetrics>,
) -> String {
    let mut lines = vec![String::from("Load Context")];

    let Some(metrics) = metrics else {
        lines.push(
            "  Load-management context unavailable because the lookback history is too short."
                .to_string(),
        );
        return lines.join("\n");
    };

    if let Some(acwr) = &metrics.acwr {
        lines.push(format!(
            "  ACWR: {:.2} ({}) — acute {:.1}, chronic {:.1}",
            acwr.ratio, acwr.state, acwr.acute_load, acwr.chronic_load
        ));
    }

    if let Some(monotony) = metrics.monotony {
        let note = if monotony > 1.5 { " high" } else { "" };
        lines.push(format!(
            "  Monotony: {:.2} (higher = samey;{note} >1.5 = concerning)",
            monotony
        ));
    }

    if let Some(strain) = metrics.strain {
        lines.push(format!("  Strain: {:.0} (total load × monotony)", strain));
    }

    if let Some(stress_tolerance) = metrics.stress_tolerance {
        lines.push(format!(
            "  Stress Tolerance: {:.2} (higher = more resilient)",
            stress_tolerance
        ));
    }

    if let Some(fatigue_index) = metrics.fatigue_index {
        lines.push(format!(
            "  Fatigue Index: {:.2} (higher = more fatigue accumulated)",
            fatigue_index
        ));
    }

    if let Some(durability_index) = metrics.durability_index {
        lines.push(format!(
            "  Durability Index: {:.3} (lower = decay risk)",
            durability_index
        ));
    }

    if lines.len() == 1 {
        lines.push(
            "  Load-management context unavailable because no deterministic load signal was found."
                .to_string(),
        );
    }

    lines.join("\n")
}

pub(crate) fn parse_activity_date(value: &str) -> Option<NaiveDate> {
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|dt| dt.date())
        .or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
}

pub(crate) fn requested_metrics(input: &Value) -> Vec<String> {
    input
        .get("metrics")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn format_duration_hhmm(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    // For durations >= 1 hour, show H:MM:SS format for clarity
    // For durations < 1 hour, show M:SS format
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

pub(crate) fn format_duration_compact(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

pub(crate) fn is_planned_workout_id(activity_id: &str) -> bool {
    activity_id.starts_with("event:")
}

pub(crate) fn build_calendar_event_rows(
    events: &[&intervals_icu_client::Event],
) -> Vec<Vec<String>> {
    events
        .iter()
        .map(|event| {
            vec![
                event
                    .start_date_local
                    .split('T')
                    .next()
                    .unwrap_or(&event.start_date_local)
                    .to_string(),
                format!("{:?}", event.category),
                event.name.clone(),
                event
                    .description
                    .clone()
                    .unwrap_or_else(|| String::from("n/a")),
            ]
        })
        .collect()
}

pub(crate) fn count_work_intervals(intervals: &[Value]) -> usize {
    if intervals.is_empty() {
        return 0;
    }

    // Collect speed and HR data for all intervals
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

    // If we don't have enough data, fall back to counting all intervals
    if speed_data.len() < 3 && hr_data.len() < 3 {
        return intervals.len();
    }

    // Calculate median speed and HR
    let median_speed =
        calculate_median(&mut speed_data.iter().map(|(_, s)| *s).collect::<Vec<_>>());
    let median_hr = calculate_median(&mut hr_data.iter().map(|(_, h)| *h).collect::<Vec<_>>());

    // Count intervals that are above median in both speed and HR (work intervals)
    // or at least above median in speed (for HR-less intervals)
    let mut work_count = 0;

    for interval in intervals.iter().filter_map(|v| v.as_object()) {
        let speed = interval.get("average_speed").and_then(|v| v.as_f64());
        let hr = interval.get("average_heartrate").and_then(|v| v.as_f64());

        let is_work = match (speed, hr) {
            (Some(s), Some(h)) => s >= median_speed && h >= median_hr,
            (Some(s), None) => s >= median_speed,
            (None, Some(h)) => h >= median_hr,
            (None, None) => true, // No data, assume work
        };

        if is_work {
            work_count += 1;
        }
    }

    // Sanity check: work intervals should be less than total
    // If all intervals are counted as work, return total
    work_count.min(intervals.len())
}

pub(crate) fn calculate_median(values: &mut [f64]) -> f64 {
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

pub(crate) fn format_pace_per_km(seconds: i64, distance_m: f64) -> Option<String> {
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

pub(crate) fn extract_exact_tss(object: &serde_json::Map<String, Value>) -> Option<f64> {
    // Try TSS field names in priority order: prefer exact TSS, then fall back to intervals.icu naming
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

pub(crate) fn build_basic_workout_metric_rows(workout_detail: Option<&Value>) -> Vec<Vec<String>> {
    let Some(obj) = workout_detail.and_then(Value::as_object) else {
        return Vec::new();
    };

    let mut rows = Vec::new();

    if let Some(v) = obj.get("distance").and_then(Value::as_f64) {
        rows.push(vec!["Distance".into(), format!("{:.2} km", v / 1000.0)]);
    }
    if let Some(v) = obj.get("moving_time").and_then(Value::as_i64) {
        rows.push(vec!["Duration".into(), format_duration_hhmm(v)]);
    }
    if let Some(v) = obj.get("average_heartrate").and_then(Value::as_f64) {
        rows.push(vec!["Avg HR".into(), format!("{} bpm", v as u32)]);
    }
    if let Some(v) = obj.get("average_watts").and_then(Value::as_f64) {
        rows.push(vec!["Avg Power".into(), format!("{v:.0} W")]);
    }
    if let Some(v) = obj.get("total_elevation_gain").and_then(Value::as_f64) {
        rows.push(vec!["Elevation".into(), format!("{v:.0} m")]);
    }

    rows
}

pub(crate) fn build_detailed_workout_rows(workout_detail: Option<&Value>) -> Vec<Vec<String>> {
    let Some(obj) = workout_detail.and_then(Value::as_object) else {
        return Vec::new();
    };

    let mut rows = Vec::new();

    let moving_time = obj.get("moving_time").and_then(Value::as_i64).unwrap_or(0);
    let distance = obj.get("distance").and_then(Value::as_f64).unwrap_or(0.0);
    if let Some(pace) = format_pace_per_km(moving_time, distance) {
        rows.push(vec!["Avg Pace".into(), pace]);
    }

    if let Some(speed) = numeric_value(obj, "average_speed") {
        rows.push(vec!["Avg Speed".into(), format!("{:.1} km/h", speed * 3.6)]);
    }

    if let Some(cadence) = numeric_value(obj, "average_cadence") {
        rows.push(vec!["Cadence".into(), format!("{cadence:.0} spm")]);
    }

    if let Some(load) = ["icu_training_load", "training_load", "load"]
        .iter()
        .find_map(|key| numeric_value(obj, key))
    {
        rows.push(vec!["Training Load".into(), format!("{load:.1}")]);
    }

    if let Some(tss) = extract_exact_tss(obj) {
        rows.push(vec!["TSS".into(), format!("{tss:.1}")]);
    }

    if let Some(temp) = numeric_value(obj, "average_temp") {
        rows.push(vec!["Temperature".into(), format!("{temp:.1} °C")]);
    }

    rows
}

pub(crate) fn build_activity_message_rows(
    messages: &[intervals_icu_client::ActivityMessage],
) -> Vec<Vec<String>> {
    messages
        .iter()
        .filter(|message| message.deleted.is_none())
        .filter_map(|message| {
            let content = message
                .content
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())?;

            let when = message
                .created
                .as_deref()
                .map(|created| created.replace('T', " ").replace('Z', ""))
                .unwrap_or_else(|| String::from("n/a"));
            let author = message
                .name
                .clone()
                .or_else(|| message.athlete_id.clone())
                .unwrap_or_else(|| String::from("Unknown"));
            let kind = message
                .message_type
                .clone()
                .unwrap_or_else(|| String::from("TEXT"));

            Some(vec![when, author, kind, content.to_string()])
        })
        .collect()
}

pub(crate) fn interval_number(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
}

pub(crate) fn stream_series<'a>(
    streams: Option<&'a Value>,
    keys: &[&str],
) -> Option<&'a Vec<Value>> {
    let object = streams?.as_object()?;
    keys.iter().find_map(|key| object.get(*key)?.as_array())
}

pub(crate) fn average_stream_slice(
    values: &[Value],
    start_index: usize,
    end_index: usize,
) -> Option<f64> {
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

pub(crate) fn format_pace_from_speed(speed_mps: f64) -> Option<String> {
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

pub(crate) fn average_numeric_stream_value(streams: Option<&Value>, keys: &[&str]) -> Option<f64> {
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

pub(crate) fn quality_output_finding(
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntervalOutputKind {
    Power,
    Pace,
}

pub(crate) enum IntervalOutputValue {
    Power(f64),
    Pace(f64),
}

impl IntervalOutputValue {
    pub(crate) fn kind(&self) -> IntervalOutputKind {
        match self {
            Self::Power(_) => IntervalOutputKind::Power,
            Self::Pace(_) => IntervalOutputKind::Pace,
        }
    }

    pub(crate) fn format(&self) -> String {
        match self {
            Self::Power(value) => format!("{value:.0} W"),
            Self::Pace(speed_mps) => {
                format_pace_from_speed(*speed_mps).unwrap_or_else(|| "n/a".to_string())
            }
        }
    }
}

pub(crate) fn derive_interval_output(
    interval: &serde_json::Map<String, Value>,
    streams: Option<&Value>,
) -> Option<IntervalOutputValue> {
    [
        "average_watts",
        "average_watts_alt",
        "average_watts_alt_acc",
        "weighted_average_watts",
    ]
    .iter()
    .find_map(|key| interval_number(interval, key))
    .map(IntervalOutputValue::Power)
    .or_else(|| {
        let start_index = interval.get("start_index").and_then(Value::as_u64)? as usize;
        let end_index = interval.get("end_index").and_then(Value::as_u64)? as usize;
        let watts_stream = stream_series(streams, &["watts", "power"])?;
        average_stream_slice(watts_stream, start_index, end_index).map(IntervalOutputValue::Power)
    })
    .or_else(|| {
        interval_number(interval, "average_speed")
            .filter(|speed| *speed > 0.0)
            .map(IntervalOutputValue::Pace)
    })
    .or_else(|| {
        let start_index = interval.get("start_index").and_then(Value::as_u64)? as usize;
        let end_index = interval.get("end_index").and_then(Value::as_u64)? as usize;
        let speed_stream = stream_series(streams, &["velocity_smooth", "pace"])?;
        average_stream_slice(speed_stream, start_index, end_index)
            .filter(|speed| *speed > 0.0)
            .map(IntervalOutputValue::Pace)
    })
}

pub(crate) fn preferred_interval_output_kind(
    intervals: &[Value],
    streams: Option<&Value>,
) -> IntervalOutputKind {
    intervals
        .iter()
        .filter_map(Value::as_object)
        .find_map(|interval| derive_interval_output(interval, streams).map(|value| value.kind()))
        .unwrap_or(IntervalOutputKind::Power)
}

pub(crate) fn build_interval_analysis_rows(
    intervals: &[Value],
    streams: Option<&Value>,
    output_kind: IntervalOutputKind,
) -> Vec<Vec<String>> {
    intervals
        .iter()
        .enumerate()
        .filter_map(|(i, interval)| {
            let obj = interval.as_object()?;
            let duration = obj.get("moving_time").and_then(Value::as_i64).unwrap_or(0);
            let avg_hr = interval_number(obj, "average_heartrate")
                .map(|value| format!("{value:.0} bpm"))
                .unwrap_or_else(|| "n/a".to_string());
            let avg_output = derive_interval_output(obj, streams)
                .filter(|value| value.kind() == output_kind)
                .map(|value| value.format())
                .unwrap_or_else(|| "n/a".to_string());

            Some(vec![
                (i + 1).to_string(),
                format!("{}:{:02}", duration / 60, duration % 60),
                avg_hr,
                avg_output,
            ])
        })
        .collect()
}

pub(crate) fn build_period_summary_rows(
    _activity_count: usize,
    period_snapshot: &crate::engines::coach_metrics::TrendSnapshot,
    weekly_hrs: f64,
) -> Vec<Vec<String>> {
    vec![
        vec![
            "Total Time".into(),
            format_duration_hhmm(period_snapshot.total_time_secs),
        ],
        vec![
            "Distance".into(),
            format!("{:.1} km", period_snapshot.total_distance_m / 1000.0),
        ],
        vec![
            "Elevation".into(),
            format!("{:.0} m", period_snapshot.total_elevation_m),
        ],
        vec!["Weekly Avg".into(), format!("{weekly_hrs:.1} hrs")],
    ]
}

pub(crate) fn build_requested_single_metric_rows(
    workout_detail: Option<&serde_json::Map<String, Value>>,
    requested: &[String],
) -> Vec<Vec<String>> {
    let mut rows = Vec::new();

    for metric in requested {
        let (value, status) = match (metric.as_str(), workout_detail) {
            ("time", Some(detail)) => detail
                .get("moving_time")
                .and_then(Value::as_i64)
                .map(|seconds| (format_duration_hhmm(seconds), "available".to_string()))
                .unwrap_or_else(|| ("n/a".to_string(), "unavailable".to_string())),
            ("distance", Some(detail)) => detail
                .get("distance")
                .and_then(Value::as_f64)
                .map(|distance| {
                    (
                        format!("{:.2} km", distance / 1000.0),
                        "available".to_string(),
                    )
                })
                .unwrap_or_else(|| ("n/a".to_string(), "unavailable".to_string())),
            ("vertical", Some(detail)) => detail
                .get("total_elevation_gain")
                .and_then(Value::as_f64)
                .map(|elevation| (format!("{:.0} m", elevation), "available".to_string()))
                .unwrap_or_else(|| ("n/a".to_string(), "unavailable".to_string())),
            ("hr", Some(detail)) => detail
                .get("average_heartrate")
                .and_then(Value::as_f64)
                .map(|hr| (format!("{:.0} bpm", hr), "available".to_string()))
                .unwrap_or_else(|| ("n/a".to_string(), "unavailable".to_string())),
            ("pace", Some(detail)) => {
                let seconds = detail
                    .get("moving_time")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let distance = detail
                    .get("distance")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                format_pace_per_km(seconds, distance)
                    .map(|pace| (pace, "available".to_string()))
                    .unwrap_or_else(|| ("n/a".to_string(), "unavailable".to_string()))
            }
            ("tss", Some(detail)) => extract_exact_tss(detail)
                .map(|tss| (format!("{:.1}", tss), "available".to_string()))
                .unwrap_or_else(|| ("n/a".to_string(), "unavailable".to_string())),
            (_, Some(_)) => ("n/a".to_string(), "unsupported".to_string()),
            (_, None) => ("n/a".to_string(), "unavailable".to_string()),
        };

        rows.push(vec![metric.to_uppercase(), value, status]);
    }

    rows
}

pub(crate) fn build_requested_period_metric_rows(
    requested: &[String],
    period: &[&intervals_icu_client::ActivitySummary],
    period_snapshot: &crate::engines::coach_metrics::TrendSnapshot,
    details: &std::collections::HashMap<String, Value>,
) -> Vec<Vec<String>> {
    let weighted_avg_hr = || {
        let mut weighted_total = 0.0;
        let mut total_seconds = 0.0;

        for activity in period {
            let Some(detail) = details.get(&activity.id).and_then(Value::as_object) else {
                continue;
            };
            let Some(seconds) = detail.get("moving_time").and_then(Value::as_i64) else {
                continue;
            };
            let Some(hr) = detail.get("average_heartrate").and_then(Value::as_f64) else {
                continue;
            };

            weighted_total += hr * seconds as f64;
            total_seconds += seconds as f64;
        }

        if total_seconds > 0.0 {
            Some(weighted_total / total_seconds)
        } else {
            None
        }
    };

    let exact_period_tss = || {
        let mut total_tss = 0.0;

        for activity in period {
            let detail = details.get(&activity.id).and_then(Value::as_object)?;
            total_tss += extract_exact_tss(detail)?;
        }

        Some(total_tss)
    };

    let mut rows = Vec::new();
    for metric in requested {
        let (value, status) = match metric.as_str() {
            "time" => (
                format_duration_hhmm(period_snapshot.total_time_secs),
                "available".to_string(),
            ),
            "distance" => (
                format!("{:.1} km", period_snapshot.total_distance_m / 1000.0),
                "available".to_string(),
            ),
            "vertical" => (
                format!("{:.0} m", period_snapshot.total_elevation_m),
                "available".to_string(),
            ),
            "hr" => weighted_avg_hr()
                .map(|hr| (format!("{:.0} bpm", hr), "available".to_string()))
                .unwrap_or_else(|| ("n/a".to_string(), "unavailable".to_string())),
            "pace" => format_pace_per_km(
                period_snapshot.total_time_secs,
                period_snapshot.total_distance_m,
            )
            .map(|pace| (pace, "available".to_string()))
            .unwrap_or_else(|| ("n/a".to_string(), "unavailable".to_string())),
            "tss" => exact_period_tss()
                .map(|tss| (format!("{:.1}", tss), "available".to_string()))
                .unwrap_or_else(|| ("n/a".to_string(), "unavailable".to_string())),
            _ => ("n/a".to_string(), "unsupported".to_string()),
        };

        rows.push(vec![metric.to_uppercase(), value, status]);
    }

    rows
}

pub(crate) fn build_zone_distribution_rows(
    zones: &serde_json::Map<String, Value>,
) -> Vec<Vec<String>> {
    let total_time: i64 = zones.values().filter_map(Value::as_i64).sum();

    zones
        .iter()
        .filter_map(|(zone, time)| {
            let time_val = time.as_i64()?;
            let pct = if total_time > 0 {
                time_val as f64 / total_time as f64 * 100.0
            } else {
                0.0
            };

            Some(vec![
                format!("Z{}", zone.replace("z", "")),
                format!("{}:{:02}", time_val / 60, time_val % 60),
                format!("{pct:.0}%"),
            ])
        })
        .collect()
}

pub(crate) fn numeric_value(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
}

pub(crate) fn format_histogram_number(value: f64) -> String {
    if (value - value.round()).abs() < 1e-6 {
        format!("{:.0}", value)
    } else {
        format!("{value:.2}")
    }
}

pub(crate) fn build_range_histogram_rows(buckets: &[Value], unit: &str) -> Vec<Vec<String>> {
    buckets
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|bucket| {
            let min = numeric_value(bucket, "min")?;
            let max = numeric_value(bucket, "max")?;
            let secs = bucket
                .get("secs")
                .and_then(Value::as_i64)
                .unwrap_or_default();

            Some(vec![
                format!(
                    "{}-{} {unit}",
                    format_histogram_number(min),
                    format_histogram_number(max)
                ),
                format_duration_compact(secs),
            ])
        })
        .collect()
}

pub(crate) fn build_bucket_histogram_rows(
    buckets: &[Value],
    average_key: Option<&str>,
    start_suffix: &str,
) -> Vec<Vec<String>> {
    buckets
        .iter()
        .filter_map(Value::as_object)
        .map(|bucket| {
            let start = bucket
                .get("start")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let secs = bucket
                .get("secs")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let moving_secs = bucket
                .get("movingSecs")
                .and_then(Value::as_i64)
                .unwrap_or(secs);
            let avg = average_key
                .and_then(|key| {
                    bucket.get(key).and_then(|value| {
                        value.as_f64().or_else(|| value.as_i64().map(|n| n as f64))
                    })
                })
                .map(|value| format!("{value:.0}"))
                .unwrap_or_else(|| "n/a".to_string());

            let bucket_label = if start_suffix.is_empty() {
                start.to_string()
            } else {
                format!("{start} {start_suffix}")
            };

            vec![
                bucket_label,
                format_duration_compact(secs),
                format_duration_compact(moving_secs),
                avg,
            ]
        })
        .collect()
}

pub(crate) fn append_histogram_section(
    content: &mut Vec<ContentBlock>,
    title: &str,
    payload: Option<&Value>,
    average_key: Option<&str>,
    start_suffix: &str,
    range_unit: &str,
) {
    let Some(payload) = payload else {
        return;
    };

    if let Some(zones) = payload
        .as_object()
        .and_then(|obj| obj.get("zones"))
        .and_then(Value::as_object)
    {
        let zone_rows = build_zone_distribution_rows(zones);
        if !zone_rows.is_empty() {
            content.push(ContentBlock::markdown(title.to_string()));
            content.push(ContentBlock::table(
                vec!["Zone".into(), "Time".into(), "%".into()],
                zone_rows,
            ));
        }
        return;
    }

    if let Some(buckets) = payload.as_array() {
        let range_rows = build_range_histogram_rows(buckets, range_unit);
        if !range_rows.is_empty() {
            content.push(ContentBlock::markdown(title.to_string()));
            content.push(ContentBlock::table(
                vec!["Range".into(), "Time".into()],
                range_rows,
            ));
            return;
        }

        let rows = build_bucket_histogram_rows(buckets, average_key, start_suffix);
        if !rows.is_empty() {
            content.push(ContentBlock::markdown(title.to_string()));
            content.push(ContentBlock::table(
                vec![
                    "Bucket Start".into(),
                    "Time".into(),
                    "Moving".into(),
                    "Avg".into(),
                ],
                rows,
            ));
        }
    }
}

pub(crate) fn best_efforts_array(best_efforts: &Value) -> Option<&Vec<Value>> {
    best_efforts
        .as_array()
        .or_else(|| best_efforts.get("best_efforts").and_then(Value::as_array))
        .or_else(|| best_efforts.get("efforts").and_then(Value::as_array))
}

pub(crate) fn format_best_effort_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

pub(crate) fn format_best_effort_average(
    best_efforts: &Value,
    effort: &serde_json::Map<String, Value>,
) -> Option<String> {
    // Priority 1: Power (if available)
    if let Some(power) = effort.get("watts").and_then(Value::as_f64) {
        return Some(format!("{power:.0} W"));
    }

    // Priority 2: Pace/Speed from average field with stream type detection
    let average = effort
        .get("average")
        .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|n| n as f64)))
        .or_else(|| {
            effort
                .get("value")
                .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|n| n as f64)))
        });

    if let Some(avg) = average {
        let stream = best_efforts.get("stream").and_then(Value::as_str);
        match stream {
            Some("watts" | "power") => return Some(format!("{avg:.1} W")),
            Some("speed" | "velocity" | "pace") if avg > 0.0 => {
                // Speed in m/s, convert to pace per km
                let secs_per_km = (1000.0 / avg).round() as i64;
                return Some(format!("{}:{:02} /km", secs_per_km / 60, secs_per_km % 60));
            }
            _ => {}
        }
    }

    // Priority 3: Check for speed/pace directly in effort object
    if let Some(speed) = effort
        .get("speed")
        .or_else(|| effort.get("velocity"))
        .and_then(Value::as_f64)
        && speed > 0.0
    {
        let secs_per_km = (1000.0 / speed).round() as i64;
        return Some(format!("{}:{:02} /km", secs_per_km / 60, secs_per_km % 60));
    }

    // Priority 4: Heart rate (fallback)
    if let Some(hr) = effort.get("heartrate").and_then(Value::as_f64) {
        return Some(format!("{hr:.0} bpm"));
    }

    // Priority 5: Generic average value
    average.map(|avg| format!("{avg:.2}"))
}

pub(crate) fn append_best_efforts_section(content: &mut Vec<ContentBlock>, best_efforts: &Value) {
    let Some(arr) = best_efforts_array(best_efforts) else {
        return;
    };
    if arr.is_empty() {
        return;
    }

    content.push(ContentBlock::markdown("Best Efforts".to_string()));

    let has_legacy_hr = arr
        .iter()
        .filter_map(Value::as_object)
        .any(|obj| obj.get("heartrate").is_some());

    if has_legacy_hr {
        let mut best_rows = vec![vec!["Duration".into(), "Power".into(), "HR".into()]];
        for effort in arr.iter().take(5) {
            if let Some(obj) = effort.as_object() {
                let secs = obj
                    .get("seconds")
                    .or_else(|| obj.get("duration"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let power = obj.get("watts").and_then(|x| x.as_f64()).unwrap_or(0.0);
                let hr = obj.get("heartrate").and_then(|x| x.as_f64()).unwrap_or(0.0);

                best_rows.push(vec![
                    format_best_effort_duration(secs),
                    format!("{power:.0} W"),
                    format!("{hr:.0} bpm"),
                ]);
            }
        }
        content.push(ContentBlock::table(
            best_rows[0].clone(),
            best_rows[1..].to_vec(),
        ));
        return;
    }

    let mut best_rows = vec![vec!["Duration".into(), "Average".into()]];
    for effort in arr.iter().take(8) {
        if let Some(obj) = effort.as_object() {
            let secs = obj
                .get("seconds")
                .or_else(|| obj.get("duration"))
                .and_then(Value::as_i64)
                .unwrap_or(0);

            if let Some(avg) = format_best_effort_average(best_efforts, obj) {
                best_rows.push(vec![format_best_effort_duration(secs), avg]);
            }
        }
    }

    if best_rows.len() > 1 {
        content.push(ContentBlock::table(
            best_rows[0].clone(),
            best_rows[1..].to_vec(),
        ));
    }
}

pub(crate) fn append_stream_insights(content: &mut Vec<ContentBlock>, streams: Option<&Value>) {
    let Some(streams) = streams.and_then(Value::as_object) else {
        content.push(ContentBlock::markdown(
            "Stream Insights\n  Stream data requested but unavailable.".to_string(),
        ));
        return;
    };

    pub(crate) fn stream_priority(name: &str) -> usize {
        match name {
            "heartrate" | "hr" | "heart_rate" => 0,
            "watts" | "power" => 1,
            "velocity_smooth" | "pace" => 2,
            "cadence" => 3,
            "distance" => 4,
            "altitude" => 5,
            "temp" => 6,
            "time" => 98,
            _ => 50,
        }
    }

    let mut rows = streams
        .iter()
        .filter_map(|(name, values)| {
            let values = values.as_array()?;
            let numeric = values
                .iter()
                .filter_map(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
                .collect::<Vec<_>>();
            if numeric.is_empty() {
                return None;
            }
            let min = numeric.iter().copied().fold(f64::INFINITY, f64::min);
            let max = numeric.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            Some(vec![
                name.clone(),
                numeric.len().to_string(),
                format!("{min:.1}"),
                format!("{max:.1}"),
            ])
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| {
        let left_name = left.first().map(String::as_str).unwrap_or_default();
        let right_name = right.first().map(String::as_str).unwrap_or_default();

        stream_priority(left_name)
            .cmp(&stream_priority(right_name))
            .then_with(|| left_name.cmp(right_name))
    });

    if rows.is_empty() {
        content.push(ContentBlock::markdown(
            "Stream Insights\n  Stream data requested but unavailable.".to_string(),
        ));
        return;
    }

    content.push(ContentBlock::markdown("Stream Insights".to_string()));
    content.push(ContentBlock::table(
        vec!["Stream".into(), "Points".into(), "Min".into(), "Max".into()],
        rows,
    ));
}

pub(crate) fn render_espe_section(
    anchors: &Option<EspePowerAnchors>,
    derived: &Option<EspeDerivedMetrics>,
) -> Option<String> {
    let anchors = anchors.as_ref()?;
    if !anchors.supported {
        return None;
    }
    let mut lines = vec!["Power-Duration Anchors".to_string()];
    if let Some(eftp) = anchors.eftp {
        lines.push(format!(
            "  eFTP: {:.0} W (functional threshold power)",
            eftp
        ));
    }
    if let Some(w_prime) = anchors.w_prime {
        lines.push(format!("  W′: {:.0} J (anaerobic work capacity)", w_prime));
    }
    if let Some(p_max) = anchors.p_max {
        lines.push(format!(
            "  pMax: {:.0} W (maximum neuromuscular power)",
            p_max
        ));
    }
    if let Some(derived) = derived {
        if let Some(glycolytic_bias) = derived.glycolytic_bias {
            lines.push(format!(
                "  Glycolytic Bias (pMax/eFTP): {:.2}",
                glycolytic_bias
            ));
        }
        if let Some(val) = derived.aerobic_durability {
            lines.push(format!("  Aerobic Durability (P60/P5): {:.2}", val));
        }
        if let Some(val) = derived.durability_gradient {
            lines.push(format!("  Durability Gradient (P60/P20): {:.2}", val));
        }
        if let Some(val) = derived.balance_score {
            lines.push(format!(
                "  Balance Score: {:.2} (deviation from ideal P1/P20 ratio)",
                val
            ));
        }
        if let Some(val) = derived.vo2_reserve_ratio {
            lines.push(format!("  VO2 Reserve Ratio (P5/eFTP): {:.2}", val));
        }
    }
    Some(lines.join("\n"))
}

pub(crate) fn render_wdrm_section(wdrm: &Option<WdrMetrics>) -> Option<String> {
    let wdrm = wdrm.as_ref()?;
    if !wdrm.supported {
        return None;
    }
    let mut lines = vec!["W′ Depletion (WDRM)".to_string()];
    if let Some(max_depletion) = wdrm.max_wbal_depletion {
        lines.push(format!(
            "  Max W′ Depletion: {:.0} J (peak W′ used)",
            max_depletion
        ));
    }
    if let Some(depletion_pct) = wdrm.depletion_pct {
        lines.push(format!(
            "  Depletion: {:.0}% (of W′ capacity)",
            depletion_pct * 100.0
        ));
    }
    if let Some(joules) = wdrm.joules_above_ftp {
        lines.push(format!(
            "  Joules Above FTP: {:.0} (work above threshold)",
            joules
        ));
    }
    if wdrm.sessions_with_data_7d > 0
        && let Some(mean_depletion) = wdrm.mean_depletion_pct_7d
    {
        lines.push(format!(
            "  Mean 7d Depletion: {:.0}% (avg daily W′ spend)",
            mean_depletion * 100.0
        ));
    }
    if wdrm.sessions_with_data_7d > 0 {
        lines.push(format!(
            "  High Depletion Sessions (7d): {} (sessions >80% depletion)",
            wdrm.high_depletion_sessions_7d
        ));
    }
    Some(lines.join("\n"))
}

pub(crate) fn render_isdm_section(decoupling: &Option<DecouplingMetrics>) -> Option<String> {
    let decoupling = decoupling.as_ref()?;
    let mut lines = vec!["Aerobic Decoupling (ISDM)".to_string()];
    lines.push(format!(
        "  Signed Decoupling: {:.1}% (negative = improving, positive = drifting)",
        decoupling.signed_decoupling_pct
    ));
    lines.push(format!(
        "  Absolute Decoupling: {:.1}%",
        decoupling.decoupling_pct
    ));
    lines.push(format!(
        "  Durability State: {}",
        decoupling.durability_state
    ));
    if let Some(ef1) = decoupling.efficiency_factor_first_half {
        lines.push(format!("  EF First Half: {:.3} (efficiency, early)", ef1));
    }
    if let Some(ef2) = decoupling.efficiency_factor_second_half {
        lines.push(format!("  EF Second Half: {:.3} (efficiency, late)", ef2));
    }
    if let Some(variance) = decoupling.z2_hr_variance {
        let stability = if variance < 25.0 {
            "stable"
        } else if variance < 50.0 {
            "moderate"
        } else {
            "unstable"
        };
        lines.push(format!(
            "  Z2 HR Variance: {:.1} bpm² ({})",
            variance, stability
        ));
    }
    Some(lines.join("\n"))
}

pub(crate) fn render_ndli_section(ndli: &Option<NdliMetrics>) -> Option<String> {
    let ndli = ndli.as_ref()?;
    if !ndli.supported {
        return None;
    }
    let mut lines = vec!["Neural Density Load Index (NDLI)".to_string()];
    lines.push(format!(
        "  State: {} (workload concentration risk)",
        ndli.ndli_state
    ));
    lines.push(format!(
        "  High-Intensity Days (7d): {}",
        ndli.high_intensity_days_7d
    ));
    if let Some(if_val) = ndli.mean_intensity_factor_7d {
        lines.push(format!(
            "  Mean IF: {:.3} (intensity factor, 1.0 = FTP)",
            if_val
        ));
    }
    if let Some(ef) = ndli.mean_efficiency_factor_7d {
        lines.push(format!(
            "  Mean EF: {:.3} (efficiency factor, higher = fresher)",
            ef
        ));
    }
    if let Some(vi) = ndli.mean_variability_index_7d {
        lines.push(format!(
            "  Mean VI: {:.2} (variability index, >1.1 = variable)",
            vi
        ));
    }
    Some(lines.join("\n"))
}

pub(crate) fn render_heat_section(heat: &Option<HeatMetrics>) -> Option<String> {
    let heat = heat.as_ref()?;
    if !heat.supported {
        return None;
    }
    let mut lines = vec!["Heat Stress Context".to_string()];
    lines.push(format!("  State: {}", heat.heat_state));
    if let Some(index) = heat.heat_index_7d {
        lines.push(format!("  Heat Index (7d): {:.2}", index));
    }
    if let Some(max_temp) = heat.heat_max_7d {
        lines.push(format!("  Max Temperature: {:.1} °C", max_temp));
    }
    Some(lines.join("\n"))
}

pub(crate) fn render_fitness_snapshot(fitness: &Option<FitnessMetrics>) -> Option<String> {
    let metrics = fitness.as_ref()?;
    let mut lines = vec!["Fitness Snapshot".to_string()];
    if let Some(ctl) = metrics.ctl {
        lines.push(format!("  CTL: {:.0}", ctl));
    }
    if let Some(atl) = metrics.atl {
        lines.push(format!("  ATL: {:.0}", atl));
    }
    if let Some(tsb) = metrics.tsb {
        let state = if tsb > 10.0 {
            "Fresh"
        } else if tsb < -10.0 {
            "Fatigued"
        } else {
            "Balanced"
        };
        lines.push(format!("  TSB: {:.0} ({})", tsb, state));
    }
    if let Some(rr) = metrics.ramp_rate {
        lines.push(format!("  Ramp Rate: {:+.1}/wk", rr));
    }
    Some(lines.join("\n"))
}

pub(crate) fn render_z2_stability_section(
    z2_lower: f64,
    z2_upper: f64,
    variance: Option<f64>,
) -> Option<String> {
    let variance = variance?;
    let stability = if variance < 25.0 {
        "stable"
    } else if variance < 50.0 {
        "moderate"
    } else {
        "unstable"
    };
    Some(format!(
        "Z2 HR Stability\n  Z2 Range: {:.0}–{:.0} bpm\n  HR Variance: {:.1} bpm²\n  Assessment: {}",
        z2_lower, z2_upper, variance, stability
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::coach::{
        AcwrMetrics, DecouplingMetrics, EspeDerivedMetrics, EspePowerAnchors, HeatMetrics,
        LoadManagementMetrics, NdliMetrics, WdrMetrics,
    };
    use crate::engines::coach_metrics::TrendSnapshot;
    use crate::intents::ContentBlock;
    use intervals_icu_client::{ActivityMessage, ActivitySummary, Event, EventCategory};
    use std::collections::HashMap;

    // ── build_load_management_text ────────────────────────────────────

    #[test]
    fn load_management_text_none() {
        let text = build_load_management_text(None);
        assert!(text.contains("Load Context"));
        assert!(text.contains("too short"));
    }

    #[test]
    fn load_management_text_all_fields() {
        let metrics = LoadManagementMetrics {
            acwr: Some(AcwrMetrics {
                acute_load: 300.0,
                chronic_load: 250.0,
                ratio: 1.2,
                state: "balanced".into(),
            }),
            monotony: Some(1.5),
            strain: Some(4500.0),
            stress_tolerance: Some(2.3),
            fatigue_index: Some(1.1),
            durability_index: Some(0.92),
        };
        let text = build_load_management_text(Some(&metrics));
        assert!(text.contains("ACWR"));
        assert!(text.contains("1.20"));
        assert!(text.contains("Monotony"));
        assert!(text.contains("Strain"));
        assert!(text.contains("Stress Tolerance"));
        assert!(text.contains("Fatigue Index"));
        assert!(text.contains("Durability Index"));
    }

    #[test]
    fn load_management_text_no_data() {
        let metrics = LoadManagementMetrics::default();
        let text = build_load_management_text(Some(&metrics));
        assert!(text.contains("Load Context"));
        assert!(text.contains("no deterministic load signal"));
    }

    // ── format_duration_compact ───────────────────────────────────────

    #[test]
    fn format_duration_compact_under_hour() {
        assert_eq!(format_duration_compact(90), "1:30");
        assert_eq!(format_duration_compact(59), "0:59");
        assert_eq!(format_duration_compact(0), "0:00");
    }

    #[test]
    fn format_duration_compact_over_hour() {
        assert_eq!(format_duration_compact(3661), "1:01:01");
        assert_eq!(format_duration_compact(3600), "1:00:00");
    }

    // ── build_calendar_event_rows ─────────────────────────────────────

    #[test]
    fn build_calendar_event_rows_basic() {
        let events = [
            Event {
                id: Some("1".into()),
                start_date_local: "2026-03-23T10:00:00".into(),
                name: "Morning Ride".into(),
                category: EventCategory::Workout,
                description: Some("Zone 2".into()),
                r#type: None,
            },
            Event {
                id: Some("2".into()),
                start_date_local: "2026-03-24".into(),
                name: "Rest Day".into(),
                category: EventCategory::Holiday,
                description: None,
                r#type: None,
            },
        ];
        let refs: Vec<&Event> = events.iter().collect();
        let rows = build_calendar_event_rows(&refs);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "2026-03-23");
        assert!(rows[0][1].contains("Workout"));
        assert_eq!(rows[0][2], "Morning Ride");
        assert_eq!(rows[0][3], "Zone 2");
        assert_eq!(rows[1][0], "2026-03-24");
        assert_eq!(rows[1][3], "n/a");
    }

    // ── count_work_intervals ──────────────────────────────────────────

    #[test]
    fn count_work_intervals_few_data_points() {
        let intervals: Vec<Value> = vec![
            serde_json::json!({"average_speed": 5.0}),
            serde_json::json!({"average_speed": 6.0}),
            serde_json::json!({}),
        ];
        // Fewer than 3 speed and 0 hr -> fallback to all intervals
        assert_eq!(count_work_intervals(&intervals), 3);
    }

    #[test]
    fn count_work_intervals_with_work_split() {
        let intervals: Vec<Value> = vec![
            serde_json::json!({"average_speed": 10.0, "average_heartrate": 150.0}),
            serde_json::json!({"average_speed": 12.0, "average_heartrate": 160.0}),
            serde_json::json!({"average_speed": 8.0, "average_heartrate": 140.0}),
            serde_json::json!({"average_speed": 5.0, "average_heartrate": 120.0}),
        ];
        // 4 data points each -> median speed ~9, median HR ~145
        // work: >=9 && >=145 -> 2 intervals (indices 0 and 1)
        let count = count_work_intervals(&intervals);
        assert_eq!(count, 2);
    }

    #[test]
    fn count_work_intervals_no_data_assume_work() {
        let intervals: Vec<Value> = vec![
            serde_json::json!({"average_speed": 10.0, "average_heartrate": 150.0}),
            serde_json::json!({"average_speed": 8.0, "average_heartrate": 130.0}),
            serde_json::json!({"average_speed": 5.0, "average_heartrate": 120.0}),
            serde_json::json!({"average_speed": 12.0, "average_heartrate": 160.0}),
            serde_json::json!({}), // No data, assume work
        ];
        let count = count_work_intervals(&intervals);
        // 4 intervals with data, 1 without data -> min(5, 5) = 5... wait, let me think.
        // median_speed = sorted [5,8,10,12] -> (8+10)/2 = 9
        // median_hr = sorted [120,130,150,160] -> (130+150)/2 = 140
        // interval 0: s=10>=9, hr=150>=140 -> work
        // interval 1: s=8<9, hr=130<140 -> not work
        // interval 2: s=5<9, hr=120<140 -> not work
        // interval 3: s=12>=9, hr=160>=140 -> work
        // interval 4: (None, None) -> assume work
        // work_count = 3
        assert_eq!(count, 3);
    }

    #[test]
    fn count_work_intervals_hr_only() {
        let intervals: Vec<Value> = vec![
            serde_json::json!({"average_heartrate": 150.0}),
            serde_json::json!({"average_heartrate": 160.0}),
            serde_json::json!({"average_heartrate": 140.0}),
            serde_json::json!({"average_heartrate": 130.0}),
        ];
        // 0 speed, 4 hr -> median_hr = 145
        // interval 0: s=None, hr=150>=145 -> work
        // interval 1: s=None, hr=160>=145 -> work
        // interval 2: s=None, hr=140<145 -> not work
        // interval 3: s=None, hr=130<145 -> not work
        assert_eq!(count_work_intervals(&intervals), 2);
    }

    #[test]
    fn count_work_intervals_speed_only() {
        let intervals: Vec<Value> = vec![
            serde_json::json!({"average_speed": 5.0}),
            serde_json::json!({"average_speed": 8.0}),
            serde_json::json!({"average_speed": 10.0}),
            serde_json::json!({"average_speed": 12.0}),
        ];
        // 4 speed, 0 hr -> median_speed = (8+10)/2 = 9
        // interval 0: s=5<9 -> not work
        // interval 1: s=8<9 -> not work
        // interval 2: s=10>=9 -> work
        // interval 3: s=12>=9 -> work
        assert_eq!(count_work_intervals(&intervals), 2);
    }

    // ── calculate_median ──────────────────────────────────────────────

    #[test]
    fn calculate_median_empty() {
        assert_eq!(calculate_median(&mut []), 0.0);
    }

    #[test]
    fn calculate_median_odd_count() {
        assert_eq!(calculate_median(&mut [1.0, 3.0, 2.0]), 2.0);
    }

    #[test]
    fn calculate_median_even_count() {
        assert_eq!(calculate_median(&mut [1.0, 4.0, 2.0, 3.0]), 2.5);
    }

    #[test]
    fn calculate_median_single_value() {
        assert_eq!(calculate_median(&mut [42.0]), 42.0);
    }

    #[test]
    fn calculate_median_nan_handling() {
        assert_eq!(calculate_median(&mut [f64::NAN, 3.0, 1.0]), 1.0);
    }

    // ── build_detailed_workout_rows ───────────────────────────────────

    #[test]
    fn build_detailed_workout_rows_empty() {
        let rows = build_detailed_workout_rows(None);
        assert!(rows.is_empty());
    }

    #[test]
    fn build_detailed_workout_rows_pace_and_metrics() {
        let detail = serde_json::json!({
            "moving_time": 3600,
            "distance": 12000.0,
            "average_speed": 3.33,
            "average_cadence": 85.0,
            "icu_training_load": 120.5,
            "tss": 115.0,
            "average_temp": 22.5
        });
        let rows = build_detailed_workout_rows(Some(&detail));
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0][0], "Avg Pace");
        assert!(rows[1][1].contains("km/h"));
        assert!(rows[2][1].contains("spm"));
        assert!(rows[3][1].contains("120.5"));
        assert!(rows[4][1].contains("115.0"));
        assert!(rows[5][1].contains("°C"));
    }

    #[test]
    fn build_detailed_workout_rows_no_pace_when_no_movement() {
        let detail = serde_json::json!({
            "moving_time": 0,
            "distance": 0.0,
        });
        let rows = build_detailed_workout_rows(Some(&detail));
        assert!(rows.is_empty());
    }

    #[test]
    fn build_detailed_workout_rows_training_load_fallback() {
        let detail = serde_json::json!({
            "moving_time": 1800,
            "distance": 5000.0,
            "load": 95.0,
        });
        let rows = build_detailed_workout_rows(Some(&detail));
        assert!(rows.iter().any(|r| r[1].contains("95.0")));
    }

    // ── build_activity_message_rows ───────────────────────────────────

    #[test]
    fn build_activity_message_rows_basic() {
        let msgs = vec![ActivityMessage {
            id: 1,
            athlete_id: Some("42".into()),
            name: Some("Coach".into()),
            created: Some("2026-03-23T10:30:00Z".into()),
            message_type: Some("TEXT".into()),
            content: Some(" Great session!  ".into()),
            activity_id: Some("100".into()),
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        }];
        let rows = build_activity_message_rows(&msgs);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0], "2026-03-23 10:30:00");
        assert_eq!(rows[0][1], "Coach");
        assert_eq!(rows[0][2], "TEXT");
        assert_eq!(rows[0][3], "Great session!");
    }

    #[test]
    fn build_activity_message_rows_deleted_filtered() {
        let msgs = vec![ActivityMessage {
            id: 1,
            athlete_id: None,
            name: None,
            created: None,
            message_type: None,
            content: Some("Hello".into()),
            activity_id: None,
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: Some("2026-03-24".into()),
        }];
        let rows = build_activity_message_rows(&msgs);
        assert!(rows.is_empty());
    }

    #[test]
    fn build_activity_message_rows_empty_content_filtered() {
        let msgs = vec![ActivityMessage {
            id: 1,
            athlete_id: None,
            name: None,
            created: None,
            message_type: None,
            content: Some("   ".into()),
            activity_id: None,
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        }];
        let rows = build_activity_message_rows(&msgs);
        assert!(rows.is_empty());
    }

    #[test]
    fn build_activity_message_rows_missing_fields() {
        let msgs = vec![ActivityMessage {
            id: 1,
            athlete_id: Some("42".into()),
            name: None,
            created: None,
            message_type: None,
            content: Some("Hello".into()),
            activity_id: None,
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        }];
        let rows = build_activity_message_rows(&msgs);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0], "n/a");
        assert_eq!(rows[0][1], "42"); // falls back to athlete_id
        assert_eq!(rows[0][2], "TEXT"); // default type
    }

    // ── interval_number i64 branch ────────────────────────────────────

    #[test]
    fn interval_number_i64() {
        let json_val = serde_json::json!({"power": 250});
        let obj = json_val.as_object().unwrap();
        assert_eq!(interval_number(obj, "power"), Some(250.0));
    }

    // ── extract_exact_tss i64 branch ──────────────────────────────────

    #[test]
    fn extract_exact_tss_i64() {
        let mut obj = serde_json::Map::new();
        obj.insert("tss".to_string(), serde_json::json!(150));
        assert_eq!(extract_exact_tss(&obj), Some(150.0));
    }

    // ── stream_series ─────────────────────────────────────────────────

    #[test]
    fn stream_series_none() {
        assert!(stream_series(None, &["watts"]).is_none());
    }

    #[test]
    fn stream_series_found() {
        let data = serde_json::json!({"watts": [100, 200, 300]});
        let result = stream_series(Some(&data), &["watts"]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn stream_series_missing_key() {
        let data = serde_json::json!({"heartrate": [100]});
        assert!(stream_series(Some(&data), &["watts", "power"]).is_none());
    }

    #[test]
    fn stream_series_not_an_object() {
        let data = serde_json::json!([1, 2, 3]);
        assert!(stream_series(Some(&data), &["watts"]).is_none());
    }

    // ── format_pace_from_speed ────────────────────────────────────────

    #[test]
    fn format_pace_from_speed_valid() {
        let pace = format_pace_from_speed(5.0); // 5 m/s = 200 s/km = 3:20 /km
        assert_eq!(pace, Some("3:20 /km".to_string()));
    }

    #[test]
    fn format_pace_from_speed_zero() {
        assert!(format_pace_from_speed(0.0).is_none());
        assert!(format_pace_from_speed(-1.0).is_none());
    }

    // ── average_numeric_stream_value ──────────────────────────────────

    #[test]
    fn average_numeric_stream_value_valid() {
        let data = serde_json::json!({"watts": [100, 200, 300]});
        let avg = average_numeric_stream_value(Some(&data), &["watts"]);
        assert_eq!(avg, Some(200.0));
    }

    #[test]
    fn average_numeric_stream_value_empty() {
        let data = serde_json::json!({"watts": []});
        let avg = average_numeric_stream_value(Some(&data), &["watts"]);
        assert!(avg.is_none());
        assert!(average_numeric_stream_value(None, &["watts"]).is_none());
    }

    #[test]
    fn average_numeric_stream_value_missing_stream() {
        let data = serde_json::json!({"hr": [100]});
        assert!(average_numeric_stream_value(Some(&data), &["watts"]).is_none());
    }

    // ── quality_output_finding ────────────────────────────────────────

    #[test]
    fn quality_output_finding_power() {
        let detail = serde_json::json!({"average_watts": 250.0});
        let result = quality_output_finding(Some(&detail), None);
        assert_eq!(result, Some("Average power tracked at 250 W.".into()));
    }

    #[test]
    fn quality_output_finding_speed() {
        let detail = serde_json::json!({"average_speed": 5.0});
        let result = quality_output_finding(Some(&detail), None);
        // 5 m/s = 3:20 /km
        assert_eq!(result, Some("Average pace held at 3:20 /km.".into()));
    }

    #[test]
    fn quality_output_finding_speed_from_streams() {
        let streams = serde_json::json!({"velocity_smooth": [4.0, 5.0, 6.0]});
        let result = quality_output_finding(None, Some(&streams));
        // avg = 5.0 m/s -> 3:20 /km
        assert_eq!(result, Some("Average pace held at 3:20 /km.".into()));
    }

    #[test]
    fn quality_output_finding_speed_fallback_kmh() {
        let detail = serde_json::json!({"average_speed": 0.0});
        let result = quality_output_finding(Some(&detail), None);
        // average_speed = 0.0, format_pace_from_speed returns None,
        // falls to km/h format: 0.0 * 3.6 = 0.0
        assert_eq!(result, Some("Average speed tracked at 0.0 km/h.".into()));
    }

    #[test]
    fn quality_output_finding_speed_from_stream_or_else() {
        let streams = serde_json::json!({"velocity_smooth": [4.0, 5.0, 6.0]});
        // no detail, no power, no average_speed in detail -> or_else fires -> stream avg = 5.0 -> 3:20 /km
        let result = quality_output_finding(None, Some(&streams));
        assert_eq!(result, Some("Average pace held at 3:20 /km.".into()));
    }

    #[test]
    fn quality_output_finding_none() {
        assert!(quality_output_finding(None, None).is_none());
    }

    // ── IntervalOutputValue ───────────────────────────────────────────

    #[test]
    fn interval_output_kind_power() {
        let val = IntervalOutputValue::Power(250.0);
        assert_eq!(val.kind(), IntervalOutputKind::Power);
    }

    #[test]
    fn interval_output_kind_pace() {
        let val = IntervalOutputValue::Pace(5.0);
        assert_eq!(val.kind(), IntervalOutputKind::Pace);
    }

    #[test]
    fn interval_output_value_format_power() {
        let val = IntervalOutputValue::Power(250.0);
        assert_eq!(val.format(), "250 W");
    }

    #[test]
    fn interval_output_value_format_pace() {
        let val = IntervalOutputValue::Pace(5.0);
        assert_eq!(val.format(), "3:20 /km");
    }

    #[test]
    fn interval_output_value_format_pace_invalid() {
        let val = IntervalOutputValue::Pace(0.0);
        assert_eq!(val.format(), "n/a");
    }

    // ── derive_interval_output ────────────────────────────────────────

    #[test]
    fn derive_interval_output_power_from_field() {
        let obj = serde_json::json!({"average_watts": 250.0});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, None);
        assert_eq!(result.map(|v| v.format()), Some("250 W".to_string()));
    }

    #[test]
    fn derive_interval_output_power_from_alt() {
        let obj = serde_json::json!({"average_watts_alt": 230.0});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, None);
        assert_eq!(result.map(|v| v.format()), Some("230 W".to_string()));
    }

    #[test]
    fn derive_interval_output_power_from_weighted() {
        let obj = serde_json::json!({"weighted_average_watts": 240.0});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, None);
        assert_eq!(result.map(|v| v.format()), Some("240 W".to_string()));
    }

    #[test]
    fn derive_interval_output_power_from_stream() {
        let obj = serde_json::json!({"start_index": 0, "end_index": 3});
        let streams = serde_json::json!({"watts": [200, 220, 240, 260]});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, Some(&streams));
        // avg of [200, 220, 240] = 220
        assert_eq!(result.map(|v| v.format()), Some("220 W".to_string()));
    }

    #[test]
    fn derive_interval_output_pace_from_field() {
        let obj = serde_json::json!({"average_speed": 5.0});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, None);
        assert_eq!(result.map(|v| v.format()), Some("3:20 /km".to_string()));
    }

    #[test]
    fn derive_interval_output_pace_from_stream() {
        let obj = serde_json::json!({"start_index": 0, "end_index": 3});
        let streams = serde_json::json!({"velocity_smooth": [4.0, 5.0, 6.0]});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, Some(&streams));
        assert_eq!(result.map(|v| v.format()), Some("3:20 /km".to_string()));
    }

    #[test]
    fn derive_interval_output_none() {
        let obj = serde_json::json!({});
        let map = obj.as_object().unwrap();
        assert!(derive_interval_output(map, None).is_none());
    }

    // ── preferred_interval_output_kind ────────────────────────────────

    #[test]
    fn preferred_interval_output_kind_power() {
        let intervals: Vec<Value> = vec![serde_json::json!({"average_watts": 250})];
        assert_eq!(
            preferred_interval_output_kind(&intervals, None),
            IntervalOutputKind::Power
        );
    }

    #[test]
    fn preferred_interval_output_kind_pace() {
        let intervals: Vec<Value> = vec![serde_json::json!({"average_speed": 5.0})];
        assert_eq!(
            preferred_interval_output_kind(&intervals, None),
            IntervalOutputKind::Pace
        );
    }

    #[test]
    fn preferred_interval_output_kind_default() {
        let intervals: Vec<Value> = vec![serde_json::json!({})];
        assert_eq!(
            preferred_interval_output_kind(&intervals, None),
            IntervalOutputKind::Power
        );
    }

    // ── build_interval_analysis_rows ──────────────────────────────────

    #[test]
    fn build_interval_analysis_rows_power() {
        let intervals: Vec<Value> = vec![
            serde_json::json!({
                "moving_time": 300,
                "average_heartrate": 150.0,
                "average_watts": 250.0,
            }),
            serde_json::json!({
                "moving_time": 180,
                "average_heartrate": 145.0,
                "average_watts": 230.0,
            }),
        ];
        let rows = build_interval_analysis_rows(&intervals, None, IntervalOutputKind::Power);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "1");
        assert_eq!(rows[0][1], "5:00");
        assert_eq!(rows[0][2], "150 bpm");
        assert_eq!(rows[0][3], "250 W");
    }

    #[test]
    fn build_interval_analysis_rows_no_hr() {
        let intervals: Vec<Value> = vec![serde_json::json!({
            "moving_time": 120,
            "average_watts": 200.0,
        })];
        let rows = build_interval_analysis_rows(&intervals, None, IntervalOutputKind::Power);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][2], "n/a");
    }

    // ── build_period_summary_rows ─────────────────────────────────────

    #[test]
    fn build_period_summary_rows_basic() {
        let snapshot = TrendSnapshot {
            activity_count: 5,
            total_time_secs: 36000,
            total_distance_m: 50000.0,
            total_elevation_m: 800.0,
        };
        let rows = build_period_summary_rows(5, &snapshot, 10.0);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0][0], "Total Time");
        assert_eq!(rows[1][0], "Distance");
        assert_eq!(rows[2][0], "Elevation");
        assert_eq!(rows[3][0], "Weekly Avg");
        assert!(rows[1][1].contains("50.0 km"));
        assert!(rows[3][1].contains("10.0 hrs"));
    }

    // ── build_requested_single_metric_rows ────────────────────────────

    #[test]
    fn build_requested_single_metric_rows_time() {
        let detail = serde_json::json!({"moving_time": 3600});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["time".to_string()]);
        assert_eq!(rows[0][1], "1:00:00");
        assert_eq!(rows[0][2], "available");
    }

    #[test]
    fn build_requested_single_metric_rows_distance() {
        let detail = serde_json::json!({"distance": 10000.0});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["distance".to_string()]);
        assert_eq!(rows[0][1], "10.00 km");
        assert_eq!(rows[0][2], "available");
    }

    #[test]
    fn build_requested_single_metric_rows_vertical() {
        let detail = serde_json::json!({"total_elevation_gain": 350.0});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["vertical".to_string()]);
        assert_eq!(rows[0][1], "350 m");
        assert_eq!(rows[0][2], "available");
    }

    #[test]
    fn build_requested_single_metric_rows_hr() {
        let detail = serde_json::json!({"average_heartrate": 145.0});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["hr".to_string()]);
        assert_eq!(rows[0][1], "145 bpm");
        assert_eq!(rows[0][2], "available");
    }

    #[test]
    fn build_requested_single_metric_rows_pace() {
        let detail = serde_json::json!({
            "moving_time": 1800,
            "distance": 6000.0,
        });
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["pace".to_string()]);
        assert!(rows[0][1].contains("/km"));
        assert_eq!(rows[0][2], "available");
    }

    #[test]
    fn build_requested_single_metric_rows_pace_unavailable() {
        let detail = serde_json::json!({"moving_time": 0, "distance": 0.0});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["pace".to_string()]);
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unavailable");
    }

    #[test]
    fn build_requested_single_metric_rows_tss() {
        let detail = serde_json::json!({"tss": 150.0});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["tss".to_string()]);
        assert_eq!(rows[0][1], "150.0");
        assert_eq!(rows[0][2], "available");
    }

    #[test]
    fn build_requested_single_metric_rows_unsupported() {
        let detail = serde_json::json!({"foo": "bar"});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["cadence".to_string()]);
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unsupported");
    }

    #[test]
    fn build_requested_single_metric_rows_no_detail() {
        let rows = build_requested_single_metric_rows(None, &["time".to_string()]);
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unavailable");
    }

    #[test]
    fn build_requested_single_metric_rows_time_unavailable() {
        let detail = serde_json::json!({"moving_time": "not_a_number"});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["time".to_string()]);
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unavailable");
    }

    #[test]
    fn build_requested_single_metric_rows_tss_unavailable() {
        let detail = serde_json::json!({"foo": "bar"});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["tss".to_string()]);
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unavailable");
    }

    #[test]
    fn build_requested_single_metric_rows_multiple() {
        let detail = serde_json::json!({
            "moving_time": 3600,
            "distance": 15000.0,
            "total_elevation_gain": 200.0,
            "average_heartrate": 140.0,
            "tss": 180.0,
        });
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(
            obj,
            &[
                "time".to_string(),
                "distance".to_string(),
                "vertical".to_string(),
                "hr".to_string(),
                "tss".to_string(),
                "unknown".to_string(),
            ],
        );
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0][0], "TIME");
        assert_eq!(rows[1][0], "DISTANCE");
        assert_eq!(rows[2][0], "VERTICAL");
        assert_eq!(rows[3][0], "HR");
        assert_eq!(rows[4][0], "TSS");
        assert_eq!(rows[5][0], "UNKNOWN");
        assert_eq!(rows[5][2], "unsupported");
    }

    // ── build_requested_period_metric_rows ────────────────────────────

    #[test]
    fn build_requested_period_metric_rows_basic() {
        let snapshot = TrendSnapshot {
            activity_count: 3,
            total_time_secs: 18000,
            total_distance_m: 30000.0,
            total_elevation_m: 500.0,
        };
        let period: Vec<&ActivitySummary> = vec![];
        let details = HashMap::new();
        let rows = build_requested_period_metric_rows(
            &[
                "time".to_string(),
                "distance".to_string(),
                "vertical".to_string(),
            ],
            &period,
            &snapshot,
            &details,
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][1], "5:00:00");
        assert_eq!(rows[1][1], "30.0 km");
        assert_eq!(rows[2][1], "500 m");
    }

    #[test]
    fn build_requested_period_metric_rows_hr() {
        let snapshot = TrendSnapshot {
            activity_count: 2,
            total_time_secs: 7200,
            total_distance_m: 0.0,
            total_elevation_m: 0.0,
        };
        let act1 = ActivitySummary {
            id: "1".into(),
            name: None,
            start_date_local: "2026-03-23".into(),
            moving_time: None,
            elapsed_time: None,
            distance: None,
            training_load: None,
        };
        let act2 = ActivitySummary {
            id: "2".into(),
            name: None,
            start_date_local: "2026-03-24".into(),
            moving_time: None,
            elapsed_time: None,
            distance: None,
            training_load: None,
        };
        let period = vec![&act1, &act2];
        let details = HashMap::new();
        let rows =
            build_requested_period_metric_rows(&["hr".to_string()], &period, &snapshot, &details);
        // No details -> weighted_avg_hr returns None -> unavailable
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unavailable");
    }

    #[test]
    fn build_requested_period_metric_rows_hr_with_details() {
        let snapshot = TrendSnapshot {
            activity_count: 0,
            total_time_secs: 0,
            total_distance_m: 0.0,
            total_elevation_m: 0.0,
        };
        let act1 = ActivitySummary {
            id: "1".into(),
            ..Default::default()
        };
        let period = vec![&act1];
        let mut details = HashMap::new();
        details.insert(
            "1".into(),
            serde_json::json!({
                "moving_time": 3600,
                "average_heartrate": 150.0,
            }),
        );
        let rows =
            build_requested_period_metric_rows(&["hr".to_string()], &period, &snapshot, &details);
        assert_eq!(rows[0][1], "150 bpm");
        assert_eq!(rows[0][2], "available");
    }

    #[test]
    fn build_requested_period_metric_rows_pace() {
        let snapshot = TrendSnapshot {
            activity_count: 0,
            total_time_secs: 1800,
            total_distance_m: 6000.0,
            total_elevation_m: 0.0,
        };
        let rows = build_requested_period_metric_rows(
            &["pace".to_string()],
            &[],
            &snapshot,
            &HashMap::new(),
        );
        assert!(rows[0][1].contains("/km"));
        assert_eq!(rows[0][2], "available");
    }

    #[test]
    fn build_requested_period_metric_rows_pace_unavailable() {
        let snapshot = TrendSnapshot {
            activity_count: 0,
            total_time_secs: 0,
            total_distance_m: 0.0,
            total_elevation_m: 0.0,
        };
        let rows = build_requested_period_metric_rows(
            &["pace".to_string()],
            &[],
            &snapshot,
            &HashMap::new(),
        );
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unavailable");
    }

    #[test]
    fn build_requested_period_metric_rows_tss() {
        let snapshot = TrendSnapshot {
            activity_count: 0,
            total_time_secs: 0,
            total_distance_m: 0.0,
            total_elevation_m: 0.0,
        };
        let act1 = ActivitySummary {
            id: "1".into(),
            ..Default::default()
        };
        let period = vec![&act1];
        let mut details = HashMap::new();
        details.insert("1".into(), serde_json::json!({"tss": 150.0}));
        let rows =
            build_requested_period_metric_rows(&["tss".to_string()], &period, &snapshot, &details);
        assert_eq!(rows[0][1], "150.0");
        assert_eq!(rows[0][2], "available");
    }

    #[test]
    fn build_requested_period_metric_rows_tss_unavailable() {
        let snapshot = TrendSnapshot {
            activity_count: 0,
            total_time_secs: 0,
            total_distance_m: 0.0,
            total_elevation_m: 0.0,
        };
        let act1 = ActivitySummary {
            id: "1".into(),
            ..Default::default()
        };
        let period = vec![&act1];
        let details = HashMap::new();
        let rows =
            build_requested_period_metric_rows(&["tss".to_string()], &period, &snapshot, &details);
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unavailable");
    }

    #[test]
    fn build_requested_period_metric_rows_unsupported() {
        let snapshot = TrendSnapshot {
            activity_count: 0,
            total_time_secs: 0,
            total_distance_m: 0.0,
            total_elevation_m: 0.0,
        };
        let rows = build_requested_period_metric_rows(
            &["unknown".to_string()],
            &[],
            &snapshot,
            &HashMap::new(),
        );
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unsupported");
    }

    // ── build_zone_distribution_rows ──────────────────────────────────

    #[test]
    fn build_zone_distribution_rows_basic() {
        let mut zones = serde_json::Map::new();
        zones.insert("z1".into(), serde_json::json!(600));
        zones.insert("z2".into(), serde_json::json!(1200));
        zones.insert("z3".into(), serde_json::json!(200));
        let rows = build_zone_distribution_rows(&zones);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][0], "Z1");
        assert!(rows[0][1].contains("10:00"));
        assert_eq!(rows[1][0], "Z2");
        assert_eq!(rows[2][0], "Z3");
    }

    #[test]
    fn build_zone_distribution_rows_zero_total() {
        let mut zones = serde_json::Map::new();
        zones.insert("z1".into(), serde_json::json!(0));
        let rows = build_zone_distribution_rows(&zones);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][2], "0%");
    }

    #[test]
    fn build_zone_distribution_rows_skips_non_number() {
        let mut zones = serde_json::Map::new();
        zones.insert("z1".into(), serde_json::json!(300));
        zones.insert("foo".into(), serde_json::json!("bar"));
        let rows = build_zone_distribution_rows(&zones);
        assert_eq!(rows.len(), 1);
    }

    // ── build_range_histogram_rows ────────────────────────────────────

    #[test]
    fn build_range_histogram_rows_basic() {
        let buckets = serde_json::json!([
            {"min": 0, "max": 100, "secs": 600},
            {"min": 100, "max": 200, "secs": 1200},
        ]);
        let buckets_arr = buckets.as_array().unwrap();
        let rows = build_range_histogram_rows(buckets_arr, "W");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "0-100 W");
        assert_eq!(rows[0][1], "10:00");
        assert_eq!(rows[1][0], "100-200 W");
        assert_eq!(rows[1][1], "20:00");
    }

    #[test]
    fn build_range_histogram_rows_missing_secs() {
        let buckets = serde_json::json!([
            {"min": 0.0, "max": 100.0},
        ]);
        let buckets_arr = buckets.as_array().unwrap();
        let rows = build_range_histogram_rows(buckets_arr, "W");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1], "0:00");
    }

    // ── build_bucket_histogram_rows ───────────────────────────────────

    #[test]
    fn build_bucket_histogram_rows_basic() {
        let buckets = serde_json::json!([
            {"start": 0, "secs": 600, "movingSecs": 590},
            {"start": 10, "secs": 300, "movingSecs": 290},
        ]);
        let rows = build_bucket_histogram_rows(buckets.as_array().unwrap(), None, "");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "0");
        assert_eq!(rows[0][1], "10:00");
        assert_eq!(rows[0][2], "9:50");
        assert_eq!(rows[0][3], "n/a");
    }

    #[test]
    fn build_bucket_histogram_rows_with_suffix() {
        let buckets = serde_json::json!([
            {"start": 5, "secs": 600},
        ]);
        let rows = build_bucket_histogram_rows(buckets.as_array().unwrap(), None, "km");
        assert_eq!(rows[0][0], "5 km");
    }

    #[test]
    fn build_bucket_histogram_rows_with_average() {
        let buckets = serde_json::json!([
            {"start": 0, "secs": 600, "movingSecs": 590, "avg_watts": 250.0},
        ]);
        let rows = build_bucket_histogram_rows(buckets.as_array().unwrap(), Some("avg_watts"), "");
        assert_eq!(rows[0][3], "250");
    }

    #[test]
    fn build_bucket_histogram_rows_average_as_i64() {
        let buckets = serde_json::json!([
            {"start": 0, "secs": 600, "movingSecs": 590, "avg_hr": 145},
        ]);
        let rows = build_bucket_histogram_rows(buckets.as_array().unwrap(), Some("avg_hr"), "");
        assert_eq!(rows[0][3], "145");
    }

    #[test]
    fn build_bucket_histogram_rows_no_moving_secs() {
        let buckets = serde_json::json!([
            {"start": 0, "secs": 600},
        ]);
        let rows = build_bucket_histogram_rows(buckets.as_array().unwrap(), None, "");
        assert_eq!(rows[0][2], "10:00"); // falls back to secs
    }

    // ── append_histogram_section ──────────────────────────────────────

    #[test]
    fn append_histogram_section_none_payload() {
        let mut content = vec![];
        append_histogram_section(&mut content, "Test", None, None, "", "W");
        assert!(content.is_empty());
    }

    #[test]
    fn append_histogram_section_with_zones() {
        let payload = serde_json::json!({
            "zones": {"z1": 600, "z2": 300},
        });
        let mut content = vec![];
        append_histogram_section(&mut content, "Power Zones", Some(&payload), None, "", "W");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0], ContentBlock::markdown("Power Zones"));
    }

    #[test]
    fn append_histogram_section_with_range_buckets() {
        let payload = serde_json::json!([
            {"min": 0.0, "max": 100.0, "secs": 600},
        ]);
        let mut content = vec![];
        append_histogram_section(&mut content, "Power Range", Some(&payload), None, "", "W");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0], ContentBlock::markdown("Power Range"));
    }

    #[test]
    fn append_histogram_section_with_bucket_buckets() {
        let payload = serde_json::json!([
            {"start": 0, "secs": 600, "movingSecs": 590},
        ]);
        let mut content = vec![];
        append_histogram_section(
            &mut content,
            "Bucketed",
            Some(&payload),
            Some("avg_watts"),
            "km",
            "W",
        );
        assert_eq!(content.len(), 2);
        assert_eq!(content[0], ContentBlock::markdown("Bucketed"));
    }

    #[test]
    fn append_histogram_section_empty_zones() {
        let payload = serde_json::json!({"zones": {}});
        let mut content = vec![];
        append_histogram_section(&mut content, "Empty Zones", Some(&payload), None, "", "W");
        assert!(content.is_empty());
    }

    #[test]
    fn append_histogram_section_empty_buckets() {
        let payload = serde_json::json!([]);
        let mut content = vec![];
        append_histogram_section(&mut content, "Empty Buckets", Some(&payload), None, "", "W");
        assert!(content.is_empty());
    }

    // ── best_efforts_array ────────────────────────────────────────────

    #[test]
    fn best_efforts_array_direct() {
        let data = serde_json::json!([{"seconds": 300}]);
        let result = best_efforts_array(&data);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn best_efforts_array_nested_best_efforts() {
        let data = serde_json::json!({
            "best_efforts": [{"seconds": 300}],
        });
        let result = best_efforts_array(&data);
        assert!(result.is_some());
    }

    #[test]
    fn best_efforts_array_nested_efforts() {
        let data = serde_json::json!({
            "efforts": [{"seconds": 300}],
        });
        let result = best_efforts_array(&data);
        assert!(result.is_some());
    }

    #[test]
    fn best_efforts_array_not_array() {
        let data = serde_json::json!({"foo": "bar"});
        assert!(best_efforts_array(&data).is_none());
    }

    // ── format_best_effort_average ────────────────────────────────────

    #[test]
    fn format_best_effort_average_watts() {
        let best_efforts = serde_json::json!({});
        let mut effort = serde_json::Map::new();
        effort.insert("watts".into(), serde_json::json!(300.0));
        let result = format_best_effort_average(&best_efforts, &effort);
        assert_eq!(result, Some("300 W".into()));
    }

    #[test]
    fn format_best_effort_average_stream_watts() {
        let best_efforts = serde_json::json!({"stream": "watts"});
        let mut effort = serde_json::Map::new();
        effort.insert("average".into(), serde_json::json!(250.0));
        let result = format_best_effort_average(&best_efforts, &effort);
        assert_eq!(result, Some("250.0 W".into()));
    }

    #[test]
    fn format_best_effort_average_stream_speed() {
        let best_efforts = serde_json::json!({"stream": "speed"});
        let mut effort = serde_json::Map::new();
        effort.insert("average".into(), serde_json::json!(5.0));
        let result = format_best_effort_average(&best_efforts, &effort);
        assert_eq!(result, Some("3:20 /km".into()));
    }

    #[test]
    fn format_best_effort_average_stream_pace() {
        let best_efforts = serde_json::json!({"stream": "pace"});
        let mut effort = serde_json::Map::new();
        effort.insert("value".into(), serde_json::json!(4.0));
        let result = format_best_effort_average(&best_efforts, &effort);
        // 1000 / 4.0 = 250. 250 / 60 = 4 min, remainder 10 sec -> 4:10 /km
        assert_eq!(result, Some("4:10 /km".into()));
    }

    #[test]
    fn format_best_effort_average_stream_other() {
        let best_efforts = serde_json::json!({"stream": "cadence"});
        let mut effort = serde_json::Map::new();
        effort.insert("average".into(), serde_json::json!(85.0));
        let result = format_best_effort_average(&best_efforts, &effort);
        // "cadence" doesn't match any special stream type -> falls through
        // Falls to priority 3: speed/velocity field -> not present
        // Falls to priority 4: heartrate -> not present
        // Falls to priority 5: generic average
        assert_eq!(result, Some("85.00".into()));
    }

    #[test]
    fn format_best_effort_average_speed_field() {
        let mut effort = serde_json::Map::new();
        effort.insert("speed".into(), serde_json::json!(5.0));
        let result = format_best_effort_average(&serde_json::json!({}), &effort);
        assert_eq!(result, Some("3:20 /km".into()));
    }

    #[test]
    fn format_best_effort_average_velocity_field() {
        let mut effort = serde_json::Map::new();
        effort.insert("velocity".into(), serde_json::json!(5.0));
        let result = format_best_effort_average(&serde_json::json!({}), &effort);
        assert_eq!(result, Some("3:20 /km".into()));
    }

    #[test]
    fn format_best_effort_average_heartrate() {
        let mut effort = serde_json::Map::new();
        effort.insert("heartrate".into(), serde_json::json!(150.0));
        let result = format_best_effort_average(&serde_json::json!({}), &effort);
        assert_eq!(result, Some("150 bpm".into()));
    }

    #[test]
    fn format_best_effort_average_fallback() {
        let mut effort = serde_json::Map::new();
        effort.insert("average".into(), serde_json::json!(42.5));
        let result = format_best_effort_average(&serde_json::json!({}), &effort);
        assert_eq!(result, Some("42.50".into()));
    }

    #[test]
    fn format_best_effort_average_none() {
        let effort = serde_json::Map::new();
        assert!(format_best_effort_average(&serde_json::json!({}), &effort).is_none());
    }

    // ── append_best_efforts_section ───────────────────────────────────

    #[test]
    fn append_best_efforts_section_empty() {
        let mut content = vec![];
        append_best_efforts_section(&mut content, &serde_json::json!([]));
        assert!(content.is_empty());
    }

    #[test]
    fn append_best_efforts_section_legacy_hr() {
        let best_efforts = serde_json::json!([
            {"seconds": 300, "watts": 300.0, "heartrate": 150.0},
            {"seconds": 600, "watts": 280.0, "heartrate": 145.0},
        ]);
        let mut content = vec![];
        append_best_efforts_section(&mut content, &best_efforts);
        assert_eq!(content.len(), 2);
        if let ContentBlock::Table { headers, rows } = &content[1] {
            assert_eq!(headers[1], "Power");
            assert_eq!(headers[2], "HR");
            assert_eq!(rows[0][1], "300 W");
            assert_eq!(rows[0][2], "150 bpm");
        } else {
            panic!("Expected Table");
        }
    }

    #[test]
    fn append_best_efforts_section_modern() {
        let best_efforts = serde_json::json!([
            {"seconds": 300, "watts": 300.0},
            {"seconds": 600, "watts": 280.0},
        ]);
        let mut content = vec![];
        append_best_efforts_section(&mut content, &best_efforts);
        assert_eq!(content.len(), 2);
        if let ContentBlock::Table { headers, rows } = &content[1] {
            assert_eq!(headers[0], "Duration");
            assert_eq!(headers[1], "Average");
            assert_eq!(rows[0][1], "300 W");
        } else {
            panic!("Expected Table");
        }
    }

    #[test]
    fn append_best_efforts_section_no_format_possible() {
        let best_efforts = serde_json::json!([
            {"seconds": 300},
            {"seconds": 600},
        ]);
        let mut content = vec![];
        append_best_efforts_section(&mut content, &best_efforts);
        // Still adds markdown header, but only header row no data rows -> only 1 content block
        assert_eq!(content.len(), 1);
        assert_eq!(content[0], ContentBlock::markdown("Best Efforts"));
    }

    // ── append_stream_insights ────────────────────────────────────────

    fn assert_content_markdown_contains(block: &ContentBlock, needle: &str) {
        if let ContentBlock::Markdown { markdown } = block {
            assert!(
                markdown.contains(needle),
                "markdown '{markdown}' does not contain '{needle}'"
            );
        } else {
            panic!("Expected Markdown block, got {block:?}");
        }
    }

    #[test]
    fn append_stream_insights_unavailable() {
        let mut content = vec![];
        append_stream_insights(&mut content, None);
        assert_eq!(content.len(), 1);
        assert_content_markdown_contains(&content[0], "unavailable");
    }

    #[test]
    fn append_stream_insights_unavailable_not_object() {
        let mut content = vec![];
        append_stream_insights(&mut content, Some(&serde_json::json!([1, 2, 3])));
        assert_eq!(content.len(), 1);
        assert_content_markdown_contains(&content[0], "unavailable");
    }

    #[test]
    fn append_stream_insights_with_data() {
        let streams = serde_json::json!({
            "heartrate": [120, 130, 140, 150],
            "watts": [200, 250, 300],
            "cadence": [80, 85, 90],
        });
        let mut content = vec![];
        append_stream_insights(&mut content, Some(&streams));
        assert_eq!(content.len(), 2);
        if let ContentBlock::Table { headers, rows } = &content[1] {
            assert_eq!(headers[0], "Stream");
            // heartrate has priority 0, watts has 1, cadence has 3
            assert_eq!(rows[0][0], "heartrate");
            assert_eq!(rows[1][0], "watts");
            assert_eq!(rows[2][0], "cadence");
        } else {
            panic!("Expected Table");
        }
    }

    #[test]
    fn append_stream_insights_empty_data() {
        let streams = serde_json::json!({
            "heartrate": [],
        });
        let mut content = vec![];
        append_stream_insights(&mut content, Some(&streams));
        assert_content_markdown_contains(&content[0], "unavailable");
    }

    #[test]
    fn append_stream_insights_sorting_by_name_tiebreaker() {
        let streams = serde_json::json!({
            "altitude": [100, 200],
            "temp": [20, 25],
        });
        let mut content = vec![];
        append_stream_insights(&mut content, Some(&streams));
        if let ContentBlock::Table { headers: _, rows } = &content[1] {
            // altitude has priority 5, temp has priority 6
            assert_eq!(rows[0][0], "altitude");
            assert_eq!(rows[1][0], "temp");
        }
    }

    #[test]
    fn append_stream_insights_unrecognized_stream_priority() {
        let streams = serde_json::json!({
            "FooStream": [1, 2, 3],
            "BarStream": [4, 5],
        });
        let mut content = vec![];
        append_stream_insights(&mut content, Some(&streams));
        if let ContentBlock::Table { headers: _, rows } = &content[1] {
            // Both have priority 50, sorted alphabetically
            assert_eq!(rows[0][0], "BarStream");
            assert_eq!(rows[1][0], "FooStream");
        }
    }

    // ── render_espe_section ──────────────────────────────────────────

    #[test]
    fn render_espe_section_none() {
        assert!(render_espe_section(&None, &None).is_none());
    }

    #[test]
    fn render_espe_section_unsupported() {
        let anchors = EspePowerAnchors::unsupported();
        assert!(render_espe_section(&Some(anchors), &None).is_none());
    }

    #[test]
    fn render_espe_section_supported() {
        let anchors = EspePowerAnchors {
            eftp: Some(250.0),
            w_prime: Some(20000.0),
            p_max: Some(800.0),
            supported: true,
            ..Default::default()
        };
        let derived = EspeDerivedMetrics {
            glycolytic_bias: Some(3.2),
            supported: true,
            ..Default::default()
        };
        let text = render_espe_section(&Some(anchors), &Some(derived));
        let text = text.expect("should render");
        assert!(text.contains("eFTP"));
        assert!(text.contains("W′"));
        assert!(text.contains("pMax"));
        assert!(text.contains("Glycolytic Bias"));
    }

    #[test]
    fn render_espe_section_no_derived() {
        let anchors = EspePowerAnchors {
            eftp: Some(250.0),
            supported: true,
            ..Default::default()
        };
        let text = render_espe_section(&Some(anchors), &None);
        let text = text.expect("should render");
        assert!(text.contains("eFTP"));
        assert!(!text.contains("Glycolytic Bias"));
    }

    // ── render_wdrm_section ──────────────────────────────────────────

    #[test]
    fn render_wdrm_section_none() {
        assert!(render_wdrm_section(&None).is_none());
    }

    #[test]
    fn render_wdrm_section_unsupported() {
        let wdrm = WdrMetrics::unsupported();
        assert!(render_wdrm_section(&Some(wdrm)).is_none());
    }

    #[test]
    fn render_wdrm_section_supported() {
        let wdrm = WdrMetrics {
            supported: true,
            max_wbal_depletion: Some(15000.0),
            depletion_pct: Some(0.75),
            joules_above_ftp: Some(5000.0),
            mean_depletion_pct_7d: Some(0.65),
            high_depletion_sessions_7d: 3,
            sessions_with_data_7d: 5,
        };
        let text = render_wdrm_section(&Some(wdrm));
        let text = text.expect("should render");
        assert!(text.contains("W′ Depletion"));
        assert!(text.contains("Max W′ Depletion"));
        assert!(text.contains("Depletion: 75%"));
        assert!(text.contains("Joules Above FTP"));
        assert!(text.contains("Mean 7d Depletion: 65%"));
        assert!(text.contains("High Depletion Sessions (7d): 3"));
    }

    #[test]
    fn render_wdrm_section_no_7d_data() {
        let wdrm = WdrMetrics {
            supported: true,
            max_wbal_depletion: Some(10000.0),
            ..Default::default()
        };
        let text = render_wdrm_section(&Some(wdrm));
        let text = text.expect("should render");
        assert!(text.contains("Max W′ Depletion"));
        assert!(!text.contains("Mean 7d"));
        assert!(!text.contains("High Depletion Sessions"));
    }

    // ── render_isdm_section ──────────────────────────────────────────

    #[test]
    fn render_isdm_section_none() {
        assert!(render_isdm_section(&None).is_none());
    }

    #[test]
    fn render_isdm_section_basic() {
        let decoupling = DecouplingMetrics {
            decoupling_pct: 5.5,
            signed_decoupling_pct: -3.2,
            state: "decoupled".into(),
            durability_state: "moderate".into(),
            ..Default::default()
        };
        let text = render_isdm_section(&Some(decoupling));
        let text = text.expect("should render");
        assert!(text.contains("Aerobic Decoupling"));
        assert!(text.contains("Signed Decoupling"));
        assert!(text.contains("-3.2%"));
        assert!(text.contains("Absolute Decoupling"));
        assert!(text.contains("5.5%"));
        assert!(text.contains("Durability State: moderate"));
    }

    #[test]
    fn render_isdm_section_with_z2_variance_stable() {
        let decoupling = DecouplingMetrics {
            decoupling_pct: 5.0,
            signed_decoupling_pct: -3.0,
            state: "some".into(),
            durability_state: "good".into(),
            efficiency_factor_first_half: Some(0.85),
            efficiency_factor_second_half: Some(0.82),
            z2_hr_variance: Some(20.0),
        };
        let text = render_isdm_section(&Some(decoupling));
        let text = text.expect("should render");
        assert!(text.contains("EF First Half"));
        assert!(text.contains("EF Second Half"));
        assert!(text.contains("Z2 HR Variance"));
        assert!(text.contains("stable"));
    }

    #[test]
    fn render_isdm_section_with_z2_variance_moderate() {
        let decoupling = DecouplingMetrics {
            decoupling_pct: 5.0,
            signed_decoupling_pct: -3.0,
            state: "some".into(),
            durability_state: "good".into(),
            z2_hr_variance: Some(35.0),
            ..Default::default()
        };
        let text = render_isdm_section(&Some(decoupling));
        assert!(text.unwrap().contains("moderate"));
    }

    #[test]
    fn render_isdm_section_with_z2_variance_unstable() {
        let decoupling = DecouplingMetrics {
            decoupling_pct: 5.0,
            signed_decoupling_pct: -3.0,
            state: "some".into(),
            durability_state: "good".into(),
            z2_hr_variance: Some(60.0),
            ..Default::default()
        };
        let text = render_isdm_section(&Some(decoupling));
        assert!(text.unwrap().contains("unstable"));
    }

    // ── render_ndli_section ──────────────────────────────────────────

    #[test]
    fn render_ndli_section_none() {
        assert!(render_ndli_section(&None).is_none());
    }

    #[test]
    fn render_ndli_section_unsupported() {
        let ndli = NdliMetrics::default();
        assert!(render_ndli_section(&Some(ndli)).is_none());
    }

    #[test]
    fn render_ndli_section_supported() {
        let ndli = NdliMetrics {
            supported: true,
            high_intensity_days_7d: 4,
            mean_intensity_factor_7d: Some(0.95),
            mean_efficiency_factor_7d: Some(0.88),
            mean_variability_index_7d: Some(1.25),
            ndli_state: "moderate".into(),
            ndli_overload_flag: false,
        };
        let text = render_ndli_section(&Some(ndli));
        let text = text.expect("should render");
        assert!(text.contains("Neural Density"));
        assert!(text.contains("State: moderate"));
        assert!(text.contains("High-Intensity Days"));
        assert!(text.contains("Mean IF: 0.950"));
        assert!(text.contains("Mean EF: 0.880"));
        assert!(text.contains("Mean VI: 1.25"));
    }

    #[test]
    fn render_ndli_section_partial_derived() {
        let ndli = NdliMetrics {
            supported: true,
            high_intensity_days_7d: 3,
            ndli_state: "low".into(),
            ..Default::default()
        };
        let text = render_ndli_section(&Some(ndli));
        let text = text.expect("should render");
        assert!(text.contains("State: low"));
        assert!(!text.contains("Mean IF"));
        assert!(!text.contains("Mean EF"));
        assert!(!text.contains("Mean VI"));
    }

    // ── render_heat_section ──────────────────────────────────────────

    #[test]
    fn render_heat_section_none() {
        assert!(render_heat_section(&None).is_none());
    }

    #[test]
    fn render_heat_section_unsupported() {
        let heat = HeatMetrics::default();
        assert!(render_heat_section(&Some(heat)).is_none());
    }

    #[test]
    fn render_heat_section_supported() {
        let heat = HeatMetrics {
            supported: true,
            heat_index_7d: Some(28.5),
            heat_max_7d: Some(35.0),
            heat_state: "elevated".into(),
        };
        let text = render_heat_section(&Some(heat));
        let text = text.expect("should render");
        assert!(text.contains("Heat Stress"));
        assert!(text.contains("State: elevated"));
        assert!(text.contains("Heat Index"));
        assert!(text.contains("Max Temperature"));
    }

    #[test]
    fn render_heat_section_partial() {
        let heat = HeatMetrics {
            supported: true,
            heat_state: "normal".into(),
            ..Default::default()
        };
        let text = render_heat_section(&Some(heat));
        let text = text.expect("should render");
        assert!(text.contains("State: normal"));
        assert!(!text.contains("Heat Index"));
        assert!(!text.contains("Max Temperature"));
    }

    // ── render_z2_stability_section ──────────────────────────────────

    #[test]
    fn render_z2_stability_section_none() {
        assert!(render_z2_stability_section(120.0, 150.0, None).is_none());
    }

    #[test]
    fn render_z2_stability_section_stable() {
        let text = render_z2_stability_section(120.0, 150.0, Some(20.0));
        let text = text.expect("should render");
        assert!(text.contains("Z2 HR Stability"));
        assert!(text.contains("120–150 bpm"));
        assert!(text.contains("HR Variance: 20.0"));
        assert!(text.contains("stable"));
    }

    #[test]
    fn render_z2_stability_section_moderate() {
        let text = render_z2_stability_section(120.0, 150.0, Some(35.0));
        let text = text.expect("should render");
        assert!(text.contains("moderate"));
    }

    #[test]
    fn render_z2_stability_section_unstable() {
        let text = render_z2_stability_section(120.0, 150.0, Some(55.0));
        let text = text.expect("should render");
        assert!(text.contains("unstable"));
    }

    // ── extract_exact_tss: training_load fallback ─────────────────────

    #[test]
    fn extract_exact_tss_training_load_string() {
        let mut obj = serde_json::Map::new();
        obj.insert("training_load".to_string(), serde_json::json!(130));
        // i64 value should be converted to f64
        assert_eq!(extract_exact_tss(&obj), Some(130.0));
    }

    // ── build_detailed_workout_rows: tss via training_load ────────────

    #[test]
    fn build_detailed_workout_rows_tss_via_training_load() {
        let detail = serde_json::json!({
            "moving_time": 1800,
            "distance": 5000.0,
            "training_load": 95.1,
        });
        let rows = build_detailed_workout_rows(Some(&detail));
        // "training_load" is found by extract_exact_tss after "tss" and "icu_training_load"
        assert!(rows.iter().any(|r| r[1].contains("95.1")));
    }

    // ── build_detailed_workout_rows: partial data ─────────────────────

    #[test]
    fn build_detailed_workout_rows_partial() {
        let detail = serde_json::json!({
            "moving_time": 900,
            "distance": 3000.0,
        });
        let rows = build_detailed_workout_rows(Some(&detail));
        assert_eq!(rows.len(), 1); // Only pace
        assert_eq!(rows[0][0], "Avg Pace");
    }

    // ── build_activity_message_rows: no content filtering ─────────────

    #[test]
    fn build_activity_message_rows_none_content() {
        let msgs = vec![ActivityMessage {
            id: 1,
            athlete_id: None,
            name: None,
            created: None,
            message_type: None,
            content: None,
            activity_id: None,
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        }];
        let rows = build_activity_message_rows(&msgs);
        assert!(rows.is_empty());
    }

    // ── derive_interval_output: stream power alt key ─────────────────

    #[test]
    fn derive_interval_output_power_stream_power_key() {
        let obj = serde_json::json!({"start_index": 0, "end_index": 3});
        let streams = serde_json::json!({"power": [200, 220, 240, 260]});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, Some(&streams));
        // avg of [200, 220, 240] = 220
        assert_eq!(result.map(|v| v.format()), Some("220 W".to_string()));
    }

    // ── derive_interval_output: pace stream with pace key ─────────────

    #[test]
    fn derive_interval_output_pace_stream_pace_key() {
        let obj = serde_json::json!({"start_index": 0, "end_index": 3});
        let streams = serde_json::json!({"pace": [4.0, 5.0, 6.0]});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, Some(&streams));
        let result = result.map(|v| v.format());
        assert_eq!(result, Some("3:20 /km".to_string()));
    }

    // ── derive_interval_output: average_speed zero filtered ───────────

    #[test]
    fn derive_interval_output_zero_speed_filtered() {
        let obj = serde_json::json!({"average_speed": 0.0});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, None);
        assert!(result.is_none());
    }

    // ── derive_interval_output: pace stream zero filtered ─────────────

    #[test]
    fn derive_interval_output_pace_stream_zero() {
        let obj = serde_json::json!({"start_index": 0, "end_index": 3});
        let streams = serde_json::json!({"velocity_smooth": [0.0, 0.0, 0.0]});
        let map = obj.as_object().unwrap();
        let result = derive_interval_output(map, Some(&streams));
        // avg = 0.0, filtered by .filter(|speed| *speed > 0.0)
        assert!(result.is_none());
    }

    // ── count_work_intervals: careful about NaN edge case ─────────────

    #[test]
    fn count_work_intervals_single_work_all_intervals() {
        let intervals: Vec<Value> = vec![serde_json::json!({"average_speed": 10.0})];
        // < 3 data points -> fallback to all intervals
        assert_eq!(count_work_intervals(&intervals), 1);
    }

    // ── build_interval_analysis_rows: no matching output kind ─────────

    #[test]
    fn build_interval_analysis_rows_no_match() {
        let intervals: Vec<Value> =
            vec![serde_json::json!({"moving_time": 300, "average_watts": 250.0})];
        let rows = build_interval_analysis_rows(&intervals, None, IntervalOutputKind::Pace);
        // 1 interval but output_kind is Pace while interval has Power -> n/a
        // Still included in the output
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][3], "n/a");
    }

    // ── build_requested_single_metric_rows: pace with no movement ─────

    #[test]
    fn build_requested_single_metric_rows_pace_no_distance() {
        let detail = serde_json::json!({});
        let obj = detail.as_object();
        let rows = build_requested_single_metric_rows(obj, &["pace".to_string()]);
        assert_eq!(rows[0][1], "n/a");
        assert_eq!(rows[0][2], "unavailable");
    }
}
