use super::*;
use crate::domains::load::{LoadObservation, LoadSource};
use crate::engines::shared::{parse_activity_date, parse_event_date};
use crate::test_support::mock::MockIntervalsClient;
use chrono::{Duration, NaiveDate};
use intervals_icu_client::{IntervalsClient, IntervalsError};
use serde_json::json;

// ── Helpers ───────────────────────────────────────────────────────────

fn activity(id: &str, date: &str) -> ActivitySummary {
    ActivitySummary {
        id: id.to_string(),
        name: Some(format!("Activity {}", id)),
        start_date_local: date.to_string(),
        ..Default::default()
    }
}

// ── Load module tests ─────────────────────────────────────────────────

#[test]
fn previous_window_matches_current_window_length() {
    let current = AnalysisWindow::new(
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
    );
    let previous = build_previous_window(&current);

    assert_eq!(previous.window_days(), current.window_days());
    assert_eq!(previous.end_date, current.start_date.pred_opt().unwrap());
}

#[test]
fn daily_load_series_fills_missing_days_with_zero() {
    let window = AnalysisWindow::new(
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 4).unwrap(),
    );
    let activities = [activity("a1", "2026-03-01"), activity("a2", "2026-03-03")];
    let refs = activities.iter().collect::<Vec<_>>();
    let details = HashMap::from([
        ("a1".to_string(), json!({"icu_training_load": 50.0})),
        ("a2".to_string(), json!({"icu_training_load": 70.0})),
    ]);

    let series = build_daily_load_series(&refs, &details, &window);

    assert_eq!(series.daily.len(), 4);
    assert_eq!(series.daily[0].1, 50.0);
    assert_eq!(series.daily[1].1, 0.0);
    assert_eq!(series.daily[2].1, 70.0);
    assert_eq!(series.daily[3].1, 0.0);
    assert_eq!(series.activities_total, 2);
    assert_eq!(series.activities_with_load, 2);
}

#[test]
fn load_extraction_never_uses_moving_time_as_load() {
    let detail = json!({"moving_time": 5400});
    assert_eq!(extract_activity_load(Some(&detail)), None);
}

#[test]
fn load_extraction_records_exact_alias_provenance() {
    assert_eq!(
        extract_activity_load(Some(&json!({"tss": 73.0}))),
        Some(LoadObservation {
            value: 73.0,
            source: LoadSource::TssAlias
        })
    );
}

#[test]
fn load_extraction_records_icu_training_load_provenance() {
    assert_eq!(
        extract_activity_load(Some(&json!({"icu_training_load": 88.0}))),
        Some(LoadObservation {
            value: 88.0,
            source: LoadSource::IcuTrainingLoad
        })
    );
}

#[test]
fn load_extraction_records_training_load_alias_provenance() {
    assert_eq!(
        extract_activity_load(Some(&json!({"training_load": 65.0}))),
        Some(LoadObservation {
            value: 65.0,
            source: LoadSource::TrainingLoadAlias
        })
    );
}

#[test]
fn load_extraction_records_icu_training_load_camel_case_provenance() {
    assert_eq!(
        extract_activity_load(Some(&json!({"icuTrainingLoad": 72.0}))),
        Some(LoadObservation {
            value: 72.0,
            source: LoadSource::TrainingLoadAlias
        })
    );
}

#[test]
fn load_extraction_prefers_canonical_over_aliases() {
    let detail = json!({
        "icu_training_load": 88.0,
        "training_load": 65.0,
        "tss": 73.0
    });
    assert_eq!(
        extract_activity_load(Some(&detail)),
        Some(LoadObservation {
            value: 88.0,
            source: LoadSource::IcuTrainingLoad
        })
    );
}

#[test]
fn activity_load_records_summary_training_load_provenance() {
    let activity = ActivitySummary {
        id: "a1".to_string(),
        training_load: Some(42),
        ..Default::default()
    };
    assert_eq!(
        activity_load(&activity, None),
        Some(LoadObservation {
            value: 42.0,
            source: LoadSource::ActivitySummaryTrainingLoad
        })
    );
}

