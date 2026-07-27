use intervals_icu_client::IntervalsClient;
use serde_json::Value;

use super::shared::*;
use crate::content::{ContentBlock, IntentError};
use crate::domains::coach::CoachMetrics;
use crate::engines::analysis::AnalysisEngine;
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{
    compute_consistency_index, derive_trend_metrics, derive_volume_metrics,
};
use crate::engines::fitness_context::FitnessContext;
use crate::intents::utils::{format_pct, parse_date};

pub async fn compare_periods(
    input: &Value,
    client: &dyn IntervalsClient,
) -> Result<AnalyzeReport, IntentError> {
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

    let a_start_date = parse_date(a_start, "period_a_start")?;
    let a_end_date = parse_date(a_end, "period_a_end")?;
    let b_start_date = parse_date(b_start, "period_b_start")?;
    let b_end_date = parse_date(b_end, "period_b_end")?;

    if a_start_date > a_end_date {
        return Err(IntentError::validation(
            "period_a_start must be on or before period_a_end".to_string(),
        ));
    }
    if b_start_date > b_end_date {
        return Err(IntentError::validation(
            "period_b_start must be on or before period_b_end".to_string(),
        ));
    }

    let a_window = crate::domains::coach::AnalysisWindow::new(a_start_date, a_end_date);
    let b_window = crate::domains::coach::AnalysisWindow::new(b_start_date, b_end_date);

    let (a_stats, b_stats) = tokio::try_join!(
        super::period::fetch_period_stats(client, a_window, workout_type),
        super::period::fetch_period_stats(client, b_window, workout_type),
    )?;

    // Delta convention: "later period relative to earlier period". The Δ
    // column always reports `(later) - (earlier)`, so a positive value means
    // the later period grew vs the earlier one. This is invariant to whether
    // the caller labelled the newer period as A or B; otherwise users who put
    // "this year" as `period_b` would see inverted (negative) signs.
    //
    // The downstream engines `derive_trend_metrics` and
    // `AnalysisEngine::compare_periods` both treat their first argument as
    // the reference period and subtract the second argument from it. So we
    // swap (stats, summary, label) together so the later period always lands
    // in the first position, and the rendered table reads
    // `[later_label, earlier_label, Δ]` with the same sign semantics.
    //
    // Tiebreak by `end_date` so identical start dates resolve deterministically.
    let a_is_later =
        a_start_date > b_start_date || (a_start_date == b_start_date && a_end_date >= b_end_date);
    let (later_stats, earlier_stats, later_label, earlier_label) = if a_is_later {
        (a_stats, b_stats, a_label, b_label)
    } else {
        (b_stats, a_stats, b_label, a_label)
    };

    let trend = derive_trend_metrics(later_stats.snapshot, earlier_stats.snapshot);
    let later_volume = derive_volume_metrics(
        later_stats.window_days,
        later_stats.snapshot.total_time_secs,
        later_stats.snapshot.total_distance_m,
        later_stats.snapshot.total_elevation_m,
        later_stats.snapshot.activity_count,
    );

    let later_consistency = compute_consistency_index(
        later_stats.snapshot.activity_count,
        later_stats.planned_count,
    );
    let earlier_consistency = compute_consistency_index(
        earlier_stats.snapshot.activity_count,
        earlier_stats.planned_count,
    );

    let later_summary = build_period_summary(&later_stats);
    let earlier_summary = build_period_summary(&earlier_stats);
    let comparison = AnalysisEngine::compare_periods(
        &later_summary,
        &earlier_summary,
        later_label,
        earlier_label,
    );

    let fitness_context = FitnessContext::load(client).await;
    let fitness_metrics = fitness_context.metrics().cloned();

    let metrics_for_guidance = CoachMetrics {
        consistency: Some(later_consistency.clone()),
        fitness: fitness_metrics.clone(),
        volume: Some(later_volume.clone()),
        ..Default::default()
    };
    let alerts = build_alerts(&metrics_for_guidance);
    let guidance = build_guidance(&metrics_for_guidance, &alerts);

    let mut content = Vec::new();
    content.push(ContentBlock::markdown(format!(
        "# Comparison: {} vs {}",
        later_label, earlier_label
    )));

    let mut rows = vec![vec![
        "Metric".into(),
        later_label.into(),
        earlier_label.into(),
        "Δ".into(),
    ]];
    for m in &comparison.metrics {
        let (formatted_a, formatted_b) = if m.name == "Workouts" {
            (
                format!("{:.0}", m.period_a_value),
                format!("{:.0}", m.period_b_value),
            )
        } else {
            (
                format!("{:.1}", m.period_a_value),
                format!("{:.1}", m.period_b_value),
            )
        };
        rows.push(vec![
            m.name.clone(),
            formatted_a,
            formatted_b,
            format!("{:+.1} ({:+.0}%)", m.delta_absolute, m.delta_percent),
        ]);
    }
    content.push(ContentBlock::table(rows[0].clone(), rows[1..].to_vec()));

    if let Some(ref fm) = fitness_metrics {
        let mut fit_lines = vec!["Fitness Snapshot".to_string()];
        if let Some(ctl) = fm.ctl {
            fit_lines.push(format!("  CTL: {:.0}", ctl));
        }
        if let Some(atl) = fm.atl {
            fit_lines.push(format!("  ATL: {:.0}", atl));
        }
        if let Some(tsb) = fm.tsb {
            let state = if tsb > 10.0 {
                "Fresh"
            } else if tsb < -10.0 {
                "Fatigued"
            } else {
                "Balanced"
            };
            fit_lines.push(format!("  TSB: {:.0} ({})", tsb, state));
        }
        if let Some(rr) = fm.ramp_rate {
            fit_lines.push(format!("  Ramp Rate: {:+.1}/wk", rr));
        }
        content.push(ContentBlock::markdown(fit_lines.join("\n")));
    }

    if !requested_metrics.is_empty() {
        let rows = requested_metrics
            .iter()
            .map(|metric| {
                let (later_value, note) = requested_metric_value(metric, &later_stats);
                let (earlier_value, _) = requested_metric_value(metric, &earlier_stats);
                vec![
                    requested_metric_label(metric),
                    later_value,
                    earlier_value,
                    note,
                ]
            })
            .collect::<Vec<_>>();
        content.push(ContentBlock::markdown("Requested Metrics".to_string()));
        content.push(ContentBlock::table(
            vec![
                "Metric".into(),
                later_label.into(),
                earlier_label.into(),
                "Status".into(),
            ],
            rows,
        ));
    }

    content.push(ContentBlock::markdown(format!(
        "Trend Context\n  Activity delta: {}\n  Time delta: {}\n  Distance delta: {}\n  Elevation delta: {}\n  Current period weekly average: {:.1} hrs\n  {} consistency: {} ({:.0}% of {} planned sessions)\n  {} consistency: {} ({:.0}% of {} planned sessions)",
        trend
            .activity_count_delta
            .map(|delta| format!("{:+}", delta))
            .unwrap_or_else(|| "n/a".into()),
        format_pct(trend.time_delta_pct),
        format_pct(trend.distance_delta_pct),
        format_pct(trend.elevation_delta_pct),
        later_volume.weekly_avg_hours,
        later_label,
        later_consistency.state.as_deref().unwrap_or("unknown"),
        later_consistency.ratio.unwrap_or(0.0) * 100.0,
        later_stats.planned_count,
        earlier_label,
        earlier_consistency.state.as_deref().unwrap_or("unknown"),
        earlier_consistency.ratio.unwrap_or(0.0) * 100.0,
        earlier_stats.planned_count,
    )));

    let mut suggestions = vec![comparison.summary.clone()];
    suggestions.extend(guidance.suggestions);

    if let Some(elev_delta) = trend.elevation_delta_pct
        && elev_delta.abs() > 30.0
    {
        suggestions.push(format!(
            "Elevation change: {:+.0}% - consider extra recovery and hill-specific work",
            elev_delta
        ));
    }

    let volume_change = if let Some(time_delta_pct) = trend.time_delta_pct {
        time_delta_pct as f32
    } else if let Some(distance_delta_pct) = trend.distance_delta_pct {
        distance_delta_pct as f32
    } else {
        0.0
    };

    let mut next_actions = vec![
        "To analyze a specific period: analyze_training with target_type: period".into(),
        "To assess recovery: assess_recovery".into(),
    ];
    next_actions.extend(guidance.next_actions);

    if volume_change > 15.0 {
        next_actions.insert(0, "Consider recovery week if volume spike continues".into());
    }

    Ok(AnalyzeReport::new(content)
        .with_suggestions(suggestions)
        .with_next_actions(next_actions))
}

