use super::*;
use crate::test_support::mock::MockIntervalsClient;
use intervals_icu_client::ActivitySummary;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn execute_returns_markdown_report() {
    let client = MockIntervalsClient::builder()
            .with_wellness(json!([
                {"date": "2026-01-01", "ctl": 60.0, "hrv": 65.0},
                {"date": "2026-01-02", "ctl": 60.2, "hrv": 64.0},
                {"date": "2026-01-03", "ctl": 60.1, "hrv": 63.0},
                {"date": "2026-01-04", "ctl": 60.0, "hrv": 63.0},
                {"date": "2026-01-05", "ctl": 60.1, "hrv": 62.0},
                {"date": "2026-01-06", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-07", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-08", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-09", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-10", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-11", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-12", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-13", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-14", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-15", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-16", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-17", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-18", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-19", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-20", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-21", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-22", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-23", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-24", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-25", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-26", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-27", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-28", "ctl": 60.0, "hrv": 62.0}
            ]))
            .with_activities(vec![
                ActivitySummary {
                    id: "act-1".into(),
                    start_date_local: "2026-01-27".into(),
                    training_load: Some(80),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "act-2".into(),
                    start_date_local: "2026-01-28".into(),
                    training_load: Some(75),
                    ..Default::default()
                },
            ])
            .with_activity_detail("act-1", json!({"icu_zone_times": [{"id": "Z1", "secs": 1800}, {"id": "Z2", "secs": 900}, {"id": "Z3", "secs": 300}], "icu_training_load": 80, "polarization_index": 0.82}))
            .with_activity_detail("act-2", json!({"icu_zone_times": [{"id": "Z1", "secs": 1700}, {"id": "Z2", "secs": 1000}, {"id": "Z3", "secs": 200}], "icu_training_load": 75, "polarization_index": 0.79}));

    let handler = TrackProgressHandler::new();
    let output = handler
        .execute(
            json!({"period_weeks": 4, "hypothesis_mode": true}),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();

    let rendered = format!("{:?}", output.content);
    assert!(rendered.contains("Progress Tracking Report"));
    assert!(rendered.contains("Plateau Detection"));
    assert!(rendered.contains("Load Context"));
    assert!(rendered.contains("HRV Context"));
    assert!(rendered.contains("lnRMSSD 7-day Rollup"));
    assert!(rendered.contains("TID Drift"));
    assert!(rendered.contains("Warnings"));
    assert!(!output.suggestions.is_empty());
    assert!(!output.next_actions.is_empty());
    assert_eq!(output.next_actions.len(), 3);
}

#[tokio::test]
async fn execute_shows_fitness_snapshot_when_available() {
    let client = MockIntervalsClient::builder()
        .with_fitness_summary(json!({
            "fitness": 65.0,
            "fatigue": 45.0,
            "form": 20.0,
            "rampRate": 3.0,
        }))
        .with_wellness(json!([
            {"date": "2026-01-01", "ctl": 60.0, "hrv": 65.0},
            {"date": "2026-01-02", "ctl": 60.0, "hrv": 65.0},
        ]))
        .with_activities(vec![ActivitySummary {
            id: "act-1".into(),
            start_date_local: "2026-01-01".into(),
            training_load: Some(50),
            ..Default::default()
        }]);

    let handler = TrackProgressHandler::new();
    let output = handler
        .execute(
            json!({"period_weeks": 4, "hypothesis_mode": false}),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();

    let rendered = format!("{:?}", output.content);
    assert!(
        rendered.contains("Fitness Snapshot"),
        "should show Fitness Snapshot section"
    );
    assert!(rendered.contains("CTL"), "should show CTL");
    assert!(rendered.contains("ATL"), "should show ATL");
    assert!(
        rendered.contains("Fresh"),
        "should show TSB with Fresh state"
    );
    assert!(rendered.contains("Ramp Rate"), "should show Ramp Rate");
}

fn short_wellness() -> serde_json::Value {
    // 10 days of CTL — under the 28-day plateau threshold, must trigger auto-expand.
    json!(
        (0..10)
            .map(|i| json!({
                "date": format!("2026-01-{:02}", i + 1),
                "fitness": 60.0 + i as f64,
            }))
            .collect::<Vec<_>>()
    )
}

#[tokio::test]
async fn execute_auto_expand_issues_second_wellness_call_with_max_range() {
    let mock = MockIntervalsClient::builder().with_wellness(short_wellness());
    let observations = mock.observations();

    let handler = TrackProgressHandler::new();
    handler
        .execute(
            json!({"period_weeks": 4, "hypothesis_mode": false}),
            Arc::new(mock) as Arc<dyn intervals_icu_client::IntervalsClient>,
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        observations.wellness_call_count(),
        2,
        "expected the handler to issue a second get_wellness call when the first was too short"
    );
    assert_eq!(
        observations.wellness_last_days_back(),
        Some(MAX_WELLNESS_DAYS_FALLBACK),
        "second call should request the maximum wellness window"
    );
}

#[tokio::test]
async fn execute_skips_auto_expand_when_request_already_at_max_weeks() {
    let mock = MockIntervalsClient::builder().with_wellness(short_wellness());
    let observations = mock.observations();

    let handler = TrackProgressHandler::new();
    handler
        .execute(
            // period_weeks=24 hits MAX_PERIOD_WEEKS, so no auto-expand attempt.
            json!({"period_weeks": 24, "hypothesis_mode": false}),
            Arc::new(mock) as Arc<dyn intervals_icu_client::IntervalsClient>,
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        observations.wellness_call_count(),
        1,
        "no second call should be made when the requested window is already at MAX_PERIOD_WEEKS"
    );
    assert_eq!(
        observations.wellness_last_days_back(),
        Some(24 * 7),
        "single call should request exactly the user-supplied period_weeks in days"
    );
}

#[tokio::test]
async fn execute_report_surfaces_actionable_warnings_without_operational_noise() {
    // 10-day wellness triggers auto-expand silently. Even though the second call
    // uses the max range, the mock returns the same 10-point payload so the report
    // should contain actionable warnings about insufficient data, but NOT
    // operational details like "auto-expanded" or "Wellness coverage".
    let mock = MockIntervalsClient::builder().with_wellness(short_wellness());
    let handler = TrackProgressHandler::new();
    let output = handler
        .execute(
            json!({"period_weeks": 4, "hypothesis_mode": false}),
            Arc::new(mock) as Arc<dyn intervals_icu_client::IntervalsClient>,
            None,
        )
        .await
        .unwrap();

    let rendered = format!("{:?}", output.content);
    assert!(
        !rendered.contains("auto-expanded"),
        "rendered output should NOT contain operational details; got: {rendered}"
    );
    assert!(
        !rendered.contains("Wellness coverage"),
        "rendered output should NOT contain coverage percentage; got: {rendered}"
    );
    assert!(
        !rendered.contains("re-queried"),
        "rendered output should NOT contain retry details; got: {rendered}"
    );
    // Should still have actionable warnings about what data is missing.
    assert!(
        rendered.contains("lnRMSSD") || rendered.contains("Plateau"),
        "rendered output should contain actionable data-availability warnings; got: {rendered}"
    );
}

#[tokio::test]
async fn execute_derives_ctl_history_from_training_load_when_wellness_is_empty() {
    let today = Utc::now().date_naive();
    let activities = (0..42)
        .map(|days_ago| ActivitySummary {
            id: format!("activity-{days_ago}"),
            start_date_local: (today - Duration::days(days_ago)).to_string(),
            training_load: Some(50),
            ..Default::default()
        })
        .collect();
    let client = MockIntervalsClient::builder()
        .with_wellness(json!([]))
        .with_activities(activities);

    let output = TrackProgressHandler::new()
        .execute(
            json!({"period_weeks": 4, "hypothesis_mode": false}),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();

    let rendered = format!("{:?}", output.content);
    assert!(
        rendered.contains("activity training-load history"),
        "expected CTL fallback provenance, got: {rendered}"
    );
    assert!(
        !rendered.contains("Plateau detection unavailable because CTL history is insufficient"),
        "activity history should make plateau detection available, got: {rendered}"
    );
    assert!(
        rendered.contains("Wellness HRV history unavailable"),
        "HRV must remain unavailable when wellness is genuinely empty, got: {rendered}"
    );
}

// ========================================================================
// Task 3: Description Routing Constraint Tests
// ========================================================================

#[test]
fn test_description_mentions_trailing_window_only() {
    let handler = TrackProgressHandler::new();
    let desc = IntentHandler::description(&handler);
    assert!(
        desc.contains("one trailing"),
        "Description should specify single trailing window, got: {}",
        desc
    );
}

#[test]
fn test_description_mentions_compare_periods_for_yoy() {
    let desc = TrackProgressHandler::new().description();
    assert!(
        desc.contains("compare_periods"),
        "Description should route YoY/two non-contiguous periods to compare_periods, got: {}",
        desc
    );
    assert!(
        desc.contains("Do NOT use for YoY"),
        "Description should explicitly forbid YoY usage, got: {}",
        desc
    );
}

/// Regression test (F-13): an empty-activity period must still produce
/// a usable report — the handler should surface "no data" sections,
/// not crash on empty wellness/activities.
#[tokio::test]
async fn execute_with_empty_activities_returns_report() {
    let client = MockIntervalsClient::builder()
        .with_wellness(json!([
            {"date": "2026-01-01", "ctl": 60.0, "hrv": 65.0},
            {"date": "2026-01-02", "ctl": 60.2, "hrv": 64.0},
            {"date": "2026-01-03", "ctl": 60.1, "hrv": 63.0},
        ]))
        .with_activities(Vec::<ActivitySummary>::new());
    let handler = TrackProgressHandler::new();
    let output = handler
        .execute(
            json!({"period_weeks": 4, "hypothesis_mode": false}),
            Arc::new(client),
            None,
        )
        .await;
    assert!(
        output.is_ok(),
        "expected Ok with empty activities, got: {output:?}"
    );
}

/// Regression test (F-13): `period_weeks` outside [MIN_PERIOD_WEEKS, MAX_PERIOD_WEEKS]
/// must be rejected with a validation error. Prior code silently accepted
/// any u64 and produced nonsensical windows (negative day arithmetic, etc.).
#[tokio::test]
async fn execute_rejects_period_weeks_out_of_range() {
    let client = MockIntervalsClient::builder();
    let handler = TrackProgressHandler::new();
    // way above MAX_PERIOD_WEEKS=24
    let result = handler
        .execute(json!({"period_weeks": 1000}), Arc::new(client), None)
        .await;
    assert!(
        result.is_err(),
        "expected validation error for period_weeks=1000, got: {result:?}"
    );
    // way below MIN_PERIOD_WEEKS=4
    let result = handler
        .execute(
            json!({"period_weeks": 1}),
            Arc::new(MockIntervalsClient::default()),
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "expected validation error for period_weeks=1, got: {result:?}"
    );
}