#[test]
fn daily_series_separates_rest_days_from_missing_load_coverage() {
    let window = AnalysisWindow::new(
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
    );
    let activities = [activity("a1", "2026-03-01"), activity("a2", "2026-03-02")];
    let refs = activities.iter().collect::<Vec<_>>();
    let details = HashMap::from([("a1".to_string(), json!({"icu_training_load": 50.0}))]);

    let series = build_daily_load_series(&refs, &details, &window);

    assert_eq!(series.daily.len(), 3);
    assert_eq!(series.daily[0].1, 50.0);
    assert_eq!(series.daily[1].1, 0.0);
    assert_eq!(series.daily[2].1, 0.0);
    assert_eq!(series.activities_total, 2);
    assert_eq!(series.activities_with_load, 1);
    assert_eq!(series.coverage_ratio(), Some(0.5));
}

#[test]
fn daily_load_series_aggregates_multiple_activities_on_same_day() {
    let window = AnalysisWindow::new(
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 2).unwrap(),
    );
    let activities = [
        activity("a1", "2026-03-01T07:00:00"),
        activity("a2", "2026-03-01T18:00:00"),
    ];
    let refs = activities.iter().collect::<Vec<_>>();
    let details = HashMap::from([
        ("a1".to_string(), json!({"icu_training_load": 35.0})),
        ("a2".to_string(), json!({"icu_training_load": 40.0})),
    ]);

    let series = build_daily_load_series(&refs, &details, &window);

    assert_eq!(series.daily[0].1, 75.0);
    assert_eq!(series.daily[1].1, 0.0);
}

// ── Decode module tests ───────────────────────────────────────────────

#[test]
fn normalize_upcoming_events_payload_backfills_missing_name() {
    let payload = json!([
        {
            "id": 94131802,
            "category": "WORKOUT",
            "start_date_local": "2026-03-21T00:00:00",
            "description": "Recovery Run Z1"
        },
        {
            "id": 94131803,
            "category": "SICK",
            "start_date_local": "2026-03-22T00:00:00"
        }
    ]);

    let normalized = normalize_upcoming_events_payload(payload);
    let events: Vec<Event> =
        serde_json::from_value(normalized).expect("normalized payload should decode");

    assert_eq!(events[0].name, "Recovery Run Z1");
    assert_eq!(events[1].name, "SICK");
}

#[test]
fn normalize_intervals_payload_extracts_icu_intervals_array() {
    let payload = json!({
        "id": "i126027814",
        "icu_intervals": [
            {"moving_time": 300, "average_heartrate": 142},
            {"moving_time": 360, "average_heartrate": 158}
        ],
        "icu_groups": [
            {"moving_time": 300, "count": 6}
        ]
    });

    let normalized = normalize_intervals_payload(payload);
    let intervals = normalized
        .as_array()
        .expect("interval payload should normalize to array");

    assert_eq!(intervals.len(), 2);
    assert_eq!(
        intervals[0].get("moving_time").and_then(Value::as_i64),
        Some(300)
    );
}

#[test]
fn normalize_intervals_payload_falls_back_to_icu_groups_when_needed() {
    let payload = json!({
        "id": "i126027814",
        "icu_groups": [
            {"moving_time": 300, "count": 6}
        ]
    });

    let normalized = normalize_intervals_payload(payload);
    let groups = normalized
        .as_array()
        .expect("group payload should normalize to array");

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].get("count").and_then(Value::as_i64), Some(6));
}

#[test]
fn normalize_streams_payload_extracts_nested_stream_map() {
    let payload = json!({
        "streams": {
            "heartrate": [140, 145, 150],
            "watts": [220, 230, 240]
        }
    });

    let normalized = normalize_streams_payload(payload);
    let object = normalized
        .as_object()
        .expect("stream payload should normalize to object map");

    assert!(object.contains_key("heartrate"));
    assert!(object.contains_key("watts"));
}

#[test]
fn normalize_streams_payload_extracts_descriptor_array() {
    let payload = json!({
        "streams": [
            {"type": "heartrate", "data": [140, 145, 150]},
            {"type": "watts", "data": [220, 230, 240]}
        ]
    });

    let normalized = normalize_streams_payload(payload);
    let object = normalized
        .as_object()
        .expect("descriptor array should normalize to object map");

    assert_eq!(
        object
            .get("heartrate")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        object.get("watts").and_then(Value::as_array).map(Vec::len),
        Some(3)
    );
}

#[test]
fn normalize_upcoming_events_payload_non_array_passthrough() {
    let payload = json!({"not": "array"});
    let result = normalize_upcoming_events_payload(payload.clone());
    assert_eq!(result, payload);
}

