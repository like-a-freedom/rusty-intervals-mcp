use serde_json::Value;

use super::presentation::*;
use crate::content::date::{NA, data_availability_block};
use crate::domains::interval_detection::{self};
use crate::domains::interval_segment::{SegmentProvenance, SegmentRole, SegmentWindow};
use crate::domains::nutrition::{compute_carb_demand, compute_protein_demand};
use crate::engines::adaptation::classify_curve_profile;
use crate::engines::analysis::WorkoutInsights;
use crate::engines::analysis_fetch::SourceFetchState;
use crate::engines::analyze_training::shared::*;
use crate::engines::interval_analysis::{
    IntervalOutputKind, preferred_interval_output_kind, quality_output_finding,
    upstream_segment_windows,
};
use crate::engines::interval_segment_metrics::{compute_structured_consistency, enrich_segments};
use crate::engines::metric_streams::parse_metric_streams;
use crate::engines::trail_execution::compute_terrain_context;

pub(crate) fn render_single(report: SingleReport) -> crate::intents::IntentOutput {
    let SingleReport {
        date,
        activity_id,
        activity_name,
        mode: analysis_mode,
        requested,
        include_hist,
        fetched,
        context: mut workout_context,
        metrics: analysis_metrics,
        grade: workout_grade,
    } = report;
    let date = date.as_str();
    let workout_detail = fetched.workout_detail.as_ref();
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

    crate::intents::IntentOutput::new(content)
        .with_suggestions(suggestions)
        .with_next_actions(next_actions)
}
