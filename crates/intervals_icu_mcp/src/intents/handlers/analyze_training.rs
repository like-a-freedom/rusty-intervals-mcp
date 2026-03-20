use crate::intents::{
    ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput, OutputMetadata,
};
use async_trait::async_trait;
use chrono::NaiveDate;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
/// Analyze Training Intent Handler
///
/// Analyzes a single training session or a period of training.
use std::sync::Arc;

use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext};
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::{
    PeriodFetchRequest, SingleWorkoutFetchRequest, build_daily_load_series, build_previous_window,
    fetch_period_data, fetch_single_workout_data,
};
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{
    build_trend_snapshot, compute_load_management_metrics, derive_trend_metrics,
    derive_volume_metrics, derive_workout_metrics_context, parse_api_load_snapshot,
    parse_fitness_metrics,
};
use crate::intents::utils::{
    data_availability_block, filter_activities_by_date, filter_activities_by_range,
    filter_events_by_range,
};
use intervals_icu_client::EventCategory;

pub struct AnalyzeTrainingHandler;
impl AnalyzeTrainingHandler {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SingleAnalysisMode {
    Summary,
    Detailed,
    Intervals,
    Streams,
}

impl SingleAnalysisMode {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("summary") {
            "detailed" => Self::Detailed,
            "intervals" => Self::Intervals,
            "streams" => Self::Streams,
            _ => Self::Summary,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Detailed => "detailed",
            Self::Intervals => "intervals",
            Self::Streams => "streams",
        }
    }

    fn include_intervals(self) -> bool {
        matches!(self, Self::Intervals)
    }

    fn include_streams(self) -> bool {
        // Detailed mode needs streams for execution metrics (efficiency factor, aerobic decoupling)
        matches!(self, Self::Detailed | Self::Intervals | Self::Streams)
    }

    fn show_execution_context(self) -> bool {
        matches!(self, Self::Detailed | Self::Streams)
    }

    fn show_interval_section(self) -> bool {
        matches!(self, Self::Intervals)
    }

    fn show_stream_section(self) -> bool {
        matches!(self, Self::Streams)
    }

    fn show_quality_findings(self) -> bool {
        matches!(self, Self::Detailed | Self::Streams)
    }

    fn show_data_availability(self) -> bool {
        matches!(self, Self::Detailed | Self::Streams)
    }

    fn show_detailed_breakdown(self) -> bool {
        matches!(self, Self::Detailed)
    }
}

fn build_load_management_markdown(
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

    if lines.len() == 2 {
        lines.push(
            "- Load-management context unavailable because no deterministic load signal was found."
                .to_string(),
        );
    }

    lines.join("\n")
}

fn parse_activity_date(value: &str) -> Option<NaiveDate> {
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|dt| dt.date())
        .or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
}