#[test]
fn normalize_intervals_payload_passthrough_when_already_array() {
    let payload = json!([{"moving_time": 300}]);
    let result = normalize_intervals_payload(payload.clone());
    assert_eq!(result, payload);
}

#[test]
fn normalize_streams_payload_no_streams_key_passthrough() {
    let payload = json!({
        "other": "field"
    });
    let result = normalize_streams_payload(payload.clone());
    assert_eq!(result, payload);
}

#[test]
fn normalize_streams_payload_streams_non_array_passthrough() {
    let payload = json!({
        "streams": "not an array"
    });
    let result = normalize_streams_payload(payload.clone());
    assert_eq!(result, payload);
}

// ── Extract Activity Load Tests ───────────────────────────────────────

#[test]
fn extract_activity_load_none_input() {
    assert_eq!(extract_activity_load(None), None);
}

#[test]
fn extract_activity_load_non_object() {
    let detail = json!("not an object");
    assert_eq!(extract_activity_load(Some(&detail)), None);
}

#[test]
fn extract_activity_load_no_load_or_time() {
    let detail = json!({"distance": 10000});
    assert_eq!(extract_activity_load(Some(&detail)), None);
}

#[test]
fn extract_activity_load_alternate_load_field_names() {
    let detail = json!({"training_load": 75.0});
    assert_eq!(
        extract_activity_load(Some(&detail)),
        Some(LoadObservation {
            value: 75.0,
            source: LoadSource::TrainingLoadAlias
        })
    );
}

#[test]
fn extract_activity_load_camelcase_load_field() {
    let detail = json!({"icuTrainingLoad": 88.0});
    assert_eq!(
        extract_activity_load(Some(&detail)),
        Some(LoadObservation {
            value: 88.0,
            source: LoadSource::TrainingLoadAlias
        })
    );
}

#[test]
fn extract_activity_load_integer_load() {
    let detail = json!({"icu_training_load": 90});
    assert_eq!(
        extract_activity_load(Some(&detail)),
        Some(LoadObservation {
            value: 90.0,
            source: LoadSource::IcuTrainingLoad
        })
    );
}

#[test]
fn extract_activity_load_moving_time_returns_none() {
    let detail = json!({"moving_time": 7200});
    assert_eq!(extract_activity_load(Some(&detail)), None);
}

// ── Build Previous Window Tests ───────────────────────────────────────

#[test]
fn build_previous_window_correct_length() {
    let current = AnalysisWindow::new(
        NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 14).unwrap(),
    );
    let previous = build_previous_window(&current);
    assert_eq!(previous.window_days(), current.window_days());
}

#[test]
fn build_previous_window_adjacent_to_current() {
    let current = AnalysisWindow::new(
        NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 14).unwrap(),
    );
    let previous = build_previous_window(&current);
    assert_eq!(previous.end_date, current.start_date.pred_opt().unwrap());
}

#[test]
fn build_previous_window_7_day_example() {
    let current = AnalysisWindow::new(
        NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 14).unwrap(),
    );
    let previous = build_previous_window(&current);
    assert_eq!(
        previous.start_date,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
    );
    assert_eq!(
        previous.end_date,
        NaiveDate::from_ymd_opt(2026, 3, 7).unwrap()
    );
}

// ── Parse Planned Workout Tests ───────────────────────────────────────

#[test]
fn parse_planned_workout_with_paired_activity_id_excluded() {
    let known_ids = std::collections::HashSet::from(["a123".to_string()]);
    let event = json!({
        "id": 1,
        "start_date_local": "2026-03-01",
        "description": "Planned",
        "paired_activity_id": "a123"
    });
    assert!(parse_planned_workout(&event, &known_ids).is_none());
}

#[test]
fn parse_planned_workout_missing_id() {
    let known_ids = std::collections::HashSet::new();
    let event = json!({
        "start_date_local": "2026-03-01",
        "description": "Planned"
    });
    assert!(parse_planned_workout(&event, &known_ids).is_none());
}

#[test]
fn parse_planned_workout_missing_start_date() {
    let known_ids = std::collections::HashSet::new();
    let event = json!({
        "id": 1,
        "description": "Planned"
    });
    assert!(parse_planned_workout(&event, &known_ids).is_none());
}

