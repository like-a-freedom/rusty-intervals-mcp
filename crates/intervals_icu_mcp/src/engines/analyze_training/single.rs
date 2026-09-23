use intervals_icu_client::IntervalsClient;
use serde_json::Value;

use super::render::*;
use super::shared::*;
use crate::content::IntentError;
use crate::content::date::{NA, data_availability_block, filter_activities_by_date, parse_date};
use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext};
use crate::domains::interval_detection::{self};
use crate::domains::interval_segment::{SegmentProvenance, SegmentRole, SegmentWindow};
use crate::domains::nutrition::{compute_carb_demand, compute_protein_demand};
use crate::engines::adaptation::classify_curve_profile;
use crate::engines::analysis::{
    AnalysisEngine, WorkoutInsights, WorkoutMetrics as AnalysisWorkoutMetrics,
};
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::{
    SingleWorkoutFetchRequest, SourceFetchState, fetch_single_workout_data,
};
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{
    derive_espe_metrics, derive_execution_metrics, derive_workout_metrics_context,
    enrich_anchors_from_activity, extract_sportinfo_anchors,
};
use crate::engines::fitness_context::FitnessContext;
use crate::engines::interval_analysis::{
    IntervalOutputKind, count_work_intervals, preferred_interval_output_kind,
    quality_output_finding, upstream_segment_windows,
};
use crate::engines::interval_segment_metrics::{compute_structured_consistency, enrich_segments};
use crate::engines::metric_streams::parse_metric_streams;
use crate::engines::trail_execution::compute_terrain_context;

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
        let content = single_no_activities_blocks(date, desc_filter);

        let suggestions = vec![
            "Check if activities are synced from your fitness device".into(),
            "Verify the date - did you train on this day?".into(),
            "Try expanding the date range to include nearby days".into(),
        ];

        let next_actions = vec![
            "To view recent activities: analyze_training with target_type: period and wider date range".into(),
            "To check athlete profile: manage_profile with action: get".into(),
        ];

        return Ok(AnalyzeReport::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions));
    }

    if matching.len() > 1 {
        let content = single_multiple_activities_blocks(date, desc_filter, &matching);

        let suggestions = vec![
            "Choose one activity from the list and retry with its `description_contains` value".into(),
            "For interval analysis, look for keywords like 'tempo', 'threshold', 'intervals', 'repeats', 'VO2'".into(),
            "Note: Only workouts created with structured intervals will show interval data".into(),
        ];

        let first_key_phrase = matching[0]
            .name
            .as_deref()
            .unwrap_or("Workout")
            .split(['-', '—', ':'])
            .next()
            .unwrap_or("Workout")
            .trim();

        let mut next_actions = vec![
            format!(
                "Retry with `description_contains` from the list above (e.g., `description_contains: \"{}\"`)",
                first_key_phrase
            ),
            "Use `analyze_training` with `target_type: period` to see all activities".into(),
        ];

        if matching.len() <= 3 {
            next_actions.push(format!(
                "Or specify activity ID directly if your MCP client supports it (e.g., `{}`)",
                matching[0].id
            ));
        }

        return Ok(AnalyzeReport::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions));
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

    let mut content = Vec::new();
    content.push(single_header_block(
        &activity_name,
        date,
        &activity_id,
        analysis_mode.as_str(),
    ));
    content.push(workout_grade_block(&workout_grade));

    let insights = WorkoutInsights::generate(&analysis_metrics, &workout_grade);
    if let Some(block) = insights_block(&insights) {
        content.push(block);
    }

    let rows = build_basic_workout_metric_rows(workout_detail);
    if !rows.is_empty() {
        content.push(basic_metrics_table_block(rows));
    }

    if let Some(block) = render_etvs_section(workout_context.metrics.etvs.as_ref()) {
        content.push(block);
    }

    if !requested.is_empty() {
        let rows = build_requested_single_metric_rows(
            workout_detail.and_then(Value::as_object),
            &requested,
            workout_context.metrics.etvs.as_ref(),
        );
        content.extend(requested_single_metrics_blocks(rows));
    }

    if analysis_mode.show_detailed_breakdown() {
        let rows = build_detailed_workout_rows(workout_detail);
        if !rows.is_empty() {
            content.extend(detailed_breakdown_blocks(rows));
        }
    }

    let activity_message_rows = build_activity_message_rows(&fetched.activity_messages);
    if !activity_message_rows.is_empty() {
        content.extend(workout_comments_blocks(activity_message_rows));
    }

    if analysis_mode.show_execution_context()
        && let Some(workout) = &workout_context.metrics.workout
        && (!workout.execution_notes.is_empty()
            || workout.efficiency_factor.is_some()
            || workout.aerobic_decoupling.is_some())
    {
        let mut lines = workout.execution_notes.clone();
        if let Some(efficiency_factor) = workout.efficiency_factor {
            lines.push(format!(
                "Efficiency Factor: {:.2} (power/HR, higher = fresher)",
                efficiency_factor
            ));
        }
        if let Some(decoupling) = &workout.aerobic_decoupling {
            lines.push(format!(
                "Aerobic Decoupling: {:.1}% ({})",
                decoupling.decoupling_pct, decoupling.state
            ));
        }
        content.push(execution_context_block(&lines));
    }

    if analysis_mode.show_execution_context() {
        let mut stream_metrics = Vec::new();
        if let Some(drift) = analysis_metrics.hr_drift_percent {
            stream_metrics.push(format!("HR Drift: {:.1}%", drift));
        }
        if let Some(variance) = analysis_metrics.pace_variance_percent {
            stream_metrics.push(format!("Pace Variance: {:.1}%", variance));
        }
        if let Some(block) = stream_metrics_block(&stream_metrics) {
            content.push(block);
        }
    }

    if let Some(block) = render_espe_section(
        &workout_context.metrics.espe_anchors,
        &workout_context.metrics.espe_derived,
    ) {
        content.push(block);
    }
    if let Some(block) = render_wdrm_section(&workout_context.metrics.wdrm) {
        content.push(block);
    }
    if let Some(workout) = &workout_context.metrics.workout
        && let Some(block) = render_isdm_section(&workout.aerobic_decoupling)
    {
        content.push(block);
    }

    if let Some(block) = render_fitness_snapshot(&workout_context.metrics.fitness) {
        content.push(block);
    }

    if analysis_mode.show_interval_section() {
        let intervals_arr = fetched
            .intervals
            .as_ref()
            .and_then(Value::as_array)
            .filter(|items| !items.is_empty());

        let streams_available =
            fetched.streams_state == SourceFetchState::Available && fetched.streams.is_some();
        let local_detection = if streams_available {
            build_local_raw_stream(fetched.streams.as_ref().unwrap())
                .map(|raw| interval_detection::detect_intervals(&raw))
        } else {
            None
        };

        if let Some(detection) = local_detection {
            let upstream_failed =
                matches!(fetched.intervals_state, SourceFetchState::Failed { .. });
            let rationale = detection.reasons.first().map(String::as_str).unwrap_or(NA);

            match detection.session_kind {
                interval_detection::SessionKind::StructuredIntervals => {
                    let work_duration_s = detection
                        .work_segments
                        .iter()
                        .map(|segment| segment.range.end - segment.range.start)
                        .sum::<f64>();
                    let mean_work_intensity = detection
                        .work_segments
                        .iter()
                        .map(|segment| segment.mean_intensity)
                        .sum::<f64>()
                        / detection.work_segments.len() as f64;
                    let confidence = detection.confidence.unwrap_or_default();
                    append_structured_interval_header(
                        &mut content,
                        detection.work_segments.len(),
                        detection.recovery_segments.len(),
                        work_duration_s,
                        mean_work_intensity,
                        confidence,
                    );

                    let metric_streams = fetched.streams.as_ref().and_then(parse_metric_streams);

                    if let Some(ref streams) = metric_streams {
                        let presentation = sport_presentation(fetched.workout_detail.as_ref());

                        let work_windows: Vec<SegmentWindow> = detection
                            .work_segments
                            .iter()
                            .map(|seg| SegmentWindow {
                                role: SegmentRole::Work,
                                range: seg.range,
                            })
                            .collect();

                        let recovery_windows: Vec<SegmentWindow> = detection
                            .recovery_segments
                            .iter()
                            .map(|seg| SegmentWindow {
                                role: SegmentRole::Recovery,
                                range: seg.range,
                            })
                            .collect();

                        let efforts = enrich_segments(streams, &work_windows);
                        let recoveries = enrich_segments(streams, &recovery_windows);
                        let consistency = compute_structured_consistency(&efforts);

                        let report = crate::domains::interval_segment::SegmentSeriesReport {
                            provenance: SegmentProvenance::LocalStructured,
                            efforts,
                            recoveries,
                            consistency,
                        };

                        append_segment_report(&mut content, &report, presentation);
                    }
                }
                interval_detection::SessionKind::Fartlek => {
                    append_fartlek_interval_header(&mut content, rationale);

                    let metric_streams = fetched.streams.as_ref().and_then(parse_metric_streams);

                    if let (Some(streams), Some(series)) =
                        (metric_streams.as_ref(), detection.fartlek_series.as_ref())
                    {
                        let presentation = sport_presentation(fetched.workout_detail.as_ref());

                        let surge_windows: Vec<SegmentWindow> = series
                            .effort_segments
                            .iter()
                            .map(|seg| SegmentWindow {
                                role: SegmentRole::Surge,
                                range: seg.range,
                            })
                            .collect();

                        let rec_windows: Vec<SegmentWindow> = series
                            .recovery_segments
                            .iter()
                            .map(|seg| SegmentWindow {
                                role: SegmentRole::Recovery,
                                range: seg.range,
                            })
                            .collect();

                        let efforts = enrich_segments(streams, &surge_windows);
                        let recoveries = enrich_segments(streams, &rec_windows);

                        let report = crate::domains::interval_segment::SegmentSeriesReport {
                            provenance: SegmentProvenance::LocalFartlek,
                            efforts,
                            recoveries,
                            consistency: None,
                        };

                        append_segment_report(&mut content, &report, presentation);
                    }
                }
                interval_detection::SessionKind::Other => {
                    append_other_interval_header(&mut content, rationale);
                }
                interval_detection::SessionKind::InsufficientData => {
                    append_insufficient_interval_header(&mut content, rationale);
                }
            }

            if upstream_failed {
                append_upstream_fallback_warning(&mut content);
            }
        } else if let Some(intervals_arr) = intervals_arr {
            let output_kind =
                preferred_interval_output_kind(intervals_arr, fetched.streams.as_ref());
            let output_header = match output_kind {
                IntervalOutputKind::Power => "Avg Power",
                IntervalOutputKind::Pace => "Avg Pace",
            };

            let interval_rows =
                build_interval_analysis_rows(intervals_arr, fetched.streams.as_ref(), output_kind);
            append_upstream_reference_section(&mut content, output_header, interval_rows);

            if let Some(ref streams) = fetched.streams {
                let metric_streams = parse_metric_streams(streams);
                if let Some(ref streams) = metric_streams {
                    let presentation = sport_presentation(fetched.workout_detail.as_ref());
                    let segment_windows = upstream_segment_windows(intervals_arr, &streams.time_s);
                    if !segment_windows.is_empty() {
                        let (work, recovery): (Vec<_>, Vec<_>) = segment_windows
                            .into_iter()
                            .partition(|sw| sw.role == SegmentRole::Work);
                        let efforts = enrich_segments(streams, &work);
                        let recoveries = enrich_segments(streams, &recovery);
                        let report = crate::domains::interval_segment::SegmentSeriesReport {
                            provenance: SegmentProvenance::UpstreamIntervalsIcu,
                            efforts,
                            recoveries,
                            consistency: None,
                        };
                        append_segment_report(&mut content, &report, presentation);
                    }
                }
            }
        } else {
            append_interval_unavailable(&mut content);
        }
    }

    append_histogram_section(
        &mut content,
        "HR Histogram",
        fetched.hr_histogram.as_ref(),
        Some("hr"),
        "bpm",
        "bpm",
    );

    if include_hist && fetched.power_histogram.is_none() {
        content.push(power_histogram_unavailable_block());
    } else {
        append_histogram_section(
            &mut content,
            "Power Histogram",
            fetched.power_histogram.as_ref(),
            Some("watts"),
            "W",
            "W",
        );
    }

    append_histogram_section(
        &mut content,
        "Pace Histogram",
        fetched.pace_histogram.as_ref(),
        None,
        "s/km",
        "m/s",
    );

    if let Some(best) = fetched.best_efforts.as_ref() {
        append_best_efforts_section(&mut content, best);
    }

    if analysis_mode.show_stream_section() {
        append_stream_insights(&mut content, fetched.streams.as_ref());
    }

    if analysis_mode.show_quality_findings()
        && let Some(workout) = &workout_context.metrics.workout
    {
        let mut findings = Vec::new();
        if let Some(count) = workout.interval_count {
            findings.push(format!("Detected {} intervals for quality review.", count));
        }
        if let Some(hr) = workout.avg_hr {
            findings.push(format!("Average heart rate held at {:.0} bpm.", hr));
        }
        if let Some(output_finding) =
            quality_output_finding(workout_detail, fetched.streams.as_ref())
        {
            findings.push(output_finding);
        }
        if let Some(block) = quality_findings_block(&findings) {
            content.push(block);
        }
    }

    if (analysis_mode.include_streams() || analysis_mode.show_detailed_breakdown())
        && let Some(hr_vec) = fetched
            .streams
            .as_ref()
            .and_then(|s| s.get("heartrate"))
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect::<Vec<_>>())
        && !hr_vec.is_empty()
    {
        let z2_bounds = workout_detail
            .and_then(Value::as_object)
            .and_then(|obj| {
                let z2_lo = obj.get("hr_zone_2_lower").and_then(Value::as_f64);
                let z2_hi = obj.get("hr_zone_2_upper").and_then(Value::as_f64);
                z2_lo.zip(z2_hi)
            })
            .or_else(|| {
                workout_detail.and_then(Value::as_object).and_then(|obj| {
                    obj.get("average_heartrate")
                        .and_then(Value::as_f64)
                        .map(|avg_hr| (avg_hr * 0.85, avg_hr * 1.05))
                })
            });
        if let Some((lower, upper)) = z2_bounds
            && lower > 0.0
            && upper > 0.0
        {
            let z2_hr_variance =
                crate::engines::coach_metrics::compute_z2_hr_variance(&hr_vec, lower, upper);
            if let Some(ref mut workout) = workout_context.metrics.workout
                && let Some(ref mut decoupling) = workout.aerobic_decoupling
            {
                decoupling.z2_hr_variance = z2_hr_variance;
            }
            if let Some(block) = render_z2_stability_section(lower, upper, z2_hr_variance) {
                content.push(block);
            }
        }
    }

    if let Some(detail_obj) = workout_detail.and_then(Value::as_object) {
        let elevation = detail_obj.get("elevation_gain").and_then(Value::as_f64);
        let distance = detail_obj.get("distance").and_then(Value::as_f64);
        let moving_time = detail_obj.get("moving_time").and_then(Value::as_i64);
        if let (Some(elev), Some(dist), Some(mtime)) = (elevation, distance, moving_time)
            && dist > 0.0
        {
            let terrain = compute_terrain_context(elev, dist, mtime, None);
            if let Some(block) = terrain_context_block(&terrain) {
                content.push(block);
            }
        }
    }

    if let Some(detail_obj) = workout_detail.and_then(Value::as_object) {
        let moving_secs = detail_obj.get("moving_time").and_then(Value::as_i64);
        let if_val = detail_obj
            .get("icu_intensity_factor")
            .and_then(Value::as_f64);
        if let Some(secs) = moving_secs
            && secs > 0
        {
            let hours = secs as f64 / 3600.0;
            let carb = compute_carb_demand(hours, if_val);
            let protein = compute_protein_demand(false);
            content.push(nutrition_context_block(carb, protein));
        }
    }

    if let Some(espe) = &workout_context.metrics.espe_derived {
        let is_running = workout_detail
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("weighted_average_pace"))
            .or_else(|| {
                workout_detail
                    .and_then(Value::as_object)
                    .and_then(|obj| obj.get("average_speed"))
            })
            .and_then(Value::as_f64)
            .is_some();
        let profile =
            classify_curve_profile(None, espe.p1m, espe.p5m, espe.p20m, espe.p60m, is_running);
        content.push(curve_profile_block(&profile));
    }

    if analysis_mode.show_data_availability()
        && let Some(block) = data_availability_block(
            &workout_context.audit.degraded_mode_reasons,
            workout_context.audit.all_available(),
        )
    {
        content.push(block);
    }

    let suggestions = workout_context.guidance.suggestions.clone();

    let mut next_actions = vec![
        "To compare with similar workouts: compare_periods".into(),
        "To analyze training load: assess_recovery".into(),
        "To view period summary: analyze_training with target_type: period".into(),
    ];
    for action in &workout_context.guidance.next_actions {
        if !next_actions.contains(action) {
            next_actions.push(action.clone());
        }
    }

    Ok(AnalyzeReport::new(content)
        .with_suggestions(suggestions)
        .with_next_actions(next_actions))
}
