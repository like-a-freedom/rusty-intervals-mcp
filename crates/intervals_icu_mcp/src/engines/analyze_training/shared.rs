use crate::content::{ContentBlock, OutputMetadata};
use crate::domains::coach::EtvsMetrics;
use crate::domains::interval_detection::RawStream;
use crate::domains::interval_segment::SportPresentation;
use crate::domains::load::{ComparableLoadSeries, LoadSource};
use crate::engines::analysis::PeriodSummary;
use crate::engines::analysis_fetch::activity_load;
use intervals_icu_client::ActivitySummary;
use serde_json::Value;
use std::collections::HashMap;

/// Report produced by the analysis engine.
pub struct AnalyzeReport {
    pub content: Vec<ContentBlock>,
    pub suggestions: Vec<String>,
    pub next_actions: Vec<String>,
    pub metadata: Option<OutputMetadata>,
}

impl AnalyzeReport {
    pub fn new(content: Vec<ContentBlock>) -> Self {
        Self {
            content,
            suggestions: Vec::new(),
            next_actions: Vec::new(),
            metadata: None,
        }
    }

    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
        self
    }

    pub fn with_next_actions(mut self, next_actions: Vec<String>) -> Self {
        self.next_actions = next_actions;
        self
    }

    pub fn with_metadata(mut self, metadata: OutputMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn into_intent_output(self) -> crate::intents::IntentOutput {
        let mut out = crate::intents::IntentOutput::new(self.content);
        out = out.with_suggestions(self.suggestions);
        out = out.with_next_actions(self.next_actions);
        if let Some(meta) = self.metadata {
            out = out.with_metadata(meta);
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleAnalysisMode {
    Summary,
    Detailed,
    Intervals,
    Streams,
}

impl SingleAnalysisMode {
    pub(crate) fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("summary") {
            "detailed" => Self::Detailed,
            "intervals" => Self::Intervals,
            "streams" => Self::Streams,
            _ => Self::Summary,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Detailed => "detailed",
            Self::Intervals => "intervals",
            Self::Streams => "streams",
        }
    }

    pub(crate) fn include_intervals(self) -> bool {
        matches!(self, Self::Intervals)
    }

    pub(crate) fn include_streams(self) -> bool {
        matches!(self, Self::Detailed | Self::Intervals | Self::Streams)
    }

    pub(crate) fn show_execution_context(self) -> bool {
        matches!(self, Self::Detailed | Self::Streams)
    }

    pub(crate) fn show_interval_section(self) -> bool {
        matches!(self, Self::Intervals)
    }

    pub(crate) fn show_stream_section(self) -> bool {
        matches!(self, Self::Streams)
    }

    pub(crate) fn show_quality_findings(self) -> bool {
        matches!(self, Self::Detailed | Self::Streams)
    }

    pub(crate) fn show_data_availability(self) -> bool {
        matches!(self, Self::Detailed | Self::Streams)
    }

    pub(crate) fn show_detailed_breakdown(self) -> bool {
        matches!(self, Self::Detailed)
    }
}

pub(crate) fn build_load_data_quality_section(series: &ComparableLoadSeries) -> ContentBlock {
    let mut lines = vec!["Training Load Data Quality".to_string()];

    if series.activities_total == 0 {
        lines.push("  No activities in period".to_string());
        return ContentBlock::markdown(lines.join("\n"));
    }

    let loaded = series.activities_with_load;
    let total = series.activities_total;
    let pct = (loaded as f64 / total as f64) * 100.0;

    lines.push(format!(
        "  Loaded: {} / {} activities ({:.0}%)",
        loaded, total, pct
    ));

    if loaded < total {
        lines.push(format!(
            "  {} activities without API load excluded",
            total - loaded
        ));
    }

    for (source, count) in &series.source_counts {
        let label = match source {
            LoadSource::IcuTrainingLoad => "icu_training_load",
            LoadSource::TrainingLoadAlias => "training_load",
            LoadSource::TssAlias => "tss",
            LoadSource::ActivitySummaryTrainingLoad => "summary training_load",
        };
        lines.push(format!(
            "  {}: {} {}",
            label,
            count,
            if *count == 1 {
                "activity"
            } else {
                "activities"
            }
        ));
    }

    lines.push("  Activities without API load are excluded from load totals".to_string());

    ContentBlock::markdown(lines.join("\n"))
}

pub(crate) fn numeric_series(streams: &Value, keys: &[&str]) -> Option<Vec<f64>> {
    keys.iter().find_map(|key| {
        streams
            .get(*key)
            .and_then(Value::as_array)
            .and_then(|values| values.iter().map(Value::as_f64).collect::<Option<Vec<_>>>())
    })
}

pub(crate) fn build_local_raw_stream(streams: &Value) -> Option<RawStream> {
    let time_s = numeric_series(streams, &["time", "time_s"])?;
    let speed = numeric_series(streams, &["velocity_smooth", "speed", "pace"])?;
    let heartrate = numeric_series(streams, &["heartrate", "hr"])?;
    if time_s.is_empty() || time_s.len() != speed.len() || time_s.len() != heartrate.len() {
        return None;
    }
    let power = numeric_series(streams, &["watts", "power"]);
    if power
        .as_ref()
        .is_some_and(|values| values.len() != time_s.len())
    {
        return None;
    }

    Some(RawStream {
        time_s,
        speed,
        heartrate,
        power,
    })
}

pub(crate) fn sport_presentation(workout_detail: Option<&Value>) -> SportPresentation {
    let type_val = workout_detail
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("type"))
        .and_then(Value::as_str);

    match type_val {
        Some("Run" | "TrailRun" | "VirtualRun" | "Walk" | "Hike") => SportPresentation::Pace,
        Some("Ride" | "VirtualRide" | "MountainBikeRide" | "GravelRide" | "EBikeRide") => {
            SportPresentation::Speed
        }
        _ => SportPresentation::Unknown,
    }
}

pub(crate) struct PeriodStats {
    pub snapshot: crate::engines::coach_metrics::TrendSnapshot,
    pub window_days: i64,
    pub activities: Vec<ActivitySummary>,
    pub activity_details: HashMap<String, Value>,
    pub planned_count: usize,
    pub etvs: Option<EtvsMetrics>,
}

pub(crate) fn build_period_summary(stats: &PeriodStats) -> PeriodSummary {
    let total_tss: f32 = stats
        .activities
        .iter()
        .filter_map(|activity| {
            let detail = stats.activity_details.get(&activity.id);
            activity_load(activity, detail).map(|obs| obs.value as f32)
        })
        .sum::<f32>();

    let total_time_hours = stats.snapshot.total_time_secs as f32 / 3600.0;
    let weeks = (stats.window_days.max(1) as f32 / 7.0).max(1.0);

    PeriodSummary {
        workout_count: stats.snapshot.activity_count as u32,
        total_time_hours,
        total_distance_km: stats.snapshot.total_distance_m as f32 / 1000.0,
        total_elevation_m: stats.snapshot.total_elevation_m as f32,
        avg_weekly_hours: total_time_hours / weeks,
        total_tss,
        avg_tss_per_week: total_tss / weeks,
    }
}

pub(crate) fn matches_workout_type(activity: &ActivitySummary, filter: &str) -> bool {
    let Some(name) = activity.name.as_ref() else {
        return false;
    };
    let name = name.to_lowercase();
    match filter.to_lowercase().as_str() {
        "intervals" => name.contains("interval"),
        "tempo" => name.contains("tempo"),
        "long_run" | "long run" => name.contains("long"),
        other => name.contains(other),
    }
}

pub(crate) fn requested_metric_value(metric: &str, stats: &PeriodStats) -> (String, String) {
    match metric {
        "volume" => (
            format!("{:.1} h", stats.snapshot.total_time_secs as f64 / 3600.0),
            "derived from total moving time".into(),
        ),
        "pace" => {
            if stats.snapshot.total_distance_m > 0.0 && stats.snapshot.total_time_secs > 0 {
                let secs_per_km = stats.snapshot.total_time_secs as f64
                    / (stats.snapshot.total_distance_m / 1000.0);
                let rounded = secs_per_km.round() as i64;
                (
                    format!("{}:{:02} /km", rounded / 60, rounded % 60),
                    "derived".into(),
                )
            } else {
                ("n/a".into(), "distance/time unavailable".into())
            }
        }
        "hr" => {
            let values = stats
                .activities
                .iter()
                .filter_map(|activity| {
                    stats
                        .activity_details
                        .get(&activity.id)
                        .and_then(|detail| detail.get("average_heartrate"))
                        .and_then(|value| value.as_f64())
                })
                .collect::<Vec<_>>();
            if values.is_empty() {
                ("n/a".into(), "average HR unavailable".into())
            } else {
                let avg = values.iter().sum::<f64>() / values.len() as f64;
                (
                    format!("{avg:.0} bpm"),
                    "average of activity HR values".into(),
                )
            }
        }
        "tss" => {
            let sum = stats
                .activities
                .iter()
                .filter_map(|activity| {
                    stats
                        .activity_details
                        .get(&activity.id)
                        .and_then(|detail| detail.get("icu_training_load"))
                        .and_then(|value| {
                            value.as_f64().or_else(|| value.as_i64().map(|n| n as f64))
                        })
                })
                .sum::<f64>();
            (format!("{sum:.1}"), "sum of training load".into())
        }
        "intensity" => {
            let total_tss: f64 = stats
                .activities
                .iter()
                .filter_map(|activity| {
                    stats
                        .activity_details
                        .get(&activity.id)
                        .and_then(|detail| detail.get("icu_training_load"))
                        .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|n| n as f64)))
                })
                .sum();
            let weeks = (stats.window_days as f64 / 7.0).max(1.0);
            let weekly_avg = total_tss / weeks;
            (
                format!("{weekly_avg:.0} TSS/wk"),
                "weekly average training load".into(),
            )
        }
        "zones" => {
            let mut zone_totals: HashMap<String, i64> = HashMap::new();
            for activity in &stats.activities {
                if let Some(detail) = stats.activity_details.get(&activity.id)
                    && let Some(zone_times) =
                        detail.get("icu_zone_times").and_then(|v| v.as_array())
                {
                    for zt in zone_times {
                        if let (Some(id), Some(secs)) = (
                            zt.get("id").and_then(|v| v.as_str()),
                            zt.get("secs").and_then(|v| v.as_i64()),
                        ) {
                            *zone_totals.entry(id.to_string()).or_default() += secs;
                        }
                    }
                }
            }
            if zone_totals.is_empty() {
                ("n/a".into(), "zone times unavailable".into())
            } else {
                let mut sorted: Vec<_> = zone_totals.into_iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                let parts: Vec<String> = sorted
                    .iter()
                    .map(|(id, secs)| {
                        let mins = *secs as f64 / 60.0;
                        format!("{}: {:.0}m", id, mins)
                    })
                    .collect();
                (parts.join(", "), "aggregated from icu_zone_times".into())
            }
        }
        "etvs" => stats
            .etvs
            .as_ref()
            .map(|metrics| {
                let coverage = metrics
                    .coverage_ratio
                    .map(|ratio| format!("{:.1}%", ratio * 100.0))
                    .unwrap_or_else(|| "unknown".into());
                (
                    format!("{:.1} weighted min", metrics.score_weighted_minutes),
                    format!(
                        "{} coverage ({}/{} activities)",
                        coverage, metrics.activities_with_zone_data, metrics.activities_total,
                    ),
                )
            })
            .unwrap_or_else(|| ("n/a".into(), "zone times unavailable".into())),
        other => ("n/a".into(), format!("metric '{}' not yet modeled", other)),
    }
}

pub(crate) fn requested_metric_label(metric: &str) -> String {
    match metric {
        "hr" => "HR".to_string(),
        "tss" => "TSS".to_string(),
        "pace" => "Pace".to_string(),
        "volume" => "Volume".to_string(),
        "zones" => "Zones".to_string(),
        "intensity" => "Intensity".to_string(),
        "etvs" => "ETVS".to_string(),
        other => other.replace('_', " ").to_uppercase(),
    }
}