#[test]
fn parse_planned_workout_no_description_uses_fallback() {
    let known_ids = std::collections::HashSet::new();
    let event = json!({
        "id": 1,
        "start_date_local": "2026-03-01"
    });
    let result = parse_planned_workout(&event, &known_ids);
    assert!(result.is_some());
    let (activity, _) = result.unwrap();
    assert_eq!(activity.name, Some("Planned workout".to_string()));
}

#[test]
fn parse_planned_workout_uses_name_if_description_missing() {
    let known_ids = std::collections::HashSet::new();
    let event = json!({
        "id": 1,
        "start_date_local": "2026-03-01",
        "name": "Named Workout"
    });
    let result = parse_planned_workout(&event, &known_ids);
    assert!(result.is_some());
    let (activity, _) = result.unwrap();
    assert_eq!(activity.name, Some("Named Workout".to_string()));
}

#[test]
fn parse_planned_workout_string_id() {
    let known_ids = std::collections::HashSet::new();
    let event = json!({
        "id": "event_1",
        "start_date_local": "2026-03-01",
        "description": "Planned"
    });
    let result = parse_planned_workout(&event, &known_ids);
    assert!(result.is_some());
    let (activity, _) = result.unwrap();
    assert_eq!(activity.id, "event:event_1");
}

// ── Fetch module tests (using MockIntervalsClient) ────────────────────

#[tokio::test]
async fn fetch_period_data_reuses_single_upcoming_fetch_for_calendar_and_planned_workouts() {
    let today = chrono::Utc::now().date_naive();
    let client = MockIntervalsClient::builder()
        .with_activities(vec![activity("a1", &today.to_string())])
        .with_upcoming_workouts(json!([
            {
                "id": 11,
                "category": "WORKOUT",
                "start_date_local": (today + Duration::days(1)).to_string(),
                "description": "Planned threshold session"
            },
            {
                "id": 12,
                "category": "NOTE",
                "start_date_local": (today + Duration::days(1)).to_string(),
                "description": "Coach note"
            }
        ]));

    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(today - Duration::days(2), today + Duration::days(2)),
        include_activity_details: false,
        include_comparison_window: false,
        include_endurance_evidence: false,
    };

    let fetched = fetch_period_data(&client as &dyn IntervalsClient, &request)
        .await
        .expect("period fetch succeeds");

    assert_eq!(client.upcoming_workouts_call_count(), 1);
    assert_eq!(fetched.calendar_events.len(), 2);
    assert!(
        fetched
            .activities
            .iter()
            .any(|activity| activity.id == "event:11")
    );
}

#[tokio::test]
async fn period_fetch_retains_all_in_window_activities() {
    let today = chrono::Utc::now().date_naive();
    let required_start = today - Duration::days(459);
    let required_end = required_start + Duration::days(90);

    let mut activities_in_window = (0..250)
        .map(|i| {
            let day_offset = (i as i64) % 91;
            let date = required_start + Duration::days(day_offset);
            activity(&format!("in_{i}"), &date.to_string())
        })
        .collect::<Vec<_>>();
    let activities_outside = (0..10)
        .map(|i| {
            let date = required_start - Duration::days(i + 1);
            activity(&format!("out_{i}"), &date.to_string())
        })
        .collect::<Vec<_>>();
    activities_in_window.extend(activities_outside);

    let client = MockIntervalsClient::builder().with_activities(activities_in_window);
    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(required_start, required_end),
        include_activity_details: false,
        include_comparison_window: false,
        include_endurance_evidence: false,
    };

    let fetched = fetch_period_data(&client as &dyn IntervalsClient, &request)
        .await
        .unwrap();

    assert_eq!(fetched.activities.len(), 250);
    assert!(fetched.activities.iter().all(|a| {
        parse_activity_date(&a.start_date_local)
            .is_some_and(|date| date >= required_start && date <= required_end)
    }));
}

#[tokio::test]
async fn period_fetch_requests_complete_historical_activity_list() {
    let today = chrono::Utc::now().date_naive();
    let start = today - Duration::days(459);
    let client = MockIntervalsClient::builder().with_activities(Vec::new());
    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(start, start + Duration::days(90)),
        include_activity_details: false,
        include_comparison_window: false,
        include_endurance_evidence: false,
    };

    fetch_period_data(&client as &dyn IntervalsClient, &request)
        .await
        .unwrap();

    assert_eq!(client.activity_calls_snapshot(), vec![(None, Some(459))]);
}

