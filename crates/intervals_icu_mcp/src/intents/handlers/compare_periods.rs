use crate::intents::{ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput};
use async_trait::async_trait;
use intervals_icu_client::{ActivitySummary, IntervalsClient};
use serde_json::{Value, json};
/// Compare Periods Intent Handler
///
/// Compares performance between two periods (like-for-like).
use std::sync::Arc;

use crate::domains::coach::AnalysisWindow;
use crate::engines::analysis_fetch::{PeriodFetchRequest, fetch_period_data};
use crate::engines::coach_metrics::{
    TrendSnapshot, build_trend_snapshot, compute_consistency_index, derive_trend_metrics,
    derive_volume_metrics,
};
use crate::intents::utils::{filter_activities_by_range, parse_date};

pub struct ComparePeriodsHandler;
impl ComparePeriodsHandler {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl IntentHandler for ComparePeriodsHandler {
    fn name(&self) -> &'static str {
        "compare_periods"
    }

    fn description(&self) -> &'static str {
        "Compares performance between two periods (like-for-like). \
         Use for comparing similar workouts or periods, assessing progress, and identifying trends."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period_a_start": {"type": "string", "description": "Period A start (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday')"},
                "period_a_end": {"type": "string", "description": "Period A end (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday')"},
                "period_a_label": {"type": "string", "description": "Period A label"},
                "period_b_start": {"type": "string", "description": "Period B start (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday')"},
                "period_b_end": {"type": "string", "description": "Period B end (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday')"},
                "period_b_label": {"type": "string", "description": "Period B label"},
                "workout_type": {"type": "string", "description": "Filter by type: tempo, intervals, long_run"},
                "metrics": {"type": "array", "items": {"type": "string"}, "description": "Metrics: volume, intensity, zones, pace, hr, tss"}
            },
            "required": ["period_a_start", "period_a_end", "period_b_start", "period_b_end"]
        })
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let a_start = input
            .get("period_a_start")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing: period_a_start"))?;
        let a_end = input
            .get("period_a_end")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing: period_a_end"))?;
        let b_start = input
            .get("period_b_start")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing: period_b_start"))?;
        let b_end = input
            .get("period_b_end")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing: period_b_end"))?;

        let a_label = input
            .get("period_a_label")
            .and_then(Value::as_str)
            .unwrap_or("Period A");
        let b_label = input
            .get("period_b_label")
            .and_then(Value::as_str)
            .unwrap_or("Period B");
        let workout_type = input.get("workout_type").and_then(Value::as_str);
        let requested_metrics = input
            .get("metrics")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();

        let a_stats = self
            .get_period_stats(client.as_ref(), a_start, a_end, workout_type)
            .await?;
        let b_stats = self
            .get_period_stats(client.as_ref(), b_start, b_end, workout_type)
            .await?;
        let trend = derive_trend_metrics(a_stats.snapshot, b_stats.snapshot);
        let a_volume = derive_volume_metrics(
            a_stats.window_days,
            a_stats.snapshot.total_time_secs,
            a_stats.snapshot.total_distance_m,
            a_stats.snapshot.total_elevation_m,
            a_stats.snapshot.activity_count,
        );

        let a_consistency =
            compute_consistency_index(a_stats.snapshot.activity_count, a_stats.planned_count);
        let b_consistency =
            compute_consistency_index(b_stats.snapshot.activity_count, b_stats.planned_count);

        let mut content = Vec::new();
        content.push(ContentBlock::markdown(format!(
            "## Comparison: {} vs {}",
            a_label, b_label
        )));

        // Build comparison table
        let mut rows = vec![vec![
            "Metric".into(),
            a_label.into(),
            b_label.into(),
            "Δ".into(),
        ]];

        let count_delta =
            a_stats.snapshot.activity_count as i32 - b_stats.snapshot.activity_count as i32;
        rows.push(vec![
            "Activities".into(),
            a_stats.snapshot.activity_count.to_string(),
            b_stats.snapshot.activity_count.to_string(),
            if count_delta >= 0 {
                format!("+{}", count_delta)
            } else {
                count_delta.to_string()
            },
        ]);

        let time_delta = a_stats.snapshot.total_time_secs - b_stats.snapshot.total_time_secs;
        rows.push(vec![
            "Total Time".into(),
            format_duration(a_stats.snapshot.total_time_secs),
            format_duration(b_stats.snapshot.total_time_secs),
            if time_delta >= 0 {
                format!("+{} min", time_delta / 60)
            } else {
                format!("{} min", time_delta / 60)
            },
        ]);

        let dist_delta = a_stats.snapshot.total_distance_m - b_stats.snapshot.total_distance_m;
        rows.push(vec![
            "Distance (km)".into(),
            format!("{:.1}", a_stats.snapshot.total_distance_m / 1000.0),
            format!("{:.1}", b_stats.snapshot.total_distance_m / 1000.0),
            if dist_delta >= 0.0 {
                format!("+{:.1}", dist_delta / 1000.0)
            } else {
                format!("{:.1}", dist_delta / 1000.0)
            },
        ]);

        let elev_delta = a_stats.snapshot.total_elevation_m - b_stats.snapshot.total_elevation_m;
        rows.push(vec![
            "Elevation (m)".into(),
            format!("{:.0}", a_stats.snapshot.total_elevation_m),
            format!("{:.0}", b_stats.snapshot.total_elevation_m),
            if elev_delta >= 0.0 {
                format!("+{:.0}", elev_delta)
            } else {
                format!("{:.0}", elev_delta)
            },
        ]);

        content.push(ContentBlock::table(rows[0].clone(), rows[1..].to_vec()));
        if !requested_metrics.is_empty() {
            let rows = requested_metrics
                .iter()
                .map(|metric| {
                    let (a_value, note) = requested_metric_value(metric, &a_stats);
                    let (b_value, _) = requested_metric_value(metric, &b_stats);
                    vec![requested_metric_label(metric), a_value, b_value, note]
                })
                .collect::<Vec<_>>();
            content.push(ContentBlock::markdown("### Requested Metrics".to_string()));
            content.push(ContentBlock::table(
                vec![
                    "Metric".into(),
                    a_label.into(),
                    b_label.into(),
                    "Status".into(),
                ],
                rows,
            ));
        }
        content.push(ContentBlock::markdown(format!(
            "### Trend Context\n\n- Activity delta: {}\n- Time delta: {}\n- Distance delta: {}\n- Elevation delta: {}\n- Current period weekly average: {:.1} hrs\n- {} consistency: {} ({:.0}% of {} planned sessions)\n- {} consistency: {} ({:.0}% of {} planned sessions)",
            trend
                .activity_count_delta
                .map(|delta| format!("{:+}", delta))
                .unwrap_or_else(|| "n/a".into()),
            format_pct(trend.time_delta_pct),
            format_pct(trend.distance_delta_pct),
            format_pct(trend.elevation_delta_pct),
            a_volume.weekly_avg_hours,
            a_label,
            a_consistency.state.as_deref().unwrap_or("unknown"),
            a_consistency.ratio.unwrap_or(0.0) * 100.0,
            a_stats.planned_count,
            b_label,
            b_consistency.state.as_deref().unwrap_or("unknown"),
            b_consistency.ratio.unwrap_or(0.0) * 100.0,
            b_stats.planned_count,
        )));

        // Analysis
        let volume_change = if let Some(time_delta_pct) = trend.time_delta_pct {
            time_delta_pct as f32
        } else if let Some(distance_delta_pct) = trend.distance_delta_pct {
            distance_delta_pct as f32
        } else {
            0.0
        };

        let mut suggestions = Vec::new();
        if volume_change.abs() <= 10.0 {
            suggestions.push(format!(
                "Volume change: {:+.0}% - within normal range (+7-10%/week)",
                volume_change
            ));
        } else if volume_change > 10.0 {
            suggestions.push(format!(
                "Volume increased by {:.0}% - monitor for overtraining",
                volume_change
            ));
        } else {
            suggestions.push(format!(
                "Volume decreased by {:.0}% - may indicate recovery or illness",
                volume_change.abs()
            ));
        }

        if let Some(elev_delta) = trend.elevation_delta_pct
            && elev_delta.abs() > 30.0
        {
            suggestions.push(format!(
                "Elevation change: {:+.0}% - consider extra recovery and hill-specific work",
                elev_delta
            ));
        }

        let mut next_actions = vec![
            "To analyze a specific period: analyze_training with target_type: period".into(),
            "To assess recovery: assess_recovery".into(),
        ];

        if volume_change > 15.0 {
            next_actions.insert(0, "Consider recovery week if volume spike continues".into());
        }

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions))
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}

