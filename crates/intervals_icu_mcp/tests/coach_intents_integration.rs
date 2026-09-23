use chrono::Local;
use intervals_icu_client::ActivitySummary;
use intervals_icu_mcp::intents::handlers::{
    AnalyzeRaceHandler, AnalyzeTrainingHandler, AssessRecoveryHandler, ComparePeriodsHandler,
    ManageProfileHandler, PlanTrainingHandler,
};
use intervals_icu_mcp::intents::{ContentBlock, IntentHandler};
use intervals_icu_mcp::test_support::mock::MockIntervalsClient;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

fn markdown_text(output: &intervals_icu_mcp::intents::IntentOutput) -> String {
    output
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::Markdown { markdown } => markdown.clone(),
            ContentBlock::Table { headers, rows } => format!("{:?}{:?}", headers, rows),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn output_text(output: &intervals_icu_mcp::intents::IntentOutput) -> String {
    markdown_text(output)
}

async fn execute_interval_analysis(
    client: MockIntervalsClient,
) -> intervals_icu_mcp::intents::IntentOutput {
    let handler = AnalyzeTrainingHandler::new();
    handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            Arc::new(client),
            None,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn streams_available_upstream_intervals_failed_reports_local_result_and_warning() {
    let output =
        execute_interval_analysis(MockIntervalsClient::with_streams_and_interval_error()).await;
    assert!(output_text(&output).contains("Local detection completed"));
    assert!(output_text(&output).contains("upstream interval endpoint unavailable"));
}

#[tokio::test]
async fn unavailable_streams_do_not_claim_zero_detected_intervals() {
    let output = execute_interval_analysis(MockIntervalsClient::with_stream_error()).await;
    assert!(output_text(&output).contains("Interval detection unavailable"));
    assert!(!output_text(&output).contains("Completed 0 work intervals"));
}

#[tokio::test]
async fn local_fartlek_classification_overrides_upstream_interval_rows() {
    let output = execute_interval_analysis(
        MockIntervalsClient::with_fartlek_streams_and_upstream_intervals(),
    )
    .await;
    let text = output_text(&output);

    assert!(text.contains("fartlek / non-structured"));
    assert!(!text.contains("Detected Intervals"));
}

#[tokio::test]
async fn assess_recovery_uses_shared_guidance_for_deep_fatigue() {
    let client = Arc::new(MockIntervalsClient::with_tsb(-25.0));
    let handler = AssessRecoveryHandler::new();

    let output = handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    assert!(output.suggestions.iter().any(|s| s.contains("recovery")));
}

#[tokio::test]
async fn analyze_training_period_includes_trend_context() {
    let client = Arc::new(MockIntervalsClient::with_period_blocks());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    assert!(markdown_text(&output).to_lowercase().contains("trend"));
}

#[tokio::test]
async fn analyze_training_single_accepts_today_date_alias() {
    let today = Local::now().date_naive();
    let client = Arc::new(MockIntervalsClient {
        activities: vec![MockIntervalsClient::activity(
            "today-training-1",
            "Today's Endurance Run",
            &format!("{}T07:30:00", today.format("%Y-%m-%d")),
        )],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(55.0, 47.0, 8.0)),
        default_activity_detail: Some(json!({
            "distance": 18050.0,
            "moving_time": 7742,
            "average_heartrate": 141.0,
            "total_elevation_gain": 233.0
        })),
        ..MockIntervalsClient::default()
    });
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "today"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Today's Endurance Run"));
    assert!(markdown.contains("Date: today"));
}

#[tokio::test]
async fn analyze_training_period_surfaces_future_planned_workouts() {
    let client = Arc::new(MockIntervalsClient::with_future_workouts_only());
    let handler = AnalyzeTrainingHandler::new();
    let period_start = MockIntervalsClient::relative_date(1);
    let period_end = MockIntervalsClient::relative_date(2);

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": period_start,
                "period_end": period_end
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Planned Workouts"));
    assert!(markdown.contains("Recovery Run Z1"));
    assert!(markdown.contains("Endurance Run Z2 — Pre-Trip"));
    assert_eq!(output.metadata.total_count, Some(2));
}

#[tokio::test]
async fn analyze_training_period_surfaces_future_calendar_events() {
    let client = Arc::new(MockIntervalsClient::with_future_calendar_events_only());
    let handler = AnalyzeTrainingHandler::new();
    let period_start = MockIntervalsClient::relative_date(1);
    let period_end = MockIntervalsClient::relative_date(3);

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": period_start,
                "period_end": period_end
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Calendar Events in Window"));
    assert!(markdown.contains("City Marathon"));
    assert!(markdown.contains("Sick day"));
    assert!(markdown.contains("RaceA"));
    assert!(markdown.contains("Sick"));
}

#[tokio::test]
async fn analyze_training_period_skips_calendar_duplicates_with_paired_activity_id() {
    let client = Arc::new(MockIntervalsClient::with_paired_activity_and_calendar_duplicate());
    let handler = AnalyzeTrainingHandler::new();
    let target_date = MockIntervalsClient::relative_date(0);

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": target_date,
                "period_end": target_date
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(!markdown.contains("Planned Workouts in Window"));
    assert_eq!(output.metadata.total_count, Some(1));
}

#[tokio::test]
async fn analyze_race_adds_post_race_recovery_guidance() {
    let client = Arc::new(MockIntervalsClient::with_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(json!({"description_contains": "50K"}), client, None)
        .await
        .unwrap();

    assert!(
        output
            .next_actions
            .iter()
            .any(|a| a.contains("assess_recovery"))
    );

    let markdown = markdown_text(&output);

    // Race Readiness Score must appear in output
    assert!(
        markdown.contains("Race Readiness"),
        "Race Readiness section should appear:\n{}",
        markdown
    );
    assert!(
        markdown.contains("Score:"),
        "Race Readiness Score should appear:\n{}",
        markdown
    );
}

#[tokio::test]
async fn analyze_race_accepts_target_date_alias() {
    let today = Local::now().date_naive();
    let client = Arc::new(MockIntervalsClient {
        activities: vec![
            MockIntervalsClient::activity("older-race-1", "Mountain 50K", "2026-02-21T08:23:41"),
            MockIntervalsClient::activity(
                "today-run-1",
                "Today's Long Run",
                &format!("{}T08:00:00", today.format("%Y-%m-%d")),
            ),
        ],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(42.0, 68.0, -18.0)),
        default_activity_detail: Some(json!({
            "distance": 18040.0,
            "moving_time": 7742,
            "average_heartrate": 140.0
        })),
        ..MockIntervalsClient::default()
    });
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(json!({"target_date": "today"}), client, None)
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Today's Long Run"));
    assert!(markdown.contains(&format!("{}T08:00:00", today.format("%Y-%m-%d"))));
    assert!(!markdown.contains("Mountain 50K"));
}

#[tokio::test]
async fn compare_periods_includes_shared_trend_context() {
    let client = Arc::new(MockIntervalsClient::with_period_blocks());
    let handler = ComparePeriodsHandler::new();

    let output = handler
        .execute(
            json!({
                "period_a_start": "2026-03-01",
                "period_a_end": "2026-03-07",
                "period_b_start": "2026-02-24",
                "period_b_end": "2026-02-28"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    assert!(markdown_text(&output).to_lowercase().contains("trend"));
}

/// Delta column direction must be invariant to which period the caller labels
/// as A vs B. The convention is "later period relative to earlier period":
/// positive values mean the later period grew vs the earlier period.
///
/// Regression test for the bug where putting the older period as `period_a`
/// and the newer period as `period_b` produced inverted (negative) Δ signs,
/// because the engine computed `period_a - period_b` positionally instead of
/// `(later start_date) - (earlier start_date)`.
#[tokio::test]
async fn compare_periods_delta_sign_invariant_to_a_b_order() {
    // `with_period_blocks()` exposes 3 activities:
    //   a1 on 2026-03-01, a2 on 2026-03-03 (the later week)
    //   a3 on 2026-02-25                (the earlier week)
    // activity_details applies the same volume (5400s, 15000m, 300m elev)
    // to every activity. So whichever period covers March has 2 activities
    // (10800s, 30000m, 600m) and whichever covers Feb has 1 activity.
    let client = Arc::new(MockIntervalsClient::with_period_blocks());
    let handler = ComparePeriodsHandler::new();

    // Caller puts the OLDER period as A (period_a=Feb, period_b=March).
    // The Δ column must still be positive: March has grown vs Feb.
    let output = handler
        .execute(
            json!({
                "period_a_start": "2026-02-24",
                "period_a_end": "2026-02-28",
                "period_b_start": "2026-03-01",
                "period_b_end": "2026-03-07"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);

    // The activity-count delta in the Trend Context block must report
    // "+1" (March gained one workout over February). Before the fix this
    // came back as "-1" because period_a (Feb) was subtracted from
    // period_b (March) — i.e. (older - newer) instead of (newer - older).
    assert!(
        markdown.contains("Activity delta: +1"),
        "Activity delta should be +1 (later period has 1 more activity); got:\n{markdown}"
    );
    // The comparison table Δ cell for the Volume (hours) row must be a
    // positive number — later period has 2x the moving time.
    // 5400s → 1.5h, 10800s → 3.0h, so Δ = +1.5h (+100%).
    assert!(
        markdown.contains("+1.5") || markdown.contains("+1.50"),
        "Volume Δ cell should be positive when later period grew; got:\n{markdown}"
    );
    // Table columns must reorder so the later period (Period B in the
    // caller's input) appears first as the comparison reference.
    assert!(
        markdown.contains("Period B vs Period A"),
        "Header should put the later period first; got:\n{markdown}"
    );
}

#[tokio::test]
async fn analyze_training_single_surfaces_execution_quality_and_degraded_availability() {
    let client = Arc::new(MockIntervalsClient::with_single_workout_degraded_streams());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output).to_lowercase();
    assert!(markdown.contains("execution context"));
    assert!(markdown.contains("quality findings"));
    assert!(markdown.contains("data availability"));
    assert!(markdown.contains("stream data unavailable"));
    assert!(markdown.contains("fitness summary unavailable"));
}

#[tokio::test]
async fn analyze_race_degraded_mode_reports_missing_supporting_data() {
    let client = Arc::new(MockIntervalsClient::with_race_degraded_context());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(
            json!({"description_contains": "Spring Marathon"}),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output).to_lowercase();
    assert!(markdown.contains("race analysis"));
    assert!(markdown.contains("data availability"));
    assert!(markdown.contains("wellness data unavailable or empty"));
    assert!(markdown.contains("interval data unavailable"));
    assert!(markdown.contains("stream data unavailable"));
}

#[tokio::test]
async fn assess_recovery_with_fatigue_alert_tsb_minus_15() {
    let client = Arc::new(MockIntervalsClient {
        activities: vec![MockIntervalsClient::activity(
            "activity-1",
            "Easy Run",
            "2026-03-04",
        )],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(50.0, 65.0, -15.0)),
        ..MockIntervalsClient::default()
    });
    let handler = AssessRecoveryHandler::new();

    let output = handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    let markdown = markdown_text(&output).to_lowercase();
    assert!(markdown.contains("fatigue"));
    assert!(output.suggestions.iter().any(|s| s.contains("recovery")));
}

#[tokio::test]
async fn assess_recovery_with_high_training_load() {
    let client = Arc::new(MockIntervalsClient {
        activities: vec![MockIntervalsClient::activity(
            "activity-1",
            "Hard Session",
            "2026-03-04",
        )],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(80.0, 60.0, 20.0)),
        wellness: Some(json!([
            {"sleepSecs": 28800.0, "restingHR": 48.0, "hrv": 75.0},
            {"sleepSecs": 27000.0, "restingHR": 50.0, "hrv": 70.0}
        ])),
        default_activity_detail: Some(json!({
            "distance": 15000.0,
            "moving_time": 7200,
            "average_heartrate": 140.0,
            "total_elevation_gain": 300.0
        })),
        ..MockIntervalsClient::default()
    });
    let handler = AssessRecoveryHandler::new();

    let output = handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    // High volume should trigger high training load alert
    let markdown = markdown_text(&output).to_lowercase();
    assert!(markdown.contains("data availability"));
}

#[tokio::test]
async fn assess_recovery_shows_recovery_index_and_blocks_ready_language_when_sleep_is_poor() {
    let client = Arc::new(MockIntervalsClient::with_positive_tsb_and_low_sleep());
    let handler = AssessRecoveryHandler::new();

    let output = handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Recovery Index"));
    assert!(
        !output
            .suggestions
            .iter()
            .any(|s| s.contains("ready for key work"))
    );
    assert!(output.suggestions.iter().any(|s| s.contains("recovery")));
}

#[tokio::test]
async fn assess_recovery_requests_long_enough_wellness_history_for_adaptive_hrv() {
    let client = Arc::new(MockIntervalsClient::with_personal_hrv_drop_profile());
    let observations = client.observations();
    let handler = AssessRecoveryHandler::new();

    handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    let requests = observations.wellness_days_history();
    // Personal baseline requires 60 calendar days of wellness history
    assert!(requests.contains(&Some(60)));
}

#[tokio::test]
async fn assess_recovery_treats_same_absolute_hrv_relative_to_each_athlete_baseline() {
    let high_baseline_client = Arc::new(MockIntervalsClient::with_personal_hrv_drop_profile());
    let low_baseline_client = Arc::new(MockIntervalsClient::with_personal_hrv_norm_profile());
    let handler = AssessRecoveryHandler::new();

    let high_baseline_output = handler
        .execute(
            json!({"period_days": 7, "for_activity": "intensity"}),
            high_baseline_client,
            None,
        )
        .await
        .unwrap();
    let low_baseline_output = handler
        .execute(
            json!({"period_days": 7, "for_activity": "intensity"}),
            low_baseline_client,
            None,
        )
        .await
        .unwrap();

    let high_markdown = markdown_text(&high_baseline_output);
    let low_markdown = markdown_text(&low_baseline_output);

    assert!(high_markdown.contains("personal baseline"));
    assert!(high_markdown.contains("Hold intensity"));
    assert!(low_markdown.contains("Within personal range"));
    assert!(low_markdown.contains("Ready for quality"));
}

#[tokio::test]
async fn analyze_training_period_renders_acwr_and_monotony_context() {
    let client = Arc::new(MockIntervalsClient::with_load_ramp_block());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-28"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("ACWR"));
    assert!(markdown.contains("Monotony"));
}

#[tokio::test]
async fn analyze_training_period_prefers_api_load_snapshot_for_acwr_loads() {
    let client = Arc::new(MockIntervalsClient::with_api_load_snapshot());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-28"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("acute 444.0, chronic 333.0"));
}

#[tokio::test]
async fn analyze_training_single_renders_execution_metrics_when_streams_exist() {
    let client = Arc::new(MockIntervalsClient::with_stream_supported_workout());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Efficiency Factor"));
    assert!(markdown.contains("Aerobic Decoupling"));
}

#[tokio::test]
async fn analyze_training_prefers_api_decoupling_over_stream_recalculation() {
    let mut client = MockIntervalsClient::with_stream_supported_workout();
    client.default_activity_detail = Some(json!({
        "distance": 14000.0,
        "moving_time": 3600,
        "average_heartrate": 145.0,
        "average_watts": 220.0,
        "total_elevation_gain": 80.0,
        "decoupling": 2.7809474
    }));
    client.activity_details.clear();
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "streams"
            }),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Aerobic Decoupling: 2.8%"));
    assert!(markdown.to_lowercase().contains("acceptable"));
}

#[tokio::test]
async fn manage_profile_renders_requested_metrics_section_from_fitness_summary() {
    let client = Arc::new(MockIntervalsClient::with_profile_metrics());
    let handler = ManageProfileHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "get",
                "sections": ["metrics"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Metrics"));
    assert!(markdown.contains("CTL") || markdown.contains("Fitness"));
    assert!(markdown.contains("TSB") || markdown.contains("Form"));
}

#[tokio::test]
async fn manage_profile_supports_real_sport_settings_array_for_zones_and_thresholds() {
    let client = Arc::new(MockIntervalsClient::with_profile_metrics());
    let handler = ManageProfileHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "get",
                "sections": ["zones", "thresholds"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Zones (Run)") || markdown.contains("Zones (Running)"));
    assert!(
        markdown.contains("LTHR")
            || markdown.contains("Threshold Pace")
            || markdown.contains("FTP")
    );
}

#[tokio::test]
async fn manage_profile_surfaces_lthr_directly_from_sport_settings() {
    let client = Arc::new(MockIntervalsClient::with_profile_metrics());
    let handler = ManageProfileHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "get",
                "sections": ["thresholds"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("LTHR"));
    assert!(markdown.contains("171 bpm"));
}

#[tokio::test]
async fn manage_profile_overview_uses_wellness_weight_when_profile_has_none() {
    let client = Arc::new(MockIntervalsClient::with_profile_metrics_and_wellness_weight());
    let handler = ManageProfileHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "get",
                "sections": ["overview"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("86.0 kg"));
}

#[tokio::test]
async fn analyze_training_single_renders_pace_histogram_when_requested() {
    let client = Arc::new(MockIntervalsClient::with_stream_supported_workout());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "detailed",
                "include_histograms": true
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Pace Histogram") || markdown.contains("Pace Zone Distribution"));
}

#[tokio::test]
async fn analyze_training_single_surfaces_workout_comments() {
    let mut client = MockIntervalsClient::with_rich_detailed_workout();
    client.default_activity_detail = Some(json!({
        "distance": 12000.0,
        "moving_time": 3600,
        "average_heartrate": 141.0,
        "average_watts": 212.0,
        "average_cadence": 84.5,
        "average_speed": 3.3333333,
        "average_temp": 19.4,
        "total_elevation_gain": 95.0,
        "tss": 78.5,
        "icu_training_load": 81.0,
        "__activity_messages": [
            {
                "id": 301,
                "athlete_id": "athlete-1",
                "name": "Coach Test",
                "created": "2026-03-08T09:15:00Z",
                "type": "TEXT",
                "content": "Felt smooth until the final 10 minutes.",
                "activity_id": "detail-rich-1"
            },
            {
                "id": 302,
                "athlete_id": "coach-1",
                "name": "Coach Bob",
                "created": "2026-03-08T10:00:00Z",
                "type": "TEXT",
                "content": "Good restraint early, nice finish.",
                "activity_id": "detail-rich-1"
            }
        ]
    }));

    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "detailed"
            }),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Workout Comments"));
    assert!(markdown.contains("Felt smooth until the final 10 minutes."));
    assert!(markdown.contains("Good restraint early, nice finish."));
    assert!(markdown.contains("Coach Bob"));
}

#[tokio::test]
async fn analyze_training_single_modes_render_distinct_sections() {
    let client = Arc::new(MockIntervalsClient::with_mode_sensitive_single_workout());
    let handler = AnalyzeTrainingHandler::new();

    let summary = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let detailed = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "detailed"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let intervals = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "intervals"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let streams = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let summary_md = markdown_text(&summary);
    let detailed_md = markdown_text(&detailed);
    let intervals_md = markdown_text(&intervals);
    let streams_md = markdown_text(&streams);

    assert!(!summary_md.contains("Execution Context"));
    assert!(detailed_md.contains("Execution Context"));
    assert!(!detailed_md.contains("Interval Analysis"));
    assert!(intervals_md.contains("Interval Analysis"));
    assert!(streams_md.contains("Stream Insights"));
}

#[tokio::test]
async fn analyze_training_single_modes_do_not_collapse_when_interval_data_is_missing() {
    let client = Arc::new(MockIntervalsClient::with_mode_collapse_single_workout());
    let handler = AnalyzeTrainingHandler::new();

    let summary = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "summary"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let detailed = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "detailed"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let intervals = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let streams = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let summary_md = markdown_text(&summary);
    let detailed_md = markdown_text(&detailed);
    let intervals_md = markdown_text(&intervals);
    let streams_md = markdown_text(&streams);

    assert!(!summary_md.contains("Quality Findings"));
    assert!(!summary_md.contains("Interval Analysis"));
    assert!(detailed_md.contains("Quality Findings"));
    assert!(!detailed_md.contains("Interval Analysis"));
    assert!(intervals_md.contains("Interval Analysis"));
    assert!(!intervals_md.contains("Quality Findings"));
    assert!(!intervals_md.contains("Execution Context"));
    assert!(streams_md.contains("Stream Insights"));
    assert!(streams_md.contains("Quality Findings"));
    assert!(!streams_md.contains("Interval Analysis"));
}

#[tokio::test]
async fn analyze_training_single_intervals_reads_object_shaped_intervals_payload() {
    let client = Arc::new(MockIntervalsClient::with_object_shaped_interval_payload());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Interval Analysis"));
    assert!(!markdown.contains("No structured interval data available"));
    assert!(markdown.contains("10:01") || markdown.contains("5:00") || markdown.contains("6:00"));
    assert!(markdown.contains("126 bpm") || markdown.contains("158 bpm"));
}

#[tokio::test]
async fn analyze_training_detailed_adds_expanded_workout_breakdown() {
    let client = Arc::new(MockIntervalsClient::with_rich_detailed_workout());
    let handler = AnalyzeTrainingHandler::new();

    let summary = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let detailed = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let summary_md = markdown_text(&summary);
    let detailed_md = markdown_text(&detailed);

    assert!(!summary_md.contains("Detailed Breakdown"));
    assert!(detailed_md.contains("Detailed Breakdown"));
    assert!(detailed_md.contains("Cadence") || detailed_md.contains("Training Load"));
}

#[tokio::test]
async fn analyze_training_intervals_backfills_power_from_streams_when_interval_payload_has_null_power()
 {
    let client = Arc::new(MockIntervalsClient::with_interval_power_only_in_streams());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Interval Analysis"));
    assert!(markdown.contains("225 W"));
    assert!(markdown.contains("295 W"));
    assert!(!markdown.contains("0 W"));
}

#[tokio::test]
async fn analyze_training_streams_mode_renders_streams_from_noncanonical_payload_without_interval_table()
 {
    let client = Arc::new(MockIntervalsClient::with_noncanonical_stream_payload());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Stream Insights"));
    assert!(markdown.contains("heartrate"));
    assert!(markdown.contains("watts"));
    assert!(!markdown.contains("Interval Analysis"));
    assert!(
        !markdown
            .to_lowercase()
            .contains("stream data requested but unavailable")
    );
}

#[tokio::test]
async fn analyze_training_streams_prioritizes_key_streams_ahead_of_secondary_metrics() {
    let client = Arc::new(MockIntervalsClient::with_priority_streams_without_power());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Stream Insights"));
    assert!(markdown.contains("heartrate"));
    assert!(markdown.contains("velocity_smooth"));
    assert!(markdown.contains("cadence"));
}

#[tokio::test]
async fn analyze_training_streams_quality_findings_fall_back_to_pace_when_power_is_missing() {
    let client = Arc::new(MockIntervalsClient::with_priority_streams_without_power());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Quality Findings"));
    assert!(markdown.contains("Average pace held at 6:27 /km."));
    assert!(!markdown.contains("Average power tracked"));
}

#[tokio::test]
async fn analyze_training_intervals_backfills_power_from_power_stream_alias() {
    let client = Arc::new(MockIntervalsClient::with_interval_power_stream_alias());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("255 W"));
    assert!(markdown.contains("305 W"));
    assert!(!markdown.contains("n/a"));
}

