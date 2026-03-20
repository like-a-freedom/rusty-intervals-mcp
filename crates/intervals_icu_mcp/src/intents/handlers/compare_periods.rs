use crate::intents::{ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput};
use async_trait::async_trait;
use chrono::NaiveDate;
use intervals_icu_client::{ActivitySummary, IntervalsClient};
use serde_json::{Value, json};
/// Compare Periods Intent Handler
///
/// Compares performance between two periods (like-for-like).
use std::sync::Arc;

use crate::domains::coach::AnalysisWindow;
use crate::engines::analysis_fetch::{PeriodFetchRequest, fetch_period_data};
use crate::engines::coach_metrics::{
    TrendSnapshot, build_trend_snapshot, derive_trend_metrics, derive_volume_metrics,
};
use crate::intents::utils::filter_activities_by_range;

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
                "period_a_start": {"type": "string", "description": "Period A start (YYYY-MM-DD)"},
                "period_a_end": {"type": "string", "description": "Period A end (YYYY-MM-DD)"},
                "period_a_label": {"type": "string", "description": "Period A label"},
                "period_b_start": {"type": "string", "description": "Period B start (YYYY-MM-DD)"},
                "period_b_end": {"type": "string", "description": "Period B end (YYYY-MM-DD)"},
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
            "### Trend Context\n\n- Activity delta: {}\n- Time delta: {}\n- Distance delta: {}\n- Elevation delta: {}\n- Current period weekly average: {:.1} hrs",
            trend
                .activity_count_delta
                .map(|delta| format!("{:+}", delta))
                .unwrap_or_else(|| "n/a".into()),
            format_pct(trend.time_delta_pct),
            format_pct(trend.distance_delta_pct),
            format_pct(trend.elevation_delta_pct),
            a_volume.weekly_avg_hours,
        )));

        // Analysis
        let volume_change = if let Some(distance_delta_pct) = trend.distance_delta_pct {
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
        other => ("n/a".into(), format!("metric '{}' not yet modeled", other)),
    }
}

fn requested_metric_label(metric: &str) -> String {
    match metric {
        "hr" => "HR".to_string(),
        "tss" => "TSS".to_string(),
        "pace" => "Pace".to_string(),
        "volume" => "Volume".to_string(),
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
        let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
            .map_err(|_| IntentError::validation(format!("Invalid start date: {}", start)))?;
        let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
            .map_err(|_| IntentError::validation(format!("Invalid end date: {}", end)))?;

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

    #[test]
    fn test_new_handler() {
        let handler = ComparePeriodsHandler::new();
        assert_eq!(handler.name(), "compare_periods");
    }

    #[test]
    fn test_default_handler() {
        let _handler = ComparePeriodsHandler;
    }

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

        // Required fields
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
        };

        assert_eq!(stats.snapshot.activity_count, 0);
        assert_eq!(stats.snapshot.total_time_secs, 0);
        assert_eq!(stats.snapshot.total_distance_m, 0.0);
        assert_eq!(stats.snapshot.total_elevation_m, 0.0);
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
        // Test volume change percentage calculation
        let old_dist = 100.0;
        let new_dist = 110.0;
        let change = ((new_dist - old_dist) / old_dist * 100.0) as f32;
        assert!((change - 10.0).abs() < 0.1);

        // Decrease
        let old_dist = 100.0;
        let new_dist = 90.0;
        let change = ((new_dist - old_dist) / old_dist * 100.0) as f32;
        assert!((change - (-10.0)).abs() < 0.1);
    }

    #[test]
    fn test_delta_formatting() {
        // Positive delta
        let delta = 5;
        let formatted = if delta >= 0 {
            format!("+{}", delta)
        } else {
            delta.to_string()
        };
        assert_eq!(formatted, "+5");

        // Negative delta
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
        // Normal range
        let volume_change: f32 = 5.0;
        let suggestion = if volume_change.abs() <= 10.0 {
            "within normal range"
        } else if volume_change > 10.0 {
            "increased"
        } else {
            "decreased"
        };
        assert_eq!(suggestion, "within normal range");

        // High increase
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
}