struct PeriodStats {
    snapshot: TrendSnapshot,
    window_days: i64,
    activities: Vec<ActivitySummary>,
    activity_details: std::collections::HashMap<String, Value>,
    planned_count: usize,
}

fn matches_workout_type(activity: &ActivitySummary, filter: &str) -> bool {
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

fn requested_metric_value(metric: &str, stats: &PeriodStats) -> (String, String) {
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
            let mut zone_totals: std::collections::HashMap<String, i64> =
                std::collections::HashMap::new();
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
        other => ("n/a".into(), format!("metric '{}' not yet modeled", other)),
    }
}

fn requested_metric_label(metric: &str) -> String {
    match metric {
        "hr" => "HR".to_string(),
        "tss" => "TSS".to_string(),
        "pace" => "Pace".to_string(),
        "volume" => "Volume".to_string(),
        "zones" => "Zones".to_string(),
        "intensity" => "Intensity".to_string(),
        other => other.replace('_', " ").to_uppercase(),
    }
}

impl ComparePeriodsHandler {
    async fn get_period_stats(
        &self,
        client: &dyn IntervalsClient,
        start: &str,
        end: &str,
        workout_type: Option<&str>,
    ) -> Result<PeriodStats, IntentError> {
        let start_date = parse_date(start, "start")?;
        let end_date = parse_date(end, "end")?;

        let window = AnalysisWindow::new(start_date, end_date);

        let fetched = fetch_period_data(
            client,
            &PeriodFetchRequest {
                window: window.clone(),
                include_activity_details: true,
                include_comparison_window: false,
            },
        )
        .await?;

        let period = filter_activities_by_range(&fetched.activities, &start_date, &end_date);

        let planned_count = fetched
            .calendar_events
            .iter()
            .filter(|event| {
                if let Ok(date) =
                    chrono::NaiveDate::parse_from_str(&event.start_date_local, "%Y-%m-%d")
                {
                    date >= start_date && date <= end_date
                } else {
                    false
                }
            })
            .count();

        if period.is_empty() {
            return Ok(PeriodStats {
                snapshot: TrendSnapshot {
                    activity_count: 0,
                    total_time_secs: 0,
                    total_distance_m: 0.0,
                    total_elevation_m: 0.0,
                },
                window_days: window.window_days(),
                activities: Vec::new(),
                activity_details: fetched.activity_details,
                planned_count,
            });
        }

        let period = if let Some(filter) = workout_type {
            period
                .into_iter()
                .filter(|activity| matches_workout_type(activity, filter))
                .collect::<Vec<_>>()
        } else {
            period
        };

        let activity_ids = period
            .iter()
            .map(|activity| activity.id.clone())
            .collect::<Vec<_>>();
        let activity_details = fetched
            .activity_details
            .into_iter()
            .filter(|(id, _)| activity_ids.contains(id))
            .collect::<std::collections::HashMap<_, _>>();

        let snapshot = if period.is_empty() {
            TrendSnapshot {
                activity_count: 0,
                total_time_secs: 0,
                total_distance_m: 0.0,
                total_elevation_m: 0.0,
            }
        } else {
            build_trend_snapshot(&period, &activity_details)
        };

        Ok(PeriodStats {
            snapshot,
            window_days: window.window_days(),
            activities: period.into_iter().cloned().collect(),
            activity_details,
            planned_count,
        })
    }
}