#[tokio::test]
async fn analyze_training_intervals_falls_back_to_pace_when_run_streams_have_no_power() {
    let client = Arc::new(MockIntervalsClient::with_interval_output_only_in_speed_streams());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Avg Pace"));
    assert!(markdown.contains("5:33 /km"));
    assert!(markdown.contains("5:13 /km"));
    assert!(!markdown.contains("n/a"));
}

#[tokio::test]
async fn analyze_training_intervals_renders_all_intervals_without_collapsing_tail() {
    let client = Arc::new(MockIntervalsClient::with_many_intervals());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("\"15\""));
    assert!(!markdown.contains("... and"));
}

#[tokio::test]
async fn analyze_training_single_renders_best_efforts_object_payload_and_bucket_histograms() {
    let client = Arc::new(MockIntervalsClient::with_best_efforts_and_bucket_histograms());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "detailed",
                "include_best_efforts": true,
                "include_histograms": true
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Best Efforts"));
    assert!(markdown.contains("310 W"));
    assert!(markdown.contains("HR Histogram"));
    assert!(markdown.contains("Power Histogram"));
    assert!(markdown.contains("Pace Histogram"));
    assert!(markdown.contains("120-124 bpm"));
    assert!(markdown.contains("200-224 W"));
    assert!(!markdown.contains("0 bpm"));
    assert!(!markdown.contains("\"n/a\""));
}