#[tokio::test]
async fn period_fetch_limits_details_to_required_window() {
    let today = chrono::Utc::now().date_naive();
    let window_start = today - Duration::days(30);
    let window_end = today - Duration::days(1);

    let activities = vec![
        activity("before", &(window_start - Duration::days(5)).to_string()),
        activity("inside", &window_start.to_string()),
        activity("after", &(window_end + Duration::days(5)).to_string()),
    ];

    let client = MockIntervalsClient::builder().with_activities(activities);
    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(window_start, window_end),
        include_activity_details: true,
        include_comparison_window: false,
        include_endurance_evidence: false,
    };

    let fetched = fetch_period_data(&client as &dyn IntervalsClient, &request)
        .await
        .expect("fetch succeeds");

    assert_eq!(fetched.activities.len(), 1);
    assert_eq!(fetched.activities[0].id, "inside");
    assert_eq!(client.requested_activity_detail_ids(), vec!["inside"]);
}

#[tokio::test]
async fn period_fetch_reports_partial_detail_coverage() {
    let today = chrono::Utc::now().date_naive();
    let window_start = today - Duration::days(30);
    let window_end = today - Duration::days(1);

    let activities = vec![
        activity("ok", &window_start.to_string()),
        activity("fail", &(window_start + Duration::days(1)).to_string()),
    ];

    let client = MockIntervalsClient::builder()
        .with_activities(activities)
        .with_failing_activity_details(vec!["fail"]);
    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(window_start, window_end),
        include_activity_details: true,
        include_comparison_window: false,
        include_endurance_evidence: false,
    };

    let fetched = fetch_period_data(&client as &dyn IntervalsClient, &request)
        .await
        .expect("fetch succeeds");

    assert_eq!(fetched.activities.len(), 2);
    assert!(fetched.activity_details.contains_key("ok"));
    assert!(!fetched.activity_details.contains_key("fail"));
    assert!(fetched.fetch_warnings.iter().any(|warning| {
        warning.contains("1 of 2 activity details unavailable")
            && warning.contains("period totals remain available")
    }));
}

#[tokio::test]
async fn fetch_period_data_degrades_gracefully_when_upcoming_workouts_are_rate_limited() {
    let today = chrono::Utc::now().date_naive();
    let client = MockIntervalsClient::builder()
        .with_activities(vec![activity("a1", &today.to_string())])
        .with_upcoming_workouts_error(IntervalsError::from_status(429, "rate limited"));

    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(today - Duration::days(2), today + Duration::days(2)),
        include_activity_details: false,
        include_comparison_window: false,
        include_endurance_evidence: false,
    };

    let fetched = fetch_period_data(&client as &dyn IntervalsClient, &request)
        .await
        .expect("period fetch degrades");

    assert_eq!(client.upcoming_workouts_call_count(), 1);
    assert!(
        fetched
            .fetch_warnings
            .iter()
            .any(|warning| warning.contains("rate limiting"))
    );
    assert_eq!(fetched.activities.len(), 1);
}

// ── Request/response type tests ───────────────────────────────────────

#[test]
fn historical_period_lookback_is_anchored_to_today() {
    let today = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap();
    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2025, 4, 1).unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
        ),
        include_activity_details: false,
        include_comparison_window: false,
        include_endurance_evidence: false,
    };

    let (start, end) = required_activity_window(&request);
    assert_eq!(start, NaiveDate::from_ymd_opt(2025, 4, 1).unwrap());
    assert_eq!(end, NaiveDate::from_ymd_opt(2025, 6, 30).unwrap());
    assert_eq!(activity_lookback_days(start, today).unwrap(), 459);
}

#[test]
fn comparison_fetch_includes_the_full_previous_window() {
    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        ),
        include_activity_details: false,
        include_comparison_window: true,
        include_endurance_evidence: false,
    };

    let (start, end) = required_activity_window(&request);
    assert_eq!(start, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    assert_eq!(end, NaiveDate::from_ymd_opt(2026, 6, 30).unwrap());
}

#[test]
fn period_fetch_request_clone() {
    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
        ),
        include_activity_details: true,
        include_comparison_window: false,
        include_endurance_evidence: false,
    };
    let _cloned = request.clone();
}

#[test]
fn period_fetch_request_debug() {
    let request = PeriodFetchRequest {
        window: AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
        ),
        include_activity_details: true,
        include_comparison_window: false,
        include_endurance_evidence: false,
    };
    let debug_str = format!("{:?}", request);
    assert!(debug_str.contains("PeriodFetchRequest"));
}