#[cfg(test)]
mod compare_tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use intervals_icu_client::ActivitySummary;
    use std::collections::HashMap;

    fn make_activity(
        id: &str,
        name: &str,
        date: &str,
        moving_time: Option<i32>,
    ) -> ActivitySummary {
        ActivitySummary {
            id: id.to_string(),
            name: Some(name.to_string()),
            start_date_local: date.to_string(),
            moving_time,
            ..Default::default()
        }
    }

    #[test]
    fn test_matches_workout_type_intervals() {
        let a = make_activity("a1", "Interval Training", "2026-03-01", None);
        assert!(matches_workout_type(&a, "intervals"));
    }

    #[test]
    fn test_matches_workout_type_tempo() {
        let a = make_activity("a1", "Tempo Run", "2026-03-01", None);
        assert!(matches_workout_type(&a, "tempo"));
    }

    #[test]
    fn test_matches_workout_type_long_run() {
        let a = make_activity("a1", "Long Run", "2026-03-01", None);
        assert!(matches_workout_type(&a, "long_run"));
    }

    #[test]
    fn test_matches_workout_type_no_name() {
        let a = ActivitySummary {
            id: "a1".to_string(),
            name: None,
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        };
        assert!(!matches_workout_type(&a, "intervals"));
    }

    #[test]
    fn test_matches_workout_type_no_match() {
        let a = make_activity("a1", "Easy Run", "2026-03-01", None);
        assert!(!matches_workout_type(&a, "intervals"));
    }

    #[test]
    fn test_matches_workout_type_case_insensitive() {
        let a = make_activity("a1", "INTERVAL SESSION", "2026-03-01", None);
        assert!(matches_workout_type(&a, "intervals"));
    }

    #[test]
    fn test_requested_metric_value_volume() {
        let stats = PeriodStats {
            snapshot: crate::engines::coach_metrics::TrendSnapshot {
                activity_count: 5,
                total_time_secs: 18000,
                total_distance_m: 50000.0,
                total_elevation_m: 500.0,
            },
            window_days: 14,
            activities: Vec::new(),
            activity_details: std::collections::HashMap::new(),
            planned_count: 5,
            etvs: None,
        };
        let (value, note) = requested_metric_value("volume", &stats);
        assert_eq!(value, "5.0 h");
        assert!(note.contains("moving time"));
    }

    #[test]
    fn test_requested_metric_value_pace() {
        let stats = PeriodStats {
            snapshot: crate::engines::coach_metrics::TrendSnapshot {
                activity_count: 3,
                total_time_secs: 7200,
                total_distance_m: 36000.0,
                total_elevation_m: 200.0,
            },
            window_days: 7,
            activities: Vec::new(),
            activity_details: std::collections::HashMap::new(),
            planned_count: 3,
            etvs: None,
        };
        let (value, _note) = requested_metric_value("pace", &stats);
        assert!(value.contains("/km"));
    }

    /// Regression test (F-8): an activity that exposes load only as `tss`
    /// (canonical alias) should still contribute to the period "tss" and
    /// "intensity" metrics. Prior implementation hard-coded `icu_training_load`
    /// and silently dropped `tss`/`training_load`/`icuTrainingLoad` aliases.
    #[test]
    fn test_requested_metric_value_tss_picks_up_tss_alias() {
        let mut details = std::collections::HashMap::new();
        details.insert("a1".to_string(), serde_json::json!({"tss": 100.0}));
        let stats = PeriodStats {
            snapshot: crate::engines::coach_metrics::TrendSnapshot {
                activity_count: 1,
                total_time_secs: 3600,
                total_distance_m: 10000.0,
                total_elevation_m: 50.0,
            },
            window_days: 7,
            activities: vec![make_activity("a1", "Long Run", "2026-03-01", Some(3600))],
            activity_details: details,
            planned_count: 1,
            etvs: None,
        };
        let (value, _) = requested_metric_value("tss", &stats);
        assert_eq!(value, "100.0", "tss-only detail should be picked up");
    }

    #[test]
    fn test_requested_metric_value_intensity_picks_up_training_load_alias() {
        let mut details = std::collections::HashMap::new();
        details.insert("a1".to_string(), serde_json::json!({"training_load": 70.0}));
        let stats = PeriodStats {
            snapshot: crate::engines::coach_metrics::TrendSnapshot {
                activity_count: 1,
                total_time_secs: 3600,
                total_distance_m: 10000.0,
                total_elevation_m: 50.0,
            },
            window_days: 7,
            activities: vec![make_activity("a1", "Tempo", "2026-03-01", Some(3600))],
            activity_details: details,
            planned_count: 1,
            etvs: None,
        };
        let (value, _) = requested_metric_value("intensity", &stats);
        assert!(value.contains("TSS/wk"), "got: {value}");
        // 70 TSS / 1 week = 70 TSS/wk
        assert!(value.starts_with("70"), "expected ~70 TSS/wk, got: {value}");
    }

    #[test]
    fn test_requested_metric_value_intensity_prefers_canonical_over_alias() {
        let mut details = std::collections::HashMap::new();
        details.insert(
            "a1".to_string(),
            serde_json::json!({"icu_training_load": 88.0, "training_load": 65.0, "tss": 73.0}),
        );
        let stats = PeriodStats {
            snapshot: crate::engines::coach_metrics::TrendSnapshot {
                activity_count: 1,
                total_time_secs: 3600,
                total_distance_m: 10000.0,
                total_elevation_m: 50.0,
            },
            window_days: 7,
            activities: vec![make_activity("a1", "Run", "2026-03-01", Some(3600))],
            activity_details: details,
            planned_count: 1,
            etvs: None,
        };
        let (value, _) = requested_metric_value("intensity", &stats);
        // canonical icu_training_load wins → 88 TSS/wk
        assert!(value.starts_with("88"), "got: {value}");
    }

    #[test]
    fn test_requested_metric_label() {
        assert_eq!(requested_metric_label("hr"), "HR");
        assert_eq!(requested_metric_label("tss"), "TSS");
        assert_eq!(requested_metric_label("pace"), "Pace");
        assert_eq!(requested_metric_label("volume"), "Volume");
        assert_eq!(requested_metric_label("zones"), "Zones");
        assert_eq!(requested_metric_label("intensity"), "Intensity");
        assert_eq!(requested_metric_label("etvs"), "ETVS");
    }

    #[test]
    fn test_build_period_summary_basic() {
        let stats = PeriodStats {
            snapshot: crate::engines::coach_metrics::TrendSnapshot {
                activity_count: 10,
                total_time_secs: 36000,
                total_distance_m: 100000.0,
                total_elevation_m: 1000.0,
            },
            window_days: 28,
            activities: Vec::new(),
            activity_details: std::collections::HashMap::new(),
            planned_count: 10,
            etvs: None,
        };
        let summary = build_period_summary(&stats);
        assert_eq!(summary.workout_count, 10);
        assert!((summary.total_time_hours - 10.0).abs() < 0.1);
        assert!((summary.total_distance_km - 100.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn test_compare_periods_empty_periods() {
        let client = MockIntervalsClient::builder();
        let input = serde_json::json!({
            "period_a_start": "2026-01-01",
            "period_a_end": "2026-01-31",
            "period_b_start": "2026-02-01",
            "period_b_end": "2026-02-28"
        });
        let result = compare_periods(&input, &client).await;
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(!report.content.is_empty());
    }

    #[tokio::test]
    async fn test_compare_periods_validates_dates() {
        let client = MockIntervalsClient::builder();
        let input = serde_json::json!({
            "period_a_start": "2026-02-01",
            "period_a_end": "2026-01-01",
            "period_b_start": "2026-02-01",
            "period_b_end": "2026-02-28"
        });
        let result = compare_periods(&input, &client).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_compare_periods_missing_required_field() {
        let client = MockIntervalsClient::builder();
        let input = serde_json::json!({
            "period_a_start": "2026-01-01"
        });
        let result = compare_periods(&input, &client).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_compare_periods_with_metrics() {
        let client = MockIntervalsClient::builder();
        let input = serde_json::json!({
            "period_a_start": "2026-01-01",
            "period_a_end": "2026-01-31",
            "period_b_start": "2026-02-01",
            "period_b_end": "2026-02-28",
            "metrics": ["volume", "pace", "hr"]
        });
        let result = compare_periods(&input, &client).await;
        assert!(result.is_ok());
    }

    /// Regression: when the caller labels the older period as `period_a`
    /// and the newer period as `period_b`, the Δ column must still report
    /// `(later) - (earlier)`. Before the fix, the engine computed
    /// `period_a - period_b` positionally, inverting the sign for users who
    /// put "this year" in the second slot.
    #[tokio::test]
    async fn test_compare_periods_delta_is_invariant_to_label_order() {
        // The later period (March) carries 2 activities; the earlier
        // period (February) carries 1. activity_details applies 5400s,
        // 15000m, 300m to each activity, so the asymmetry surfaces as a
        // detectable Δ sign.
        let client = MockIntervalsClient::builder()
            .with_activities(vec![
                make_activity("mar-1", "Mar Run 1", "2026-03-01", Some(5400)),
                make_activity("mar-2", "Mar Run 2", "2026-03-03", Some(5400)),
                make_activity("feb-1", "Feb Run", "2026-02-25", Some(5400)),
            ])
            .with_activity_details_map(HashMap::from([
                (
                    "mar-1".to_string(),
                    serde_json::json!({
                        "moving_time": 5400,
                        "distance": 15000.0,
                        "total_elevation_gain": 300.0
                    }),
                ),
                (
                    "mar-2".to_string(),
                    serde_json::json!({
                        "moving_time": 5400,
                        "distance": 15000.0,
                        "total_elevation_gain": 300.0
                    }),
                ),
                (
                    "feb-1".to_string(),
                    serde_json::json!({
                        "moving_time": 5400,
                        "distance": 15000.0,
                        "total_elevation_gain": 300.0
                    }),
                ),
            ]));

        // Caller labels the OLDER period as A.
        let input = serde_json::json!({
            "period_a_start": "2026-02-24",
            "period_a_end": "2026-02-28",
            "period_b_start": "2026-03-01",
            "period_b_end": "2026-03-07"
        });
        let report = compare_periods(&input, &client).await.unwrap();
        let rendered = report
            .content
            .iter()
            .map(|b| match b {
                crate::intents::ContentBlock::Markdown { markdown } => markdown.clone(),
                crate::intents::ContentBlock::Table { headers, rows } => {
                    let mut s = headers.join(" | ");
                    for row in rows {
                        s.push('\n');
                        s.push_str(&row.join(" | "));
                    }
                    s
                }
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");

        // The header shows the later period (Period B in caller input)
        // first, because delta is anchored to the later period.
        assert!(
            rendered.contains("Comparison: Period B vs Period A"),
            "later period (Period B in caller input) should appear first; got:\n{rendered}"
        );
        // The Trend Context reports +1 (later period gained one activity).
        assert!(
            rendered.contains("Activity delta: +1"),
            "activity delta must be +1 (later period has 1 more activity); got:\n{rendered}"
        );
    }
}