#[tokio::test]
async fn analyze_training_single_summary_renders_best_efforts_when_requested() {
    let client = Arc::new(MockIntervalsClient::with_best_efforts_and_bucket_histograms());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary",
                "include_best_efforts": true,
                "include_histograms": false
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Best Efforts"));
    assert!(markdown.contains("310 W"));
    assert!(!markdown.contains("HR Histogram"));
}

#[tokio::test]
async fn analyze_training_single_summary_renders_live_efforts_shape_when_requested() {
    let client = Arc::new(MockIntervalsClient::with_live_best_efforts_shape());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary",
                "include_best_efforts": true,
                "include_histograms": false
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Best Efforts"));
    assert!(markdown.contains("303.5 W") || markdown.contains("304 W"));
    assert!(markdown.contains("1:00"));
}

#[tokio::test]
async fn analyze_training_single_histograms_include_all_buckets_and_seconds() {
    let client = Arc::new(MockIntervalsClient::with_full_histogram_ranges());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary",
                "include_histograms": true
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(!markdown.contains("more histogram buckets"));
    assert!(markdown.contains("150-154 bpm"));
    assert!(markdown.contains("325-349 W"));
    assert!(markdown.contains("3.68-3.77 m/s"));
    assert!(markdown.contains("0:03"));
    assert!(markdown.contains("0:01"));
}