#[test]
fn single_workout_fetch_request_clone() {
    let request = SingleWorkoutFetchRequest {
        activity_id: "a123".to_string(),
        include_intervals: true,
        include_streams: true,
        include_best_efforts: false,
        include_hr_histogram: true,
        include_power_histogram: true,
        include_pace_histogram: false,
    };
    let _cloned = request.clone();
}

#[test]
fn single_workout_fetch_request_debug() {
    let request = SingleWorkoutFetchRequest {
        activity_id: "a123".to_string(),
        include_intervals: true,
        include_streams: false,
        include_best_efforts: false,
        include_hr_histogram: false,
        include_power_histogram: false,
        include_pace_histogram: false,
    };
    let debug_str = format!("{:?}", request);
    assert!(debug_str.contains("SingleWorkoutFetchRequest"));
}

#[test]
fn recovery_fetch_request_clone() {
    let request = RecoveryFetchRequest {
        period_days: 7,
        include_wellness: true,
    };
    let _cloned = request.clone();
}

#[test]
fn recovery_fetch_request_debug() {
    let request = RecoveryFetchRequest {
        period_days: 14,
        include_wellness: false,
    };
    let debug_str = format!("{:?}", request);
    assert!(debug_str.contains("RecoveryFetchRequest"));
}

#[test]
fn race_fetch_request_clone() {
    let request = RaceFetchRequest {
        activity_id: "r123".to_string(),
        include_intervals: true,
        include_streams: false,
    };
    let _cloned = request.clone();
}

#[test]
fn race_fetch_request_debug() {
    let request = RaceFetchRequest {
        activity_id: "r123".to_string(),
        include_intervals: false,
        include_streams: true,
    };
    let debug_str = format!("{:?}", request);
    assert!(debug_str.contains("RaceFetchRequest"));
}

#[test]
fn fetched_analysis_data_default() {
    let data = FetchedAnalysisData::default();
    assert!(data.activities.is_empty());
    assert!(data.comparison_activities.is_empty());
    assert!(data.calendar_events.is_empty());
    assert!(data.activity_messages.is_empty());
    assert!(data.activity_details.is_empty());
    assert!(data.workout_detail.is_none());
    assert!(data.fitness.is_none());
    assert!(data.wellness.is_none());
    assert!(data.intervals.is_none());
    assert!(data.streams.is_none());
    assert!(data.best_efforts.is_none());
    assert!(data.hr_histogram.is_none());
    assert!(data.power_histogram.is_none());
    assert!(data.pace_histogram.is_none());
}

#[test]
fn fetched_analysis_data_clone() {
    let data = FetchedAnalysisData {
        activities: vec![ActivitySummary {
            id: "a1".to_string(),
            name: Some("Test".to_string()),
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let _cloned = data.clone();
}

#[test]
fn fetched_analysis_data_debug() {
    let data = FetchedAnalysisData::default();
    let debug_str = format!("{:?}", data);
    assert!(debug_str.contains("FetchedAnalysisData"));
}

// ── Parse Activity Date Tests ─────────────────────────────────────────

#[test]
fn parse_activity_date_with_time() {
    let result = parse_activity_date("2026-03-01T10:30:00");
    assert!(result.is_some());
    assert_eq!(
        result.unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
    );
}

#[test]
fn parse_activity_date_without_time() {
    let result = parse_activity_date("2026-03-01");
    assert!(result.is_some());
    assert_eq!(
        result.unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
    );
}

#[test]
fn parse_activity_date_invalid_format() {
    assert!(parse_activity_date("invalid").is_none());
    assert!(parse_activity_date("01-03-2026").is_none());
    assert!(parse_activity_date("").is_none());
}

// ── Parse Event Date Tests ────────────────────────────────────────────

#[test]
fn parse_event_date_with_time() {
    let result = parse_event_date("2026-03-01T10:30:00");
    assert!(result.is_some());
    assert_eq!(
        result.unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
    );
}

#[test]
fn parse_event_date_without_time() {
    let result = parse_event_date("2026-03-01");
    assert!(result.is_some());
    assert_eq!(
        result.unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
    );
}

#[test]
fn parse_event_date_invalid() {
    assert!(parse_event_date("not-a-date").is_none());
}
