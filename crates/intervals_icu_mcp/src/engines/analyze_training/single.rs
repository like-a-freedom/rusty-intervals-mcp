use intervals_icu_client::IntervalsClient;
use serde_json::Value;

use super::shared::*;
use crate::content::IntentError;
use crate::content::date::{filter_activities_by_date, parse_date};
use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext};
use crate::engines::analysis::{AnalysisEngine, WorkoutMetrics as AnalysisWorkoutMetrics};
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::{SingleWorkoutFetchRequest, fetch_single_workout_data};
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{
    derive_espe_metrics, derive_execution_metrics, derive_workout_metrics_context,
    enrich_anchors_from_activity, extract_sportinfo_anchors,
};
use crate::engines::fitness_context::FitnessContext;
use crate::engines::interval_analysis::count_work_intervals;

pub async fn analyze_single(
    input: &Value,
    client: &dyn IntervalsClient,
) -> Result<AnalyzeReport, IntentError> {
    let date = input
        .get("date")
        .and_then(Value::as_str)
        .ok_or_else(|| IntentError::validation("Missing required field for single: date"))?;
    let desc_filter = input.get("description_contains").and_then(Value::as_str);

    let target_date = parse_date(date, "date")?;

    let activities = client
        .get_recent_activities(Some(50), Some(30))
        .await
        .map_err(|e| IntentError::api(format!("Failed to fetch activities: {}", e)))?;

    tracing::debug!("Fetched {} activities", activities.len());
    for a in &activities {
        tracing::debug!(
            "Activity: id={}, name={}, date={}",
            a.id,
            a.name.as_deref().unwrap_or("N/A"),
            a.start_date_local
        );
    }

    let mut matching = filter_activities_by_date(&activities, &target_date);

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

    if matching.is_empty() {
        return Ok(AnalyzeReport::SingleEmpty {
            date: date.to_owned(),
            filter: desc_filter.map(str::to_owned),
        });
    }

    if matching.len() > 1 {
        return Ok(AnalyzeReport::SingleMultiple {
            date: date.to_owned(),
            filter: desc_filter.map(str::to_owned),
            activities: matching.into_iter().cloned().collect(),
        });
    }

    let activity = &matching[0];
    let activity_id = activity.id.clone();
    let activity_name = activity
        .name
        .as_deref()
        .unwrap_or("Unknown Activity")
        .to_string();

    let analysis_mode =
        SingleAnalysisMode::parse(input.get("analysis_type").and_then(Value::as_str));
    let requested = requested_metrics(input);
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
    .await
    .map_err(|e| IntentError::api(e.to_string()))?;
    fetched.activities = vec![(*activity).clone()];
    let fitness_context = FitnessContext::load(client).await;

    let mut workout_context = CoachContext::new(
        AnalysisKind::TrainingSingle,
        AnalysisWindow::new(target_date, target_date),
    );
    workout_context.audit = build_data_audit(&fetched);
    workout_context.metrics.fitness = fitness_context.metrics().cloned();

    let workout_detail = fetched.workout_detail.as_ref();
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
        if workout_context.audit.streams_available {
            execution_notes.push("Stream data available for deeper execution review.".into());
        } else {
            execution_notes.push("Stream review requested; stream data unavailable.".into());
        }
    }
    let (efficiency_factor, aerobic_decoupling) =
        derive_execution_metrics(fetched.workout_detail.as_ref(), fetched.streams.as_ref());
    let workout_metrics = derive_workout_metrics_context(
        work_interval_count,
        avg_hr,
        avg_power,
        efficiency_factor,
        aerobic_decoupling,
        execution_notes,
    );

    let analysis_metrics = {
        let obj = workout_detail.and_then(Value::as_object);
        let duration_secs = obj
            .and_then(|o| o.get("moving_time"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let distance_m = obj
            .and_then(|o| o.get("distance"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let elevation = obj
            .and_then(|o| o.get("total_elevation_gain"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);

        let hr_drift_from_streams = fetched.streams.as_ref().and_then(|s| {
            let hr_vec = s
                .get("heartrate")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect::<Vec<_>>())?;
            if hr_vec.len() < 2 {
                return None;
            }
            let mid = hr_vec.len() / 2;
            let first_half_avg = hr_vec[..mid].iter().sum::<f64>() / mid as f64;
            let second_half_avg = hr_vec[mid..].iter().sum::<f64>() / (hr_vec.len() - mid) as f64;
            let avg = hr_vec.iter().sum::<f64>() / hr_vec.len() as f64;
            Some(AnalysisEngine::calculate_hr_drift(
                avg as u32,
                first_half_avg as u32,
                second_half_avg as u32,
            ))
        });

        let pace_variance = fetched.streams.as_ref().and_then(|s| {
            let paces: Vec<f32> =
                s.get("velocity_smooth")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_f64())
                            .filter(|&v| v > 0.0)
                            .map(|v| (60.0 / (v * 60.0 / 1000.0)) as f32)
                            .collect()
                    })?;
            if paces.len() < 2 {
                return None;
            }
            Some(AnalysisEngine::calculate_pace_variance(&paces))
        });

        let hr_drift = hr_drift_from_streams.or_else(|| {
            workout_metrics
                .aerobic_decoupling
                .as_ref()
                .map(|d| d.decoupling_pct as f32)
        });

        AnalysisWorkoutMetrics {
            duration_minutes: (duration_secs / 60) as u32,
            distance_km: (distance_m / 1000.0) as f32,
            elevation_gain_m: elevation as f32,
            avg_hr: avg_hr.map(|v| v as u32),
            avg_power: avg_power.map(|v| v as f32),
            hr_drift_percent: hr_drift,
            pace_variance_percent: pace_variance,
            ..Default::default()
        }
    };
    let workout_grade = AnalysisEngine::grade_workout(&analysis_metrics, None);
    workout_context.metrics.workout = Some(workout_metrics);

    let mut espe_anchors = extract_sportinfo_anchors(fetched.wellness.as_ref());
    enrich_anchors_from_activity(&mut espe_anchors, fetched.workout_detail.as_ref());
    let wdrm = crate::engines::coach_metrics::compute_wdr_metrics(
        fetched.intervals.as_ref(),
        fetched.workout_detail.as_ref(),
        espe_anchors.w_prime,
    );
    let espe_derived = derive_espe_metrics(&espe_anchors, None, None, None, None);
    workout_context.metrics.espe_anchors = Some(espe_anchors);
    workout_context.metrics.espe_derived = Some(espe_derived);
    workout_context.metrics.wdrm = Some(wdrm);

    let moving_seconds = activity.moving_time.map(f64::from).or_else(|| {
        workout_detail
            .and_then(|detail| detail.get("moving_time"))
            .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
    });
    workout_context.metrics.etvs = crate::engines::coach_metrics::compute_etvs(
        workout_detail.and_then(|detail| detail.get("icu_zone_times")),
        moving_seconds,
    );

    workout_context.alerts = build_alerts(&workout_context.metrics);
    workout_context.guidance = build_guidance(&workout_context.metrics, &workout_context.alerts);

    Ok(AnalyzeReport::Single(SingleReport {
        date: date.to_owned(),
        activity_id,
        activity_name,
        mode: analysis_mode,
        requested,
        include_hist,
        fetched,
        context: workout_context,
        metrics: analysis_metrics,
        grade: workout_grade,
    }))
}