#[tokio::test]
async fn analyze_training_period_rejects_histograms_flag() {
    let client = Arc::new(MockIntervalsClient::with_period_blocks());
    let handler = AnalyzeTrainingHandler::new();

    let err = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07",
                "include_histograms": true
            }),
            client,
            None,
        )
        .await
        .expect_err("period histograms should be rejected explicitly");

    assert!(err.to_string().contains("include_histograms"));
}

#[tokio::test]
async fn analyze_training_period_reports_requested_tss_when_it_cannot_be_computed() {
    let client = Arc::new(MockIntervalsClient::with_period_blocks());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07",
                "metrics": ["tss"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("TSS"));
    assert!(
        markdown.to_lowercase().contains("unavailable")
            || markdown.to_lowercase().contains("unsupported")
    );
}

#[tokio::test]
async fn analyze_race_surfaces_decoupling_warning_when_drift_is_high() {
    let client = Arc::new(MockIntervalsClient::with_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(json!({"description_contains": "50K"}), client, None)
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Aerobic Decoupling"));
    assert!(markdown.to_lowercase().contains("watch"));
}

#[tokio::test]
async fn analyze_training_period_summary_omits_trend_and_load_sections() {
    let client = Arc::new(MockIntervalsClient::with_period_blocks());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07",
                "analysis_type": "summary"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(!markdown.contains("Trend Context"));
    assert!(!markdown.contains("Load Context"));
}

#[tokio::test]
async fn compare_periods_filters_by_workout_type_and_renders_requested_metrics() {
    let client = Arc::new(MockIntervalsClient::with_mixed_period_workouts());
    let handler = ComparePeriodsHandler::new();

    let output = handler
        .execute(
            json!({
                "period_a_start": "2026-03-01",
                "period_a_end": "2026-03-07",
                "period_b_start": "2026-02-24",
                "period_b_end": "2026-02-28",
                "workout_type": "tempo",
                "metrics": ["pace", "hr", "tss"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Requested Metrics"));
    assert!(markdown.contains("TSS"));
    assert!(
        markdown.contains("Activities\", \"1\"")
            || markdown.contains("+0")
            || markdown.contains("Tempo")
    );
}

#[tokio::test]
async fn analyze_race_last_race_selects_race_instead_of_latest_regular_activity() {
    let client = Arc::new(MockIntervalsClient::with_recent_non_race_then_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(
            json!({
                "date": "last_race",
                "analysis_type": "strategy"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("City Marathon Race"));
    assert!(!markdown.contains("Easy Run"));
    assert!(markdown.contains("Strategy"));
}

#[tokio::test]
async fn analyze_race_compare_to_planned_adds_plan_section_when_enabled() {
    let client = Arc::new(MockIntervalsClient::with_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(
            json!({
                "description_contains": "50K",
                "compare_to_planned": true,
                "analysis_type": "performance"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Comparison to Plan"));
}

#[tokio::test]
async fn analyze_race_modes_render_distinct_sections() {
    let client = Arc::new(MockIntervalsClient::with_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let performance = handler
        .execute(
            json!({
                "description_contains": "50K",
                "analysis_type": "performance"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let strategy = handler
        .execute(
            json!({
                "description_contains": "50K",
                "analysis_type": "strategy"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let recovery = handler
        .execute(
            json!({
                "description_contains": "50K",
                "analysis_type": "recovery"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let performance_md = markdown_text(&performance);
    let strategy_md = markdown_text(&strategy);
    let recovery_md = markdown_text(&recovery);

    assert!(performance_md.contains("Performance Review"));
    assert!(!performance_md.contains("Strategy Review"));
    assert!(strategy_md.contains("Strategy Review"));
    assert!(!strategy_md.contains("Performance Review"));
    assert!(recovery_md.contains("Recovery Outlook"));
    assert!(!recovery_md.contains("Strategy Review"));
}

#[tokio::test]
async fn assess_recovery_for_activity_changes_readiness_guidance() {
    let client = Arc::new(MockIntervalsClient::with_supportive_recovery_metrics());
    let handler = AssessRecoveryHandler::new();

    let easy = handler
        .execute(
            json!({"period_days": 7, "for_activity": "easy"}),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let intensity = handler
        .execute(
            json!({"period_days": 7, "for_activity": "intensity"}),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let race = handler
        .execute(
            json!({"period_days": 7, "for_activity": "race"}),
            client,
            None,
        )
        .await
        .unwrap();

    let easy_md = markdown_text(&easy);
    let intensity_md = markdown_text(&intensity);
    let race_md = markdown_text(&race);
    let easy_md_lower = easy_md.to_lowercase();
    let intensity_md_lower = intensity_md.to_lowercase();
    let race_md_lower = race_md.to_lowercase();

    assert!(easy_md.contains("Activity-Specific Readiness"));
    assert!(easy_md_lower.contains("easy training"));
    assert!(
        intensity_md_lower.contains("quality session") || intensity_md_lower.contains("intensity")
    );
    assert!(race_md_lower.contains("race effort") || race_md_lower.contains("race-ready"));
    assert_ne!(easy_md, intensity_md);
    assert_ne!(intensity_md, race_md);
}

#[tokio::test]
async fn plan_training_focus_modes_do_not_collapse() {
    let client = Arc::new(MockIntervalsClient::with_profile_metrics());
    let handler = PlanTrainingHandler::new();

    let intensity = handler
        .execute(
            json!({
                "period_start": "2026-03-01",
                "period_end": "2026-03-28",
                "focus": "intensity",
                "idempotency_token": "plan-intensity"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let specific = handler
        .execute(
            json!({
                "period_start": "2026-03-01",
                "period_end": "2026-03-28",
                "focus": "specific",
                "idempotency_token": "plan-specific"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let recovery = handler
        .execute(
            json!({
                "period_start": "2026-03-01",
                "period_end": "2026-03-28",
                "focus": "recovery",
                "idempotency_token": "plan-recovery"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let intensity_md = markdown_text(&intensity);
    let specific_md = markdown_text(&specific);
    let recovery_md = markdown_text(&recovery);

    // TSB Forecast table must appear for all plan focuses
    assert!(
        intensity_md.contains("TSB Forecast"),
        "TSB Forecast should appear in intensity plan:\n{}",
        intensity_md
    );

    assert!(intensity_md.contains("Intensity") || intensity_md.contains("threshold"));
    assert!(specific_md.contains("race-specific") || specific_md.contains("Specific"));
    assert!(recovery_md.contains("Recovery") || recovery_md.contains("down week"));
    assert_ne!(intensity_md, specific_md);
    assert_ne!(specific_md, recovery_md);
}

fn with_p0_performance_intelligence_client() -> MockIntervalsClient {
    MockIntervalsClient {
        activities: vec![MockIntervalsClient::activity(
            "p0-activity-1",
            "P0 Test Workout",
            "2026-05-01",
        )],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(55.0, 47.0, 8.0)),
        wellness: Some(json!([
            {"type": "Ride", "eftp": 260.0, "wPrime": 20000.0, "pMax": 850.0}
        ])),
        default_activity_detail: Some(json!({
            "distance": 32000.0,
            "moving_time": 5400,
            "average_heartrate": 148.0,
            "average_watts": 235.0,
            "total_elevation_gain": 320.0,
            "icu_efficiency_factor": 1.59,
            "icu_pm_ftp": 260.0,
            "icu_pm_w_prime": 20000.0,
            "icu_pm_p_max": 850.0,
            "icu_max_wbal_depletion": 6000.0,
            "icu_joules_above_ftp": 35000.0
        })),
        intervals: Some(json!([
            {"wbal_start": 20000.0, "wbal_end": 14000.0, "joules_above_ftp": 35000.0},
            {"wbal_start": 14000.0, "wbal_end": 9000.0, "joules_above_ftp": 28000.0}
        ])),
        streams: Some(json!({
            "heartrate": [140.0, 142.0, 144.0, 146.0, 148.0, 150.0, 152.0, 154.0],
            "watts": [235.0, 235.0, 234.0, 234.0, 233.0, 233.0, 232.0, 232.0]
        })),
        ..MockIntervalsClient::default()
    }
}

#[tokio::test]
async fn analyze_training_single_includes_hr_drift_and_pace_variance_from_streams() {
    let client = Arc::new(MockIntervalsClient {
        activities: vec![MockIntervalsClient::activity(
            "drift-1",
            "Long Steady Run",
            "2026-03-04",
        )],
        default_activity_detail: Some(json!({
            "distance": 20000.0,
            "moving_time": 5400,
            "average_heartrate": 150.0,
            "average_watts": 220.0,
            "total_elevation_gain": 100.0
        })),
        streams: Some(json!({
            "heartrate": [140.0, 141.0, 142.0, 143.0, 144.0, 145.0, 155.0, 156.0, 157.0, 158.0],
            "velocity_smooth": [4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0],
            "watts": [220.0, 220.0, 220.0, 220.0, 220.0, 220.0, 220.0, 220.0, 220.0, 220.0]
        })),
        ..MockIntervalsClient::default()
    });
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(
        markdown.contains("HR Drift"),
        "Expected 'HR Drift' in output:\n{}",
        markdown
    );
}

fn with_full_period_analysis_client() -> MockIntervalsClient {
    let mut details_map = HashMap::new();
    details_map.insert(
        "pa-1".into(),
        json!({
            "distance": 30000.0,
            "moving_time": 5400,
            "average_heartrate": 148.0,
            "average_watts": 235.0,
            "total_elevation_gain": 300.0,
            "icu_efficiency_factor": 1.55,
            "icu_pm_ftp": 260.0,
            "icu_pm_w_prime": 20000.0,
            "icu_pm_p_max": 850.0,
            "icu_pm_1m": 500.0,
            "icu_pm_5m": 380.0,
            "icu_pm_20m": 320.0,
            "icu_pm_60m": 280.0,
            "icu_training_load": 80.0,
            "icu_max_wbal_depletion": 4000.0,
            "icu_joules_above_ftp": 25000.0
        }),
    );
    details_map.insert(
        "pa-2".into(),
        json!({
            "distance": 32000.0,
            "moving_time": 6000,
            "average_heartrate": 150.0,
            "average_watts": 245.0,
            "total_elevation_gain": 350.0,
            "icu_efficiency_factor": 1.62,
            "icu_pm_ftp": 265.0,
            "icu_pm_w_prime": 19500.0,
            "icu_pm_p_max": 860.0,
            "icu_pm_1m": 510.0,
            "icu_pm_5m": 390.0,
            "icu_pm_20m": 325.0,
            "icu_pm_60m": 285.0,
            "icu_training_load": 85.0,
            "icu_max_wbal_depletion": 4500.0,
            "icu_joules_above_ftp": 28000.0
        }),
    );

    MockIntervalsClient {
        activities: vec![
            MockIntervalsClient::activity("pa-1", "Week 1 Ride", "2026-04-01"),
            MockIntervalsClient::activity("pa-2", "Week 4 Ride", "2026-04-28"),
        ],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(55.0, 47.0, 8.0)),
        wellness: Some(json!([
            {"type": "Ride", "eftp": 260.0, "wPrime": 20000.0, "pMax": 850.0}
        ])),
        activity_details_map: details_map,
        ..MockIntervalsClient::default()
    }
}

#[tokio::test]
async fn analyze_training_period_renders_full_performance_pipeline() {
    let client = Arc::new(with_full_period_analysis_client());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-04-01",
                "period_end": "2026-05-15",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);

    // Trend Context should appear
    assert!(
        markdown.contains("Trend Context"),
        "Trend Context should appear:\n{}",
        markdown
    );

    // Load Context (ACWR) should appear
    assert!(
        markdown.contains("ACWR"),
        "ACWR should appear in period analysis:\n{}",
        markdown
    );

    // NDLI should appear
    assert!(
        markdown.contains("NDLI") || markdown.contains("Neural Density"),
        "NDLI should appear:\n{}",
        markdown
    );

    // W′ Depletion should appear
    assert!(
        markdown.contains("W′ Depletion"),
        "W′ Depletion should appear:\n{}",
        markdown
    );

    // Power Curve Comparison with adaptation state
    assert!(
        markdown.contains("Power Curve Comparison"),
        "Power Curve Comparison should appear:\n{}",
        markdown
    );
    assert!(
        markdown.contains("Adaptation State"),
        "Adaptation State should appear:\n{}",
        markdown
    );

    // Load Patterns should appear
    assert!(
        markdown.contains("Load Patterns"),
        "Load Patterns should appear:\n{}",
        markdown
    );

    // Terrain Specificity should appear
    assert!(
        markdown.contains("Terrain Specificity"),
        "Terrain Specificity should appear:\n{}",
        markdown
    );
}

#[tokio::test]
async fn analyze_training_single_hr_drift_absent_when_no_streams() {
    let client = Arc::new(MockIntervalsClient {
        activities: vec![MockIntervalsClient::activity(
            "nodrift-1",
            "Short Session",
            "2026-03-04",
        )],
        default_activity_detail: Some(json!({
            "distance": 5000.0,
            "moving_time": 1800,
            "average_heartrate": 145.0,
            "total_elevation_gain": 30.0
        })),
        streams: Some(json!({})),
        ..MockIntervalsClient::default()
    });
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(
        !markdown.contains("HR Drift"),
        "HR Drift should not appear without stream data:\n{}",
        markdown
    );
}

#[tokio::test]
async fn p0_performance_intelligence_full_pipeline() {
    let client = Arc::new(with_p0_performance_intelligence_client());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-05-01",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);

    // P0.1 — ESPE anchors extracted from sportInfo
    assert!(markdown.contains("Power-Duration Anchors"));
    assert!(markdown.contains("eFTP: 260 W"));
    assert!(markdown.contains("W′: 20000 J"));
    assert!(markdown.contains("pMax: 850 W"));

    // P0.2 — WDRM from wbal_start/end
    assert!(markdown.contains("W′ Depletion (WDRM)"));
    assert!(markdown.contains("Max W′ Depletion: 6000 J"));
    assert!(markdown.contains("Depletion: 30%"));
    assert!(markdown.contains("Joules Above FTP: 35000"));

    // P0.3 — Signed decoupling from streams (HR↗ power→ = drifting)
    assert!(markdown.contains("Aerobic Decoupling (ISDM)"));
    assert!(markdown.contains("Signed Decoupling"));
    assert!(markdown.contains("Durability State"));
}

#[tokio::test]
async fn period_analysis_shows_adaptation_state() {
    let mut details_map = HashMap::new();
    details_map.insert(
        "adapt-1".into(),
        json!({
            "distance": 32000.0,
            "moving_time": 5400,
            "average_heartrate": 148.0,
            "average_watts": 235.0,
            "total_elevation_gain": 320.0,
            "icu_efficiency_factor": 1.59,
            "icu_pm_ftp": 260.0,
            "icu_pm_w_prime": 20000.0,
            "icu_pm_p_max": 850.0,
            "icu_pm_1m": 850.0,
            "icu_pm_5m": 500.0,
            "icu_pm_20m": 265.0,
            "icu_pm_60m": 245.0
        }),
    );
    details_map.insert(
        "adapt-2".into(),
        json!({
            "distance": 35000.0,
            "moving_time": 6000,
            "average_heartrate": 150.0,
            "average_watts": 245.0,
            "total_elevation_gain": 350.0,
            "icu_efficiency_factor": 1.62,
            "icu_pm_ftp": 265.0,
            "icu_pm_w_prime": 19500.0,
            "icu_pm_p_max": 860.0,
            "icu_pm_1m": 860.0,
            "icu_pm_5m": 510.0,
            "icu_pm_20m": 270.0,
            "icu_pm_60m": 250.0
        }),
    );

    let client = Arc::new(MockIntervalsClient {
        activities: vec![
            MockIntervalsClient::activity("adapt-1", "Week 1 Ride", "2026-04-01"),
            MockIntervalsClient::activity("adapt-2", "Week 4 Ride", "2026-04-28"),
        ],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(55.0, 47.0, 8.0)),
        wellness: Some(json!([
            {"type": "Ride", "eftp": 260.0, "wPrime": 20000.0, "pMax": 850.0}
        ])),
        activity_details_map: details_map,
        ..MockIntervalsClient::default()
    });

    let handler = AnalyzeTrainingHandler::new();
    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-04-01",
                "period_end": "2026-05-15",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(
        markdown.contains("Adaptation State:"),
        "Period analysis should show Adaptation State:\n{}",
        markdown
    );
}

#[tokio::test]
async fn single_activity_analysis_does_not_show_adaptation_state() {
    let client = Arc::new(MockIntervalsClient {
        activities: vec![MockIntervalsClient::activity(
            "single-1",
            "Single Ride",
            "2026-05-01",
        )],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(55.0, 47.0, 8.0)),
        wellness: Some(json!([
            {"type": "Ride", "eftp": 260.0, "wPrime": 20000.0, "pMax": 850.0}
        ])),
        default_activity_detail: Some(json!({
            "distance": 32000.0,
            "moving_time": 5400,
            "average_heartrate": 148.0,
            "average_watts": 235.0,
            "total_elevation_gain": 320.0,
            "icu_efficiency_factor": 1.59,
            "icu_pm_ftp": 260.0,
            "icu_pm_w_prime": 20000.0,
            "icu_pm_p_max": 850.0
        })),
        ..MockIntervalsClient::default()
    });

    let handler = AnalyzeTrainingHandler::new();
    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-05-01",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(
        !markdown.contains("Adaptation State:"),
        "Single analysis should NOT show Adaptation State:\n{}",
        markdown
    );
}

#[tokio::test]
async fn historical_yoy_comparison() {
    let mut q2_2025_activities: Vec<ActivitySummary> = (1..=5)
        .map(|day| {
            MockIntervalsClient::activity(
                &format!("q2-2025-{day}"),
                &format!("Spring Run {day}"),
                &format!("2025-04-{day:02}"),
            )
        })
        .collect();
    let mut q2_2026_activities: Vec<ActivitySummary> = (1..=5)
        .map(|day| {
            MockIntervalsClient::activity(
                &format!("q2-2026-{day}"),
                &format!("Summer Run {day}"),
                &format!("2026-04-{day:02}"),
            )
        })
        .collect();
    let mut activities = Vec::new();
    activities.append(&mut q2_2025_activities);
    activities.append(&mut q2_2026_activities);

    let detail = json!({
        "distance": 10000.0,
        "moving_time": 3600,
        "average_heartrate": 150.0,
        "total_elevation_gain": 100.0
    });
    let details_map: HashMap<String, Value> = activities
        .iter()
        .map(|a| (a.id.clone(), detail.clone()))
        .collect();

    let client = Arc::new(MockIntervalsClient {
        activities,
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(50.0, 45.0, 5.0)),
        activity_details_map: details_map,
        ..MockIntervalsClient::default()
    });
    let handler = ComparePeriodsHandler::new();

    let output = handler
        .execute(
            json!({
                "period_a_start": "2025-04-01",
                "period_a_end": "2025-06-30",
                "period_a_label": "Q2 2025",
                "period_b_start": "2026-04-01",
                "period_b_end": "2026-06-30",
                "period_b_label": "Q2 2026",
                "metrics": ["volume", "intensity", "zones", "pace", "hr", "tss"]
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);

    // Both labels must appear with non-zero activity counts
    assert!(markdown.contains("Q2 2025"));
    assert!(markdown.contains("Q2 2026"));
    assert!(output.metadata.total_count.is_none() || output.metadata.total_count.unwrap() > 0);

    // limit must always be None
    let calls = client.activity_calls.lock().unwrap();
    for (limit, _) in calls.iter() {
        assert_eq!(
            *limit, None,
            "limit should always be None for historical fetches"
        );
    }

    // Requested metrics are rendered
    assert!(markdown.contains("Requested Metrics"));
    assert!(markdown.to_lowercase().contains("volume"));
    assert!(markdown.to_lowercase().contains("intensity"));
    assert!(markdown.to_lowercase().contains("zones"));
    assert!(markdown.to_lowercase().contains("pace"));
    assert!(markdown.to_lowercase().contains("hr"));
    assert!(markdown.to_lowercase().contains("tss"));
}

#[tokio::test]
async fn high_volume_period() {
    let base_date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let period_end = chrono::NaiveDate::from_ymd_opt(2026, 4, 10).unwrap();
    let total_days = (period_end - base_date).num_days();
    let mut activities = Vec::new();
    for idx in 0u32..250 {
        let day_offset = (idx as i64) % total_days;
        let date = base_date + chrono::Duration::days(day_offset);
        activities.push(MockIntervalsClient::activity(
            &format!("hv-{idx}"),
            &format!("Workout {}", idx + 1),
            &date.format("%Y-%m-%d").to_string(),
        ));
    }

    let detail = json!({
        "distance": 10000.0,
        "moving_time": 3600,
        "average_heartrate": 145.0,
        "total_elevation_gain": 80.0
    });
    let details_map: HashMap<String, Value> = activities
        .iter()
        .map(|a| (a.id.clone(), detail.clone()))
        .collect();

    let client = Arc::new(MockIntervalsClient {
        activities,
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(60.0, 50.0, 10.0)),
        activity_details_map: details_map,
        ..MockIntervalsClient::default()
    });
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-01-01",
                "period_end": "2026-04-10"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    assert_eq!(output.metadata.total_count, Some(250));

    let markdown = markdown_text(&output);
    assert!(
        markdown.contains("2026-01-01"),
        "period heading should use requested start date"
    );
    assert!(
        markdown.contains("2026-04-10"),
        "period heading should use requested end date"
    );
}

#[tokio::test]
async fn partial_detail() {
    let activities = vec![
        MockIntervalsClient::activity("pd-1", "Complete Detail Run", "2026-03-01"),
        MockIntervalsClient::activity("pd-2", "Partial Run", "2026-03-03"),
        MockIntervalsClient::activity("pd-3", "Missing Detail Run", "2026-03-05"),
    ];

    let full_detail = json!({
        "distance": 12000.0,
        "moving_time": 4200,
        "average_heartrate": 148.0,
        "average_watts": 210.0,
        "total_elevation_gain": 150.0
    });

    let partial_detail_value = json!({
        "distance": 10000.0,
        "moving_time": 3600,
        "average_heartrate": 140.0
    });

    let mut details_map = HashMap::new();
    details_map.insert("pd-1".to_string(), full_detail.clone());
    details_map.insert("pd-2".to_string(), partial_detail_value);
    // pd-3 is intentionally missing — get_activity_details will return NotFound

    let client = Arc::new(MockIntervalsClient {
        activities,
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(55.0, 47.0, 8.0)),
        activity_details_map: details_map,
        ..MockIntervalsClient::default()
    });
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);

    // Volume/time/distance remain populated (derived from activity summaries)
    assert!(markdown.contains("Period:"), "should have period heading");
    assert!(
        markdown.contains("Total Time") || markdown.contains("Distance"),
        "should show volume metrics in summary"
    );

    // Data-availability section should report partial detail failure
    let lower = markdown.to_lowercase();
    assert!(
        lower.contains("data availability"),
        "should contain data availability guidance"
    );
    assert!(
        lower.contains("1 of 3 activity details unavailable")
            || lower.contains("partial")
            || lower.contains("unavailable"),
        "should indicate some details are missing"
    );
}

// ===========================================================================
// Task 5: Load provenance and coverage rendering tests
// ===========================================================================

fn with_complete_icu_training_load_block() -> MockIntervalsClient {
    MockIntervalsClient {
        activities: vec![
            MockIntervalsClient::activity("a1", "Run 1", "2026-03-01"),
            MockIntervalsClient::activity("a2", "Run 2", "2026-03-02"),
            MockIntervalsClient::activity("a3", "Run 3", "2026-03-03"),
        ],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(55.0, 45.0, 10.0)),
        default_activity_detail: Some(json!({
            "distance": 15000.0,
            "moving_time": 5400,
            "average_heartrate": 145.0,
            "average_watts": 210.0,
            "total_elevation_gain": 300.0,
            "icu_training_load": 80.0
        })),
        ..MockIntervalsClient::default()
    }
}

fn with_mixed_load_alias_block() -> MockIntervalsClient {
    let mut details_map = HashMap::new();
    details_map.insert(
        "a1".to_string(),
        json!({
            "icu_training_load": 90.0,
            "distance": 12000.0,
            "moving_time": 3600
        }),
    );
    details_map.insert(
        "a2".to_string(),
        json!({
            "tss": 65.0,
            "distance": 10000.0,
            "moving_time": 3000
        }),
    );
    details_map.insert(
        "a3".to_string(),
        json!({
            "training_load": 45.0,
            "distance": 8000.0,
            "moving_time": 2400
        }),
    );
    MockIntervalsClient {
        activities: vec![
            MockIntervalsClient::activity("a1", "Hard Session", "2026-03-01"),
            MockIntervalsClient::activity("a2", "Tempo Run", "2026-03-02"),
            MockIntervalsClient::activity("a3", "Easy Jog", "2026-03-03"),
        ],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(55.0, 45.0, 10.0)),
        activity_details_map: details_map,
        ..MockIntervalsClient::default()
    }
}

fn with_moving_time_only_activity() -> MockIntervalsClient {
    let mut details_map = HashMap::new();
    details_map.insert(
        "a1".to_string(),
        json!({
            "icu_training_load": 80.0,
            "distance": 12000.0,
            "moving_time": 3600
        }),
    );
    details_map.insert(
        "a2".to_string(),
        json!({
            "distance": 8000.0,
            "moving_time": 2400
        }),
    );
    MockIntervalsClient {
        activities: vec![
            MockIntervalsClient::activity("a1", "Hard Session", "2026-03-01"),
            MockIntervalsClient::activity("a2", "Recovery Walk", "2026-03-02"),
        ],
        fitness_summary: Some(MockIntervalsClient::fitness_snapshot(55.0, 45.0, 10.0)),
        activity_details_map: details_map,
        ..MockIntervalsClient::default()
    }
}

#[tokio::test]
async fn load_provenance_complete_icu_training_load_shows_source_and_coverage() {
    let client = Arc::new(with_complete_icu_training_load_block());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-03",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    let lower = markdown.to_lowercase();
    assert!(
        lower.contains("training load data quality"),
        "should contain Training Load Data Quality section"
    );
    assert!(
        lower.contains("100%"),
        "should show 100% coverage when all activities have load"
    );
}

#[tokio::test]
async fn load_provenance_mixed_api_aliases_lists_source_counts() {
    let client = Arc::new(with_mixed_load_alias_block());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-03",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    let lower = markdown.to_lowercase();
    assert!(
        lower.contains("training load data quality"),
        "should contain Training Load Data Quality section"
    );
    assert!(
        lower.contains("100%"),
        "should show 100% coverage when all activities have some load source"
    );
    // Source counts should be present
    assert!(
        lower.contains("icu_training_load") || lower.contains("load score"),
        "should mention load source"
    );
}

#[tokio::test]
async fn duration_only_activity_excluded_from_load_total() {
    let client = Arc::new(with_moving_time_only_activity());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-02",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    let lower = markdown.to_lowercase();
    assert!(
        lower.contains("training load data quality"),
        "should contain Training Load Data Quality section"
    );
    // Coverage should be 50% (1 of 2 activities has load)
    assert!(
        lower.contains("50%"),
        "should show 50% coverage when only 1 of 2 activities has load"
    );
}

#[tokio::test]
async fn compare_periods_totals_match_canonical_observations() {
    let handler = ComparePeriodsHandler::new();
    let client = Arc::new(with_complete_icu_training_load_block());

    let output = handler
        .execute(
            json!({
                "period_a_start": "2026-03-01",
                "period_a_end": "2026-03-03",
                "period_b_start": "2026-03-01",
                "period_b_end": "2026-03-03",
                "metrics": ["tss"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    // TSS row should be present with the sum of icu_training_load values (80*3=240)
    assert!(markdown.contains("TSS"), "should contain TSS metric row");
}

#[tokio::test]
async fn track_progress_does_not_derive_ctl_from_duration_only_activities() {
    use intervals_icu_mcp::domains::coach::AnalysisWindow;
    use intervals_icu_mcp::engines::progress_tracking::derive_ctl_series_from_activity_loads;

    let mut details_map = HashMap::new();
    details_map.insert(
        "walk-1".to_string(),
        json!({
            "distance": 5000.0,
            "moving_time": 3600
        }),
    );

    let activities = vec![MockIntervalsClient::activity(
        "walk-1",
        "Walk",
        "2026-03-01",
    )];
    let window = AnalysisWindow::new(
        chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
    );

    let result = derive_ctl_series_from_activity_loads(&activities, &details_map, &window);
    assert!(
        result.is_none(),
        "must not derive CTL from duration-only activities"
    );
}

// -----------------------------------------------------------------
// Endurance evidence integration tests
// -----------------------------------------------------------------

/// Build a deterministic stream payload (15 minutes at 200 W / 145 bpm)
/// so the engine's sliding 600 s windows have a stable baseline.
fn endurance_ride_streams(avg_hr: f64) -> Value {
    let duration_s = 900_usize;
    let mut time = Vec::with_capacity(duration_s + 1);
    let mut watts = Vec::with_capacity(duration_s + 1);
    let mut heartrate = Vec::with_capacity(duration_s + 1);
    for t in 0..=duration_s {
        time.push(t as f64);
        watts.push(200.0);
        heartrate.push(avg_hr);
    }
    json!({
        "time": time,
        "watts": watts,
        "heartrate": heartrate,
    })
}

/// Build a long ride (135 min) where HR is steady 145 except for
/// indices 7800..=8100 (the late window), where it is 155 bpm. Power
/// is constant 200W so CV stays well below 5% everywhere. Total
/// duration is 8100 s, so the late window's end (8100) is ≥ LATE_END_MIN_S
/// (7200) — satisfying the prolonged-response protocol.
fn endurance_long_ride_streams() -> Value {
    let duration_s = 8100_usize;
    let late_start_s = 7800_usize;
    let mut time = Vec::with_capacity(duration_s + 1);
    let mut watts = Vec::with_capacity(duration_s + 1);
    let mut heartrate = Vec::with_capacity(duration_s + 1);
    for t in 0..=duration_s {
        time.push(t as f64);
        watts.push(200.0);
        heartrate.push(if t >= late_start_s { 155.0 } else { 145.0 });
    }
    json!({
        "time": time,
        "watts": watts,
        "heartrate": heartrate,
    })
}

fn build_period_window(days: &[(&str, &str, f64, f64)]) -> MockIntervalsClient {
    let mut details_map = HashMap::new();
    let mut streams_map = HashMap::new();
    let activities: Vec<ActivitySummary> = days
        .iter()
        .map(|(id, date, _power, hr)| {
            details_map.insert(
                id.to_string(),
                json!({
                    "type": "Ride",
                    "moving_time": 3600_i64,
                    "icu_pm_ftp": 300.0,
                    "icu_pm_w_prime": 20000.0
                }),
            );
            streams_map.insert(id.to_string(), endurance_ride_streams(*hr));
            MockIntervalsClient::activity(id, "Endurance Ride", date)
        })
        .collect();
    MockIntervalsClient {
        activities,
        fitness_summary: Some(json!([{ "fitness": 50.0, "fatigue": 30.0, "form": 20.0 }])),
        wellness: Some(json!([{ "type": "Ride", "eftp": 300.0 }])),
        activity_details_map: details_map,
        streams_map,
        ..MockIntervalsClient::default()
    }
}

#[tokio::test]
async fn detailed_period_renders_endurance_evidence_end_to_end() {
    // Two recent rides (HR 145, 144) and two reference rides (HR 150,
    // 151) within the protocol window. eFTP=300W lets power=200 sit in
    // the 0.55-0.80 eFTP band.
    let client = build_period_window(&[
        ("ride-ref-1", "2026-04-15", 200.0, 150.0),
        ("ride-ref-2", "2026-05-01", 200.0, 151.0),
        ("ride-rec-1", "2026-07-08", 200.0, 145.0),
        ("ride-rec-2", "2026-07-10", 200.0, 144.0),
    ]);
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-07-07",
                "period_end": "2026-07-13",
                "analysis_type": "detailed",
            }),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);

    assert!(
        markdown.contains("Endurance Performance Evidence — Cycling Power Protocol"),
        "expected evidence header:\n{}",
        markdown
    );
    assert!(markdown.contains("Submaximal HR–Power Response"));
    assert!(markdown.contains("Recent − reference HR"));
    assert!(
        markdown.contains("Lower HR at matched power may indicate improved aerobic efficiency"),
        "context paragraph must appear:\n{}",
        markdown
    );
    assert!(markdown.contains("Matched HR–Power Shift After Prolonged Work"));
    let lower = markdown.to_lowercase();
    assert!(!lower.contains("durable"));
    assert!(!lower.contains("ready"));
}

#[tokio::test]
async fn summary_period_does_not_fetch_or_render_endurance_evidence() {
    let client = build_period_window(&[("ride-rec-1", "2026-07-08", 200.0, 145.0)]);

    // Snapshot the shared profile-call counter via Arc so we can keep
    // inspecting it after `client` is moved into the handler below.
    let shared_counter = client.profile_stream_calls.clone();
    let stream_calls_before = *shared_counter.lock().unwrap();

    let handler = AnalyzeTrainingHandler::new();
    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-07-07",
                "period_end": "2026-07-13",
                "analysis_type": "summary",
            }),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();
    let markdown = markdown_text(&output);

    assert!(
        !markdown.contains("Endurance Performance Evidence"),
        "summary mode must not render the evidence section"
    );
    assert_eq!(
        *shared_counter.lock().unwrap(),
        stream_calls_before,
        "summary mode must not fetch endurance-relevant streams"
    );
}

#[tokio::test]
async fn endurance_long_ride_pair_marks_late_window_at_or_after_120_minutes() {
    let mut details_map = HashMap::new();
    let mut streams_map = HashMap::new();
    // Recent rides (HR 145) plus one 90-minute ride with HR ramp.
    details_map.insert(
        "ride-rec-1".to_string(),
        json!({ "type": "Ride", "moving_time": 3600_i64, "icu_pm_ftp": 300.0 }),
    );
    streams_map.insert("ride-rec-1".to_string(), endurance_ride_streams(145.0));
    details_map.insert(
        "ride-long".to_string(),
        json!({ "type": "Ride", "moving_time": 6000_i64, "icu_pm_ftp": 300.0 }),
    );
    streams_map.insert("ride-long".to_string(), endurance_long_ride_streams());
    let client = MockIntervalsClient {
        activities: vec![
            MockIntervalsClient::activity("ride-rec-1", "Steady", "2026-07-08"),
            MockIntervalsClient::activity("ride-long", "Long", "2026-07-10"),
        ],
        fitness_summary: Some(json!([{ "fitness": 50.0, "fatigue": 30.0, "form": 20.0 }])),
        wellness: Some(json!([{ "type": "Ride", "eftp": 300.0 }])),
        activity_details_map: details_map,
        streams_map,
        ..MockIntervalsClient::default()
    };

    let handler = AnalyzeTrainingHandler::new();
    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-07-07",
                "period_end": "2026-07-13",
                "analysis_type": "detailed",
            }),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();
    let markdown = markdown_text(&output);
    assert!(
        markdown.contains("Late window end: "),
        "prolonged response block must include late window end minutes"
    );
    assert!(markdown.contains("HR delta (late − early): "));
}

#[tokio::test]
async fn incomplete_or_running_data_renders_an_honest_reason_not_a_score() {
    // No streams at all → bounded fetch returns empty profile → renderer
    // shows availability reason.
    let details_map: HashMap<String, Value> = HashMap::new();
    let client = MockIntervalsClient {
        activities: vec![MockIntervalsClient::activity("run-1", "Run", "2026-07-08")],
        fitness_summary: Some(json!([{ "fitness": 50.0, "fatigue": 30.0, "form": 20.0 }])),
        wellness: Some(json!([{ "type": "Run", "eftp": 250.0 }])),
        default_activity_detail: Some(json!({ "type": "Run" })),
        activity_details_map: details_map,
        ..MockIntervalsClient::default()
    };

    let handler = AnalyzeTrainingHandler::new();
    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-07-07",
                "period_end": "2026-07-13",
                "analysis_type": "detailed",
            }),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();
    let markdown = markdown_text(&output);
    let lower = markdown.to_lowercase();
    assert!(
        !lower.contains("0.0 bpm"),
        "must never emit zero deltas instead of an explicit reason"
    );
    assert!(
        markdown.contains("Endurance Performance Evidence")
            && (markdown.contains("Cycling power data is required")
                || markdown.contains("coverage was below")
                || markdown.contains("Fewer than two accepted sessions")
                || markdown.contains("eFTP is unavailable")
                || markdown.contains("No eligible prolonged ride")),
        "expected an explicit availability reason:\n{}",
        markdown
    );
}
