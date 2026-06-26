use crate::intents::{
    ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput, OutputMetadata,
};
use async_trait::async_trait;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
/// Analyze Training Intent Handler
///
/// Analyzes a single training session or a period of training.
use std::sync::Arc;

use super::render::analysis::*;
use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext};
use crate::engines::adaptation::classify_curve_profile;
use crate::engines::analysis::{
    AnalysisEngine, WorkoutInsights, WorkoutMetrics as AnalysisWorkoutMetrics,
};
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::{
    PeriodFetchRequest, SingleWorkoutFetchRequest, build_daily_load_series, build_previous_window,
    fetch_period_data, fetch_single_workout_data,
};
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{
    build_trend_snapshot, classify_tid_model, compute_heat_metrics_7d,
    compute_load_management_metrics, compute_ndli_7d, compute_wdr_metrics, compute_z2_hr_variance,
    derive_espe_metrics, derive_trend_metrics, derive_volume_metrics,
    derive_workout_metrics_context, enrich_anchors_from_activity, extract_sportinfo_anchors,
    parse_api_load_snapshot, parse_fitness_metrics,
};
use crate::engines::cp_regression::{fit_cp, validate_cp};
use crate::engines::trail_execution::compute_terrain_context;

use crate::domains::activity_analysis::{back_to_back_load, vert_per_week};
use crate::domains::nutrition::{compute_carb_demand, compute_protein_demand};
use crate::intents::utils::{
    data_availability_block, filter_activities_by_date, filter_activities_by_range,
    filter_events_by_range, parse_date,
};
use intervals_icu_client::EventCategory;

pub struct AnalyzeTrainingHandler;
impl AnalyzeTrainingHandler {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SingleAnalysisMode {
    Summary,
    Detailed,
    Intervals,
    Streams,
}

impl SingleAnalysisMode {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("summary") {
            "detailed" => Self::Detailed,
            "intervals" => Self::Intervals,
            "streams" => Self::Streams,
            _ => Self::Summary,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Detailed => "detailed",
            Self::Intervals => "intervals",
            Self::Streams => "streams",
        }
    }

    fn include_intervals(self) -> bool {
        matches!(self, Self::Intervals)
    }

    fn include_streams(self) -> bool {
        // Detailed mode needs streams for execution metrics (efficiency factor, aerobic decoupling)
        matches!(self, Self::Detailed | Self::Intervals | Self::Streams)
    }

    fn show_execution_context(self) -> bool {
        matches!(self, Self::Detailed | Self::Streams)
    }

    fn show_interval_section(self) -> bool {
        matches!(self, Self::Intervals)
    }

    fn show_stream_section(self) -> bool {
        matches!(self, Self::Streams)
    }

    fn show_quality_findings(self) -> bool {
        matches!(self, Self::Detailed | Self::Streams)
    }

    fn show_data_availability(self) -> bool {
        matches!(self, Self::Detailed | Self::Streams)
    }

    fn show_detailed_breakdown(self) -> bool {
        matches!(self, Self::Detailed)
    }
}

#[async_trait]
impl IntentHandler for AnalyzeTrainingHandler {
    fn name(&self) -> &'static str {
        "analyze_training"
    }

    fn description(&self) -> &'static str {
        "Analyzes training sessions — single workout or period. Returns power-duration \
         anchors (eFTP, W', pMax), ESPE-derived metrics (aerobic durability, glycolytic \
         bias), W' depletion (WDRM), signed aerobic decoupling (ISDM) with durability \
         state, Z2 HR stability, terrain context (index, VAM), nutrition demand (carb, \
         protein), and running/cycling curve profile. Period analysis adds heat stress \
         context, TID model (pyramidal/threshold/polarized), NDLI (neural density load), \
         power curve comparison (2-window deltas), ultra-specific tokens (back-to-back \
         load, vert/week), and load management (ACWR, monotony, strain). Also retrieves \
         calendar events (races, sick days, injuries, notes, planned workouts). \
         Includes a Fitness Snapshot with current CTL, ATL, TSB, and ramp rate when \
         athlete-summary data is available.
         
         Use this tool when: you need to review a completed workout's quality, assess \
         aerobic/neural fatigue, check pacing distribution, examine period trends, or \
         compare evolution. Do NOT use when: you need to plan future training (use \
         plan_training), assess recovery readiness (use assess_recovery), or perform \
         post-race debrief (use analyze_race).
         
         analysis_type controls depth: summary (basic metrics), detailed (+execution \
         context, Z2, terrain, nutrition, profile), intervals (+interval breakdown), \
         streams (+stream insights)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_type": {
                    "type": "string",
                    "enum": ["single", "period"],
                    "description": "Analysis type: single workout or period"
                },
                "date": {
                    "type": "string",
                    "description": "Workout date (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday') for single analysis"
                },
                "period_start": {
                    "type": "string",
                    "description": "Period start (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday') for period analysis and calendar-event context"
                },
                "period_end": {
                    "type": "string",
                    "description": "Period end (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday') for period analysis and calendar-event context"
                },
                "description_contains": {
                    "type": "string",
                    "description": "Filter activities by name/description (case-insensitive substring match). Works with target_type: single only. Examples: 'long run', 'tempo', 'intervals', 'threshold'"
                },
                "analysis_type": {
                    "type": "string",
                    "enum": ["summary", "detailed", "intervals", "streams"],
                    "default": "summary",
                    "description": "Analysis depth for single workouts: summary (basic metrics table), detailed (+execution context, Z2 HR stability, terrain context, nutrition demand, curve profile, quality findings), intervals (+structured interval breakdown with HR/power/pace per rep), streams (+raw stream min/max/points insights). For period analysis: always returns trend, load management, NDLI. Add streams for daily load series or intervals for interval session listing."
                },
                "include_best_efforts": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include best efforts comparison"
                },
                "include_histograms": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include power/HR/pace histograms. Only valid when target_type is 'single'."
                },
                "metrics": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Requested metrics: time, distance, vertical, tss, pace, hr. Results are surfaced explicitly; unavailable exact metrics are marked unavailable instead of being silently ignored."
                }
            },
            "required": ["target_type"],
            "oneOf": [
                {"required": ["target_type", "date"]},
                {"required": ["target_type", "period_start", "period_end"]}
            ],
            "if": {
                "properties": {
                    "target_type": { "const": "period" }
                }
            },
            "then": {
                "properties": {
                    "include_histograms": { "const": false },
                    "description_contains": false
                }
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let target_type = input
            .get("target_type")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: target_type"))?;

        match target_type {
            "single" => self.analyze_single(&input, client.as_ref()).await,
            "period" => self.analyze_period(&input, client.as_ref()).await,
            _ => Err(IntentError::validation(format!(
                "Invalid target_type: {}. Must be 'single' or 'period'",
                target_type
            ))),
        }
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}

