use chrono::NaiveDate;
use serde_json::Value;

use crate::intents::ContentBlock;

pub(crate) fn build_load_management_markdown(
    metrics: Option<&crate::domains::coach::LoadManagementMetrics>,
) -> String {
    let mut lines = vec!["### Load Context".to_string(), String::new()];

    let Some(metrics) = metrics else {
        lines.push(
            "- Load-management context unavailable because the lookback history is too short."
                .to_string(),
        );
        return lines.join("\n");
    };

    if let Some(acwr) = &metrics.acwr {
        lines.push(format!(
            "- ACWR: {:.2} ({}) — acute {:.1}, chronic {:.1}",
            acwr.ratio, acwr.state, acwr.acute_load, acwr.chronic_load
        ));
    }

    if let Some(monotony) = metrics.monotony {
        lines.push(format!("- Monotony: {:.2}", monotony));
    }

    if let Some(strain) = metrics.strain {
        lines.push(format!("- Strain: {:.0}", strain));
    }

    if let Some(stress_tolerance) = metrics.stress_tolerance {
        let status = if (3.0..=6.0).contains(&stress_tolerance) {
            "✅"
        } else if stress_tolerance > 6.0 {
            "⚠️"
        } else {
            "⚪"
        };
        lines.push(format!(
            "- Stress Tolerance: {:.2} {}",
            stress_tolerance, status
        ));
    }

    if let Some(fatigue_index) = metrics.fatigue_index {
        let status = if fatigue_index <= 2.5 {
            "✅"
        } else {
            "⚠️"
        };
        lines.push(format!("- Fatigue Index: {:.2} {}", fatigue_index, status));
    }

    if let Some(durability_index) = metrics.durability_index {
        let status = if durability_index >= 0.9 {
            "✅"
        } else {
            "⚠️"
        };
        lines.push(format!(
            "- Durability Index: {:.3} {}",
            durability_index, status
        ));
    }

    if lines.len() == 2 {
        lines.push(
            "- Load-management context unavailable because no deterministic load signal was found."
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
                .map(|metric| metric.to_lowercase())
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
                    .unwrap_or_else(|| "n/a".to_string()),
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
                .unwrap_or_else(|| "n/a".to_string());
            let author = message
                .name
                .clone()
                .or_else(|| message.athlete_id.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            let kind = message
                .message_type
                .clone()
                .unwrap_or_else(|| "TEXT".to_string());

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
    let total_time: i64 = zones.values().filter_map(|x| x.as_i64()).sum();

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
            content.push(ContentBlock::markdown(format!("\n### {title}\n")));
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
            content.push(ContentBlock::markdown(format!("\n### {title}\n")));
            content.push(ContentBlock::table(
                vec!["Range".into(), "Time".into()],
                range_rows,
            ));
            return;
        }

        let rows = build_bucket_histogram_rows(buckets, average_key, start_suffix);
        if !rows.is_empty() {
            content.push(ContentBlock::markdown(format!("\n### {title}\n")));
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
            Some("watts") | Some("power") => return Some(format!("{avg:.1} W")),
            Some("speed") | Some("velocity") | Some("pace") => {
                // Speed in m/s, convert to pace per km
                if avg > 0.0 {
                    let secs_per_km = (1000.0 / avg).round() as i64;
                    return Some(format!("{}:{:02} /km", secs_per_km / 60, secs_per_km % 60));
                }
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

    content.push(ContentBlock::markdown("\n### Best Efforts\n".to_string()));

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
            "### Stream Insights\n\n- Stream data requested but unavailable.".to_string(),
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
            "### Stream Insights\n\n- Stream data requested but unavailable.".to_string(),
        ));
        return;
    }

    content.push(ContentBlock::markdown("### Stream Insights".to_string()));
    content.push(ContentBlock::table(
        vec!["Stream".into(), "Points".into(), "Min".into(), "Max".into()],
        rows,
    ));
}
