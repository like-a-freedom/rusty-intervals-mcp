use intervals_icu_client::IntervalsClient;
use serde_json::Value;

use super::render::*;
use super::shared::*;
use crate::content::date::{
    data_availability_block, filter_activities_by_range, filter_events_by_range, format_pct,
    parse_date,
};
use crate::content::{ContentBlock, IntentError, OutputMetadata};
use crate::domains::activity_analysis::{back_to_back_load, vert_per_week};
use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext};
use crate::engines::analysis::AnalysisEngine;
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::{
    PeriodFetchRequest, build_daily_load_series, build_previous_window, extract_activity_load,
    fetch_period_data,
};
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{
    aggregate_period_etvs, build_trend_snapshot, classify_tid_model, compute_consistency_index,
    compute_heat_metrics_7d, compute_load_management_metrics, compute_ndli_7d,
    compute_wdr_7d_rollup, derive_espe_metrics, derive_trend_metrics, derive_volume_metrics,
    enrich_anchors_from_activity, extract_sportinfo_anchors, parse_api_load_snapshot,
    parse_polarisation_from_api,
};
use crate::engines::cp_regression::{fit_cp, validate_cp};
use crate::engines::endurance_evidence::{CyclingSessionInput, compute_endurance_evidence};
use crate::engines::interval_analysis::is_planned_workout_id;
use crate::engines::metric_streams::parse_metric_streams;
use crate::engines::shared::parse_activity_date;
use intervals_icu_client::EventCategory;