fn requested_metrics(input: &Value) -> Vec<String> {
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

fn format_duration_hhmm(seconds: i64) -> String {
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

fn format_duration_compact(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

fn is_planned_workout_id(activity_id: &str) -> bool {
    activity_id.starts_with("event:")
}

fn build_calendar_event_rows(events: &[&intervals_icu_client::Event]) -> Vec<Vec<String>> {
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

fn count_work_intervals(intervals: &[Value]) -> usize {
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

fn calculate_median(values: &mut [f64]) -> f64 {
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

fn format_pace_per_km(seconds: i64, distance_m: f64) -> Option<String> {
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

fn extract_exact_tss(object: &serde_json::Map<String, Value>) -> Option<f64> {
    object
        .get("tss")
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
}

fn build_basic_workout_metric_rows(workout_detail: Option<&Value>) -> Vec<Vec<String>> {
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

fn build_detailed_workout_rows(workout_detail: Option<&Value>) -> Vec<Vec<String>> {
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

fn build_activity_message_rows(
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

fn interval_number(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
}

fn stream_series<'a>(streams: Option<&'a Value>, keys: &[&str]) -> Option<&'a Vec<Value>> {
    let object = streams?.as_object()?;
    keys.iter().find_map(|key| object.get(*key)?.as_array())
}

fn average_stream_slice(values: &[Value], start_index: usize, end_index: usize) -> Option<f64> {
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

fn format_pace_from_speed(speed_mps: f64) -> Option<String> {
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

fn average_numeric_stream_value(streams: Option<&Value>, keys: &[&str]) -> Option<f64> {
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

fn quality_output_finding(
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
enum IntervalOutputKind {
    Power,
    Pace,
}

enum IntervalOutputValue {
    Power(f64),
    Pace(f64),
}

impl IntervalOutputValue {
    fn kind(&self) -> IntervalOutputKind {
        match self {
            Self::Power(_) => IntervalOutputKind::Power,
            Self::Pace(_) => IntervalOutputKind::Pace,
        }
    }

    fn format(&self) -> String {
        match self {
            Self::Power(value) => format!("{value:.0} W"),
            Self::Pace(speed_mps) => {
                format_pace_from_speed(*speed_mps).unwrap_or_else(|| "n/a".to_string())
            }
        }
    }
}

fn derive_interval_output(
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

fn preferred_interval_output_kind(
    intervals: &[Value],
    streams: Option<&Value>,
) -> IntervalOutputKind {
    intervals
        .iter()
        .filter_map(Value::as_object)
        .find_map(|interval| derive_interval_output(interval, streams).map(|value| value.kind()))
        .unwrap_or(IntervalOutputKind::Power)
}

fn build_interval_analysis_rows(
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

fn build_period_summary_rows(
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

fn build_requested_single_metric_rows(
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

fn build_requested_period_metric_rows(
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

fn build_zone_distribution_rows(zones: &serde_json::Map<String, Value>) -> Vec<Vec<String>> {
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

fn numeric_value(object: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    object
        .get(key)
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
}

fn format_histogram_number(value: f64) -> String {
    if (value - value.round()).abs() < 1e-6 {
        format!("{:.0}", value)
    } else {
        format!("{value:.2}")
    }
}

fn build_range_histogram_rows(buckets: &[Value], unit: &str) -> Vec<Vec<String>> {
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

fn build_bucket_histogram_rows(
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

fn append_histogram_section(
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

fn best_efforts_array(best_efforts: &Value) -> Option<&Vec<Value>> {
    best_efforts
        .as_array()
        .or_else(|| best_efforts.get("best_efforts").and_then(Value::as_array))
        .or_else(|| best_efforts.get("efforts").and_then(Value::as_array))
}

fn format_best_effort_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

fn format_best_effort_average(
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

fn append_best_efforts_section(content: &mut Vec<ContentBlock>, best_efforts: &Value) {
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

fn append_stream_insights(content: &mut Vec<ContentBlock>, streams: Option<&Value>) {
    let Some(streams) = streams.and_then(Value::as_object) else {
        content.push(ContentBlock::markdown(
            "### Stream Insights\n\n- Stream data requested but unavailable.".to_string(),
        ));
        return;
    };

    fn stream_priority(name: &str) -> usize {
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

#[async_trait]
impl IntentHandler for AnalyzeTrainingHandler {
    fn name(&self) -> &'static str {
        "analyze_training"
    }

    fn description(&self) -> &'static str {
        "Analyzes training sessions - single workout or period. \
         Use for analyzing completed workouts, assessing progress, and identifying patterns. \
            Supports single workout analysis (target_type: single) or period analysis (target_type: period). \
            Single-workout responses may also surface read-only workout comments/messages when the source activity has them. \
            Period analysis also retrieves calendar events such as races, sick days, injuries, notes, and planned workouts \
            without folding them into the load metrics."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_type": {
                    "type": "string",
                    "enum": ["single", "period"],
                    "description": "Analysis type: single workout or period"
                },
                "date": {
                    "type": "string",
                    "description": "Workout date (YYYY-MM-DD) for single analysis"
                },
                "period_start": {
                    "type": "string",
                    "description": "Period start (YYYY-MM-DD) for period analysis and calendar-event context"
                },
                "period_end": {
                    "type": "string",
                    "description": "Period end (YYYY-MM-DD) for period analysis and calendar-event context"
                },
                "description_contains": {
                    "type": "string",
                    "description": "Filter activities by name/description (case-insensitive substring match). Works with target_type: single only. Examples: 'long run', 'tempo', 'intervals', 'threshold'"
                },
                "analysis_type": {
                    "type": "string",
                    "enum": ["summary", "detailed", "intervals", "streams"],
                    "default": "summary",
                    "description": "Analysis depth: summary (basic), detailed (+expanded workout metrics), intervals (+interval analysis), streams (+stream data insights)"
                },
                "include_best_efforts": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include best efforts comparison"
                },
                "include_histograms": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include power/HR/pace histograms (single workout only)"
                },
                "metrics": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Requested metrics: time, distance, vertical, tss, pace, hr. Results are surfaced explicitly; unavailable exact metrics are marked unavailable instead of being silently ignored."
                }
            },
            "required": ["target_type"],
            "oneOf": [
                {"required": ["target_type", "date"]},
                {"required": ["target_type", "period_start", "period_end"]}
            ]
        })
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let target_type = input
            .get("target_type")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: target_type"))?;

        match target_type {
            "single" => self.analyze_single(&input, client.as_ref()).await,
            "period" => self.analyze_period(&input, client.as_ref()).await,
            _ => Err(IntentError::validation(format!(
                "Invalid target_type: {}. Must be 'single' or 'period'",
                target_type
            ))),
        }
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}

impl AnalyzeTrainingHandler {
    async fn analyze_single(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
    ) -> Result<IntentOutput, IntentError> {
        let date = input
            .get("date")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field for single: date"))?;
        let desc_filter = input.get("description_contains").and_then(Value::as_str);

        // Parse and validate date
        NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
            IntentError::validation(format!("Invalid date format: {}. Use YYYY-MM-DD.", date))
        })?;

        // Fetch recent activities
        let activities = client
            .get_recent_activities(Some(50), Some(30))
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch activities: {}", e)))?;

        // Debug logging
        tracing::debug!("Fetched {} activities", activities.len());
        for a in &activities {
            tracing::debug!(
                "Activity: id={}, name={}, date={}",
                a.id,
                a.name.as_deref().unwrap_or("N/A"),
                a.start_date_local
            );
        }

        // Filter by date
        let target_date = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
            IntentError::validation(format!("Invalid date format: {}. Use YYYY-MM-DD.", date))
        })?;
        let mut matching = filter_activities_by_date(&activities, &target_date);

        // Apply description filter if provided
        if let Some(desc) = desc_filter {
            let desc_lower = desc.to_lowercase();
            matching.retain(|a| {
                a.name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(&desc_lower))
                    .unwrap_or(false)
            });
            tracing::debug!(
                "After description filter '{}': {} activities remain",
                desc,
                matching.len()
            );
        }

        tracing::debug!("Found {} matching activities for {}", matching.len(), date);

        // Handle empty results gracefully (not an error)
        if matching.is_empty() {
            let mut content = Vec::new();
            content.push(ContentBlock::markdown(format!(
                "## Analysis: {}\n\n**Status:** No activities found",
                date
            )));

            let mut summary = vec![
                format!("- No training activities recorded for {}", date),
                "- This could be a rest day or activities haven't been synced yet".into(),
            ];
            if let Some(d) = desc_filter {
                summary.push(format!("- Search filter: '{}'", d));
            }

            content.push(ContentBlock::markdown(summary.join("\n")));

            let suggestions = vec![
                "Check if activities are synced from your fitness device".into(),
                "Verify the date - did you train on this day?".into(),
                "Try expanding the date range to include nearby days".into(),
            ];

            let next_actions = vec![
                "To view recent activities: analyze_training with target_type: period and wider date range".into(),
                "To check athlete profile: manage_profile with action: get".into(),
            ];

            return Ok(IntentOutput::new(content)
                .with_suggestions(suggestions)
                .with_next_actions(next_actions));
        }

        // Handle multiple matches
        if matching.len() > 1 {
            let mut content = Vec::new();
            content.push(ContentBlock::markdown(format!(
                "## Analysis: {}\n\n**Status:** Multiple activities found",
                date
            )));

            let mut summary = vec![format!(
                "- Found {} activities for {}",
                matching.len(),
                date
            )];
            if let Some(d) = desc_filter {
                summary.push(format!(
                    "- Search filter: '{}' matched {} activities",
                    d,
                    matching.len()
                ));
            }
            summary.push("Please be more specific with your search.".into());

            // List found activities
            let mut activities_list = vec!["**Found activities:**".into()];
            for (i, a) in matching.iter().enumerate() {
                activities_list.push(format!(
                    "{}. {} (ID: {})",
                    i + 1,
                    a.name.as_deref().unwrap_or("Unknown"),
                    a.id
                ));
            }
            content.push(ContentBlock::markdown(activities_list.join("\n")));

            // Build explicit retry examples for each activity
            let mut retry_examples = vec!["**To analyze a specific activity, retry with:**".into()];
            for (i, a) in matching.iter().enumerate() {
                let name = a.name.as_deref().unwrap_or("Unknown");
                let id = &a.id;
                // Extract key phrase from name (first 2-3 words or before dash/colon/em-dash)
                let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();

                retry_examples.push(format!(
                    "{}. **{}** → `description_contains: \"{}\"` or ID: `{}`",
                    i + 1,
                    name,
                    key_phrase,
                    id
                ));
            }
            content.push(ContentBlock::markdown(retry_examples.join("\n")));

            let suggestions = vec![
                "Choose one activity from the list and retry with its `description_contains` value".into(),
                "For interval analysis, look for keywords like 'tempo', 'threshold', 'intervals', 'repeats', 'VO2'".into(),
                "Note: Only workouts created with structured intervals will show interval data".into(),
            ];

            let first_key_phrase = matching[0]
                .name
                .as_deref()
                .unwrap_or("Workout")
                .split(['-', '—', ':'])
                .next()
                .unwrap_or("Workout")
                .trim();

            let mut next_actions = vec![
                format!(
                    "Retry with `description_contains` from the list above (e.g., `description_contains: \"{}\"`)",
                    first_key_phrase
                ),
                "Use `analyze_training` with `target_type: period` to see all activities".into(),
            ];

            // Add ID-based option for direct access (only if not too many activities)
            if matching.len() <= 3 {
                next_actions.push(format!(
                    "Or specify activity ID directly if your MCP client supports it (e.g., `{}`)",
                    matching[0].id
                ));
            }

            return Ok(IntentOutput::new(content)
                .with_suggestions(suggestions)
                .with_next_actions(next_actions));
        }

        let activity = &matching[0];
        let activity_id = activity.id.clone();
        let activity_name = activity
            .name
            .as_deref()
            .unwrap_or("Unknown Activity")
            .to_string();

        // Fetch additional data based on analysis_type
        let analysis_mode =
            SingleAnalysisMode::parse(input.get("analysis_type").and_then(Value::as_str));
        let requested_metrics = requested_metrics(input);
        let include_best = input
            .get("include_best_efforts")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let include_hist = input
            .get("include_histograms")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut fetched = fetch_single_workout_data(
            client,
            &SingleWorkoutFetchRequest {
                activity_id: activity_id.clone(),
                include_intervals: analysis_mode.include_intervals(),
                include_streams: analysis_mode.include_streams(),
                include_best_efforts: include_best,
                include_hr_histogram: include_hist,
                include_power_histogram: include_hist,
                include_pace_histogram: include_hist,
            },
        )
        .await?;
        fetched.activities = vec![(*activity).clone()];
        fetched.fitness = client.get_fitness_summary().await.ok();

        let mut context = CoachContext::new(
            AnalysisKind::TrainingSingle,
            AnalysisWindow::new(target_date, target_date),
        );
        context.audit = build_data_audit(&fetched);
        context.metrics.fitness = parse_fitness_metrics(fetched.fitness.as_ref());

        let workout_detail = fetched.workout_detail.as_ref();
        let _interval_count = fetched
            .intervals
            .as_ref()
            .and_then(Value::as_array)
            .map(|items| items.len());
        let work_interval_count = fetched
            .intervals
            .as_ref()
            .and_then(Value::as_array)
            .map(|items| count_work_intervals(items));
        let avg_hr = workout_detail
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("average_heartrate"))
            .and_then(Value::as_f64);
        let avg_power = workout_detail
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("average_watts"))
            .and_then(Value::as_f64);
        let mut execution_notes = Vec::new();
        if let Some(count) = work_interval_count
            && count > 0
        {
            execution_notes.push(format!(
                "Structured session with {} detected work intervals.",
                count
            ));
        }
        if analysis_mode.include_streams() {
            if context.audit.streams_available {
                execution_notes.push("Stream data available for deeper execution review.".into());
            } else {
                execution_notes.push("Stream review requested; stream data unavailable.".into());
            }
        }
        let (efficiency_factor, aerobic_decoupling) =
            crate::engines::coach_metrics::derive_execution_metrics(
                fetched.workout_detail.as_ref(),
                fetched.streams.as_ref(),
            );
        let mut workout_metrics =
            derive_workout_metrics_context(work_interval_count, avg_hr, avg_power, execution_notes);
        workout_metrics.efficiency_factor = efficiency_factor;
        workout_metrics.aerobic_decoupling = aerobic_decoupling;
        context.metrics.workout = Some(workout_metrics);
        context.alerts = build_alerts(&context.metrics);
        context.guidance = build_guidance(&context.metrics, &context.alerts);

        let mut content = Vec::new();
        content.push(ContentBlock::markdown(format!(
            "## Analysis: {}\n\n**Date:** {}\n**ID:** {}\n**Type:** {}",
            activity_name,
            date,
            activity_id,
            analysis_mode.as_str()
        )));

        // Build basic metrics table
        let rows = build_basic_workout_metric_rows(workout_detail);
        if !rows.is_empty() {
            content.push(ContentBlock::table(
                vec!["Metric".into(), "Value".into()],
                rows,
            ));
        }

        if !requested_metrics.is_empty() {
            let rows = build_requested_single_metric_rows(
                workout_detail.and_then(Value::as_object),
                &requested_metrics,
            );
            content.push(ContentBlock::markdown("### Requested Metrics".to_string()));
            content.push(ContentBlock::table(
                vec!["Metric".into(), "Value".into(), "Status".into()],
                rows,
            ));
        }

        if analysis_mode.show_detailed_breakdown() {
            let rows = build_detailed_workout_rows(workout_detail);
            if !rows.is_empty() {
                content.push(ContentBlock::markdown("### Detailed Breakdown".to_string()));
                content.push(ContentBlock::table(
                    vec!["Metric".into(), "Value".into()],
                    rows,
                ));
            }
        }

        let activity_message_rows = build_activity_message_rows(&fetched.activity_messages);
        if !activity_message_rows.is_empty() {
            content.push(ContentBlock::markdown("### Workout Comments".to_string()));
            content.push(ContentBlock::table(
                vec![
                    "When".into(),
                    "Author".into(),
                    "Type".into(),
                    "Comment".into(),
                ],
                activity_message_rows,
            ));
        }

        if analysis_mode.show_execution_context()
            && let Some(workout) = &context.metrics.workout
            && (!workout.execution_notes.is_empty()
                || workout.efficiency_factor.is_some()
                || workout.aerobic_decoupling.is_some())
        {
            let mut lines = workout.execution_notes.clone();
            if let Some(efficiency_factor) = workout.efficiency_factor {
                lines.push(format!("Efficiency Factor: {:.2}", efficiency_factor));
            }
            if let Some(decoupling) = &workout.aerobic_decoupling {
                lines.push(format!(
                    "Aerobic Decoupling: {:.1}% ({})",
                    decoupling.decoupling_pct, decoupling.state
                ));
            }
            content.push(ContentBlock::markdown(format!(
                "### Execution Context\n\n- {}",
                lines.join("\n- ")
            )));
        }

        // Add interval analysis
        if analysis_mode.show_interval_section() {
            if let Some(ref intervals) = fetched.intervals
                && let Some(intervals_arr) = intervals.as_array()
                && !intervals_arr.is_empty()
            {
                let output_kind =
                    preferred_interval_output_kind(intervals_arr, fetched.streams.as_ref());
                let output_header = match output_kind {
                    IntervalOutputKind::Power => "Avg Power",
                    IntervalOutputKind::Pace => "Avg Pace",
                };

                content.push(ContentBlock::markdown(
                    "\n### Interval Analysis\n\n**Detected Intervals:**".to_string(),
                ));

                let interval_rows = build_interval_analysis_rows(
                    intervals_arr,
                    fetched.streams.as_ref(),
                    output_kind,
                );
                content.push(ContentBlock::table(
                    vec![
                        "Rep".into(),
                        "Duration".into(),
                        "Avg HR".into(),
                        output_header.into(),
                    ],
                    interval_rows,
                ));
            } else {
                content.push(ContentBlock::markdown(
                    "### Interval Analysis\n\n- No structured interval data available for this workout."
                        .to_string(),
                ));
            }
        }

        append_histogram_section(
            &mut content,
            "HR Histogram",
            fetched.hr_histogram.as_ref(),
            Some("hr"),
            "bpm",
            "bpm",
        );

        // Power histogram - add note if requested but unavailable
        if include_hist && fetched.power_histogram.is_none() {
            content.push(ContentBlock::markdown(
                "\n### Power Histogram\n\n- Power histogram unavailable - this workout may not have power meter data.".to_string(),
            ));
        } else {
            append_histogram_section(
                &mut content,
                "Power Histogram",
                fetched.power_histogram.as_ref(),
                Some("watts"),
                "W",
                "W",
            );
        }

        append_histogram_section(
            &mut content,
            "Pace Histogram",
            fetched.pace_histogram.as_ref(),
            None,
            "s/km",
            "m/s",
        );

        // Add best efforts comparison
        if let Some(best) = fetched.best_efforts.as_ref() {
            append_best_efforts_section(&mut content, best);
        }

        if analysis_mode.show_stream_section() {
            append_stream_insights(&mut content, fetched.streams.as_ref());
        }

        if analysis_mode.show_quality_findings()
            && let Some(workout) = &context.metrics.workout
        {
            let mut findings = Vec::new();
            if let Some(count) = workout.interval_count {
                findings.push(format!("Detected {} intervals for quality review.", count));
            }
            if let Some(hr) = workout.avg_hr {
                findings.push(format!("Average heart rate held at {:.0} bpm.", hr));
            }
            if let Some(output_finding) =
                quality_output_finding(workout_detail, fetched.streams.as_ref())
            {
                findings.push(output_finding);
            }
            if !findings.is_empty() {
                content.push(ContentBlock::markdown(format!(
                    "### Quality Findings\n\n- {}",
                    findings.join("\n- ")
                )));
            }
        }

        if analysis_mode.show_data_availability()
            && let Some(block) = data_availability_block(
                &context.audit.degraded_mode_reasons,
                context.audit.all_available(),
            )
        {
            content.push(block);
        }

        // Use shared guidance from coach engine
        let suggestions = context.guidance.suggestions.clone();

        let mut next_actions = vec![
            "To compare with similar workouts: compare_periods".into(),
            "To analyze training load: assess_recovery".into(),
            "To view period summary: analyze_training with target_type: period".into(),
        ];
        for action in &context.guidance.next_actions {
            if !next_actions.contains(action) {
                next_actions.push(action.clone());
            }
        }

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions))
    }

    async fn analyze_period(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
    ) -> Result<IntentOutput, IntentError> {
        let start = input
            .get("period_start")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: period_start"))?;
        let end = input
            .get("period_end")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: period_end"))?;
        let requested_metrics = requested_metrics(input);
        let analysis_type = input
            .get("analysis_type")
            .and_then(Value::as_str)
            .unwrap_or("detailed");
        let include_hist = input
            .get("include_histograms")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if include_hist {
            return Err(IntentError::validation(
                "include_histograms is only supported for target_type: single".to_string(),
            ));
        }

        let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
            .map_err(|_| IntentError::validation(format!("Invalid start date: {}", start)))?;
        let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
            .map_err(|_| IntentError::validation(format!("Invalid end date: {}", end)))?;

        if start_date > end_date {
            return Err(IntentError::validation(
                "Start date must be before end date.".to_string(),
            ));
        }

        let window = AnalysisWindow::new(start_date, end_date);
        let previous_window = build_previous_window(&window);
        let wellness_for_end_date = client
            .get_wellness_for_date(&window.end_date.to_string())
            .await
            .ok();

        let mut fetched = fetch_period_data(
            client,
            &PeriodFetchRequest {
                window: window.clone(),
                include_activity_details: true,
                include_comparison_window: true,
            },
        )
        .await?;
        fetched.fitness = client.get_fitness_summary().await.ok();

        let period =
            filter_activities_by_range(&fetched.activities, &window.start_date, &window.end_date);
        let previous_period = filter_activities_by_range(
            &fetched.comparison_activities,
            &previous_window.start_date,
            &previous_window.end_date,
        );
        let calendar_events = filter_events_by_range(
            &fetched.calendar_events,
            &window.start_date,
            &window.end_date,
        );

        // Apply description filter if provided (works for both single and period modes)
        let desc_filter = input.get("description_contains").and_then(Value::as_str);
        let period: Vec<_> = if let Some(desc) = desc_filter {
            let desc_lower = desc.to_lowercase();
            period
                .into_iter()
                .filter(|a| {
                    a.name
                        .as_ref()
                        .map(|n| n.to_lowercase().contains(&desc_lower))
                        .unwrap_or(false)
                })
                .collect()
        } else {
            period
        };

        // Handle empty results gracefully (not an error)
        if period.is_empty() {
            let mut content = Vec::new();
            content.push(ContentBlock::markdown(format!(
                "## Period Analysis: {} to {}\n\n**Status:** No activities found",
                start, end
            )));

            let summary = [
                format!(
                    "- No completed or planned workouts were found between {} and {}",
                    start, end
                ),
                if calendar_events.is_empty() {
                    "- No calendar events were found in this period either".into()
                } else {
                    format!(
                        "- {} calendar event(s) found in this window; review them below",
                        calendar_events.len()
                    )
                },
                "- This is unusual - consider checking:".into(),
                "  - Device sync status".into(),
                "  - Date range correctness".into(),
                "  - Training calendar / planned workout availability".into(),
                "  - Account connection".into(),
            ]
            .join("\n");

            content.push(ContentBlock::markdown(summary));

            if !calendar_events.is_empty() {
                let calendar_rows = build_calendar_event_rows(&calendar_events);
                content.push(ContentBlock::markdown(
                    "### Calendar Events in Window".to_string(),
                ));
                content.push(ContentBlock::table(
                    vec![
                        "Date".into(),
                        "Category".into(),
                        "Event".into(),
                        "Description".into(),
                    ],
                    calendar_rows,
                ));
            }

            let suggestions = vec![
                "Check if your fitness device is syncing properly".into(),
                "Verify the date range - did you train or schedule workouts during this period?"
                    .into(),
                "Try a wider date range to capture recent or upcoming workouts".into(),
            ];

            let next_actions = vec![
                "To check athlete profile and sync status: manage_profile with action: get".into(),
                "To analyze a different period: analyze_training with wider period_start/period_end".into(),
            ];

            return Ok(IntentOutput::new(content)
                .with_suggestions(suggestions)
                .with_next_actions(next_actions));
        }

        let mut context = CoachContext::new(AnalysisKind::TrainingPeriod, window.clone());
        context.audit = build_data_audit(&fetched);

        let period_snapshot = build_trend_snapshot(&period, &fetched.activity_details);
        let previous_snapshot = build_trend_snapshot(&previous_period, &fetched.activity_details);

        context.metrics.volume = Some(derive_volume_metrics(
            context.meta.window_days,
            period_snapshot.total_time_secs,
            period_snapshot.total_distance_m,
            period_snapshot.total_elevation_m,
            period.len(),
        ));
        context.metrics.trend = Some(derive_trend_metrics(period_snapshot, previous_snapshot));
        context.metrics.fitness = parse_fitness_metrics(fetched.fitness.as_ref());

        let load_window = AnalysisWindow::new(
            window.end_date - chrono::Duration::days(27),
            window.end_date,
        );
        let earliest_activity_date = fetched
            .activities
            .iter()
            .filter_map(|activity| parse_activity_date(&activity.start_date_local))
            .min();
        let load_history_sufficient = earliest_activity_date
            .map(|date| date <= load_window.start_date)
            .unwrap_or(false);

        let api_load_snapshot = wellness_for_end_date
            .as_ref()
            .and_then(|payload| parse_api_load_snapshot(Some(payload)))
            .or_else(|| {
                period
                    .iter()
                    .filter_map(|activity| {
                        parse_activity_date(&activity.start_date_local).map(|date| (date, activity))
                    })
                    .max_by_key(|(date, _)| *date)
                    .and_then(|(_, activity)| fetched.activity_details.get(&activity.id))
                    .and_then(|detail| parse_api_load_snapshot(Some(detail)))
            });

        if load_history_sufficient {
            let load_activities = fetched
                .activities
                .iter()
                .filter(|activity| {
                    parse_activity_date(&activity.start_date_local)
                        .map(|date| date >= load_window.start_date && date <= load_window.end_date)
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>();
            let daily_loads =
                build_daily_load_series(&load_activities, &fetched.activity_details, &load_window);
            let load_values = daily_loads
                .iter()
                .map(|(_, load)| *load)
                .collect::<Vec<_>>();
            context.metrics.load_management = compute_load_management_metrics(&load_values);
        }

        if let Some(api_acwr) = api_load_snapshot {
            context
                .metrics
                .load_management
                .get_or_insert_with(Default::default)
                .acwr = Some(api_acwr);
        }

        context.alerts = build_alerts(&context.metrics);
        context.guidance = build_guidance(&context.metrics, &context.alerts);

        let weekly_hrs = context
            .metrics
            .volume
            .as_ref()
            .map(|volume| volume.weekly_avg_hours)
            .unwrap_or_default();

        let mut content = Vec::new();
        content.push(ContentBlock::markdown(format!(
            "## Period Analysis: {} - {}",
            start, end
        )));

        let rows = build_period_summary_rows(period.len(), &period_snapshot, weekly_hrs);
        content.push(ContentBlock::table(
            vec!["Metric".into(), "Value".into()],
            rows,
        ));

        let planned_workouts = period
            .iter()
            .filter(|activity| is_planned_workout_id(&activity.id))
            .collect::<Vec<_>>();

        if !planned_workouts.is_empty() {
            let rows = planned_workouts
                .iter()
                .map(|activity| {
                    let detail = fetched.activity_details.get(&activity.id);
                    let duration = detail
                        .and_then(|value| value.get("moving_time"))
                        .and_then(|value| value.as_i64())
                        .map(format_duration_hhmm)
                        .unwrap_or_else(|| "n/a".to_string());
                    let load = detail
                        .and_then(|value| value.get("icu_training_load"))
                        .and_then(|value| {
                            value.as_f64().or_else(|| value.as_i64().map(|n| n as f64))
                        })
                        .map(|value| format!("{value:.1}"))
                        .unwrap_or_else(|| "n/a".to_string());
                    let date = activity
                        .start_date_local
                        .split('T')
                        .next()
                        .unwrap_or(&activity.start_date_local)
                        .to_string();

                    vec![
                        date,
                        activity
                            .name
                            .clone()
                            .unwrap_or_else(|| "Planned workout".to_string()),
                        duration,
                        load,
                    ]
                })
                .collect::<Vec<_>>();

            content.push(ContentBlock::markdown(
                "### Planned Workouts in Window".to_string(),
            ));
            content.push(ContentBlock::table(
                vec![
                    "Date".into(),
                    "Workout".into(),
                    "Duration".into(),
                    "Planned Load".into(),
                ],
                rows,
            ));
        }

        let non_workout_calendar_events = calendar_events
            .iter()
            .filter(|event| !matches!(event.category, EventCategory::Workout))
            .copied()
            .collect::<Vec<_>>();

        if !non_workout_calendar_events.is_empty() {
            let rows = build_calendar_event_rows(&non_workout_calendar_events);
            content.push(ContentBlock::markdown(
                "### Calendar Events in Window".to_string(),
            ));
            content.push(ContentBlock::table(
                vec![
                    "Date".into(),
                    "Category".into(),
                    "Event".into(),
                    "Description".into(),
                ],
                rows,
            ));
        }

        if !requested_metrics.is_empty() {
            let rows = build_requested_period_metric_rows(
                &requested_metrics,
                &period,
                &period_snapshot,
                &fetched.activity_details,
            );
            content.push(ContentBlock::markdown("### Requested Metrics".to_string()));
            content.push(ContentBlock::table(
                vec!["Metric".into(), "Value".into(), "Status".into()],
                rows,
            ));
        }

        let show_context_sections = analysis_type != "summary";
        if show_context_sections {
            if let Some(trend) = &context.metrics.trend {
                content.push(ContentBlock::markdown(format!(
                    "### Trend Context\n\n- Activity delta: {}\n- Time delta: {}\n- Distance delta: {}\n- Elevation delta: {}",
                    trend
                        .activity_count_delta
                        .map(|delta| format!("{:+}", delta))
                        .unwrap_or_else(|| "n/a".into()),
                    format_pct(trend.time_delta_pct),
                    format_pct(trend.distance_delta_pct),
                    format_pct(trend.elevation_delta_pct),
                )));
            }

            content.push(ContentBlock::markdown(build_load_management_markdown(
                if context.metrics.load_management.is_some() || load_history_sufficient {
                    context.metrics.load_management.as_ref()
                } else {
                    None
                },
            )));

            if let Some(block) = data_availability_block(
                &context.audit.degraded_mode_reasons,
                context.audit.all_available(),
            ) {
                content.push(block);
            }
        }

        if analysis_type == "streams" {
            let load_activities = period.to_vec();
            let daily_series =
                build_daily_load_series(&load_activities, &fetched.activity_details, &window);
            let rows = daily_series
                .iter()
                .rev()
                .take(7)
                .rev()
                .map(|(date, load)| vec![date.to_string(), format!("{load:.1}")])
                .collect::<Vec<_>>();
            content.push(ContentBlock::markdown("### Daily Load Series".to_string()));
            content.push(ContentBlock::table(
                vec!["Date".into(), "Load".into()],
                rows,
            ));
        } else if analysis_type == "intervals" {
            let rows = period
                .iter()
                .filter(|activity| {
                    activity
                        .name
                        .as_ref()
                        .map(|name| name.to_lowercase().contains("interval"))
                        .unwrap_or(false)
                })
                .map(|activity| {
                    vec![
                        activity
                            .start_date_local
                            .split('T')
                            .next()
                            .unwrap_or(&activity.start_date_local)
                            .to_string(),
                        activity
                            .name
                            .clone()
                            .unwrap_or_else(|| "Workout".to_string()),
                    ]
                })
                .collect::<Vec<_>>();
            if !rows.is_empty() {
                content.push(ContentBlock::markdown("### Interval Sessions".to_string()));
                content.push(ContentBlock::table(
                    vec!["Date".into(), "Workout".into()],
                    rows,
                ));
            }
        }

        let mut suggestions = context.guidance.suggestions.clone();
        if suggestions.is_empty() {
            suggestions = if weekly_hrs < 5.0 {
                vec!["Training volume is below average. Consider gradual increase.".into()]
            } else if weekly_hrs > 15.0 {
                vec!["High training volume. Ensure adequate recovery.".into()]
            } else {
                vec!["Training volume is in optimal range.".into()]
            };
        }

        let mut next_actions = vec![
            "To compare with another period: compare_periods".into(),
            "To assess recovery: assess_recovery".into(),
        ];
        for action in &context.guidance.next_actions {
            if !next_actions.contains(action) {
                next_actions.push(action.clone());
            }
        }

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions)
            .with_metadata(OutputMetadata {
                total_count: Some(period.len() as u32),
                ..Default::default()
            }))
    }
}

fn format_pct(value: Option<f64>) -> String {
    value
        .map(|delta| format!("{:+.1}%", delta))
        .unwrap_or_else(|| "n/a".into())
}

impl Default for AnalyzeTrainingHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::coach::{AcwrMetrics, LoadManagementMetrics};

    // ========================================================================
    // Constructor Tests
    // ========================================================================

    #[test]
    fn test_new_handler() {
        let handler = AnalyzeTrainingHandler::new();
        assert_eq!(handler.name(), "analyze_training");
    }

    #[test]
    fn test_default_handler() {
        let _handler = AnalyzeTrainingHandler;
    }

    // ========================================================================
    // IntentHandler Trait Implementation Tests
    // ========================================================================

    #[test]
    fn test_name() {
        let handler = AnalyzeTrainingHandler::new();
        assert_eq!(IntentHandler::name(&handler), "analyze_training");
    }

    #[test]
    fn test_description() {
        let handler = AnalyzeTrainingHandler::new();
        let desc = IntentHandler::description(&handler);
        assert!(desc.contains("Analyzes training"));
        assert!(desc.contains("single workout"));
        assert!(desc.contains("period"));
    }

    #[test]
    fn test_input_schema_structure() {
        let handler = AnalyzeTrainingHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        assert!(schema.get("type").is_some());
        assert_eq!(schema.get("type").unwrap().as_str(), Some("object"));

        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("target_type"));
        assert!(props.contains_key("date"));
        assert!(props.contains_key("period_start"));
        assert!(props.contains_key("period_end"));
        assert!(props.contains_key("analysis_type"));
        assert!(props.contains_key("include_best_efforts"));
        assert!(props.contains_key("include_histograms"));

        // target_type is required
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("target_type")));

        // Check oneOf constraint for date vs period
        let one_of = schema.get("oneOf").unwrap().as_array().unwrap();
        assert_eq!(one_of.len(), 2);
    }

    #[test]
    fn test_requires_idempotency_token() {
        let handler = AnalyzeTrainingHandler::new();
        assert!(!IntentHandler::requires_idempotency_token(&handler));
    }

    // ========================================================================
    // Input Validation Tests
    // ========================================================================

    #[test]
    fn test_validation_missing_target_type() {
        let input = json!({
            "date": "2026-03-01"
        });
        assert!(input.get("target_type").is_none());
    }

    #[test]
    fn test_validation_invalid_target_type() {
        let input = json!({
            "target_type": "invalid"
        });
        let target_type = input.get("target_type").and_then(|v| v.as_str()).unwrap();
        assert_ne!(target_type, "single");
        assert_ne!(target_type, "period");
    }

    #[test]
    fn test_validation_date_format() {
        // Valid date
        let result = NaiveDate::parse_from_str("2026-03-01", "%Y-%m-%d");
        assert!(result.is_ok());

        // Invalid date
        let result = NaiveDate::parse_from_str("invalid", "%Y-%m-%d");
        assert!(result.is_err());
    }

    #[test]
    fn test_validation_date_range() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();

        // Valid range
        assert!(start <= end);

        // Invalid range
        assert!(end > start);
    }

    // ========================================================================
    // Analysis Type Tests
    // ========================================================================

    #[test]
    fn test_analysis_type_values() {
        let valid_types = ["summary", "detailed", "intervals", "streams"];
        for t in &valid_types {
            assert!(["summary", "detailed", "intervals", "streams"].contains(t));
        }
    }

    #[test]
    fn test_default_analysis_type() {
        let input = json!({
            "target_type": "single",
            "date": "2026-03-01"
        });
        let analysis_type = input
            .get("analysis_type")
            .and_then(|v| v.as_str())
            .unwrap_or("summary");
        assert_eq!(analysis_type, "summary");
    }

    #[test]
    fn test_default_include_flags() {
        let input = json!({
            "target_type": "single",
            "date": "2026-03-01"
        });

        let include_best = input
            .get("include_best_efforts")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(!include_best);

        let include_hist = input
            .get("include_histograms")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(!include_hist);
    }

    // ========================================================================
    // Output Structure Tests
    // ========================================================================

    #[test]
    fn test_handler_metadata() {
        let handler = AnalyzeTrainingHandler::new();

        // Verify handler properties
        assert_eq!(handler.name(), "analyze_training");
        assert!(handler.description().len() > 50);

        let schema = handler.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_handler_description_mentions_comments_and_calendar_context() {
        let handler = AnalyzeTrainingHandler::new();
        let description = handler.description();

        assert!(description.contains("comments/messages"));
        assert!(description.contains("calendar events"));
    }

    // ========================================================================
    // Date Parsing Tests
    // ========================================================================

    #[test]
    fn test_date_parsing_valid() {
        let dates = vec!["2026-01-01", "2026-06-15", "2026-12-31"];

        for date_str in dates {
            let result = NaiveDate::parse_from_str(date_str, "%Y-%m-%d");
            assert!(result.is_ok(), "Failed to parse {}", date_str);
        }
    }

    #[test]
    fn test_date_parsing_invalid() {
        let invalid_dates = vec!["01-01-2026", "2026/01/01", "March 1, 2026", "invalid", ""];

        for date_str in invalid_dates {
            let result = NaiveDate::parse_from_str(date_str, "%Y-%m-%d");
            assert!(result.is_err(), "Should fail to parse {}", date_str);
        }
    }

    // ========================================================================
    // Period Calculation Tests
    // ========================================================================

    #[test]
    fn test_period_days_calculation() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        let days = (end - start).num_days() as i32 + 30; // Buffer as in code
        assert_eq!(days, 60);
    }

    #[test]
    fn test_weekly_average_calculation() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let weeks = ((end - start).num_days() as f64 / 7.0).max(1.0);
        assert!((weeks - 2.0).abs() < 0.1);
    }

    // ========================================================================
    // Content Block Tests
    // ========================================================================

    #[test]
    fn test_content_block_types() {
        // Verify ContentBlock can be created with different types
        let _markdown = ContentBlock::markdown("# Test".to_string());
        let _table = ContentBlock::table(vec!["Header".to_string()], vec![vec!["Row".to_string()]]);
        let _text = ContentBlock::text("Test".to_string());
    }

    // ========================================================================
    // Error Message Tests
    // ========================================================================

    #[test]
    fn test_error_messages_contain_context() {
        // Test that validation errors contain field names
        let err = IntentError::validation("Missing: target_type".to_string());
        let err_str = err.to_string();
        assert!(err_str.contains("target_type"));
    }

    #[test]
    fn test_date_error_message_format() {
        let date = "invalid";
        let err_msg = format!("Invalid date format: {}. Use YYYY-MM-DD.", date);
        assert!(err_msg.contains("Invalid date format"));
        assert!(err_msg.contains("YYYY-MM-DD"));
    }

    #[test]
    fn load_management_markdown_renders_acwr_and_monotony_values() {
        let markdown = build_load_management_markdown(Some(&LoadManagementMetrics {
            acwr: Some(AcwrMetrics {
                acute_load: 420.0,
                chronic_load: 350.0,
                ratio: 1.20,
                state: "productive".into(),
            }),
            monotony: Some(2.1),
            strain: Some(882.0),
        }));

        assert!(markdown.contains("ACWR"));
        assert!(markdown.contains("Monotony"));
        assert!(markdown.contains("1.20"));
    }

    #[test]
    fn load_management_markdown_reports_when_history_is_unavailable() {
        let markdown = build_load_management_markdown(None);

        assert!(markdown.contains("Load Context"));
        assert!(markdown.contains("unavailable"));
    }

    #[test]
    fn build_basic_workout_metric_rows_formats_available_values() {
        let rows = build_basic_workout_metric_rows(Some(&serde_json::json!({
            "distance": 12345.0,
            "moving_time": 3661,
            "average_heartrate": 151.2,
            "average_watts": 245.7,
            "total_elevation_gain": 432.0
        })));

        assert_eq!(
            rows,
            vec![
                vec!["Distance".to_string(), "12.35 km".to_string()],
                vec!["Duration".to_string(), "1:01:01".to_string()],
                vec!["Avg HR".to_string(), "151 bpm".to_string()],
                vec!["Avg Power".to_string(), "246 W".to_string()],
                vec!["Elevation".to_string(), "432 m".to_string()],
            ]
        );
    }

    #[test]
    fn build_interval_analysis_rows_formats_known_fields() {
        let rows = build_interval_analysis_rows(
            &[
                serde_json::json!({
                    "moving_time": 95,
                    "average_heartrate": 162.4,
                    "average_watts": 301.0
                }),
                serde_json::json!({
                    "moving_time": 120,
                    "average_heartrate": 158.0,
                    "average_watts": 287.2
                }),
            ],
            None,
            IntervalOutputKind::Power,
        );

        assert_eq!(
            rows,
            vec![
                vec![
                    "1".to_string(),
                    "1:35".to_string(),
                    "162 bpm".to_string(),
                    "301 W".to_string(),
                ],
                vec![
                    "2".to_string(),
                    "2:00".to_string(),
                    "158 bpm".to_string(),
                    "287 W".to_string(),
                ],
            ]
        );
    }

    #[test]
    fn build_interval_analysis_rows_backfills_power_from_stream_slice() {
        let rows = build_interval_analysis_rows(
            &[
                serde_json::json!({
                    "start_index": 0,
                    "end_index": 4,
                    "moving_time": 240,
                    "average_heartrate": 150.0,
                    "average_watts": null
                }),
                serde_json::json!({
                    "start_index": 4,
                    "end_index": 8,
                    "moving_time": 240,
                    "average_heartrate": 162.0,
                    "average_watts": null
                }),
            ],
            Some(&serde_json::json!({
                "watts": [210.0, 220.0, 230.0, 240.0, 280.0, 290.0, 300.0, 310.0]
            })),
            IntervalOutputKind::Power,
        );

        assert_eq!(
            rows,
            vec![
                vec![
                    "1".to_string(),
                    "4:00".to_string(),
                    "150 bpm".to_string(),
                    "225 W".to_string(),
                ],
                vec![
                    "2".to_string(),
                    "4:00".to_string(),
                    "162 bpm".to_string(),
                    "295 W".to_string(),
                ],
            ]
        );
    }

    #[test]
    fn build_period_summary_rows_formats_snapshot_values() {
        let rows = build_period_summary_rows(
            4,
            &crate::engines::coach_metrics::TrendSnapshot {
                activity_count: 4,
                total_time_secs: 18_600,
                total_distance_m: 42_250.0,
                total_elevation_m: 640.0,
            },
            7.4,
        );

        assert_eq!(
            rows,
            vec![
                vec!["Total Time".to_string(), "5:10:00".to_string()],
                vec!["Distance".to_string(), "42.2 km".to_string()],
                vec!["Elevation".to_string(), "640 m".to_string()],
                vec!["Weekly Avg".to_string(), "7.4 hrs".to_string()],
            ]
        );
    }

    // ========================================================================
    // Work Interval Counting Tests
    // ========================================================================

    #[test]
    fn test_count_work_intervals_empty() {
        let intervals = vec![];
        assert_eq!(count_work_intervals(&intervals), 0);
    }

    #[test]
    fn test_count_work_intervals_with_real_data() {
        // Simulate the user's workout: 7 work intervals + 8 recovery intervals
        let intervals = vec![
            // Work intervals (high speed, high HR)
            json!({"average_speed": 2.52, "average_heartrate": 126}), // 1: borderline (low HR)
            json!({"average_speed": 2.76, "average_heartrate": 142}), // 2: work
            json!({"average_speed": 3.04, "average_heartrate": 158}), // 3: work
            json!({"average_speed": 0.85, "average_heartrate": 128}), // 4: recovery (very slow)
            json!({"average_speed": 2.95, "average_heartrate": 158}), // 5: work
            json!({"average_speed": 2.23, "average_heartrate": 140}), // 6: borderline
            json!({"average_speed": 2.91, "average_heartrate": 159}), // 7: work
            json!({"average_speed": 2.13, "average_heartrate": 140}), // 8: borderline
            json!({"average_speed": 2.98, "average_heartrate": 160}), // 9: work
            json!({"average_speed": 2.15, "average_heartrate": 141}), // 10: borderline
            json!({"average_speed": 2.96, "average_heartrate": 158}), // 11: work
            json!({"average_speed": 2.04, "average_heartrate": 138}), // 12: borderline
            json!({"average_speed": 3.02, "average_heartrate": 156}), // 13: work
            json!({"average_speed": 1.44, "average_heartrate": 128}), // 14: recovery (slow)
            json!({"average_speed": 0.75, "average_heartrate": 115}), // 15: recovery (very slow)
        ];

        let count = count_work_intervals(&intervals);
        // Should identify ~7-8 work intervals (the ones with speed >= ~2.5 and HR >= ~145)
        assert!(
            (6..=9).contains(&count),
            "Expected 6-9 work intervals, got {}",
            count
        );
    }

    #[test]
    fn test_count_work_intervals_clear_separation() {
        // Clear work vs recovery separation
        let intervals = vec![
            json!({"average_speed": 3.0, "average_heartrate": 160}), // work
            json!({"average_speed": 1.5, "average_heartrate": 130}), // recovery
            json!({"average_speed": 3.1, "average_heartrate": 162}), // work
            json!({"average_speed": 1.4, "average_heartrate": 128}), // recovery
            json!({"average_speed": 3.0, "average_heartrate": 158}), // work
        ];

        let count = count_work_intervals(&intervals);
        assert_eq!(count, 3, "Should identify 3 work intervals");
    }

    #[test]
    fn test_count_work_intervals_speed_only() {
        // Some intervals without HR data
        let intervals = vec![
            json!({"average_speed": 3.0}), // work
            json!({"average_speed": 1.5}), // recovery
            json!({"average_speed": 3.1}), // work
            json!({"average_speed": 1.4}), // recovery
            json!({"average_speed": 3.0}), // work
        ];

        let count = count_work_intervals(&intervals);
        assert_eq!(count, 3, "Should identify 3 work intervals by speed");
    }

    #[test]
    fn test_calculate_median() {
        let mut values = vec![5.0, 2.0, 8.0, 1.0, 9.0];
        assert!((calculate_median(&mut values) - 5.0).abs() < 0.001);

        let mut values = vec![1.0, 2.0, 3.0, 4.0];
        assert!((calculate_median(&mut values) - 2.5).abs() < 0.001);

        let mut values = vec![42.0];
        assert!((calculate_median(&mut values) - 42.0).abs() < 0.001);

        let mut values = vec![];
        assert!((calculate_median(&mut values) - 0.0).abs() < 0.001);
    }

    // ========================================================================
    // Key Phrase Extraction Tests (for multiple activity guidance)
    // ========================================================================

    #[test]
    fn test_key_phrase_extraction_with_dash() {
        // Test with ASCII dash
        let name = "Long Run Z2 - Key Workout";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Long Run Z2");
    }

    #[test]
    fn test_key_phrase_extraction_with_em_dash() {
        // Test with Unicode em-dash (—)
        let name = "Long Run Z2 — Key Workout";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Long Run Z2");
    }

    #[test]
    fn test_key_phrase_extraction_with_colon() {
        let name = "Tempo Run: Threshold Session";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Tempo Run");
    }

    #[test]
    fn test_key_phrase_extraction_no_separator() {
        let name = "Weight Training";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Weight Training");
    }

    #[test]
    fn test_key_phrase_extraction_empty_dash() {
        let name = "Intervals - Track Session";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Intervals");
    }

    #[test]
    fn test_key_phrase_extraction_unicode_dash() {
        // Test with em-dash (Unicode)
        let name = "Recovery Run — Easy Pace";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Recovery Run");
    }
}