fn format_duration(total_time_secs: i64) -> String {
    format!(
        "{}:{:02}",
        total_time_secs / 3600,
        (total_time_secs % 3600) / 60
    )
}

fn format_pct(value: Option<f64>) -> String {
    value
        .map(|delta| format!("{:+.1}%", delta))
        .unwrap_or_else(|| "n/a".into())
}

impl Default for ComparePeriodsHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::NaiveDate;
    use intervals_icu_client::{AthleteProfile, IntervalsError};
    use std::collections::HashMap;
    use std::sync::Arc;

    // ========================================================================
    // Constructor Tests
    // ========================================================================

    #[test]
    fn test_new_handler() {
        let handler = ComparePeriodsHandler::new();
        assert_eq!(handler.name(), "compare_periods");
    }

    #[test]
    fn test_default_handler() {
        let _handler = ComparePeriodsHandler;
    }

    // ========================================================================
    // IntentHandler Trait Implementation Tests
    // ========================================================================

    #[test]
    fn test_name() {
        let handler = ComparePeriodsHandler::new();
        assert_eq!(IntentHandler::name(&handler), "compare_periods");
    }

    #[test]
    fn test_description() {
        let handler = ComparePeriodsHandler::new();
        let desc = IntentHandler::description(&handler);
        assert!(desc.contains("Compares performance"));
        assert!(desc.contains("like-for-like"));
    }

    #[test]
    fn test_input_schema_structure() {
        let handler = ComparePeriodsHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("period_a_start"));
        assert!(props.contains_key("period_a_end"));
        assert!(props.contains_key("period_b_start"));
        assert!(props.contains_key("period_b_end"));
        assert!(props.contains_key("period_a_label"));
        assert!(props.contains_key("period_b_label"));
        assert!(props.contains_key("workout_type"));
        assert!(props.contains_key("metrics"));

        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("period_a_start")));
        assert!(required.contains(&json!("period_a_end")));
        assert!(required.contains(&json!("period_b_start")));
        assert!(required.contains(&json!("period_b_end")));
    }

    #[test]
    fn test_requires_idempotency_token() {
        let handler = ComparePeriodsHandler::new();
        assert!(!IntentHandler::requires_idempotency_token(&handler));
    }

    // ========================================================================
    // matches_workout_type() Tests
    // ========================================================================

    #[test]
    fn test_matches_workout_type_intervals() {
        let activity = ActivitySummary {
            id: "a1".to_string(),
            name: Some("Interval Training".to_string()),
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        };
        assert!(matches_workout_type(&activity, "intervals"));
    }

    #[test]
    fn test_matches_workout_type_tempo() {
        let activity = ActivitySummary {
            id: "a1".to_string(),
            name: Some("Tempo Run".to_string()),
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        };
        assert!(matches_workout_type(&activity, "tempo"));
    }

    #[test]
    fn test_matches_workout_type_long_run() {
        let activity = ActivitySummary {
            id: "a1".to_string(),
            name: Some("Long Run".to_string()),
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        };
        assert!(matches_workout_type(&activity, "long_run"));
    }

    #[test]
    fn test_matches_workout_type_no_name() {
        let activity = ActivitySummary {
            id: "a1".to_string(),
            name: None,
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        };
        assert!(!matches_workout_type(&activity, "intervals"));
    }

    #[test]
    fn test_matches_workout_type_no_match() {
        let activity = ActivitySummary {
            id: "a1".to_string(),
            name: Some("Easy Run".to_string()),
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        };
        assert!(!matches_workout_type(&activity, "intervals"));
    }

    #[test]
    fn test_matches_workout_type_case_insensitive() {
        let activity = ActivitySummary {
            id: "a1".to_string(),
            name: Some("INTERVAL SESSION".to_string()),
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        };
        assert!(matches_workout_type(&activity, "intervals"));
    }

    // ========================================================================
    // requested_metric_value() Tests
    // ========================================================================

    #[test]
    fn test_requested_metric_value_volume() {
        let stats = PeriodStats {
            snapshot: TrendSnapshot {
                activity_count: 5,
                total_time_secs: 18000, // 5 hours
                total_distance_m: 50000.0,
                total_elevation_m: 500.0,
            },
            window_days: 7,
            activities: vec![],
            activity_details: HashMap::new(),
            planned_count: 0,
        };

        let (value, note) = requested_metric_value("volume", &stats);
        assert!(value.contains("5.0"));
        assert!(note.contains("derived from total moving time"));
    }

    #[test]
    fn test_requested_metric_value_pace() {
        let stats = PeriodStats {
            snapshot: TrendSnapshot {
                activity_count: 1,
                total_time_secs: 3600,     // 1 hour
                total_distance_m: 10000.0, // 10 km
                total_elevation_m: 100.0,
            },
            window_days: 7,
            activities: vec![],
            activity_details: HashMap::new(),
            planned_count: 0,
        };

        let (value, note) = requested_metric_value("pace", &stats);
        assert!(value.contains("6:00")); // 6 min/km
        assert!(note.contains("derived"));
    }

    #[test]
    fn test_requested_metric_value_pace_no_data() {
        let stats = PeriodStats {
            snapshot: TrendSnapshot {
                activity_count: 0,
                total_time_secs: 0,
                total_distance_m: 0.0,
                total_elevation_m: 0.0,
            },
            window_days: 7,
            activities: vec![],
            activity_details: HashMap::new(),
            planned_count: 0,
        };

        let (value, note) = requested_metric_value("pace", &stats);
        assert_eq!(value, "n/a");
        assert!(note.contains("unavailable"));
    }

    #[test]
    fn test_requested_metric_value_hr() {
        let stats = PeriodStats {
            snapshot: TrendSnapshot {
                activity_count: 1,
                total_time_secs: 3600,
                total_distance_m: 10000.0,
                total_elevation_m: 100.0,
            },
            window_days: 7,
            activities: vec![ActivitySummary {
                id: "a1".to_string(),
                name: Some("Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            }],
            activity_details: HashMap::from([(
                "a1".to_string(),
                json!({"average_heartrate": 150.0}),
            )]),
            planned_count: 0,
        };

        let (value, note) = requested_metric_value("hr", &stats);
        assert!(value.contains("150"));
        assert!(note.contains("average"));
    }

    #[test]
    fn test_requested_metric_value_hr_no_data() {
        let stats = PeriodStats {
            snapshot: TrendSnapshot {
                activity_count: 1,
                total_time_secs: 3600,
                total_distance_m: 10000.0,
                total_elevation_m: 100.0,
            },
            window_days: 7,
            activities: vec![ActivitySummary {
                id: "a1".to_string(),
                name: Some("Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            }],
            activity_details: HashMap::new(),
            planned_count: 0,
        };

        let (value, note) = requested_metric_value("hr", &stats);
        assert_eq!(value, "n/a");
        assert!(note.contains("unavailable"));
    }

    #[test]
    fn test_requested_metric_value_tss() {
        let stats = PeriodStats {
            snapshot: TrendSnapshot {
                activity_count: 1,
                total_time_secs: 3600,
                total_distance_m: 10000.0,
                total_elevation_m: 100.0,
            },
            window_days: 7,
            activities: vec![ActivitySummary {
                id: "a1".to_string(),
                name: Some("Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            }],
            activity_details: HashMap::from([(
                "a1".to_string(),
                json!({"icu_training_load": 75.0}),
            )]),
            planned_count: 0,
        };

        let (value, note) = requested_metric_value("tss", &stats);
        assert!(value.contains("75.0"));
        assert!(note.contains("training load"));
    }

    #[test]
    fn test_requested_metric_value_unknown() {
        let stats = PeriodStats {
            snapshot: TrendSnapshot {
                activity_count: 0,
                total_time_secs: 0,
                total_distance_m: 0.0,
                total_elevation_m: 0.0,
            },
            window_days: 7,
            activities: vec![],
            activity_details: HashMap::new(),
            planned_count: 0,
        };

        let (value, note) = requested_metric_value("unknown_metric", &stats);
        assert_eq!(value, "n/a");
        assert!(note.contains("not yet modeled"));
    }

    // ========================================================================
    // requested_metric_label() Tests
    // ========================================================================

    #[test]
    fn test_requested_metric_label_hr() {
        assert_eq!(requested_metric_label("hr"), "HR");
    }

    #[test]
    fn test_requested_metric_label_tss() {
        assert_eq!(requested_metric_label("tss"), "TSS");
    }

    #[test]
    fn test_requested_metric_label_pace() {
        assert_eq!(requested_metric_label("pace"), "Pace");
    }

    #[test]
    fn test_requested_metric_label_volume() {
        assert_eq!(requested_metric_label("volume"), "Volume");
    }

    #[test]
    fn test_requested_metric_label_custom() {
        assert_eq!(requested_metric_label("custom_metric"), "CUSTOM METRIC");
    }

    // ========================================================================
    // format_duration() Tests
    // ========================================================================

    #[test]
    fn test_format_duration_one_hour() {
        assert_eq!(format_duration(3600), "1:00");
    }

    #[test]
    fn test_format_duration_two_hours_thirty() {
        assert_eq!(format_duration(9000), "2:30");
    }

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "0:00");
    }

    #[test]
    fn test_format_duration_minutes_only() {
        assert_eq!(format_duration(1800), "0:30");
    }

    #[test]
    fn test_format_duration_with_seconds() {
        assert_eq!(format_duration(3659), "1:00"); // Seconds are truncated
    }

    // ========================================================================
    // format_pct() Tests
    // ========================================================================

    #[test]
    fn test_format_pct_positive() {
        assert_eq!(format_pct(Some(10.5)), "+10.5%");
    }

    #[test]
    fn test_format_pct_negative() {
        assert_eq!(format_pct(Some(-5.25)), "-5.2%");
    }

    #[test]
    fn test_format_pct_zero() {
        assert_eq!(format_pct(Some(0.0)), "+0.0%");
    }

    #[test]
    fn test_format_pct_none() {
        assert_eq!(format_pct(None), "n/a");
    }

    // ========================================================================
    // PeriodStats Struct Tests
    // ========================================================================

    #[test]
    fn test_period_stats_structure() {
        let stats = PeriodStats {
            snapshot: TrendSnapshot {
                activity_count: 10,
                total_time_secs: 36_000,
                total_distance_m: 100_500.0,
                total_elevation_m: 1500.0,
            },
            window_days: 28,
            activities: Vec::new(),
            activity_details: std::collections::HashMap::new(),
            planned_count: 0,
        };

        assert_eq!(stats.snapshot.activity_count, 10);
        assert_eq!(stats.snapshot.total_time_secs, 36_000);
        assert!((stats.snapshot.total_distance_m - 100_500.0).abs() < 0.01);
        assert!((stats.snapshot.total_elevation_m - 1500.0).abs() < 0.01);
    }

    #[test]
    fn test_empty_period_stats() {
        let stats = PeriodStats {
            snapshot: TrendSnapshot {
                activity_count: 0,
                total_time_secs: 0,
                total_distance_m: 0.0,
                total_elevation_m: 0.0,
            },
            window_days: 7,
            activities: Vec::new(),
            activity_details: std::collections::HashMap::new(),
            planned_count: 0,
        };

        assert_eq!(stats.snapshot.activity_count, 0);
        assert_eq!(stats.snapshot.total_time_secs, 0);
        assert_eq!(stats.snapshot.total_distance_m, 0.0);
        assert_eq!(stats.snapshot.total_elevation_m, 0.0);
    }

    // ========================================================================
    // Input Validation and Default Value Tests
    // ========================================================================

    #[test]
    fn test_default_labels() {
        let input = json!({
            "period_a_start": "2026-01-01",
            "period_a_end": "2026-01-31",
            "period_b_start": "2026-02-01",
            "period_b_end": "2026-02-28"
        });

        let a_label = input
            .get("period_a_label")
            .and_then(|v| v.as_str())
            .unwrap_or("Period A");
        let b_label = input
            .get("period_b_label")
            .and_then(|v| v.as_str())
            .unwrap_or("Period B");

        assert_eq!(a_label, "Period A");
        assert_eq!(b_label, "Period B");
    }

    #[test]
    fn test_date_parsing() {
        let valid_date = "2026-03-01";
        let result = NaiveDate::parse_from_str(valid_date, "%Y-%m-%d");
        assert!(result.is_ok());

        let invalid_date = "01-03-2026";
        let result = NaiveDate::parse_from_str(invalid_date, "%Y-%m-%d");
        assert!(result.is_err());
    }

    #[test]
    fn test_volume_change_calculation() {
        let old_dist = 100.0;
        let new_dist = 110.0;
        let change = ((new_dist - old_dist) / old_dist * 100.0) as f32;
        assert!((change - 10.0).abs() < 0.1);

        let old_dist = 100.0;
        let new_dist = 90.0;
        let change = ((new_dist - old_dist) / old_dist * 100.0) as f32;
        assert!((change - (-10.0)).abs() < 0.1);
    }

    #[test]
    fn test_delta_formatting() {
        let delta = 5;
        let formatted = if delta >= 0 {
            format!("+{}", delta)
        } else {
            delta.to_string()
        };
        assert_eq!(formatted, "+5");

        let delta = -5;
        let formatted = if delta >= 0 {
            format!("+{}", delta)
        } else {
            delta.to_string()
        };
        assert_eq!(formatted, "-5");
    }

    #[test]
    fn test_time_formatting() {
        let minutes = 125;
        let formatted = format!("{}:{:02}", minutes / 60, minutes % 60);
        assert_eq!(formatted, "2:05");

        let minutes = 60;
        let formatted = format!("{}:{:02}", minutes / 60, minutes % 60);
        assert_eq!(formatted, "1:00");
    }

    #[test]
    fn test_suggestions_based_on_volume_change() {
        let volume_change: f32 = 5.0;
        let suggestion = if volume_change.abs() <= 10.0 {
            "within normal range"
        } else if volume_change > 10.0 {
            "increased"
        } else {
            "decreased"
        };
        assert_eq!(suggestion, "within normal range");

        let volume_change: f32 = 15.0;
        let suggestion = if volume_change.abs() <= 10.0 {
            "within normal range"
        } else if volume_change > 10.0 {
            "increased"
        } else {
            "decreased"
        };
        assert_eq!(suggestion, "increased");
    }

    // ========================================================================
    // Mock Client for Integration Tests
    // ========================================================================

    struct MockClient;

    impl Default for MockClient {
        fn default() -> Self {
            Self
        }
    }

    #[async_trait]
    impl IntervalsClient for MockClient {
        async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
            Ok(AthleteProfile {
                id: "ath1".to_string(),
                name: Some("Test".to_string()),
            })
        }

        async fn get_recent_activities(
            &self,
            _limit: Option<u32>,
            _days_back: Option<i32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
            Ok(vec![
                ActivitySummary {
                    id: "a1".to_string(),
                    name: Some("Run".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "a2".to_string(),
                    name: Some("Interval Run".to_string()),
                    start_date_local: "2026-03-05".to_string(),
                    ..Default::default()
                },
            ])
        }

        async fn get_activity_details(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!({
                "moving_time": 3600,
                "elapsed_time": 4200,
                "distance": 10000.0,
                "average_heartrate": 150.0,
                "icu_training_load": 75.0
            }))
        }

        async fn get_fitness_summary(&self) -> Result<Value, IntervalsError> {
            Ok(json!({
                "ctl": 45.0,
                "atl": 30.0,
                "tsb": 15.0
            }))
        }

        async fn get_events(
            &self,
            _days_back: Option<i32>,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }

        async fn get_upcoming_workouts(
            &self,
            _days_ahead: Option<u32>,
            _limit: Option<u32>,
            _category: Option<String>,
        ) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        // Stub implementations for other required methods
        async fn create_event(
            &self,
            event: intervals_icu_client::Event,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            Ok(event)
        }

        async fn get_event(
            &self,
            event_id: &str,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            Ok(intervals_icu_client::Event {
                id: Some(event_id.to_string()),
                start_date_local: "2026-03-04".to_string(),
                name: "Mock event".to_string(),
                category: intervals_icu_client::EventCategory::Workout,
                description: None,
                r#type: None,
            })
        }

        async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn bulk_create_events(
            &self,
            _events: Vec<intervals_icu_client::Event>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }

        async fn get_activity_streams(
            &self,
            _activity_id: &str,
            _streams: Option<Vec<String>>,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_activity_intervals(
            &self,
            _activity_id: &str,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_best_efforts(
            &self,
            _activity_id: &str,
            _options: Option<intervals_icu_client::BestEffortsOptions>,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn search_activities(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
            Ok(vec![])
        }

        async fn search_activities_full(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
            Ok("id,name\n1,Run".to_string())
        }

        async fn update_activity(
            &self,
            _activity_id: &str,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn download_activity_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }

        async fn download_activity_file_with_progress(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
            _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
            _cancel_rx: tokio::sync::watch::Receiver<bool>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }

        async fn download_fit_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }

        async fn download_gpx_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }

        async fn get_gear_list(&self) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_sport_settings(&self) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_power_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_gap_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn delete_activity(&self, _activity_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn get_activities_around(
            &self,
            _activity_id: &str,
            _limit: Option<u32>,
            _route_id: Option<i64>,
        ) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn search_intervals(
            &self,
            _min_secs: u32,
            _max_secs: u32,
            _min_intensity: u32,
            _max_intensity: u32,
            _interval_type: Option<String>,
            _min_reps: Option<u32>,
            _max_reps: Option<u32>,
            _limit: Option<u32>,
        ) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_power_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_hr_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_pace_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_wellness(&self, _days_back: Option<i32>) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_wellness_for_date(&self, _date: &str) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_wellness(
            &self,
            _date: &str,
            _data: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_event(
            &self,
            _event_id: &str,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn bulk_delete_events(&self, _event_ids: Vec<String>) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn duplicate_event(
            &self,
            _event_id: &str,
            _num_copies: Option<u32>,
            _weeks_between: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }

        async fn get_hr_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_pace_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_workout_library(&self) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_workouts_in_folder(&self, _folder_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn create_folder(&self, _folder: &Value) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_folder(
            &self,
            _folder_id: &str,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn delete_folder(&self, _folder_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn create_gear(&self, _gear: &Value) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_gear(
            &self,
            _gear_id: &str,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn delete_gear(&self, _gear_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn create_gear_reminder(
            &self,
            _gear_id: &str,
            _reminder: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_gear_reminder(
            &self,
            _gear_id: &str,
            _reminder_id: &str,
            _reset: bool,
            _snooze_days: u32,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_sport_settings(
            &self,
            _sport_type: &str,
            _recalc_hr_zones: bool,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn apply_sport_settings(&self, _sport_type: &str) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn create_sport_settings(&self, _settings: &Value) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn update_wellness_bulk(&self, _entries: &[Value]) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn get_weather_config(&self) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_weather_config(&self, _config: &Value) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn list_routes(&self) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_route(
            &self,
            _route_id: i64,
            _include_path: bool,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_route(
            &self,
            _route_id: i64,
            _route: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_route_similarity(
            &self,
            _route_id: i64,
            _other_id: i64,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }
    }

    // ========================================================================
    // Handler Execution Tests
    // ========================================================================

    #[tokio::test]
    async fn test_execute_compare_periods_basic() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockClient);
        let input = json!({
            "period_a_start": "2026-03-01",
            "period_a_end": "2026-03-07",
            "period_b_start": "2026-03-08",
            "period_b_end": "2026-03-14"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn test_execute_compare_periods_with_labels() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockClient);
        let input = json!({
            "period_a_start": "2026-03-01",
            "period_a_end": "2026-03-07",
            "period_b_start": "2026-03-08",
            "period_b_end": "2026-03-14",
            "period_a_label": "Week 1",
            "period_b_label": "Week 2"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        let content_text = format!("{:?}", output.content);
        assert!(content_text.contains("Week 1"));
        assert!(content_text.contains("Week 2"));
    }

    #[tokio::test]
    async fn test_execute_compare_periods_with_workout_type_filter() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockClient);
        let input = json!({
            "period_a_start": "2026-03-01",
            "period_a_end": "2026-03-07",
            "period_b_start": "2026-03-08",
            "period_b_end": "2026-03-14",
            "workout_type": "intervals"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_compare_periods_with_metrics() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockClient);
        let input = json!({
            "period_a_start": "2026-03-01",
            "period_a_end": "2026-03-07",
            "period_b_start": "2026-03-08",
            "period_b_end": "2026-03-14",
            "metrics": ["volume", "pace", "hr", "tss"]
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn test_execute_compare_periods_missing_required_field() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockClient);
        let input = json!({
            "period_a_start": "2026-03-01",
            "period_a_end": "2026-03-07",
            "period_b_start": "2026-03-08"
            // Missing period_b_end
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_compare_periods_invalid_date() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockClient);
        let input = json!({
            "period_a_start": "invalid-date",
            "period_a_end": "2026-03-07",
            "period_b_start": "2026-03-08",
            "period_b_end": "2026-03-14"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_compare_periods_suggestions() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockClient);
        let input = json!({
            "period_a_start": "2026-03-01",
            "period_a_end": "2026-03-07",
            "period_b_start": "2026-03-08",
            "period_b_end": "2026-03-14"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.suggestions.is_empty());
    }

    #[tokio::test]
    async fn test_execute_compare_periods_next_actions() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockClient);
        let input = json!({
            "period_a_start": "2026-03-01",
            "period_a_end": "2026-03-07",
            "period_b_start": "2026-03-08",
            "period_b_end": "2026-03-14"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.next_actions.is_empty());
        assert!(
            output
                .next_actions
                .iter()
                .any(|a| a.contains("analyze_training"))
        );
    }
}
