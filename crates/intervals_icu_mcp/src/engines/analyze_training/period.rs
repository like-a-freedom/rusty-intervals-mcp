use intervals_icu_client::IntervalsClient;
use serde_json::Value;

use super::shared::*;
use crate::content::IntentError;
use crate::content::date::{
    filter_activities_by_range, filter_events_by_range, parse_date, validate_date_range,
};
use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext};
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::{
    PeriodFetchRequest, build_daily_load_series, build_previous_window, fetch_period_data,
};
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{
    aggregate_period_etvs, build_trend_snapshot, compute_consistency_index,
    compute_heat_metrics_7d, compute_load_management_metrics, compute_ndli_7d,
    compute_wdr_7d_rollup, derive_espe_metrics, derive_trend_metrics, derive_volume_metrics,
    enrich_anchors_from_activity, extract_sportinfo_anchors, parse_api_load_snapshot,
    parse_polarisation_from_api,
};
use crate::engines::endurance_evidence::{CyclingSessionInput, compute_endurance_evidence};
use crate::engines::metric_streams::parse_metric_streams;
use crate::engines::shared::parse_activity_date;
use intervals_icu_client::EventCategory;

fn period_load_management(
    loads: &[f64],
    wellness: Option<&crate::domains::coach::WellnessMetrics>,
) -> Option<crate::domains::coach::LoadManagementMetrics> {
    compute_load_management_metrics(
        loads,
        wellness.and_then(|w| w.baseline_relative_recovery_index()),
    )
}

#[cfg(test)]
mod recovery_scale_tests {
    use super::*;
    use crate::domains::coach::WellnessMetrics;

    #[test]
    fn fatigue_index_requires_both_baselines() {
        let loads = vec![25.0; 28];
        for (hrv_baseline, resting_hr_baseline, expected) in [
            (None, None, None),
            (Some(60.0), None, None),
            (None, Some(50.0), None),
            (Some(60.0), Some(50.0), Some(350.0)),
        ] {
            let wellness = WellnessMetrics {
                hrv_baseline,
                resting_hr_baseline,
                recovery_index: Some(0.5),
                ..Default::default()
            };
            let result = period_load_management(&loads, Some(&wellness)).unwrap();
            assert_eq!(result.fatigue_index, expected);
            assert!(result.stress_tolerance.is_some());
        }
    }
}

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

    let planned_count =
        filter_events_by_range(&fetched.calendar_events, &start_date, &end_date).len();

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
    let requested = requested_metrics(input);
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
    validate_date_range(&start_date, &end_date, None)?;

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
        return Ok(AnalyzeReport::PeriodEmpty {
            start: start.to_owned(),
            end: end.to_owned(),
            events: calendar_events.into_iter().cloned().collect(),
        });
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
        period_context.metrics.load_management =
            period_load_management(&load_values, period_context.metrics.wellness.as_ref());
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

    let period = period.into_iter().cloned().collect();
    let previous_period = previous_period.into_iter().cloned().collect();
    let events = calendar_events.into_iter().cloned().collect();
    Ok(AnalyzeReport::Period(PeriodReport {
        start: start.to_owned(),
        end: end.to_owned(),
        analysis_type: analysis_type.to_owned(),
        filter: desc_filter.map(str::to_owned),
        requested,
        fetched,
        period,
        previous_period,
        events,
        context: period_context,
        snapshot: period_snapshot,
        window,
        period_ids,
        weekly_hrs,
    }))
}

// ═══════════════════════════════════════════════════════════════════════════
// compare_periods — like-for-like comparison of two training windows
// ═══════════════════════════════════════════════════════════════════════════

use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use chrono::{Duration, Utc};
    use intervals_icu_client::Event;

    #[tokio::test]
    async fn fetch_period_stats_counts_calendar_timestamps_within_period() {
        let end = Utc::now().date_naive() - Duration::days(1);
        let start = end - Duration::days(2);
        let event = |id: &str, date: String| Event {
            id: Some(id.into()),
            start_date_local: date,
            name: "Workout".into(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        };
        let client = MockIntervalsClient::builder().with_events(vec![
            event("start", format!("{start}T08:30:00")),
            event("date-only", (start + Duration::days(1)).to_string()),
            event("end", format!("{end}T23:59:59")),
            event("before", format!("{}T23:59:59", start - Duration::days(1))),
            event("after", format!("{}T00:00:00", end + Duration::days(1))),
            event("invalid", "invalid".into()),
        ]);

        let stats = fetch_period_stats(&client, AnalysisWindow::new(start, end), None)
            .await
            .expect("period stats");
        assert_eq!(stats.planned_count, 3);
    }
}