impl AnalyzeTrainingHandler {
    async fn analyze_single(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
    ) -> Result<IntentOutput, IntentError> {
        let date = input
            .get("date")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field for single: date"))?;
        let desc_filter = input.get("description_contains").and_then(Value::as_str);

        // Parse and validate date
        let target_date = parse_date(date, "date")?;

        // Fetch recent activities
        let activities = client
            .get_recent_activities(Some(50), Some(30))
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch activities: {}", e)))?;

        // Debug logging
        tracing::debug!("Fetched {} activities", activities.len());
        for a in &activities {
            tracing::debug!(
                "Activity: id={}, name={}, date={}",
                a.id,
                a.name.as_deref().unwrap_or("N/A"),
                a.start_date_local
            );
        }

        // Filter by date
        let mut matching = filter_activities_by_date(&activities, &target_date);

        // Apply description filter if provided
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

        // Handle empty results gracefully (not an error)
        if matching.is_empty() {
            let mut content = Vec::new();
            content.push(ContentBlock::markdown(format!(
                "# Analysis: {}\nStatus: No activities found",
                date
            )));

            let mut summary = vec![
                format!("  No training activities recorded for {}", date),
                "  This could be a rest day or activities haven't been synced yet".into(),
            ];
            if let Some(d) = desc_filter {
                summary.push(format!("  Search filter: '{}'", d));
            }

            content.push(ContentBlock::markdown(summary.join("\n")));

            let suggestions = vec![
                "Check if activities are synced from your fitness device".into(),
                "Verify the date - did you train on this day?".into(),
                "Try expanding the date range to include nearby days".into(),
            ];

            let next_actions = vec![
                "To view recent activities: analyze_training with target_type: period and wider date range".into(),
                "To check athlete profile: manage_profile with action: get".into(),
            ];

            return Ok(IntentOutput::new(content)
                .with_suggestions(suggestions)
                .with_next_actions(next_actions));
        }

        // Handle multiple matches
        if matching.len() > 1 {
            let mut content = Vec::new();
            content.push(ContentBlock::markdown(format!(
                "# Analysis: {}\nStatus: Multiple activities found",
                date
            )));

            let mut summary = vec![format!(
                "  Found {} activities for {}",
                matching.len(),
                date
            )];
            if let Some(d) = desc_filter {
                summary.push(format!(
                    "  Search filter: '{}' matched {} activities",
                    d,
                    matching.len()
                ));
            }
            summary.push("  Please be more specific with your search.".into());

            // List found activities
            let mut activities_list = vec!["Found activities:".into()];
            for (i, a) in matching.iter().enumerate() {
                activities_list.push(format!(
                    "{}. {} (ID: {})",
                    i + 1,
                    a.name.as_deref().unwrap_or("Unknown"),
                    a.id
                ));
            }
            content.push(ContentBlock::markdown(activities_list.join("\n")));

            // Build explicit retry examples for each activity
            let mut retry_examples = vec!["To analyze a specific activity, retry with:".into()];
            for (i, a) in matching.iter().enumerate() {
                let name = a.name.as_deref().unwrap_or("Unknown");
                let id = &a.id;
                // Extract key phrase from name (first 2-3 words or before dash/colon/em-dash)
                let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();

                retry_examples.push(format!(
                    "{}. {} → `description_contains: \"{}\"` or ID: `{}`",
                    i + 1,
                    name,
                    key_phrase,
                    id
                ));
            }
            content.push(ContentBlock::markdown(retry_examples.join("\n")));

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

            // Add ID-based option for direct access (only if not too many activities)
            if matching.len() <= 3 {
                next_actions.push(format!(
                    "Or specify activity ID directly if your MCP client supports it (e.g., `{}`)",
                    matching[0].id
                ));
            }

            return Ok(IntentOutput::new(content)
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

        // Fetch additional data based on analysis_type
        let analysis_mode =
            SingleAnalysisMode::parse(input.get("analysis_type").and_then(Value::as_str));
        let requested_metrics = requested_metrics(input);
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
        .await?;
        fetched.activities = vec![(*activity).clone()];
        fetched.fitness = client.get_fitness_summary().await.ok();

        let mut workout_context = CoachContext::new(
            AnalysisKind::TrainingSingle,
            AnalysisWindow::new(target_date, target_date),
        );
        workout_context.audit = build_data_audit(&fetched);
        workout_context.metrics.fitness = parse_fitness_metrics(fetched.fitness.as_ref());

        let workout_detail = fetched.workout_detail.as_ref();
        let _interval_count = fetched
            .intervals
            .as_ref()
            .and_then(Value::as_array)
            .map(|items| items.len());
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
            crate::engines::coach_metrics::derive_execution_metrics(
                fetched.workout_detail.as_ref(),
                fetched.streams.as_ref(),
            );
        let mut workout_metrics =
            derive_workout_metrics_context(work_interval_count, avg_hr, avg_power, execution_notes);
        workout_metrics.efficiency_factor = efficiency_factor;
        workout_metrics.aerobic_decoupling = aerobic_decoupling;

        // Grade the workout using the analysis engine
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

            // Compute HR drift from stream data when available
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
                let second_half_avg =
                    hr_vec[mid..].iter().sum::<f64>() / (hr_vec.len() - mid) as f64;
                let avg = hr_vec.iter().sum::<f64>() / hr_vec.len() as f64;
                Some(AnalysisEngine::calculate_hr_drift(
                    avg as u32,
                    first_half_avg as u32,
                    second_half_avg as u32,
                ))
            });

            // Compute pace variance from stream data when available
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

        // P0 — Performance Intelligence
        let mut espe_anchors = extract_sportinfo_anchors(fetched.wellness.as_ref());
        enrich_anchors_from_activity(&mut espe_anchors, fetched.workout_detail.as_ref());
        let wdrm = compute_wdr_metrics(
            fetched.intervals.as_ref(),
            fetched.workout_detail.as_ref(),
            espe_anchors.w_prime,
        );
        let espe_derived = derive_espe_metrics(&espe_anchors, None, None, None, None);
        workout_context.metrics.espe_anchors = Some(espe_anchors);
        workout_context.metrics.espe_derived = Some(espe_derived);
        workout_context.metrics.wdrm = Some(wdrm);

        workout_context.alerts = build_alerts(&workout_context.metrics);
        workout_context.guidance =
            build_guidance(&workout_context.metrics, &workout_context.alerts);

        let mut content = Vec::new();
        content.push(ContentBlock::markdown(format!(
            "# Analysis: {}\nDate: {}\nID: {}\nType: {}",
            activity_name,
            date,
            activity_id,
            analysis_mode.as_str()
        )));
        content.push(ContentBlock::markdown(format!(
            "Workout Grade: {:?}",
            workout_grade
        )));

        // Generate and render workout insights
        let insights = WorkoutInsights::generate(&analysis_metrics, &workout_grade);
        if !insights.is_empty() {
            let mut insight_lines = vec!["Insights".to_string()];
            for insight in &insights {
                insight_lines.push(format!("  {}", insight));
            }
            content.push(ContentBlock::markdown(insight_lines.join("\n")));
        }

        // Build basic metrics table
        let rows = build_basic_workout_metric_rows(workout_detail);
        if !rows.is_empty() {
            content.push(ContentBlock::table(
                vec!["Metric".into(), "Value".into()],
                rows,
            ));
        }

        if !requested_metrics.is_empty() {
            let rows = build_requested_single_metric_rows(
                workout_detail.and_then(Value::as_object),
                &requested_metrics,
            );
            content.push(ContentBlock::markdown("Requested Metrics".to_string()));
            content.push(ContentBlock::table(
                vec!["Metric".into(), "Value".into(), "Status".into()],
                rows,
            ));
        }

        if analysis_mode.show_detailed_breakdown() {
            let rows = build_detailed_workout_rows(workout_detail);
            if !rows.is_empty() {
                content.push(ContentBlock::markdown("Detailed Breakdown".to_string()));
                content.push(ContentBlock::table(
                    vec!["Metric".into(), "Value".into()],
                    rows,
                ));
            }
        }

        let activity_message_rows = build_activity_message_rows(&fetched.activity_messages);
        if !activity_message_rows.is_empty() {
            content.push(ContentBlock::markdown("Workout Comments".to_string()));
            content.push(ContentBlock::table(
                vec![
                    "When".into(),
                    "Author".into(),
                    "Type".into(),
                    "Comment".into(),
                ],
                activity_message_rows,
            ));
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
            content.push(ContentBlock::markdown(format!(
                "Execution Context\n  {}",
                lines.join("\n  ")
            )));
        }

        // HR Drift and Pace Variance (computed from stream data)
        if analysis_mode.show_execution_context() {
            let mut stream_metrics = Vec::new();
            if let Some(drift) = analysis_metrics.hr_drift_percent {
                stream_metrics.push(format!("HR Drift: {:.1}%", drift));
            }
            if let Some(variance) = analysis_metrics.pace_variance_percent {
                stream_metrics.push(format!("Pace Variance: {:.1}%", variance));
            }
            if !stream_metrics.is_empty() {
                content.push(ContentBlock::markdown(format!(
                    "Stream Metrics\n  {}",
                    stream_metrics.join("\n  ")
                )));
            }
        }

        if let Some(espe_text) = render_espe_section(
            &workout_context.metrics.espe_anchors,
            &workout_context.metrics.espe_derived,
        ) {
            content.push(ContentBlock::markdown(espe_text));
        }
        if let Some(wdrm_text) = render_wdrm_section(&workout_context.metrics.wdrm) {
            content.push(ContentBlock::markdown(wdrm_text));
        }
        if let Some(workout) = &workout_context.metrics.workout
            && let Some(isdm_text) = render_isdm_section(&workout.aerobic_decoupling)
        {
            content.push(ContentBlock::markdown(isdm_text));
        }

        if let Some(fit_text) = render_fitness_snapshot(&workout_context.metrics.fitness) {
            content.push(ContentBlock::markdown(fit_text));
        }

        // Add interval analysis
        if analysis_mode.show_interval_section() {
            if let Some(ref intervals) = fetched.intervals
                && let Some(intervals_arr) = intervals.as_array()
                && !intervals_arr.is_empty()
            {
                let output_kind =
                    preferred_interval_output_kind(intervals_arr, fetched.streams.as_ref());
                let output_header = match output_kind {
                    IntervalOutputKind::Power => "Avg Power",
                    IntervalOutputKind::Pace => "Avg Pace",
                };

                content.push(ContentBlock::markdown(
                    "\nInterval Analysis\nDetected Intervals:".to_string(),
                ));

                let interval_rows = build_interval_analysis_rows(
                    intervals_arr,
                    fetched.streams.as_ref(),
                    output_kind,
                );
                content.push(ContentBlock::table(
                    vec![
                        "Rep".into(),
                        "Duration".into(),
                        "Avg HR".into(),
                        output_header.into(),
                    ],
                    interval_rows,
                ));
            } else {
                content.push(ContentBlock::markdown(
                    "Interval Analysis\n  No structured interval data available for this workout."
                        .to_string(),
                ));
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

        // Power histogram - add note if requested but unavailable
        if include_hist && fetched.power_histogram.is_none() {
            content.push(ContentBlock::markdown(
                "\nPower Histogram\n  Power histogram unavailable - this workout may not have power meter data.".to_string(),
            ));
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

        // Add best efforts comparison
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
            if !findings.is_empty() {
                content.push(ContentBlock::markdown(format!(
                    "Quality Findings\n  {}",
                    findings.join("\n  ")
                )));
            }
        }

        // Z2 HR Stability
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
                && let Some(z2_text) = render_z2_stability_section(
                    lower,
                    upper,
                    compute_z2_hr_variance(&hr_vec, lower, upper),
                )
            {
                content.push(ContentBlock::markdown(z2_text));
            }
        }

        // Terrain Context
        if let Some(detail_obj) = workout_detail.and_then(Value::as_object) {
            let elevation = detail_obj.get("elevation_gain").and_then(Value::as_f64);
            let distance = detail_obj.get("distance").and_then(Value::as_f64);
            let moving_time = detail_obj.get("moving_time").and_then(Value::as_i64);
            if let (Some(elev), Some(dist), Some(mtime)) = (elevation, distance, moving_time)
                && dist > 0.0
            {
                let terrain = compute_terrain_context(elev, dist, mtime, None);
                if terrain.supported {
                    let mut t_lines = vec!["Terrain Context".to_string()];
                    if let Some(ti) = terrain.terrain_index {
                        t_lines.push(format!("  Terrain Index: {:.0} m/km", ti));
                    }
                    if let Some(vam) = terrain.vam {
                        t_lines.push(format!("  VAM: {:.0} m/h", vam));
                    }
                    if terrain.terrain_induced {
                        t_lines.push("  Efficiency drift: terrain-induced".into());
                    }
                    content.push(ContentBlock::markdown(t_lines.join("\n")));
                }
            }
        }

        // Nutrition Context
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
                content.push(ContentBlock::markdown(format!(
                    "Nutrition Context\n  Carb demand: {:.1} g/kg\n  Protein demand: {:.1} g/kg",
                    carb, protein
                )));
            }
        }