pub(crate) async fn fetch_period_stats(
    client: &dyn IntervalsClient,
    window: crate::domains::coach::AnalysisWindow,
    workout_type: Option<&str>,
) -> Result<PeriodStats, IntentError> {
    let start_date = window.start_date;
    let end_date = window.end_date;

    let fetched = fetch_period_data(
        client,
        &PeriodFetchRequest {
            window: window.clone(),
            include_activity_details: true,
            include_comparison_window: false,
            include_endurance_evidence: false,
        },
    )
    .await
    .map_err(|e| IntentError::api(e.to_string()))?;

    let period = filter_activities_by_range(&fetched.activities, &start_date, &end_date);

    let planned_count = fetched
        .calendar_events
        .iter()
        .filter(|event| {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&event.start_date_local, "%Y-%m-%d")
            {
                date >= start_date && date <= end_date
            } else {
                false
            }
        })
        .count();

    if period.is_empty() {
        return Ok(PeriodStats {
            snapshot: crate::engines::coach_metrics::TrendSnapshot {
                activity_count: 0,
                total_time_secs: 0,
                total_distance_m: 0.0,
                total_elevation_m: 0.0,
            },
            window_days: window.window_days(),
            activities: Vec::new(),
            activity_details: fetched.activity_details,
            planned_count,
            etvs: None,
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
        .collect::<HashMap<_, _>>();

    let snapshot = if period.is_empty() {
        crate::engines::coach_metrics::TrendSnapshot {
            activity_count: 0,
            total_time_secs: 0,
            total_distance_m: 0.0,
            total_elevation_m: 0.0,
        }
    } else {
        build_trend_snapshot(&period, &activity_details)
    };

    let etvs = aggregate_period_etvs(&period, &activity_details);

    Ok(PeriodStats {
        snapshot,
        window_days: window.window_days(),
        activities: period.into_iter().cloned().collect(),
        activity_details,
        planned_count,
        etvs,
    })
}

pub async fn analyze_period(
    input: &Value,
    client: &dyn IntervalsClient,
) -> Result<AnalyzeReport, IntentError> {
    let start = input
            .get("period_start")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                IntentError::validation(
                    "target_type=\"period\" requires period_start and period_end; do not use start_date/end_date".to_string(),
                )
            })?;
    let end = input
            .get("period_end")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                IntentError::validation(
                    "target_type=\"period\" requires period_start and period_end; do not use start_date/end_date".to_string(),
                )
            })?;
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

    let start_date = parse_date(start, "period_start")?;
    let end_date = parse_date(end, "period_end")?;

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

    let fetched = fetch_period_data(
        client,
        &PeriodFetchRequest {
            window: window.clone(),
            include_activity_details: true,
            include_comparison_window: true,
            include_endurance_evidence: analysis_type != "summary",
        },
    )
    .await
    .map_err(|e| IntentError::api(e.to_string()))?;
    let fitness_context = crate::engines::fitness_context::FitnessContext::load(client).await;

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
            "# Period: {} to {}\nStatus: No activities found",
            start, end
        )));

        let summary = [
                format!(
                    "  No completed activities returned for {} to {}",
                    start, end
                ),
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
            let calendar_rows = build_calendar_event_rows(&calendar_events);
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

        let suggestions = vec![
            "Verify the date range - did you train during this period?".into(),
            "Try a wider date range to capture recent or upcoming workouts".into(),
            "Use compare_periods to contrast with a nearby range that has activity".into(),
        ];

        let next_actions = vec![
            "To analyze a different period: analyze_training with wider period_start/period_end"
                .into(),
            "To compare with another period: compare_periods".into(),
        ];

        return Ok(AnalyzeReport::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions));
    }

    let mut period_context = CoachContext::new(AnalysisKind::TrainingPeriod, window.clone());
    period_context.audit = build_data_audit(&fetched);

    let period_snapshot = build_trend_snapshot(&period, &fetched.activity_details);
    let previous_snapshot = build_trend_snapshot(&previous_period, &fetched.activity_details);

    period_context.metrics.volume = Some(derive_volume_metrics(
        period_context.meta.window_days,
        period_snapshot.total_time_secs,
        period_snapshot.total_distance_m,
        period_snapshot.total_elevation_m,
        period.len(),
    ));
    period_context.metrics.trend = Some(derive_trend_metrics(period_snapshot, previous_snapshot));
    period_context.metrics.fitness = fitness_context.metrics().cloned();

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
        let load_values = daily_loads.values().collect::<Vec<_>>();
        let recovery_index = period_context
            .metrics
            .wellness
            .as_ref()
            .and_then(|w| w.recovery_index);
        period_context.metrics.load_management =
            compute_load_management_metrics(&load_values, recovery_index);
    }

    if let Some(api_acwr) = api_load_snapshot {
        period_context
            .metrics
            .load_management
            .get_or_insert_with(Default::default)
            .acwr = Some(api_acwr);
    }

    let period_ids: Vec<String> = period.iter().map(|a| a.id.clone()).collect();
    let ndli = compute_ndli_7d(&fetched.activity_details, &period_ids);
    period_context.metrics.ndli = Some(ndli);

    period_context.metrics.heat = Some(compute_heat_metrics_7d(
        &fetched.activity_details,
        &period_ids,
    ));

    // Polarisation / TID from the most recent activity's zone distribution
    if let Some(last_id) = period_ids.last()
        && let Some(last_detail) = fetched.activity_details.get(last_id)
    {
        period_context.metrics.polarisation =
            parse_polarisation_from_api(Some(last_detail), last_detail.get("icu_zone_times"));
    }

    let mut espe_anchors = extract_sportinfo_anchors(fetched.wellness.as_ref());
    if let Some(last_activity_id) = period_ids.last()
        && let Some(last_detail) = fetched.activity_details.get(last_activity_id)
    {
        enrich_anchors_from_activity(&mut espe_anchors, Some(last_detail));
    }
    let w_prime = espe_anchors.w_prime;
    let espe_derived = derive_espe_metrics(&espe_anchors, None, None, None, None);
    period_context.metrics.espe_anchors = Some(espe_anchors);
    period_context.metrics.espe_derived = Some(espe_derived);

    // ── Endurance evidence (power-based, cycling-only v1) ──────────
    //
    // Built from the bounded profile streams fetched during the
    // period data retrieval. Streams that weren't fetched (or
    // couldn't be parsed) simply don't appear in `profile_sessions`,
    // which the engine degrades to the appropriate `MissingEftp` /
    // `InsufficientCandidateSessions` status.
    let eftp = period_context
        .metrics
        .espe_anchors
        .as_ref()
        .and_then(|a| a.eftp);
    let profile_sessions: Vec<CyclingSessionInput> = fetched
        .endurance_profile_activities
        .iter()
        .filter_map(|activity| {
            let stream = fetched.endurance_profile_streams.get(&activity.id)?;
            let streams = parse_metric_streams(stream)?;
            Some(CyclingSessionInput {
                activity_id: activity.id.clone(),
                date: crate::engines::shared::parse_activity_date(&activity.start_date_local)?,
                streams,
            })
        })
        .collect();
    period_context.metrics.endurance_evidence = Some(compute_endurance_evidence(
        &profile_sessions,
        window.end_date,
        eftp,
    ));

    // W5 — WDR 7-day rollup across period activities
    period_context.metrics.wdrm = Some(compute_wdr_7d_rollup(
        &fetched.activity_details,
        &period_ids,
        w_prime,
    ));

    // ETVS — aggregate Effective Training Volume Score across period
    period_context.metrics.etvs = aggregate_period_etvs(&period, &fetched.activity_details);

    // Consistency: planned vs completed workouts
    let planned_count = calendar_events
        .iter()
        .filter(|event| matches!(event.category, EventCategory::Workout))
        .count();
    period_context.metrics.consistency = Some(compute_consistency_index(
        period_snapshot.activity_count,
        planned_count,
    ));

    period_context.alerts = build_alerts(&period_context.metrics);
    period_context.guidance = build_guidance(&period_context.metrics, &period_context.alerts);

    let weekly_hrs = period_context
        .metrics
        .volume
        .as_ref()
        .map(|volume| volume.weekly_avg_hours)
        .unwrap_or_default();

    let mut content = Vec::new();
    content.push(ContentBlock::markdown(format!(
        "# Period: {} to {}",
        start, end
    )));

    let rows = build_period_summary_rows(period.len(), &period_snapshot, weekly_hrs);
    content.push(ContentBlock::table(
        vec!["Metric".into(), "Value".into()],
        rows,
    ));

    if let Some(etvs_text) = render_etvs_section(period_context.metrics.etvs.as_ref()) {
        content.push(ContentBlock::markdown(etvs_text));
    }

    // Endurance evidence is intentionally a `detailed`/`intervals`/
    // `streams`-only artefact. `summary` mode skips both the
    // fetch AND the renderer call to guarantee zero extra HTTP
    // pressure and zero output surface.
    if analysis_type != "summary"
        && let Some(evidence_text) =
            render_endurance_evidence(period_context.metrics.endurance_evidence.as_ref())
    {
        content.push(ContentBlock::markdown(evidence_text));
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
                let duration = match (moving_duration, elapsed_duration) {
                    (Some(mov), Some(elp)) => format!("{} (elapsed: {})", mov, elp),
                    (Some(mov), None) => mov,
                    (None, Some(elp)) => format!("elapsed: {}", elp),
                    (None, None) => "n/a".to_string(),
                };
                let load = detail
                    .and_then(|d| extract_activity_load(Some(d)))
                    .map(|obs| format!("{:.1}", obs.value))
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

        content.push(ContentBlock::markdown("Planned Workouts".to_string()));
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
            period_context.metrics.etvs.as_ref(),
        );
        content.push(ContentBlock::markdown("Requested Metrics".to_string()));
        content.push(ContentBlock::table(
            vec!["Metric".into(), "Value".into(), "Status".into()],
            rows,
        ));
    }

    let show_context_sections = analysis_type != "summary";
    if show_context_sections {
        if let Some(trend) = &period_context.metrics.trend {
            content.push(ContentBlock::markdown(format!(
                    "Trend Context\n  Activity delta: {}\n  Time delta: {}\n  Distance delta: {}\n  Elevation delta: {}",
                    trend
                        .activity_count_delta
                        .map(|delta| format!("{:+}", delta))
                        .unwrap_or_else(|| "n/a".into()),
                    format_pct(trend.time_delta_pct),
                    format_pct(trend.distance_delta_pct),
                    format_pct(trend.elevation_delta_pct),
                )));
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

            if !trend_insights.is_empty() {
                let mut trend_lines = vec!["Linear Trends".to_string()];
                for insight in &trend_insights {
                    trend_lines.push(format!("  {}", insight.description));
                }
                content.push(ContentBlock::markdown(trend_lines.join("\n")));
            }
        }

        content.push(ContentBlock::markdown(build_load_management_text(
            period_context.metrics.load_management.as_ref(),
        )));

        if let Some(ndli_text) = render_ndli_section(&period_context.metrics.ndli) {
            content.push(ContentBlock::markdown(ndli_text));
        }

        // Heat Stress Context
        if let Some(heat_text) = render_heat_section(&period_context.metrics.heat) {
            content.push(ContentBlock::markdown(heat_text));
        }

        // ESPE Power-Duration Anchors
        if let Some(espe_text) = render_espe_section(
            &period_context.metrics.espe_anchors,
            &period_context.metrics.espe_derived,
        ) {
            content.push(ContentBlock::markdown(espe_text));
        }

        if let Some(fit_text) = render_fitness_snapshot(&period_context.metrics.fitness) {
            content.push(ContentBlock::markdown(fit_text));
        }

        // Training Intensity Distribution
        if let Some(pol) = &period_context.metrics.polarisation
            && let (Some(z1), Some(z2), Some(z3)) = (pol.z1_pct, pol.z2_pct, pol.z3_pct)
        {
            let (tid_model, pi) = classify_tid_model(z1, z2, z3);
            let mut tid_lines = vec!["Training Intensity Distribution".to_string()];
            tid_lines.push(format!("  TID Model: {}", tid_model));
            tid_lines.push(format!("  Z1: {:.1}%  Z2: {:.1}%  Z3: {:.1}%", z1, z2, z3));
            if let Some(pi_val) = pi {
                tid_lines.push(format!("  Polarization Index: {:.3}", pi_val));
            }
            if let Some(tid_model_str) = &pol.tid_model {
                tid_lines.push(format!("  Classification: {}", tid_model_str));
            }
            content.push(ContentBlock::markdown(tid_lines.join("\n")));
        }

        // Zone Distribution
        if let Some(last_id) = period_ids.last()
            && let Some(last_detail) = fetched.activity_details.get(last_id)
            && let Some(zones_obj) = last_detail.get("icu_zone_times").and_then(Value::as_object)
        {
            let zone_rows = build_zone_distribution_rows(zones_obj);
            if !zone_rows.is_empty() {
                content.push(ContentBlock::markdown("Time in Zones".to_string()));
                content.push(ContentBlock::table(
                    vec!["Zone".into(), "Time".into(), "%".into()],
                    zone_rows,
                ));
            }
        }

        // W′ Depletion Rollup (WDR 7-day)
        if let Some(wdrm_text) = render_wdrm_section(&period_context.metrics.wdrm) {
            content.push(ContentBlock::markdown(wdrm_text));
        }

        // Consistency: planned vs completed
        if let Some(consistency) = &period_context.metrics.consistency {
            let pct = consistency
                .ratio
                .map(|r| format!("{:.0}%", r * 100.0))
                .unwrap_or_else(|| "n/a".to_string());
            let state = consistency.state.as_deref().unwrap_or("unknown");
            content.push(ContentBlock::markdown(format!(
                    "Training Consistency\n  Planned sessions: {}\n  Completed sessions: {}\n  Adherence: {} ({})",
                    consistency.sessions_planned,
                    consistency.sessions_completed,
                    pct,
                    state,
                )));
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
                if !deltas.is_empty() {
                    let mut pc_lines = vec!["Power Curve Comparison".to_string()];
                    for d in &["1m", "5m", "20m", "60m"] {
                        if let Some(delta) = deltas.get(*d) {
                            let status = statuses.get(*d).map(|s| s.as_str()).unwrap_or("");
                            pc_lines.push(format!("  {}: {:+.1}% ({})", d, delta, status));
                        }
                    }
                    pc_lines.push(format!("  Rotation Index: {:.3}", rotation));
                    if let Some(ref state) = adaptation_state {
                        pc_lines.push(format!("  Adaptation State: {}", state));
                    }
                    content.push(ContentBlock::markdown(pc_lines.join("\n")));
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
                if let Some(anchors) = &period_context.metrics.espe_anchors
                    && let (Some(api_ftp), Some(api_wp)) = (anchors.eftp, anchors.w_prime)
                {
                    let (cp_diff, wp_diff) = validate_cp(&cp_result, api_ftp, api_wp);
                    cp_lines.push(format!(
                        "  Difference from API estimate: CP Δ{:.1}%, W′ Δ{:.1}%",
                        cp_diff, wp_diff
                    ));
                }
                content.push(ContentBlock::markdown(cp_lines.join("\n")));
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
                    content.push(ContentBlock::markdown(format!(
                        "Load Patterns\n  Back-to-Back Peak Load: {:.1}",
                        b2b
                    )));
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
                    content.push(ContentBlock::markdown(format!(
                        "Terrain Specificity\n  Weekly Vert: {:.0} m",
                        vert
                    )));
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
        content.push(ContentBlock::markdown("Daily Load Series".to_string()));
        content.push(ContentBlock::table(
            vec!["Date".into(), "Load".into()],
            rows,
        ));
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
            content.push(ContentBlock::markdown("Interval Sessions".to_string()));
            content.push(ContentBlock::table(
                vec!["Date".into(), "Workout".into()],
                rows,
            ));
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

    Ok(AnalyzeReport::new(content)
        .with_suggestions(suggestions)
        .with_next_actions(next_actions)
        .with_metadata(OutputMetadata {
            total_count: Some(period.len() as u32),
            ..Default::default()
        }))
}

// ═══════════════════════════════════════════════════════════════════════════
// compare_periods — like-for-like comparison of two training windows
// ═══════════════════════════════════════════════════════════════════════════

use std::collections::HashMap;
