use serde_json::Value;

use super::presentation::*;
use crate::content::date::data_availability_block;
use crate::domains::activity_analysis::{back_to_back_load, vert_per_week};
use crate::engines::analysis::AnalysisEngine;
use crate::engines::analysis_fetch::{build_daily_load_series, extract_activity_load};
use crate::engines::analyze_training::shared::*;
use crate::engines::coach_metrics::{
    classify_tid_model, derive_espe_metrics, extract_sportinfo_anchors,
};
use crate::engines::cp_regression::{fit_cp, validate_cp};
use crate::engines::interval_analysis::is_planned_workout_id;
use crate::engines::shared::parse_activity_date;
use intervals_icu_client::EventCategory;

pub(crate) fn render_period(report: PeriodReport) -> crate::intents::IntentOutput {
    let PeriodReport {
        start,
        end,
        analysis_type,
        filter,
        requested,
        fetched,
        period,
        previous_period,
        events,
        context: mut period_context,
        snapshot: period_snapshot,
        window,
        period_ids,
        weekly_hrs,
    } = report;
    let start = start.as_str();
    let end = end.as_str();
    let analysis_type = analysis_type.as_str();
    let desc_filter = filter.as_deref();
    let period: Vec<_> = period.iter().collect();
    let previous_period: Vec<_> = previous_period.iter().collect();
    let calendar_events: Vec<_> = events.iter().collect();
    let mut content = Vec::new();
    content.push(period_header_block(start, end));

    let rows = build_period_summary_rows(period.len(), &period_snapshot, weekly_hrs);
    content.push(period_summary_table_block(rows));

    if let Some(block) = render_etvs_section(period_context.metrics.etvs.as_ref()) {
        content.push(block);
    }

    // Endurance evidence is intentionally a `detailed`/`intervals`/
    // `streams`-only artefact. `summary` mode skips both the
    // fetch AND the renderer call to guarantee zero extra HTTP
    // pressure and zero output surface.
    if analysis_type != "summary"
        && let Some(block) =
            render_endurance_evidence(period_context.metrics.endurance_evidence.as_ref())
    {
        content.push(block);
    }

    let planned_workouts = period
        .iter()
        .filter(|activity| is_planned_workout_id(&activity.id))
        .collect::<Vec<_>>();

    if !planned_workouts.is_empty() {
        let rows = planned_workouts
            .iter()
            .map(|activity| {
                let detail = fetched.activity_details.get(&activity.id);
                let moving_duration = detail
                    .and_then(|value| value.get("moving_time"))
                    .and_then(|value| value.as_i64())
                    .map(format_duration_hhmm);
                let elapsed_duration = detail
                    .and_then(|value| value.get("elapsed_time"))
                    .and_then(|value| value.as_i64())
                    .map(format_duration_hhmm);
                let duration = planned_workout_duration_cell(moving_duration, elapsed_duration);
                let load = planned_workout_load_cell(
                    detail
                        .and_then(|d| extract_activity_load(Some(d)))
                        .map(|obs| obs.value),
                );
                let date = planned_workout_date_cell(activity);

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

        content.extend(planned_workouts_blocks(rows));
    }

    let non_workout_calendar_events = calendar_events
        .iter()
        .filter(|event| !matches!(event.category, EventCategory::Workout))
        .copied()
        .collect::<Vec<_>>();

    if !non_workout_calendar_events.is_empty() {
        let rows = build_calendar_event_rows(&non_workout_calendar_events);
        content.extend(calendar_events_blocks(rows));
    }

    if !requested.is_empty() {
        let rows = build_requested_period_metric_rows(
            &requested,
            &period,
            &period_snapshot,
            &fetched.activity_details,
            period_context.metrics.etvs.as_ref(),
        );
        content.extend(requested_period_metrics_blocks(rows));
    }

    let show_context_sections = analysis_type != "summary";
    if show_context_sections {
        if let Some(trend) = &period_context.metrics.trend {
            content.push(trend_context_block(trend));
        }

        // Linear trend analysis using AnalysisEngine::analyze_trend
        {
            let mut tss_series: Vec<(chrono::NaiveDate, f32)> = Vec::new();
            let mut distance_series: Vec<(chrono::NaiveDate, f32)> = Vec::new();
            let mut time_series: Vec<(chrono::NaiveDate, f32)> = Vec::new();

            for activity in &period {
                if let Some(date) = parse_activity_date(&activity.start_date_local)
                    && let Some(detail) = fetched.activity_details.get(&activity.id)
                    && let Some(obj) = detail.as_object()
                {
                    if let Some(obs) = extract_activity_load(Some(detail)) {
                        tss_series.push((date, obs.value as f32));
                    }
                    if let Some(dist) = obj.get("distance").and_then(|v| v.as_f64()) {
                        distance_series.push((date, dist as f32 / 1000.0));
                    }
                    if let Some(moving) = obj
                        .get("moving_time")
                        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|n| n as i64)))
                    {
                        time_series.push((date, moving as f32 / 3600.0));
                    }
                }
            }

            let mut trend_insights = Vec::new();
            if let Some(insight) = AnalysisEngine::analyze_trend(
                &tss_series,
                crate::engines::analysis::TrendWindows::MEDIUM,
                "TSS",
            ) {
                trend_insights.push(insight);
            }
            if let Some(insight) = AnalysisEngine::analyze_trend(
                &distance_series,
                crate::engines::analysis::TrendWindows::MEDIUM,
                "Distance",
            ) {
                trend_insights.push(insight);
            }
            if let Some(insight) = AnalysisEngine::analyze_trend(
                &time_series,
                crate::engines::analysis::TrendWindows::MEDIUM,
                "Duration",
            ) {
                trend_insights.push(insight);
            }

            let insight_descriptions = trend_insights
                .iter()
                .map(|insight| insight.description.clone())
                .collect::<Vec<_>>();
            if let Some(block) = linear_trends_block(&insight_descriptions) {
                content.push(block);
            }
        }

        content.push(load_management_block(
            period_context.metrics.load_management.as_ref(),
        ));

        if let Some(block) = render_ndli_section(&period_context.metrics.ndli) {
            content.push(block);
        }

        // Heat Stress Context
        if let Some(block) = render_heat_section(&period_context.metrics.heat) {
            content.push(block);
        }

        // ESPE Power-Duration Anchors
        if let Some(block) = render_espe_section(
            &period_context.metrics.espe_anchors,
            &period_context.metrics.espe_derived,
        ) {
            content.push(block);
        }

        if let Some(block) = render_fitness_snapshot(&period_context.metrics.fitness) {
            content.push(block);
        }

        // Training Intensity Distribution
        if let Some(pol) = &period_context.metrics.polarisation
            && let (Some(z1), Some(z2), Some(z3)) = (pol.z1_pct, pol.z2_pct, pol.z3_pct)
        {
            let (tid_model, pi) = classify_tid_model(z1, z2, z3);
            content.push(tid_distribution_block(pol, z1, z2, z3, tid_model, pi));
        }

        // Zone Distribution
        if let Some(last_id) = period_ids.last()
            && let Some(last_detail) = fetched.activity_details.get(last_id)
            && let Some(zones_obj) = last_detail.get("icu_zone_times").and_then(Value::as_object)
        {
            let zone_rows = build_zone_distribution_rows(zones_obj);
            if !zone_rows.is_empty() {
                content.extend(zone_distribution_blocks(zone_rows));
            }
        }

        // W′ Depletion Rollup (WDR 7-day)
        if let Some(block) = render_wdrm_section(&period_context.metrics.wdrm) {
            content.push(block);
        }

        // Consistency: planned vs completed
        if let Some(consistency) = &period_context.metrics.consistency {
            content.push(consistency_block(consistency));
        }

        // Power Curve Comparison
        if period_context.metrics.espe_derived.is_some() {
            let period_ids: Vec<String> = period.iter().map(|a| a.id.clone()).collect();
            if let Some(last_id) = period_ids.last()
                && let Some(last_detail) = fetched.activity_details.get(last_id)
            {
                let anchors = extract_sportinfo_anchors(fetched.wellness.as_ref());
                let cur_mmp_p1m = last_detail.get("icu_pm_1m").and_then(Value::as_f64);
                let cur_mmp_p5m = last_detail.get("icu_pm_5m").and_then(Value::as_f64);
                let cur_mmp_p20m = last_detail.get("icu_pm_20m").and_then(Value::as_f64);
                let cur_mmp_p60m = last_detail.get("icu_pm_60m").and_then(Value::as_f64);
                let espe_current = derive_espe_metrics(
                    &anchors,
                    cur_mmp_p1m,
                    cur_mmp_p5m,
                    cur_mmp_p20m,
                    cur_mmp_p60m,
                );

                // Also derive ESPE from previous period's last activity
                let prev_ids: Vec<String> = previous_period.iter().map(|a| a.id.clone()).collect();
                let espe_previous = if let Some(prev_last_id) = prev_ids.last()
                    && let Some(prev_detail) = fetched.activity_details.get(prev_last_id)
                {
                    let prev_mmp_p1m = prev_detail.get("icu_pm_1m").and_then(Value::as_f64);
                    let prev_mmp_p5m = prev_detail.get("icu_pm_5m").and_then(Value::as_f64);
                    let prev_mmp_p20m = prev_detail.get("icu_pm_20m").and_then(Value::as_f64);
                    let prev_mmp_p60m = prev_detail.get("icu_pm_60m").and_then(Value::as_f64);
                    derive_espe_metrics(
                        &anchors,
                        prev_mmp_p1m,
                        prev_mmp_p5m,
                        prev_mmp_p20m,
                        prev_mmp_p60m,
                    )
                } else {
                    // Fall back to comparing against current (no previous data = no meaningful deltas)
                    espe_current.clone()
                };

                let (deltas, rotation, statuses, adaptation_state) =
                    crate::engines::coach_metrics::compare_power_curves(
                        &espe_current,
                        &espe_previous,
                    );
                if let Some(block) =
                    power_curve_block(&deltas, rotation, &statuses, &adaptation_state)
                {
                    content.push(block);
                }
                // Store adaptation_state back into period_context
                if let Some(ref mut espe_mut) = period_context.metrics.espe_derived {
                    espe_mut.adaptation_state = adaptation_state;
                }
            }
        }

        // CP Regression — validate API eFTP/W' against MMP-derived curve
        if let Some(espe) = &period_context.metrics.espe_derived {
            let mut cp_data: Vec<(f64, f64)> = Vec::new();
            if let Some(p1) = espe.p1m {
                cp_data.push((60.0, p1));
            }
            if let Some(p5) = espe.p5m {
                cp_data.push((300.0, p5));
            }
            if let Some(p20) = espe.p20m {
                cp_data.push((1200.0, p20));
            }
            if let Some(p60) = espe.p60m {
                cp_data.push((3600.0, p60));
            }
            if cp_data.len() >= 3
                && let Some(cp_result) = fit_cp(&cp_data)
            {
                let api_diff = period_context
                    .metrics
                    .espe_anchors
                    .as_ref()
                    .and_then(|anchors| Some((anchors.eftp?, anchors.w_prime?)))
                    .map(|(api_ftp, api_wp)| validate_cp(&cp_result, api_ftp, api_wp));
                content.push(cp_diagnostics_block(&cp_result, api_diff));
            }
        }

        // Ultra-specific tokens
        if !period.is_empty() {
            let period_ids: Vec<String> = period.iter().map(|a| a.id.clone()).collect();
            let daily_loads = build_daily_load_series(&period, &fetched.activity_details, &window);

            // Training Load Data Quality: provenance and coverage
            content.push(build_load_data_quality_section(&daily_loads));

            let loads: Vec<f64> = daily_loads.values().collect();
            if !loads.is_empty() {
                let b2b = back_to_back_load(&loads);
                if b2b > 0.0 {
                    content.push(load_patterns_block(b2b));
                }
            }
            let detail_refs: Vec<&serde_json::Map<String, Value>> = period_ids
                .iter()
                .filter_map(|id| fetched.activity_details.get(id))
                .filter_map(|v| v.as_object())
                .collect();
            if !detail_refs.is_empty() {
                let vert = vert_per_week(&detail_refs);
                if vert > 0.0 {
                    content.push(terrain_specificity_block(vert));
                }
            }
        }

        if let Some(block) = data_availability_block(
            &period_context.audit.degraded_mode_reasons,
            period_context.audit.all_available(),
        ) {
            content.push(block);
        }
    }

    if analysis_type == "streams" {
        let load_activities = period.to_vec();
        let daily_series =
            build_daily_load_series(&load_activities, &fetched.activity_details, &window);
        let rows = daily_series
            .daily
            .iter()
            .rev()
            .take(7)
            .rev()
            .map(|(date, load)| vec![date.to_string(), format!("{load:.1}")])
            .collect::<Vec<_>>();
        content.extend(daily_load_series_blocks(rows));
    } else if analysis_type == "intervals" {
        let interval_keyword = desc_filter
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "interval".to_string());
        let rows = period
            .iter()
            .filter(|activity| {
                activity
                    .name
                    .as_ref()
                    .map(|name| name.to_lowercase().contains(&interval_keyword))
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
            content.extend(interval_sessions_blocks(rows));
        }
    }

    let mut suggestions = period_context.guidance.suggestions.clone();
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
    for action in &period_context.guidance.next_actions {
        if !next_actions.contains(action) {
            next_actions.push(action.clone());
        }
    }

    crate::intents::IntentOutput::new(content)
        .with_suggestions(suggestions)
        .with_next_actions(next_actions)
        .with_metadata(crate::content::OutputMetadata {
            total_count: Some(period.len() as u32),
            ..Default::default()
        })
}