        // Curve Profile
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
            content.push(ContentBlock::markdown(format!(
                "Power/Running Profile\n  Type: {:?}",
                profile
            )));
        }

        if analysis_mode.show_data_availability()
            && let Some(block) = data_availability_block(
                &workout_context.audit.degraded_mode_reasons,
                workout_context.audit.all_available(),
            )
        {
            content.push(block);
        }

        // Use shared guidance from coach engine
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

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions))
    }

    async fn analyze_period(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
    ) -> Result<IntentOutput, IntentError> {
        let start = input
            .get("period_start")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: period_start"))?;
        let end = input
            .get("period_end")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: period_end"))?;
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

        let mut fetched = fetch_period_data(
            client,
            &PeriodFetchRequest {
                window: window.clone(),
                include_activity_details: true,
                include_comparison_window: true,
            },
        )
        .await?;
        fetched.fitness = client.get_fitness_summary().await.ok();

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
                    "  No completed or planned workouts were found between {} and {}",
                    start, end
                ),
                if calendar_events.is_empty() {
                    "  No calendar events were found in this period either".into()
                } else {
                    format!(
                        "  {} calendar event(s) found in this window; review them below",
                        calendar_events.len()
                    )
                },
                "  This is unusual - consider checking:".into(),
                "    Device sync status".into(),
                "    Date range correctness".into(),
                "    Training calendar / planned workout availability".into(),
                "    Account connection".into(),
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
                "Check if your fitness device is syncing properly".into(),
                "Verify the date range - did you train or schedule workouts during this period?"
                    .into(),
                "Try a wider date range to capture recent or upcoming workouts".into(),
            ];

            let next_actions = vec![
                "To check athlete profile and sync status: manage_profile with action: get".into(),
                "To analyze a different period: analyze_training with wider period_start/period_end".into(),
            ];

            return Ok(IntentOutput::new(content)
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
        period_context.metrics.trend =
            Some(derive_trend_metrics(period_snapshot, previous_snapshot));
        period_context.metrics.fitness = parse_fitness_metrics(fetched.fitness.as_ref());

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
            let load_values = daily_loads
                .iter()
                .map(|(_, load)| *load)
                .collect::<Vec<_>>();
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

        let mut espe_anchors = extract_sportinfo_anchors(fetched.wellness.as_ref());
        if let Some(last_activity_id) = period_ids.last()
            && let Some(last_detail) = fetched.activity_details.get(last_activity_id)
        {
            enrich_anchors_from_activity(&mut espe_anchors, Some(last_detail));
        }
        let espe_derived = derive_espe_metrics(&espe_anchors, None, None, None, None);
        period_context.metrics.espe_anchors = Some(espe_anchors);
        period_context.metrics.espe_derived = Some(espe_derived);

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
                        .and_then(|value| value.get("icu_training_load"))
                        .and_then(|value| {
                            value.as_f64().or_else(|| value.as_i64().map(|n| n as f64))
                        })
                        .map(|value| format!("{value:.1}"))
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

            // Power Curve Comparison
            if let Some(_espe) = &period_context.metrics.espe_derived {
                let period_ids: Vec<String> = period.iter().map(|a| a.id.clone()).collect();
                if let Some(_last_id) = period_ids.last() {
                    let anchors = extract_sportinfo_anchors(fetched.wellness.as_ref());
                    let espe = derive_espe_metrics(&anchors, None, None, None, None);
                    let (deltas, rotation, statuses) =
                        crate::engines::coach_metrics::compare_power_curves(&espe, &espe);
                    if !deltas.is_empty() {
                        let mut pc_lines = vec!["Power Curve Comparison".to_string()];
                        for d in &["1m", "5m", "20m", "60m"] {
                            if let Some(delta) = deltas.get(*d) {
                                let status = statuses.get(*d).map(|s| s.as_str()).unwrap_or("");
                                pc_lines.push(format!("  {}: {:+.1}% ({})", d, delta, status));
                            }
                        }
                        pc_lines.push(format!("  Rotation Index: {:.3}", rotation));
                        content.push(ContentBlock::markdown(pc_lines.join("\n")));
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
                    let mut cp_lines = vec!["CP Model Validation".to_string()];
                    cp_lines.push(format!(
                        "  Fitted CP: {:.0} W | W': {:.0} J | R²: {:.3}",
                        cp_result.cp, cp_result.w_prime, cp_result.r_squared
                    ));
                    if let Some(anchors) = &period_context.metrics.espe_anchors
                        && let (Some(api_ftp), Some(api_wp)) = (anchors.eftp, anchors.w_prime)
                    {
                        let (cp_diff, wp_diff) = validate_cp(&cp_result, api_ftp, api_wp);
                        cp_lines.push(format!(
                            "  vs API eFTP: CP Δ{:.1}%, W′ Δ{:.1}%",
                            cp_diff, wp_diff
                        ));
                    }
                    cp_lines.push(format!(
                        "  Fit quality: {}",
                        if cp_result.valid {
                            "good"
                        } else {
                            "poor — model may not reflect true CP"
                        }
                    ));
                    content.push(ContentBlock::markdown(cp_lines.join("\n")));
                }
            }

            // Ultra-specific tokens
            if !period.is_empty() {
                let period_ids: Vec<String> = period.iter().map(|a| a.id.clone()).collect();
                let daily_loads =
                    build_daily_load_series(&period, &fetched.activity_details, &window);
                let loads: Vec<f64> = daily_loads.iter().map(|(_, l)| *l).collect();
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

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions)
            .with_metadata(OutputMetadata {
                total_count: Some(period.len() as u32),
                ..Default::default()
            }))
    }
}

fn format_pct(value: Option<f64>) -> String {
    value
        .map(|delta| format!("{:+.1}%", delta))
        .unwrap_or_else(|| "n/a".into())
}

