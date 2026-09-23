use super::*;
use crate::content::ContentBlock;
use crate::content::date::{NA, format_pct};
use crate::domains::coach::{ConsistencyMetrics, PolarisationMetrics, TrendMetrics, VolumeMetrics};
use crate::domains::load::{ComparableLoadSeries, LoadSource};
use crate::engines::analysis::LikeForLikeComparison;
use crate::engines::cp_regression::CpResult;
use intervals_icu_client::{ActivitySummary, Event};

pub(crate) fn period_no_activities_blocks(
    start: &str,
    end: &str,
    calendar_events: &[&Event],
) -> Vec<ContentBlock> {
    let mut content = Vec::new();
    content.push(ContentBlock::markdown(format!(
        "# Period: {} to {}\nStatus: No activities found",
        start, end
    )));

    let summary = [
        format!("  No completed activities returned for {} to {}", start, end),
        "  The uncapped activity query completed for the full requested range".into(),
        if calendar_events.is_empty() {
            "  No calendar events were found in this period either".into()
        } else {
            format!(
                "  {} calendar event(s) found in this window; review them below",
                calendar_events.len()
            )
        },
        "  Consider checking:".into(),
        "    Whether this period genuinely has no completed activities".into(),
        "    Adjusting period_start/period_end or using compare_periods to contrast with a nearby range".into(),
    ]
    .join("\n");

    content.push(ContentBlock::markdown(summary));

    if !calendar_events.is_empty() {
        let calendar_rows = build_calendar_event_rows(calendar_events);
        content.push(ContentBlock::markdown(
            "Calendar Events in Window".to_string(),
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

    content
}

pub(crate) fn period_header_block(start: &str, end: &str) -> ContentBlock {
    ContentBlock::markdown(format!("# Period: {} to {}", start, end))
}

pub(crate) fn period_summary_table_block(rows: Vec<Vec<String>>) -> ContentBlock {
    ContentBlock::table(vec!["Metric".into(), "Value".into()], rows)
}

pub(crate) fn planned_workouts_blocks(rows: Vec<Vec<String>>) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("Planned Workouts".to_string()),
        ContentBlock::table(
            vec![
                "Date".into(),
                "Workout".into(),
                "Duration".into(),
                "Planned Load".into(),
            ],
            rows,
        ),
    ]
}

pub(crate) fn planned_workout_duration_cell(
    moving_duration: Option<String>,
    elapsed_duration: Option<String>,
) -> String {
    match (moving_duration, elapsed_duration) {
        (Some(mov), Some(elp)) => format!("{} (elapsed: {})", mov, elp),
        (Some(mov), None) => mov,
        (None, Some(elp)) => format!("elapsed: {}", elp),
        (None, None) => NA.to_string(),
    }
}

pub(crate) fn planned_workout_load_cell(load: Option<f64>) -> String {
    load.map(|value| format!("{:.1}", value))
        .unwrap_or_else(|| NA.to_string())
}

pub(crate) fn calendar_events_blocks(rows: Vec<Vec<String>>) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("### Calendar Events in Window".to_string()),
        ContentBlock::table(
            vec![
                "Date".into(),
                "Category".into(),
                "Event".into(),
                "Description".into(),
            ],
            rows,
        ),
    ]
}

pub(crate) fn requested_period_metrics_blocks(rows: Vec<Vec<String>>) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("Requested Metrics".to_string()),
        ContentBlock::table(vec!["Metric".into(), "Value".into(), "Status".into()], rows),
    ]
}

pub(crate) fn trend_context_block(trend: &TrendMetrics) -> ContentBlock {
    ContentBlock::markdown(format!(
        "Trend Context\n  Activity delta: {}\n  Time delta: {}\n  Distance delta: {}\n  Elevation delta: {}",
        trend
            .activity_count_delta
            .map(|delta| format!("{:+}", delta))
            .unwrap_or_else(|| NA.into()),
        format_pct(trend.time_delta_pct),
        format_pct(trend.distance_delta_pct),
        format_pct(trend.elevation_delta_pct),
    ))
}

