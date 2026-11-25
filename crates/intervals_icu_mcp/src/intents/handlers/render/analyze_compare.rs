use super::presentation::*;
use crate::engines::analyze_training::shared::*;

pub(crate) fn render_comparison(report: ComparisonReport) -> crate::intents::IntentOutput {
    let ComparisonReport {
        later_label,
        earlier_label,
        later_stats,
        earlier_stats,
        comparison,
        trend,
        later_volume,
        later_consistency,
        earlier_consistency,
        fitness_metrics,
        guidance,
        requested,
    } = report;
    let later_label = later_label.as_str();
    let earlier_label = earlier_label.as_str();
    let mut content = Vec::new();
    content.push(comparison_header_block(later_label, earlier_label));
    content.push(like_for_like_table_block(
        &comparison,
        later_label,
        earlier_label,
    ));

    if let Some(block) = render_fitness_snapshot(&fitness_metrics) {
        content.push(block);
    }

    if !requested.is_empty() {
        let rows = requested
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
        content.extend(requested_compare_metrics_blocks(
            rows,
            later_label,
            earlier_label,
        ));
    }

    content.push(comparison_trend_context_block(
        later_label,
        earlier_label,
        &trend,
        &later_volume,
        &later_consistency,
        later_stats.planned_count,
        &earlier_consistency,
        earlier_stats.planned_count,
    ));

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

    crate::intents::IntentOutput::new(content)
        .with_suggestions(suggestions)
        .with_next_actions(next_actions)
}