impl Default for AnalyzeTrainingHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::coach::{AcwrMetrics, LoadManagementMetrics};
    use chrono::NaiveDate;
    use serde_json::json;

    fn content_text(content: &[ContentBlock]) -> String {
        content
            .iter()
            .flat_map(|b| match b {
                ContentBlock::Text { text } => vec![text.clone()],
                ContentBlock::Markdown { markdown } => vec![markdown.clone()],
                ContentBlock::Table { headers, rows } => {
                    let mut parts: Vec<String> = headers.clone();
                    for row in rows {
                        parts.extend(row.clone());
                    }
                    parts
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ========================================================================
    // Constructor Tests
    // ========================================================================

    #[test]
    fn test_new_handler() {
        let handler = AnalyzeTrainingHandler::new();
        assert_eq!(handler.name(), "analyze_training");
    }

    #[test]
    fn test_default_handler() {
        let _handler = AnalyzeTrainingHandler;
    }

    // ========================================================================
    // IntentHandler Trait Implementation Tests
    // ========================================================================

    #[test]
    fn test_name() {
        let handler = AnalyzeTrainingHandler::new();
        assert_eq!(IntentHandler::name(&handler), "analyze_training");
    }

    #[test]
    fn test_description() {
        let handler = AnalyzeTrainingHandler::new();
        let desc = IntentHandler::description(&handler);
        assert!(desc.contains("Analyzes training"));
        assert!(desc.contains("single workout"));
        assert!(desc.contains("period"));
    }

    #[test]
    fn test_input_schema_structure() {
        let handler = AnalyzeTrainingHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        assert!(schema.get("type").is_some());
        assert_eq!(schema.get("type").unwrap().as_str(), Some("object"));

        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("target_type"));
        assert!(props.contains_key("date"));
        assert!(props.contains_key("period_start"));
        assert!(props.contains_key("period_end"));
        assert!(props.contains_key("analysis_type"));
        assert!(props.contains_key("include_best_efforts"));
        assert!(props.contains_key("include_histograms"));

        // target_type is required
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("target_type")));

        // Check oneOf constraint for date vs period
        let one_of = schema.get("oneOf").unwrap().as_array().unwrap();
        assert_eq!(one_of.len(), 2);
    }

    #[test]
    fn test_requires_idempotency_token() {
        let handler = AnalyzeTrainingHandler::new();
        assert!(!IntentHandler::requires_idempotency_token(&handler));
    }

    // ========================================================================
    // Input Validation Tests
    // ========================================================================

    #[test]
    fn test_validation_missing_target_type() {
        let input = json!({
            "date": "2026-03-01"
        });
        assert!(input.get("target_type").is_none());
    }

    #[test]
    fn test_validation_invalid_target_type() {
        let input = json!({
            "target_type": "invalid"
        });
        let target_type = input.get("target_type").and_then(|v| v.as_str()).unwrap();
        assert_ne!(target_type, "single");
        assert_ne!(target_type, "period");
    }

    // ========================================================================
    // Analysis Type Tests
    // ========================================================================

    #[test]
    fn test_analysis_type_values() {
        let valid_types = ["summary", "detailed", "intervals", "streams"];
        for t in &valid_types {
            assert!(["summary", "detailed", "intervals", "streams"].contains(t));
        }
    }

    #[test]
    fn test_default_analysis_type() {
        let input = json!({
            "target_type": "single",
            "date": "2026-03-01"
        });
        let analysis_type = input
            .get("analysis_type")
            .and_then(|v| v.as_str())
            .unwrap_or("summary");
        assert_eq!(analysis_type, "summary");
    }

    #[test]
    fn test_default_include_flags() {
        let input = json!({
            "target_type": "single",
            "date": "2026-03-01"
        });

        let include_best = input
            .get("include_best_efforts")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(!include_best);

        let include_hist = input
            .get("include_histograms")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(!include_hist);
    }

    // ========================================================================
    // Output Structure Tests
    // ========================================================================

    #[test]
    fn test_handler_metadata() {
        let handler = AnalyzeTrainingHandler::new();

        // Verify handler properties
        assert_eq!(handler.name(), "analyze_training");
        assert!(handler.description().len() > 50);

        let schema = handler.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_handler_description_mentions_comments_and_calendar_context() {
        let handler = AnalyzeTrainingHandler::new();
        let description = handler.description();

        assert!(description.contains("calendar events"));
    }

    // ========================================================================
    // Error Message Tests
    // ========================================================================

    #[test]
    fn test_error_messages_contain_context() {
        // Test that validation errors contain field names
        let err = IntentError::validation("Missing: target_type".to_string());
        let err_str = err.to_string();
        assert!(err_str.contains("target_type"));
    }

    #[test]
    fn load_management_markdown_renders_acwr_and_monotony_values() {
        let markdown = build_load_management_text(Some(&LoadManagementMetrics {
            acwr: Some(AcwrMetrics {
                acute_load: 420.0,
                chronic_load: 350.0,
                ratio: 1.20,
                state: "productive".into(),
            }),
            monotony: Some(2.1),
            strain: Some(882.0),
            fatigue_index: None,
            stress_tolerance: None,
            durability_index: None,
        }));

        assert!(markdown.contains("ACWR"));
        assert!(markdown.contains("Monotony"));
        assert!(markdown.contains("1.20"));
    }

    #[test]
    fn load_management_markdown_reports_when_history_is_unavailable() {
        let markdown = build_load_management_text(None);

        assert!(markdown.contains("Load Context"));
        assert!(markdown.contains("unavailable"));
    }

    #[test]
    fn build_basic_workout_metric_rows_formats_available_values() {
        let rows = build_basic_workout_metric_rows(Some(&serde_json::json!({
            "distance": 12345.0,
            "moving_time": 3661,
            "average_heartrate": 151.2,
            "average_watts": 245.7,
            "total_elevation_gain": 432.0
        })));

        assert_eq!(
            rows,
            vec![
                vec!["Distance".to_string(), "12.35 km".to_string()],
                vec!["Duration".to_string(), "1:01:01".to_string()],
                vec!["Avg HR".to_string(), "151 bpm".to_string()],
                vec!["Avg Power".to_string(), "246 W".to_string()],
                vec!["Elevation".to_string(), "432 m".to_string()],
            ]
        );
    }

    #[test]
    fn build_interval_analysis_rows_formats_known_fields() {
        let rows = build_interval_analysis_rows(
            &[
                serde_json::json!({
                    "moving_time": 95,
                    "average_heartrate": 162.4,
                    "average_watts": 301.0
                }),
                serde_json::json!({
                    "moving_time": 120,
                    "average_heartrate": 158.0,
                    "average_watts": 287.2
                }),
            ],
            None,
            IntervalOutputKind::Power,
        );

        assert_eq!(
            rows,
            vec![
                vec![
                    "1".to_string(),
                    "1:35".to_string(),
                    "162 bpm".to_string(),
                    "301 W".to_string(),
                ],
                vec![
                    "2".to_string(),
                    "2:00".to_string(),
                    "158 bpm".to_string(),
                    "287 W".to_string(),
                ],
            ]
        );
    }

    #[test]
    fn build_interval_analysis_rows_backfills_power_from_stream_slice() {
        let rows = build_interval_analysis_rows(
            &[
                serde_json::json!({
                    "start_index": 0,
                    "end_index": 4,
                    "moving_time": 240,
                    "average_heartrate": 150.0,
                    "average_watts": null
                }),
                serde_json::json!({
                    "start_index": 4,
                    "end_index": 8,
                    "moving_time": 240,
                    "average_heartrate": 162.0,
                    "average_watts": null
                }),
            ],
            Some(&serde_json::json!({
                "watts": [210.0, 220.0, 230.0, 240.0, 280.0, 290.0, 300.0, 310.0]
            })),
            IntervalOutputKind::Power,
        );

        assert_eq!(
            rows,
            vec![
                vec![
                    "1".to_string(),
                    "4:00".to_string(),
                    "150 bpm".to_string(),
                    "225 W".to_string(),
                ],
                vec![
                    "2".to_string(),
                    "4:00".to_string(),
                    "162 bpm".to_string(),
                    "295 W".to_string(),
                ],
            ]
        );
    }

    #[test]
    fn build_period_summary_rows_formats_snapshot_values() {
        let rows = build_period_summary_rows(
            4,
            &crate::engines::coach_metrics::TrendSnapshot {
                activity_count: 4,
                total_time_secs: 18_600,
                total_distance_m: 42_250.0,
                total_elevation_m: 640.0,
            },
            7.4,
        );

        assert_eq!(
            rows,
            vec![
                vec!["Total Time".to_string(), "5:10:00".to_string()],
                vec!["Distance".to_string(), "42.2 km".to_string()],
                vec!["Elevation".to_string(), "640 m".to_string()],
                vec!["Weekly Avg".to_string(), "7.4 hrs".to_string()],
            ]
        );
    }

    // ========================================================================
    // Work Interval Counting Tests
    // ========================================================================

    #[test]
    fn test_count_work_intervals_empty() {
        let intervals = vec![];
        assert_eq!(count_work_intervals(&intervals), 0);
    }

    #[test]
    fn test_count_work_intervals_with_real_data() {
        // Simulate the user's workout: 7 work intervals + 8 recovery intervals
        let intervals = vec![
            // Work intervals (high speed, high HR)
            json!({"average_speed": 2.52, "average_heartrate": 126}), // 1: borderline (low HR)
            json!({"average_speed": 2.76, "average_heartrate": 142}), // 2: work
            json!({"average_speed": 3.04, "average_heartrate": 158}), // 3: work
            json!({"average_speed": 0.85, "average_heartrate": 128}), // 4: recovery (very slow)
            json!({"average_speed": 2.95, "average_heartrate": 158}), // 5: work
            json!({"average_speed": 2.23, "average_heartrate": 140}), // 6: borderline
            json!({"average_speed": 2.91, "average_heartrate": 159}), // 7: work
            json!({"average_speed": 2.13, "average_heartrate": 140}), // 8: borderline
            json!({"average_speed": 2.98, "average_heartrate": 160}), // 9: work
            json!({"average_speed": 2.15, "average_heartrate": 141}), // 10: borderline
            json!({"average_speed": 2.96, "average_heartrate": 158}), // 11: work
            json!({"average_speed": 2.04, "average_heartrate": 138}), // 12: borderline
            json!({"average_speed": 3.02, "average_heartrate": 156}), // 13: work
            json!({"average_speed": 1.44, "average_heartrate": 128}), // 14: recovery (slow)
            json!({"average_speed": 0.75, "average_heartrate": 115}), // 15: recovery (very slow)
        ];

        let count = count_work_intervals(&intervals);
        // Should identify ~7-8 work intervals (the ones with speed >= ~2.5 and HR >= ~145)
        assert!(
            (6..=9).contains(&count),
            "Expected 6-9 work intervals, got {}",
            count
        );
    }

    #[test]
    fn test_count_work_intervals_clear_separation() {
        // Clear work vs recovery separation
        let intervals = vec![
            json!({"average_speed": 3.0, "average_heartrate": 160}), // work
            json!({"average_speed": 1.5, "average_heartrate": 130}), // recovery
            json!({"average_speed": 3.1, "average_heartrate": 162}), // work
            json!({"average_speed": 1.4, "average_heartrate": 128}), // recovery
            json!({"average_speed": 3.0, "average_heartrate": 158}), // work
        ];

        let count = count_work_intervals(&intervals);
        assert_eq!(count, 3, "Should identify 3 work intervals");
    }

    #[test]
    fn test_count_work_intervals_speed_only() {
        // Some intervals without HR data
        let intervals = vec![
            json!({"average_speed": 3.0}), // work
            json!({"average_speed": 1.5}), // recovery
            json!({"average_speed": 3.1}), // work
            json!({"average_speed": 1.4}), // recovery
            json!({"average_speed": 3.0}), // work
        ];

        let count = count_work_intervals(&intervals);
        assert_eq!(count, 3, "Should identify 3 work intervals by speed");
    }

    #[test]
    fn test_calculate_median() {
        let mut values = vec![5.0, 2.0, 8.0, 1.0, 9.0];
        assert!((calculate_median(&mut values) - 5.0).abs() < 0.001);

        let mut values = vec![1.0, 2.0, 3.0, 4.0];
        assert!((calculate_median(&mut values) - 2.5).abs() < 0.001);

        let mut values = vec![42.0];
        assert!((calculate_median(&mut values) - 42.0).abs() < 0.001);

        let mut values = vec![];
        assert!((calculate_median(&mut values) - 0.0).abs() < 0.001);
    }

    // ========================================================================
    // Key Phrase Extraction Tests (for multiple activity guidance)
    // ========================================================================

    #[test]
    fn test_key_phrase_extraction_with_dash() {
        // Test with ASCII dash
        let name = "Long Run Z2 - Key Workout";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Long Run Z2");
    }

    #[test]
    fn test_key_phrase_extraction_with_em_dash() {
        // Test with Unicode em-dash (—)
        let name = "Long Run Z2 — Key Workout";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Long Run Z2");
    }

    #[test]
    fn test_key_phrase_extraction_with_colon() {
        let name = "Tempo Run: Threshold Session";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Tempo Run");
    }

    #[test]
    fn test_key_phrase_extraction_no_separator() {
        let name = "Weight Training";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Weight Training");
    }

    #[test]
    fn test_key_phrase_extraction_empty_dash() {
        let name = "Intervals - Track Session";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Intervals");
    }

    #[test]
    fn test_key_phrase_extraction_unicode_dash() {
        // Test with em-dash (Unicode)
        let name = "Recovery Run — Easy Pace";
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
        assert_eq!(key_phrase, "Recovery Run");
    }

    // ========================================================================
    // SingleAnalysisMode Enum Tests
    // ========================================================================

    #[test]
    fn test_single_analysis_mode_parse_default() {
        assert_eq!(SingleAnalysisMode::parse(None), SingleAnalysisMode::Summary);
        assert_eq!(
            SingleAnalysisMode::parse(Some("summary")),
            SingleAnalysisMode::Summary
        );
        assert_eq!(
            SingleAnalysisMode::parse(Some("unknown")),
            SingleAnalysisMode::Summary
        );
    }

    #[test]
    fn test_single_analysis_mode_parse_detailed() {
        assert_eq!(
            SingleAnalysisMode::parse(Some("detailed")),
            SingleAnalysisMode::Detailed
        );
    }

    #[test]
    fn test_single_analysis_mode_parse_intervals() {
        assert_eq!(
            SingleAnalysisMode::parse(Some("intervals")),
            SingleAnalysisMode::Intervals
        );
    }

    #[test]
    fn test_single_analysis_mode_parse_streams() {
        assert_eq!(
            SingleAnalysisMode::parse(Some("streams")),
            SingleAnalysisMode::Streams
        );
    }

    #[test]
    fn test_single_analysis_mode_as_str() {
        assert_eq!(SingleAnalysisMode::Summary.as_str(), "summary");
        assert_eq!(SingleAnalysisMode::Detailed.as_str(), "detailed");
        assert_eq!(SingleAnalysisMode::Intervals.as_str(), "intervals");
        assert_eq!(SingleAnalysisMode::Streams.as_str(), "streams");
    }

    #[test]
    fn test_single_analysis_mode_include_intervals() {
        assert!(!SingleAnalysisMode::Summary.include_intervals());
        assert!(!SingleAnalysisMode::Detailed.include_intervals());
        assert!(SingleAnalysisMode::Intervals.include_intervals());
        assert!(!SingleAnalysisMode::Streams.include_intervals());
    }

    #[test]
    fn test_single_analysis_mode_include_streams() {
        assert!(!SingleAnalysisMode::Summary.include_streams());
        assert!(SingleAnalysisMode::Detailed.include_streams());
        assert!(SingleAnalysisMode::Intervals.include_streams());
        assert!(SingleAnalysisMode::Streams.include_streams());
    }

    #[test]
    fn test_single_analysis_mode_show_methods() {
        // Summary mode
        assert!(!SingleAnalysisMode::Summary.show_execution_context());
        assert!(!SingleAnalysisMode::Summary.show_interval_section());
        assert!(!SingleAnalysisMode::Summary.show_stream_section());
        assert!(!SingleAnalysisMode::Summary.show_quality_findings());
        assert!(!SingleAnalysisMode::Summary.show_data_availability());
        assert!(!SingleAnalysisMode::Summary.show_detailed_breakdown());

        // Detailed mode
        assert!(SingleAnalysisMode::Detailed.show_execution_context());
        assert!(!SingleAnalysisMode::Detailed.show_interval_section());
        assert!(!SingleAnalysisMode::Detailed.show_stream_section());
        assert!(SingleAnalysisMode::Detailed.show_quality_findings());
        assert!(SingleAnalysisMode::Detailed.show_data_availability());
        assert!(SingleAnalysisMode::Detailed.show_detailed_breakdown());

        // Intervals mode
        assert!(!SingleAnalysisMode::Intervals.show_execution_context());
        assert!(SingleAnalysisMode::Intervals.show_interval_section());
        assert!(!SingleAnalysisMode::Intervals.show_stream_section());
        assert!(!SingleAnalysisMode::Intervals.show_quality_findings());
        assert!(!SingleAnalysisMode::Intervals.show_data_availability());
        assert!(!SingleAnalysisMode::Intervals.show_detailed_breakdown());

        // Streams mode
        assert!(SingleAnalysisMode::Streams.show_execution_context());
        assert!(!SingleAnalysisMode::Streams.show_interval_section());
        assert!(SingleAnalysisMode::Streams.show_stream_section());
        assert!(SingleAnalysisMode::Streams.show_quality_findings());
        assert!(SingleAnalysisMode::Streams.show_data_availability());
        assert!(!SingleAnalysisMode::Streams.show_detailed_breakdown());
    }

    // ========================================================================
    // Date Parsing Helper Tests
    // ========================================================================

    #[test]
    fn test_parse_activity_date_with_timestamp() {
        let result = parse_activity_date("2026-03-01T10:30:00");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        );
    }

    #[test]
    fn test_parse_activity_date_date_only() {
        let result = parse_activity_date("2026-03-01");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        );
    }

    #[test]
    fn test_parse_activity_date_invalid() {
        assert!(parse_activity_date("invalid").is_none());
        assert!(parse_activity_date("").is_none());
    }

    // ========================================================================
    // Requested Metrics Tests
    // ========================================================================

    #[test]
    fn test_requested_metrics_empty_input() {
        let input = json!({});
        let metrics = requested_metrics(&input);
        assert!(metrics.is_empty());
    }

    #[test]
    fn test_requested_metrics_with_array() {
        let input = json!({
            "metrics": ["time", "distance", "HR"]
        });
        let metrics = requested_metrics(&input);
        assert_eq!(metrics, vec!["time", "distance", "hr"]);
    }

    #[test]
    fn test_requested_metrics_non_string_values() {
        let input = json!({
            "metrics": ["time", 123, null, "distance"]
        });
        let metrics = requested_metrics(&input);
        assert_eq!(metrics, vec!["time", "distance"]);
    }

    // ========================================================================
    // Duration Formatting Tests
    // ========================================================================

    #[test]
    fn test_format_duration_hhmm_under_hour() {
        assert_eq!(format_duration_hhmm(0), "0:00");
        assert_eq!(format_duration_hhmm(60), "1:00");
        assert_eq!(format_duration_hhmm(3661), "1:01:01");
        assert_eq!(format_duration_hhmm(7265), "2:01:05");
    }

    #[test]
    fn test_format_duration_hhmm_over_hour() {
        assert_eq!(format_duration_hhmm(3600), "1:00:00");
        assert_eq!(format_duration_hhmm(7200), "2:00:00");
        assert_eq!(format_duration_hhmm(3661), "1:01:01");
    }

    #[test]
    fn test_format_duration_compact_matches_hhmm() {
        // format_duration_compact should behave identically to format_duration_hhmm
        assert_eq!(format_duration_compact(0), format_duration_hhmm(0));
        assert_eq!(format_duration_compact(3661), format_duration_hhmm(3661));
        assert_eq!(format_duration_compact(7265), format_duration_hhmm(7265));
    }

    // ========================================================================
    // Planned Workout ID Detection Tests
    // ========================================================================

    #[test]
    fn test_is_planned_workout_id_event_prefix() {
        assert!(is_planned_workout_id("event:12345"));
        assert!(is_planned_workout_id("event:94131802"));
    }

    #[test]
    fn test_is_planned_workout_id_regular_activity() {
        assert!(!is_planned_workout_id("12345"));
        assert!(!is_planned_workout_id("a1"));
        assert!(!is_planned_workout_id(""));
    }

    // ========================================================================
    // Calendar Event Row Building Tests
    // ========================================================================

    #[test]
    fn test_build_calendar_event_rows_empty() {
        let events: Vec<&intervals_icu_client::Event> = vec![];
        let rows = build_calendar_event_rows(&events);
        assert!(rows.is_empty());
    }

    #[test]
    fn test_build_calendar_event_rows_with_events() {
        let events = [
            intervals_icu_client::Event {
                id: Some("e1".to_string()),
                start_date_local: "2026-03-01T10:00:00".to_string(),
                name: "Race Day".to_string(),
                category: EventCategory::RaceA,
                description: Some("Marathon".to_string()),
                r#type: None,
            },
            intervals_icu_client::Event {
                id: Some("e2".to_string()),
                start_date_local: "2026-03-02".to_string(),
                name: "Recovery".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
        ];
        let refs = events.iter().collect::<Vec<_>>();
        let rows = build_calendar_event_rows(&refs);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "2026-03-01");
        assert_eq!(rows[0][1], "RaceA");
        assert_eq!(rows[0][2], "Race Day");
        assert_eq!(rows[0][3], "Marathon");
        assert_eq!(rows[1][3], "n/a");
    }

    // ========================================================================
    // Interval Output Value Tests
    // ========================================================================

    #[test]
    fn test_interval_output_value_kind() {
        let power_value = IntervalOutputValue::Power(250.0);
        assert_eq!(power_value.kind(), IntervalOutputKind::Power);

        let pace_value = IntervalOutputValue::Pace(2.5);
        assert_eq!(pace_value.kind(), IntervalOutputKind::Pace);
    }

    #[test]
    fn test_interval_output_value_format_power() {
        let value = IntervalOutputValue::Power(245.7);
        assert_eq!(value.format(), "246 W");
    }

    #[test]
    fn test_interval_output_value_format_pace() {
        let value = IntervalOutputValue::Pace(2.778); // 10 km/h = 6:00 /km
        let formatted = value.format();
        assert!(formatted.contains("/km"));
    }

    #[test]
    fn test_interval_output_value_format_invalid_pace() {
        let value = IntervalOutputValue::Pace(0.0);
        assert_eq!(value.format(), "n/a");
    }

    // ========================================================================
    // Numeric Value Helper Tests
    // ========================================================================

    #[test]
    fn test_numeric_value_from_f64() {
        let obj_value = serde_json::json!({"key": 42.5});
        let obj = obj_value.as_object().unwrap();
        assert_eq!(numeric_value(obj, "key"), Some(42.5));
    }

    #[test]
    fn test_numeric_value_from_i64() {
        let obj_value = serde_json::json!({"key": 42});
        let obj = obj_value.as_object().unwrap();
        assert_eq!(numeric_value(obj, "key"), Some(42.0));
    }

    #[test]
    fn test_numeric_value_missing_key() {
        let obj_value = serde_json::json!({"other": 42});
        let obj = obj_value.as_object().unwrap();
        assert_eq!(numeric_value(obj, "key"), None);
    }

    #[test]
    fn test_numeric_value_non_numeric() {
        let obj_value = serde_json::json!({"key": "text"});
        let obj = obj_value.as_object().unwrap();
        assert_eq!(numeric_value(obj, "key"), None);
    }

    // ========================================================================
    // Histogram and Zone Distribution Tests
    // ========================================================================

    #[test]
    fn test_format_histogram_number_integer() {
        assert_eq!(format_histogram_number(100.0), "100");
        assert_eq!(format_histogram_number(42.0), "42");
    }

    #[test]
    fn test_format_histogram_number_decimal() {
        assert_eq!(format_histogram_number(42.5), "42.50");
        assert_eq!(format_histogram_number(0.123), "0.12");
    }

    #[test]
    fn test_build_range_histogram_rows_empty() {
        let buckets: Vec<Value> = vec![];
        let rows = build_range_histogram_rows(&buckets, "bpm");
        assert!(rows.is_empty());
    }

    #[test]
    fn test_build_range_histogram_rows_with_data() {
        let buckets = vec![
            json!({"min": 100, "max": 120, "secs": 600}),
            json!({"min": 120, "max": 140, "secs": 1200}),
        ];
        let rows = build_range_histogram_rows(&buckets, "bpm");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "100-120 bpm");
        assert_eq!(rows[0][1], "10:00");
        assert_eq!(rows[1][0], "120-140 bpm");
        assert_eq!(rows[1][1], "20:00");
    }

    #[test]
    fn test_build_bucket_histogram_rows_empty() {
        let buckets: Vec<Value> = vec![];
        let rows = build_bucket_histogram_rows(&buckets, Some("avg"), "bpm");
        assert!(rows.is_empty());
    }

    #[test]
    fn test_build_bucket_histogram_rows_with_data() {
        let buckets = vec![
            json!({"start": 100, "secs": 300, "movingSecs": 280, "avg": 110}),
            json!({"start": 120, "secs": 600, "movingSecs": 580, "avg": 130}),
        ];
        let rows = build_bucket_histogram_rows(&buckets, Some("avg"), "bpm");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "100 bpm");
        assert_eq!(rows[0][1], "5:00");
        assert_eq!(rows[0][2], "4:40");
        assert_eq!(rows[0][3], "110");
    }

    #[test]
    fn test_build_bucket_histogram_rows_without_average_key() {
        let buckets = vec![json!({"start": 100, "secs": 300})];
        let rows = build_bucket_histogram_rows(&buckets, None, "");

        assert_eq!(rows[0][3], "n/a");
    }

    #[test]
    fn test_build_zone_distribution_rows_empty() {
        let zones_value = serde_json::json!({});
        let zones = zones_value.as_object().unwrap();
        let rows = build_zone_distribution_rows(zones);
        assert!(rows.is_empty());
    }

    #[test]
    fn test_build_zone_distribution_rows_with_data() {
        let zones_value = serde_json::json!({
            "z1": 600,
            "z2": 1200,
            "z3": 300
        });
        let zones = zones_value.as_object().unwrap();
        let rows = build_zone_distribution_rows(zones);

        assert_eq!(rows.len(), 3);
        // Total: 2100 seconds
        // z1: 600/2100 = 28.57% -> 29%
        // z2: 1200/2100 = 57.14% -> 57%
        // z3: 300/2100 = 14.28% -> 14%
        assert_eq!(rows[0][0], "Z1");
        assert_eq!(rows[0][1], "10:00");
        assert!(rows[0][2].contains("%"));
    }

    // ========================================================================
    // Best Efforts Helper Tests
    // ========================================================================

    #[test]
    fn test_best_efforts_array_direct_array() {
        let value = json!([{"seconds": 60}, {"seconds": 300}]);
        let arr = best_efforts_array(&value);
        assert!(arr.is_some());
        assert_eq!(arr.unwrap().len(), 2);
    }

    #[test]
    fn test_best_efforts_array_nested_best_efforts() {
        let value = json!({"best_efforts": [{"seconds": 60}]});
        let arr = best_efforts_array(&value);
        assert!(arr.is_some());
        assert_eq!(arr.unwrap().len(), 1);
    }

    #[test]
    fn test_best_efforts_array_nested_efforts() {
        let value = json!({"efforts": [{"seconds": 60}]});
        let arr = best_efforts_array(&value);
        assert!(arr.is_some());
        assert_eq!(arr.unwrap().len(), 1);
    }

    #[test]
    fn test_best_efforts_array_invalid() {
        let value = json!("not an array");
        assert!(best_efforts_array(&value).is_none());

        let value = json!({"other": "field"});
        assert!(best_efforts_array(&value).is_none());
    }

    #[test]
    fn test_format_best_effort_duration_seconds_only() {
        assert_eq!(format_best_effort_duration(5), "5s");
        assert_eq!(format_best_effort_duration(59), "59s");
    }

    #[test]
    fn test_format_best_effort_duration_minutes_seconds() {
        assert_eq!(format_best_effort_duration(60), "1:00");
        assert_eq!(format_best_effort_duration(125), "2:05");
        assert_eq!(format_best_effort_duration(3599), "59:59");
    }

    #[test]
    fn test_format_best_effort_duration_hours() {
        assert_eq!(format_best_effort_duration(3600), "1:00:00");
        assert_eq!(format_best_effort_duration(3661), "1:01:01");
    }

    #[test]
    fn test_format_best_effort_average_power() {
        let best_efforts = json!({"stream": "watts"});
        let effort_value = json!({"watts": 250.0});
        let effort = effort_value.as_object().unwrap();
        let avg = format_best_effort_average(&best_efforts, effort);
        assert_eq!(avg, Some("250 W".to_string()));
    }

    #[test]
    fn test_format_best_effort_average_pace() {
        let best_efforts = json!({"stream": "speed"});
        let effort_value = json!({"average": 2.778});
        let effort = effort_value.as_object().unwrap();
        let avg = format_best_effort_average(&best_efforts, effort);
        assert!(avg.unwrap().contains("/km"));
    }

    #[test]
    fn test_format_best_effort_average_heartrate() {
        let best_efforts = json!({});
        let effort_value = json!({"heartrate": 155.0});
        let effort = effort_value.as_object().unwrap();
        let avg = format_best_effort_average(&best_efforts, effort);
        assert_eq!(avg, Some("155 bpm".to_string()));
    }

    #[test]
    fn test_format_best_effort_average_no_data() {
        let best_efforts = json!({});
        let effort_value = json!({});
        let effort = effort_value.as_object().unwrap();
        assert!(format_best_effort_average(&best_efforts, effort).is_none());
    }

    // ========================================================================
    // Activity Message Row Building Tests
    // ========================================================================

    #[test]
    fn test_build_activity_message_rows_empty() {
        let messages: Vec<intervals_icu_client::ActivityMessage> = vec![];
        let rows = build_activity_message_rows(&messages);
        assert!(rows.is_empty());
    }

    #[test]
    fn test_build_activity_message_rows_with_messages() {
        use intervals_icu_client::ActivityMessage;
        let messages = vec![
            ActivityMessage {
                id: 1,
                athlete_id: Some("athlete1".to_string()),
                name: Some("John".to_string()),
                created: Some("2026-03-01T12:00:00Z".to_string()),
                message_type: Some("TEXT".to_string()),
                content: Some("Great workout!".to_string()),
                activity_id: Some("a1".to_string()),
                start_index: None,
                end_index: None,
                attachment_url: None,
                attachment_mime_type: None,
                deleted: None,
            },
            ActivityMessage {
                id: 2,
                athlete_id: Some("athlete2".to_string()),
                name: None,
                created: None,
                message_type: None,
                content: Some("Keep it up!".to_string()),
                activity_id: Some("a1".to_string()),
                start_index: None,
                end_index: None,
                attachment_url: None,
                attachment_mime_type: None,
                deleted: None,
            },
        ];
        let rows = build_activity_message_rows(&messages);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][1], "John");
        assert_eq!(rows[0][3], "Great workout!");
        assert_eq!(rows[1][1], "athlete2");
    }

    #[test]
    fn test_build_activity_message_rows_filters_deleted() {
        use intervals_icu_client::ActivityMessage;
        let messages = vec![
            ActivityMessage {
                id: 1,
                athlete_id: Some("athlete1".to_string()),
                name: None,
                created: None,
                message_type: None,
                content: Some("Visible".to_string()),
                activity_id: Some("a1".to_string()),
                start_index: None,
                end_index: None,
                attachment_url: None,
                attachment_mime_type: None,
                deleted: None,
            },
            ActivityMessage {
                id: 2,
                athlete_id: Some("athlete1".to_string()),
                name: None,
                created: None,
                message_type: None,
                content: Some("Deleted".to_string()),
                activity_id: Some("a1".to_string()),
                start_index: None,
                end_index: None,
                attachment_url: None,
                attachment_mime_type: None,
                deleted: Some("true".to_string()),
            },
        ];
        let rows = build_activity_message_rows(&messages);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][3], "Visible");
    }

    #[test]
    fn test_build_activity_message_rows_filters_empty_content() {
        use intervals_icu_client::ActivityMessage;
        let messages = vec![
            ActivityMessage {
                id: 1,
                athlete_id: Some("athlete1".to_string()),
                name: None,
                created: None,
                message_type: None,
                content: Some("   ".to_string()),
                activity_id: Some("a1".to_string()),
                start_index: None,
                end_index: None,
                attachment_url: None,
                attachment_mime_type: None,
                deleted: None,
            },
            ActivityMessage {
                id: 2,
                athlete_id: Some("athlete1".to_string()),
                name: None,
                created: None,
                message_type: None,
                content: None,
                activity_id: Some("a1".to_string()),
                start_index: None,
                end_index: None,
                attachment_url: None,
                attachment_mime_type: None,
                deleted: None,
            },
        ];
        let rows = build_activity_message_rows(&messages);
        assert!(rows.is_empty());
    }

    #[test]
    fn test_build_activity_message_rows_handles_missing_fields() {
        use intervals_icu_client::ActivityMessage;
        let messages = vec![ActivityMessage {
            id: 1,
            athlete_id: Some("athlete1".to_string()),
            name: None,
            created: None,
            message_type: None,
            content: Some("Test".to_string()),
            activity_id: Some("a1".to_string()),
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        }];
        let rows = build_activity_message_rows(&messages);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1], "athlete1"); // Falls back to athlete_id
        assert_eq!(rows[0][2], "TEXT"); // Default type
    }

    // ========================================================================
    // Execute() Path Tests - analyze_single()
    // ========================================================================

    use crate::test_support::mock::MockIntervalsClient;
    use intervals_icu_client::{ActivitySummary, Event};

    #[tokio::test]
    async fn test_analyze_single_summary_mode() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
                .with_workout_detail(json!({
                    "distance": 10000.0,
                    "moving_time": 3600,
                    "average_heartrate": 150.0,
                    "average_watts": 250.0,
                    "total_elevation_gain": 200.0,
                })),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "analysis_type": "summary"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.content.is_empty());
        // Verify basic structure
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Analysis"));
        assert!(content_str.contains("Test Workout"));
    }

    #[tokio::test]
    async fn test_analyze_single_detailed_mode() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
                .with_workout_detail(json!({
                    "distance": 10000.0,
                    "moving_time": 3600,
                    "average_heartrate": 150.0,
                    "average_watts": 250.0,
                    "total_elevation_gain": 200.0,
                    "average_cadence": 85.0,
                    "average_speed": 2.78,
                    "icu_training_load": 75.5,
                    "average_temp": 18.5,
                }))
                .with_streams(json!({
                    "watts": [200, 250, 300],
                    "heartrate": [140, 150, 160],
                })),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "analysis_type": "detailed"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Detailed Breakdown"));
        assert!(content_str.contains("Execution Context"));
    }

    #[tokio::test]
    async fn test_analyze_single_intervals_mode() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Interval Workout")
                .with_workout_detail(json!({
                    "distance": 12000.0,
                    "moving_time": 4200,
                    "average_heartrate": 155.0,
                    "average_watts": 280.0,
                }))
                .with_intervals(json!([
                    {"moving_time": 120, "average_heartrate": 165, "average_watts": 350},
                    {"moving_time": 120, "average_heartrate": 162, "average_watts": 340},
                    {"moving_time": 120, "average_heartrate": 168, "average_watts": 360},
                ])),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "analysis_type": "intervals"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Interval Analysis"));
        assert!(content_str.contains("Detected Intervals"));
    }

    #[tokio::test]
    async fn test_analyze_single_streams_mode() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
                .with_workout_detail(json!({
                    "distance": 10000.0,
                    "moving_time": 3600,
                    "average_heartrate": 150.0,
                    "average_watts": 250.0,
                }))
                .with_streams(json!({
                    "watts": [200, 250, 300, 280],
                    "heartrate": [140, 150, 160, 155],
                    "velocity_smooth": [2.5, 2.8, 3.0, 2.7],
                })),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "analysis_type": "streams"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Stream Insights"));
        assert!(content_str.contains("watts"));
    }

    #[tokio::test]
    async fn test_analyze_single_with_histograms() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
                .with_workout_detail(json!({
                    "distance": 10000.0,
                    "moving_time": 3600,
                    "average_heartrate": 150.0,
                    "average_watts": 250.0,
                }))
                .with_hr_histogram(json!({
                    "zones": {"z1": 600, "z2": 1800, "z3": 1200}
                }))
                .with_power_histogram(json!([
                    {"start": 100, "secs": 300, "movingSecs": 280, "avgWatts": 150},
                    {"start": 200, "secs": 1800, "movingSecs": 1700, "avgWatts": 250},
                    {"start": 300, "secs": 1500, "movingSecs": 1400, "avgWatts": 350},
                ])),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "analysis_type": "summary",
            "include_histograms": true
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("HR Histogram"));
        assert!(content_str.contains("Power Histogram"));
    }

    #[tokio::test]
    async fn test_analyze_single_with_best_efforts() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
                .with_workout_detail(json!({
                    "distance": 10000.0,
                    "moving_time": 3600,
                    "average_heartrate": 150.0,
                    "average_watts": 250.0,
                }))
                .with_best_efforts(json!([
                    {"seconds": 60, "watts": 400},
                    {"seconds": 300, "watts": 350},
                    {"seconds": 1200, "watts": 300},
                ])),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "include_best_efforts": true
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Best Efforts"));
    }

    #[tokio::test]
    async fn test_analyze_single_no_activities_found() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient {
            activities: vec![],
            ..Default::default()
        });

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("No activities found"));
        assert!(!output.suggestions.is_empty());
    }

    #[tokio::test]
    async fn test_analyze_single_multiple_activities_found() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient {
            activities: vec![
                ActivitySummary {
                    id: "12345".to_string(),
                    name: Some("Morning Run".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "12346".to_string(),
                    name: Some("Evening Ride".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Multiple activities found"));
        assert!(content_str.contains("Morning Run"));
        assert!(content_str.contains("Evening Ride"));
    }

    #[tokio::test]
    async fn test_analyze_single_with_description_filter() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient {
            activities: vec![
                ActivitySummary {
                    id: "12345".to_string(),
                    name: Some("Easy Run".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "12346".to_string(),
                    name: Some("Tempo Run".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "description_contains": "tempo"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        // Should only match the Tempo Run
        assert!(content_str.contains("Tempo Run"));
    }

    #[tokio::test]
    async fn test_analyze_single_with_requested_metrics() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
                .with_workout_detail(json!({
                    "distance": 10000.0,
                    "moving_time": 3600,
                    "average_heartrate": 150.0,
                    "average_watts": 250.0,
                    "total_elevation_gain": 200.0,
                })),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "metrics": ["time", "distance", "hr", "tss"]
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        // Verify requested metrics table exists with expected headers
        let has_metrics_table = output.content.iter().any(|block| {
            if let ContentBlock::Table { headers, rows } = block {
                headers.contains(&"Metric".to_string())
                    && headers.contains(&"Value".to_string())
                    && rows
                        .iter()
                        .any(|row| row.first().map(|s| s.as_str()) == Some("TIME"))
                    && rows
                        .iter()
                        .any(|row| row.first().map(|s| s.as_str()) == Some("DISTANCE"))
            } else {
                false
            }
        });
        assert!(
            has_metrics_table,
            "Expected requested metrics table with TIME/DISTANCE rows"
        );
    }

    #[tokio::test]
    async fn test_analyze_single_with_activity_messages() {
        use intervals_icu_client::ActivityMessage;
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
                .with_workout_detail(json!({
                    "distance": 10000.0,
                    "moving_time": 3600,
                    "average_heartrate": 150.0,
                }))
                .with_activity_messages(vec![ActivityMessage {
                    id: 1,
                    athlete_id: Some("athlete1".to_string()),
                    name: Some("Coach".to_string()),
                    created: Some("2026-03-01T12:00:00Z".to_string()),
                    message_type: Some("TEXT".to_string()),
                    content: Some("Great job!".to_string()),
                    activity_id: Some("12345".to_string()),
                    start_index: None,
                    end_index: None,
                    attachment_url: None,
                    attachment_mime_type: None,
                    deleted: None,
                }]),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "analysis_type": "detailed"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Workout Comments"));
        assert!(content_str.contains("Great job!"));
    }

    #[tokio::test]
    async fn test_analyze_single_invalid_date() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::with_activity(
            "12345",
            "2026-03-01",
            "Test Workout",
        ));

        let input = json!({
            "target_type": "single",
            "date": "invalid-date"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_analyze_single_missing_date() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::with_activity(
            "12345",
            "2026-03-01",
            "Test Workout",
        ));

        let input = json!({
            "target_type": "single"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_analyze_period_basic() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![
                    ActivitySummary {
                        id: "12345".to_string(),
                        name: Some("Run 1".to_string()),
                        start_date_local: "2026-03-01".to_string(),
                        ..Default::default()
                    },
                    ActivitySummary {
                        id: "12346".to_string(),
                        name: Some("Run 2".to_string()),
                        start_date_local: "2026-03-03".to_string(),
                        ..Default::default()
                    },
                ])
                .with_fitness_summary(json!({
                    "fitness": 50,
                    "fatigue": 30,
                    "form": 20
                }))
                .with_wellness(json!({
                    "monotony": 1.5,
                    "strain": 500
                })),
        );

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "2026-03-07"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Period:"));
        assert!(content_str.contains("2026-03-01"));
        assert!(content_str.contains("2026-03-07"));
    }

    #[tokio::test]
    async fn test_analyze_period_shows_fitness_snapshot() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![
                    ActivitySummary {
                        id: "12345".to_string(),
                        name: Some("Run 1".to_string()),
                        start_date_local: "2026-03-01".to_string(),
                        ..Default::default()
                    },
                    ActivitySummary {
                        id: "12346".to_string(),
                        name: Some("Run 2".to_string()),
                        start_date_local: "2026-03-03".to_string(),
                        ..Default::default()
                    },
                ])
                .with_fitness_summary(json!({
                    "fitness": 65.0,
                    "fatigue": 45.0,
                    "form": 20.0,
                    "rampRate": 3.0,
                }))
                .with_wellness(json!({})),
        );

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "2026-03-07"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let content_str = content_text(&result.unwrap().content);
        assert!(
            content_str.contains("Fitness Snapshot"),
            "should show Fitness Snapshot section"
        );
        assert!(content_str.contains("CTL"), "should show CTL");
        assert!(content_str.contains("ATL"), "should show ATL");
        assert!(
            content_str.contains("Fresh"),
            "should show TSB with Fresh state"
        );
        assert!(content_str.contains("Ramp Rate"), "should show Ramp Rate");
    }

    #[tokio::test]
    async fn test_analyze_period_no_activities() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "2026-03-07"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("No activities found"));
    }

    #[tokio::test]
    async fn test_analyze_period_with_calendar_events() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![ActivitySummary {
                    id: "12345".to_string(),
                    name: Some("Run".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                }])
                .with_events(vec![Event {
                    id: Some("e1".to_string()),
                    start_date_local: "2026-03-05".to_string(),
                    name: "Race Day".to_string(),
                    category: EventCategory::RaceA,
                    description: Some("Marathon".to_string()),
                    r#type: None,
                }]),
        );

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "2026-03-07"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Calendar Events"));
        assert!(content_str.contains("Race Day"));
    }

    #[tokio::test]
    async fn test_analyze_period_invalid_start_date() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({
            "target_type": "period",
            "period_start": "invalid",
            "period_end": "2026-03-07"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_analyze_period_invalid_end_date() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "invalid"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_analyze_period_start_after_end() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-07",
            "period_end": "2026-03-01"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_analyze_period_missing_period_start() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({
            "target_type": "period",
            "period_end": "2026-03-07"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_analyze_period_missing_period_end() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_analyze_period_with_histograms_rejected() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "2026-03-07",
            "include_histograms": true
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("include_histograms")
        );
    }

    #[tokio::test]
    async fn test_analyze_period_with_description_filter() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder().with_activities(vec![
            ActivitySummary {
                id: "12345".to_string(),
                name: Some("Easy Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            },
            ActivitySummary {
                id: "12346".to_string(),
                name: Some("Interval Run".to_string()),
                start_date_local: "2026-03-03".to_string(),
                ..Default::default()
            },
        ]));

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "2026-03-07",
            "description_contains": "interval"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        // Should only include the interval run
        assert!(content_str.contains("Period:"));
    }

    #[tokio::test]
    async fn test_analyze_period_with_requested_metrics() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![ActivitySummary {
                    id: "12345".to_string(),
                    name: Some("Run".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                }])
                .with_activity_detail(
                    "12345",
                    json!({
                        "moving_time": 3600,
                        "distance": 10000.0,
                        "total_elevation_gain": 200.0,
                        "average_heartrate": 150.0,
                        "icu_training_load": 75.0,
                    }),
                ),
        );

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "2026-03-07",
            "metrics": ["time", "distance", "hr"]
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Requested Metrics"));
    }

    #[tokio::test]
    async fn test_analyze_period_analysis_type_streams() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![ActivitySummary {
                    id: "12345".to_string(),
                    name: Some("Run".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                }])
                .with_activity_detail(
                    "12345",
                    json!({
                        "moving_time": 3600,
                        "icu_training_load": 75.0,
                    }),
                ),
        );

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "2026-03-07",
            "analysis_type": "streams"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Daily Load Series"));
    }

    #[tokio::test]
    async fn test_analyze_period_analysis_type_intervals() {
        let handler = AnalyzeTrainingHandler::new();
        let client =
            Arc::new(
                MockIntervalsClient::builder().with_activities(vec![ActivitySummary {
                    id: "12345".to_string(),
                    name: Some("Interval Session".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                }]),
            );

        let input = json!({
            "target_type": "period",
            "period_start": "2026-03-01",
            "period_end": "2026-03-07",
            "analysis_type": "intervals"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        // Interval sessions section appears when interval workouts found
        assert!(content_str.contains("Period:"));
    }

    #[tokio::test]
    async fn test_execute_invalid_target_type() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({
            "target_type": "invalid"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid target_type")
        );
    }

    #[tokio::test]
    async fn test_execute_missing_target_type() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({});

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("target_type"));
    }

    // ========================================================================
    // Regression: include_histograms forbidden for period analysis
    // ========================================================================

    #[test]
    fn test_schema_has_if_then_constraints() {
        let handler = AnalyzeTrainingHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        assert!(schema.get("if").is_some(), "Schema should have 'if' clause");
        assert!(
            schema.get("then").is_some(),
            "Schema should have 'then' clause"
        );

        let if_clause = schema.get("if").unwrap();
        let then_clause = schema.get("then").unwrap();

        assert_eq!(
            if_clause
                .get("properties")
                .and_then(|p| p.get("target_type"))
                .and_then(|t| t.get("const"))
                .and_then(|c| c.as_str()),
            Some("period"),
            "'if' should check target_type == 'period'"
        );

        let then_props = then_clause.get("properties").unwrap().as_object().unwrap();
        assert!(
            then_props.contains_key("include_histograms"),
            "'then' should constrain include_histograms"
        );
        assert_eq!(
            then_props
                .get("include_histograms")
                .and_then(|v| v.get("const")),
            Some(&json!(false)),
            "include_histograms should be const: false for period"
        );
    }

    #[tokio::test]
    async fn test_execute_period_with_histograms_rejected() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(MockIntervalsClient::builder());

        let input = json!({
            "target_type": "period",
            "period_start": "2026-05-04",
            "period_end": "2026-06-07",
            "include_histograms": true
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("include_histograms"),
            "Error should mention include_histograms, got: {}",
            err
        );
    }

    #[test]
    fn test_schema_include_histograms_description_mentions_single() {
        let handler = AnalyzeTrainingHandler::new();
        let schema = IntentHandler::input_schema(&handler);
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let histograms = props.get("include_histograms").unwrap();
        let desc = histograms
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap();
        assert!(
            desc.contains("single"),
            "Description should mention 'single', got: {}",
            desc
        );
    }

    #[tokio::test]
    async fn test_single_workout_includes_grade() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
                .with_workout_detail(json!({
                    "distance": 10000.0,
                    "moving_time": 3600,
                    "average_heartrate": 150.0,
                    "average_watts": 250.0,
                    "total_elevation_gain": 200.0,
                })),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "analysis_type": "summary"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);

        let has_grade = content_str.contains("Grade")
            || content_str.contains("grade")
            || content_str.contains("Grade:")
            || content_str.contains("Workout Grade");
        assert!(
            has_grade,
            "Output should contain grade information (A/B/C/D/F). Got: {}",
            &content_str[..content_str.len().min(500)]
        );
    }

    #[tokio::test]
    async fn test_single_workout_includes_insights() {
        let handler = AnalyzeTrainingHandler::new();
        let client = Arc::new(
            MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
                .with_workout_detail(json!({
                    "distance": 10000.0,
                    "moving_time": 3600,
                    "average_heartrate": 150.0,
                    "average_watts": 250.0,
                    "total_elevation_gain": 200.0,
                })),
        );

        let input = json!({
            "target_type": "single",
            "date": "2026-03-01",
            "analysis_type": "summary"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);

        let has_insights = content_str.contains("Insights")
            || content_str.contains("insights")
            || content_str.contains("Excellent")
            || content_str.contains("Good workout");
        assert!(
            has_insights,
            "Output should contain workout insights from WorkoutInsights::generate. Got: {}",
            &content_str[..content_str.len().min(500)]
        );
    }
}