pub(crate) fn linear_trends_block(insight_descriptions: &[String]) -> Option<ContentBlock> {
    if insight_descriptions.is_empty() {
        return None;
    }
    let mut trend_lines = vec!["Linear Trends".to_string()];
    for description in insight_descriptions {
        trend_lines.push(format!("  {}", description));
    }
    Some(ContentBlock::markdown(trend_lines.join("\n")))
}

pub(crate) fn tid_distribution_block(
    pol: &PolarisationMetrics,
    z1: f64,
    z2: f64,
    z3: f64,
    tid_model: String,
    pi: Option<f64>,
) -> ContentBlock {
    let mut tid_lines = vec!["Training Intensity Distribution".to_string()];
    tid_lines.push(format!("  TID Model: {}", tid_model));
    tid_lines.push(format!("  Z1: {:.1}%  Z2: {:.1}%  Z3: {:.1}%", z1, z2, z3));
    if let Some(pi_val) = pi {
        tid_lines.push(format!("  Polarization Index: {:.3}", pi_val));
    }
    if let Some(tid_model_str) = &pol.tid_model {
        tid_lines.push(format!("  Classification: {}", tid_model_str));
    }
    ContentBlock::markdown(tid_lines.join("\n"))
}

pub(crate) fn zone_distribution_blocks(rows: Vec<Vec<String>>) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("Time in Zones".to_string()),
        ContentBlock::table(vec!["Zone".into(), "Time".into(), "%".into()], rows),
    ]
}

pub(crate) fn consistency_block(consistency: &ConsistencyMetrics) -> ContentBlock {
    let pct = consistency
        .ratio
        .map(|r| format!("{:.0}%", r * 100.0))
        .unwrap_or_else(|| NA.to_string());
    let state = consistency.state.as_deref().unwrap_or("unknown");
    ContentBlock::markdown(format!(
        "Training Consistency\n  Planned sessions: {}\n  Completed sessions: {}\n  Adherence: {} ({})",
        consistency.sessions_planned, consistency.sessions_completed, pct, state,
    ))
}

pub(crate) fn power_curve_block(
    deltas: &std::collections::HashMap<String, f64>,
    rotation: f64,
    statuses: &std::collections::HashMap<String, String>,
    adaptation_state: &Option<String>,
) -> Option<ContentBlock> {
    if deltas.is_empty() {
        return None;
    }
    let mut pc_lines = vec!["Power Curve Comparison".to_string()];
    for d in &["1m", "5m", "20m", "60m"] {
        if let Some(delta) = deltas.get(*d) {
            let status = statuses.get(*d).map(|s| s.as_str()).unwrap_or("");
            pc_lines.push(format!("  {}: {:+.1}% ({})", d, delta, status));
        }
    }
    pc_lines.push(format!("  Rotation Index: {:.3}", rotation));
    if let Some(state) = adaptation_state {
        pc_lines.push(format!("  Adaptation State: {}", state));
    }
    Some(ContentBlock::markdown(pc_lines.join("\n")))
}

pub(crate) fn cp_diagnostics_block(
    cp_result: &CpResult,
    api_diff: Option<(f64, f64)>,
) -> ContentBlock {
    let mut cp_lines = vec!["CP Model Diagnostics".to_string()];
    cp_lines.push(format!(
        "  Fitted CP: {:.0} W | W': {:.0} J | R²: {:.3}",
        cp_result.cp, cp_result.w_prime, cp_result.r_squared
    ));
    cp_lines.push(format!(
        "  Samples: {} | Duration: {:.0}s – {:.0}s",
        cp_result.diagnostics.sample_count,
        cp_result.diagnostics.min_duration_secs,
        cp_result.diagnostics.max_duration_secs,
    ));
    cp_lines.push(format!(
        "  RMSE: {:.1} W | Max residual: {:.1} W",
        cp_result.diagnostics.rmse_watts, cp_result.diagnostics.max_abs_residual_watts,
    ));
    if let Some(cp_se) = cp_result.diagnostics.cp_standard_error_watts {
        cp_lines.push(format!("  CP standard error: {:.1} W", cp_se));
    }
    if let Some(wp_se) = cp_result.diagnostics.w_prime_standard_error_joules {
        cp_lines.push(format!("  W′ standard error: {:.0} J", wp_se));
    }
    if let Some((cp_diff, wp_diff)) = api_diff {
        cp_lines.push(format!(
            "  Difference from API estimate: CP Δ{:.1}%, W′ Δ{:.1}%",
            cp_diff, wp_diff
        ));
    }
    ContentBlock::markdown(cp_lines.join("\n"))
}

pub(crate) fn load_patterns_block(b2b: f64) -> ContentBlock {
    ContentBlock::markdown(format!(
        "Load Patterns\n  Back-to-Back Peak Load: {:.1}",
        b2b
    ))
}

pub(crate) fn terrain_specificity_block(vert: f64) -> ContentBlock {
    ContentBlock::markdown(format!("Terrain Specificity\n  Weekly Vert: {:.0} m", vert))
}

pub(crate) fn daily_load_series_blocks(rows: Vec<Vec<String>>) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("Daily Load Series".to_string()),
        ContentBlock::table(vec!["Date".into(), "Load".into()], rows),
    ]
}

pub(crate) fn interval_sessions_blocks(rows: Vec<Vec<String>>) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("Interval Sessions".to_string()),
        ContentBlock::table(vec!["Date".into(), "Workout".into()], rows),
    ]
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

pub(crate) fn weekly_hours(volume: Option<&VolumeMetrics>) -> f64 {
    volume
        .map(|volume| volume.weekly_avg_hours)
        .unwrap_or_default()
}

pub(crate) fn planned_workout_date_cell(activity: &ActivitySummary) -> String {
    activity
        .start_date_local
        .split('T')
        .next()
        .unwrap_or(&activity.start_date_local)
        .to_string()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn comparison_trend_context_block(
    later_label: &str,
    earlier_label: &str,
    trend: &TrendMetrics,
    later_volume: &VolumeMetrics,
    later_consistency: &ConsistencyMetrics,
    later_planned: usize,
    earlier_consistency: &ConsistencyMetrics,
    earlier_planned: usize,
) -> ContentBlock {
    ContentBlock::markdown(format!(
        "Trend Context\n  Activity delta: {}\n  Time delta: {}\n  Distance delta: {}\n  Elevation delta: {}\n  Current period weekly average: {:.1} hrs\n  {} consistency: {} ({:.0}% of {} planned sessions)\n  {} consistency: {} ({:.0}% of {} planned sessions)",
        trend
            .activity_count_delta
            .map(|delta| format!("{:+}", delta))
            .unwrap_or_else(|| NA.into()),
        format_pct(trend.time_delta_pct),
        format_pct(trend.distance_delta_pct),
        format_pct(trend.elevation_delta_pct),
        later_volume.weekly_avg_hours,
        later_label,
        later_consistency.state.as_deref().unwrap_or("unknown"),
        later_consistency.ratio.unwrap_or(0.0) * 100.0,
        later_planned,
        earlier_label,
        earlier_consistency.state.as_deref().unwrap_or("unknown"),
        earlier_consistency.ratio.unwrap_or(0.0) * 100.0,
        earlier_planned,
    ))
}

pub(crate) fn like_for_like_table_block(
    comparison: &LikeForLikeComparison,
    later_label: &str,
    earlier_label: &str,
) -> ContentBlock {
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
    ContentBlock::table(rows[0].clone(), rows[1..].to_vec())
}
