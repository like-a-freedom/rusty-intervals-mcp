use super::*;
use crate::test_support::mock::MockIntervalsClient;
use intervals_icu_client::{ActivitySummary, IntervalsError};
use std::sync::Arc;

fn make_activity(id: &str, date: &str, name: &str) -> ActivitySummary {
    ActivitySummary {
        id: id.to_string(),
        name: Some(name.to_string()),
        start_date_local: date.to_string(),
        ..Default::default()
    }
}

fn make_client_with_activity(
    id: &str,
    date: &str,
    name: &str,
    detail: serde_json::Value,
) -> MockIntervalsClient {
    MockIntervalsClient::builder()
        .with_activities(vec![make_activity(id, date, name)])
        .with_activity_detail(id, detail)
        .with_streams(json!({}))
        .with_intervals(json!({}))
}

fn make_minimal_client(id: &str, date: &str, name: &str) -> MockIntervalsClient {
    make_client_with_activity(id, date, name, json!({}))
}

#[test]
fn test_new_handler() {
    let handler = AnalyzeRaceHandler::new();
    assert_eq!(handler.name(), "analyze_race");
}

#[test]
fn test_default_handler() {
    let _handler = AnalyzeRaceHandler;
}

#[test]
fn test_name() {
    let handler = AnalyzeRaceHandler::new();
    assert_eq!(IntentHandler::name(&handler), "analyze_race");
}

#[test]
fn test_description() {
    let handler = AnalyzeRaceHandler::new();
    let desc = IntentHandler::description(&handler);
    assert!(desc.contains("Post-race analysis"));
    assert!(desc.contains("results"));
    assert!(desc.contains("strategy"));
}

#[test]
fn test_input_schema_structure() {
    let handler = AnalyzeRaceHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(props.contains_key("date"));
    assert!(props.contains_key("target_date"));
    assert!(props.contains_key("description_contains"));
    assert!(props.contains_key("analysis_type"));
    assert!(props.contains_key("compare_to_planned"));

    let analysis_type = props.get("analysis_type").unwrap();
    let analysis_enum = analysis_type.get("enum").unwrap().as_array().unwrap();
    assert!(analysis_enum.contains(&json!("performance")));
    assert!(analysis_enum.contains(&json!("strategy")));
    assert!(analysis_enum.contains(&json!("recovery")));
}

#[test]
fn test_requires_idempotency_token() {
    let handler = AnalyzeRaceHandler::new();
    assert!(!IntentHandler::requires_idempotency_token(&handler));
}

#[test]
fn test_default_values() {
    let input = json!({});

    let analysis_type = input
        .get("analysis_type")
        .and_then(|v| v.as_str())
        .unwrap_or("performance");
    let compare = input
        .get("compare_to_planned")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    assert_eq!(analysis_type, "performance");
    assert!(compare);
}

#[test]
fn test_analysis_type_values() {
    let valid_types = ["performance", "strategy", "recovery"];
    for t in &valid_types {
        assert!(["performance", "strategy", "recovery"].contains(t));
    }
}

#[test]
fn test_date_values() {
    let valid_dates = ["2026-03-01", "last_race"];
    for date in &valid_dates {
        assert!(!date.is_empty());
    }
}

#[test]
fn test_description_filter() {
    let input = json!({
        "description_contains": "marathon"
    });

    let desc = input.get("description_contains").and_then(|v| v.as_str());
    assert_eq!(desc, Some("marathon"));
}

#[test]
fn test_target_date_alias_falls_back_when_date_missing() {
    let input = json!({
        "target_date": "2026-03-21"
    });

    let date = input
        .get("date")
        .and_then(Value::as_str)
        .or_else(|| input.get("target_date").and_then(Value::as_str));

    assert_eq!(date, Some("2026-03-21"));
}

#[test]
fn test_parse_requested_date_supports_today_alias() {
    let parsed = AnalyzeRaceHandler::parse_requested_date("today").unwrap();

    assert_eq!(parsed, chrono::Local::now().date_naive());
}

#[test]
fn test_race_search_logic() {
    let activities = ["Race 5K", "Training Run", "Marathon Race", "Easy Run"];
    let search_term = "marathon";

    let matches: Vec<_> = activities
        .iter()
        .filter(|a| a.to_lowercase().contains(&search_term.to_lowercase()))
        .collect();

    assert_eq!(matches.len(), 1);
    assert_eq!(*matches[0], "Marathon Race");
}

#[test]
fn test_looks_like_race_keywords() {
    use super::AnalyzeRaceHandler;
    assert!(AnalyzeRaceHandler::looks_like_race(Some("City Marathon")));
    assert!(AnalyzeRaceHandler::looks_like_race(Some("Half Marathon")));
    assert!(AnalyzeRaceHandler::looks_like_race(Some("5K Race")));
    assert!(AnalyzeRaceHandler::looks_like_race(Some("10K")));
    assert!(AnalyzeRaceHandler::looks_like_race(Some("Ultra 100K")));
    assert!(AnalyzeRaceHandler::looks_like_race(Some(
        "Sprint Triathlon"
    )));
    assert!(AnalyzeRaceHandler::looks_like_race(Some("CCC Trail")));
    assert!(AnalyzeRaceHandler::looks_like_race(Some("Trail Running")));
    assert!(AnalyzeRaceHandler::looks_like_race(Some(
        "Skyrace Chamonix"
    )));
    assert!(AnalyzeRaceHandler::looks_like_race(Some("Vertical KM")));
    assert!(AnalyzeRaceHandler::looks_like_race(Some("km vert")));
    assert!(!AnalyzeRaceHandler::looks_like_race(Some("Easy Run")));
    assert!(!AnalyzeRaceHandler::looks_like_race(Some("Recovery")));
    assert!(!AnalyzeRaceHandler::looks_like_race(None));
    assert!(!AnalyzeRaceHandler::looks_like_race(Some("UTMB")));
    assert!(!AnalyzeRaceHandler::looks_like_race(Some("CCC")));
}

#[test]
fn test_empty_results_handling() {
    let race_found = false;

    if !race_found {
        let suggestions = [
            "Use description_contains with specific race name".to_string(),
            "Check if activities are properly tagged".to_string(),
        ];
        assert!(!suggestions.is_empty());
    }
}

#[test]
fn test_content_structure() {
    let handler = AnalyzeRaceHandler::new();

    assert_eq!(handler.name(), "analyze_race");
    assert!(handler.description().len() > 50);

    let schema = handler.input_schema();
    assert!(schema.get("properties").is_some());
}

#[test]
fn test_next_actions_for_race_analysis() {
    let next_actions = [
        "To assess recovery: assess_recovery with period_days: 14".to_string(),
        "To plan next buildup: plan_training".to_string(),
    ];

    assert_eq!(next_actions.len(), 2);
    assert!(next_actions[0].contains("assess_recovery"));
    assert!(next_actions[1].contains("plan_training"));
}

#[test]
fn test_recovery_period_after_race() {
    let period_days = 14;
    assert!((7..=21).contains(&period_days));
}

#[test]
fn test_parse_requested_date_valid() {
    let parsed = AnalyzeRaceHandler::parse_requested_date("2026-05-24").unwrap();
    assert_eq!(
        parsed,
        NaiveDate::parse_from_str("2026-05-24", "%Y-%m-%d").unwrap()
    );
}

#[test]
fn test_parse_requested_date_invalid() {
    let result = AnalyzeRaceHandler::parse_requested_date("not-a-date");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid race date")
    );
}

#[test]
fn test_parse_activity_date_date_only() {
    assert_eq!(
        parse_activity_date("2026-05-24"),
        Some(NaiveDate::from_ymd_opt(2026, 5, 24).unwrap())
    );
}

#[test]
fn test_parse_activity_date_datetime() {
    assert_eq!(
        parse_activity_date("2026-05-24T10:30:00"),
        Some(NaiveDate::from_ymd_opt(2026, 5, 24).unwrap())
    );
}

#[test]
fn test_parse_activity_date_invalid() {
    assert_eq!(parse_activity_date("garbage"), None);
}

#[test]
fn test_race_analysis_mode_parse_performance() {
    assert_eq!(
        RaceAnalysisMode::parse(Some("performance")),
        RaceAnalysisMode::Performance
    );
}

#[test]
fn test_race_analysis_mode_parse_strategy() {
    assert_eq!(
        RaceAnalysisMode::parse(Some("strategy")),
        RaceAnalysisMode::Strategy
    );
}

#[test]
fn test_race_analysis_mode_parse_recovery() {
    assert_eq!(
        RaceAnalysisMode::parse(Some("recovery")),
        RaceAnalysisMode::Recovery
    );
}

#[test]
fn test_race_analysis_mode_parse_default() {
    assert_eq!(RaceAnalysisMode::parse(None), RaceAnalysisMode::Performance);
    assert_eq!(
        RaceAnalysisMode::parse(Some("unknown")),
        RaceAnalysisMode::Performance
    );
}

// ====================================================================
// execute() integration tests
// ====================================================================

#[tokio::test]
async fn test_execute_no_race_found_empty_activities() {
    let handler = AnalyzeRaceHandler::new();
    let client = Arc::new(MockIntervalsClient::default());
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("No recent race activities found"));
    assert!(!output.suggestions.is_empty());
    assert!(!output.next_actions.is_empty());
}

#[tokio::test]
async fn test_execute_no_race_found_with_desc_filter() {
    let handler = AnalyzeRaceHandler::new();
    let client = Arc::new(MockIntervalsClient::default());
    let result = handler
        .execute(json!({"description_contains": "marathon"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("No race found matching"));
    assert!(content_str.contains("marathon"));
}

#[tokio::test]
async fn test_execute_no_race_found_last_race_empty() {
    let handler = AnalyzeRaceHandler::new();
    let client = Arc::new(MockIntervalsClient::default());
    let result = handler
        .execute(json!({"date": "last_race"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("No recent race activities found"));
}

#[tokio::test]
async fn test_execute_no_race_found_by_date_no_match() {
    let handler = AnalyzeRaceHandler::new();
    let client = Arc::new(MockIntervalsClient::with_activity(
        "act-1",
        "2026-05-01",
        "Easy Run",
    ));
    let result = handler
        .execute(json!({"date": "2026-05-24"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("No recent race activities found"));
}

#[tokio::test]
async fn test_execute_invalid_date_returns_error() {
    let handler = AnalyzeRaceHandler::new();
    let client = Arc::new(MockIntervalsClient::with_activity(
        "act-1",
        "2026-05-01",
        "Easy Run",
    ));
    let result = handler
        .execute(json!({"date": "not-a-date"}), client, None)
        .await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid race date")
    );
}

#[tokio::test]
async fn test_execute_fallback_finds_looks_like_race() {
    let activities = vec![
        make_activity("act-1", "2026-05-20", "Easy Run"),
        make_activity("act-2", "2026-05-21", "Sunday Race"),
    ];
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(activities)
            .with_activity_detail("act-2", json!({}))
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Sunday Race"));
}

#[tokio::test]
async fn test_execute_fallback_finds_first_when_no_race_keyword() {
    let activities = vec![
        make_activity("act-1", "2026-05-20", "Morning Jog"),
        make_activity("act-2", "2026-05-21", "Evening Jog"),
    ];
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(activities)
            .with_activity_detail("act-1", json!({}))
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Morning Jog"));
}

#[tokio::test]
async fn test_execute_last_race_selects_latest() {
    let activities = vec![
        make_activity("act-old", "2026-05-01", "Trail Race"),
        make_activity("act-new", "2026-05-21", "10K Race"),
    ];
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(activities)
            .with_activity_detail("act-new", json!({}))
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"date": "last_race"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("10K Race"));
}

#[tokio::test]
async fn test_execute_by_date_exact_match() {
    let activities = vec![
        make_activity("act-1", "2026-05-20", "Easy Run"),
        make_activity("act-race", "2026-05-24", "Marathon Race"),
    ];
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(activities)
            .with_activity_detail("act-race", json!({}))
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"date": "2026-05-24"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Marathon Race"));
}

#[tokio::test]
async fn test_execute_by_date_with_desc_filter_match() {
    let activities = vec![
        make_activity("act-1", "2026-05-24", "Morning Run"),
        make_activity("act-race", "2026-05-24", "Marathon Race"),
    ];
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(activities)
            .with_activity_detail("act-race", json!({}))
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(
            json!({"date": "2026-05-24", "description_contains": "marathon"}),
            client,
            None,
        )
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Marathon Race"));
}

#[tokio::test]
async fn test_execute_by_date_with_desc_filter_no_match() {
    let activities = vec![
        make_activity("act-1", "2026-05-24", "Morning Run"),
        make_activity("act-2", "2026-05-24", "Evening Run"),
    ];
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(activities)
            .with_activity_detail("act-1", json!({}))
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(
            json!({"date": "2026-05-24", "description_contains": "marathon"}),
            client,
            None,
        )
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("No race found matching"));
    assert!(content_str.contains("marathon"));
}

#[tokio::test]
async fn test_execute_by_desc_filter_only_match() {
    let activities = vec![
        make_activity("act-1", "2026-05-20", "Easy Run"),
        make_activity("act-race", "2026-05-24", "Boston Marathon"),
    ];
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(activities)
            .with_activity_detail("act-race", json!({}))
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"description_contains": "marathon"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Boston Marathon"));
}

#[tokio::test]
async fn test_execute_by_desc_filter_only_no_match() {
    let activities = vec![
        make_activity("act-1", "2026-05-20", "Easy Run"),
        make_activity("act-2", "2026-05-24", "Recovery"),
    ];
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(activities)
            .with_activity_detail("act-1", json!({}))
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"description_contains": "marathon"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("No race found matching"));
}

#[tokio::test]
async fn test_execute_full_detail_table_rows() {
    let detail = json!({
        "distance": 42195.0,
        "moving_time": 14400,
        "average_heartrate": 162.0
    });
    let client = Arc::new(make_client_with_activity(
        "race-1",
        "2026-05-24",
        "Marathon",
        detail,
    ));
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("42.20 km"));
    assert!(content_str.contains("4:00:00"));
    assert!(content_str.contains("162 bpm"));
}

#[tokio::test]
async fn test_execute_with_intervals_and_execution_metrics() {
    let detail = json!({
        "distance": 10000.0,
        "moving_time": 2700,
        "average_heartrate": 170.0,
        "efficiency_factor": 1.05
    });
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "10K Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!([{"start": 0, "end": 600}])),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("1 race segments"));
    assert!(content_str.contains("10.00 km"));
}

#[tokio::test]
async fn test_execute_with_decoupling_metrics() {
    let detail = json!({
        "distance": 10000.0,
        "moving_time": 2700,
        "average_heartrate": 170.0
    });
    let streams = json!({
        "heartrate": [150, 160, 170, 175],
        "watts": [200, 210, 220, 230],
        "cadence": [85, 86, 85, 84],
        "speed": [4.0, 4.1, 4.0, 3.9],
        "distance": [0.0, 2500.0, 5000.0, 10000.0]
    });
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "10K Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(streams)
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_with_fitness_high_tsb() {
    let detail = json!({"distance": 10000.0, "moving_time": 2700});
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "10K Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
            .with_fitness_summary(json!({"ctl": 50.0, "atl": 80.0, "tsb": -15.0})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("recovery block recommended"));
}

#[tokio::test]
async fn test_execute_with_fitness_low_tsb() {
    let detail = json!({"distance": 10000.0, "moving_time": 2700});
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "10K Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
            .with_fitness_summary(json!({"ctl": 50.0, "atl": 45.0, "tsb": 5.0})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("looks manageable"));
}

#[tokio::test]
async fn test_execute_strategy_mode() {
    let client = Arc::new(make_minimal_client("race-1", "2026-05-24", "Race"));
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"analysis_type": "strategy"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Strategy Review"));
    assert!(content_str.contains("pacing discipline"));
}

#[tokio::test]
async fn test_execute_recovery_mode_with_note() {
    let detail = json!({"distance": 10000.0, "moving_time": 2700});
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
            .with_fitness_summary(json!({"ctl": 50.0, "atl": 80.0, "tsb": -15.0})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"analysis_type": "recovery"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Recovery Outlook"));
    assert!(content_str.contains("recovery block recommended"));
}

#[tokio::test]
async fn test_execute_recovery_mode_no_tsb() {
    let detail = json!({"distance": 10000.0, "moving_time": 2700});
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"analysis_type": "recovery"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(!content_str.contains("Recovery Outlook"));
}

#[tokio::test]
async fn test_execute_post_race_load_context_not_recovery() {
    let detail = json!({"distance": 10000.0, "moving_time": 2700});
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
            .with_fitness_summary(json!({"ctl": 50.0, "atl": 80.0, "tsb": -12.0})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"analysis_type": "performance"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Post-Race Load Context"));
}

#[tokio::test]
async fn test_execute_compare_true_with_matching_plan() {
    let detail = json!({"distance": 42195.0, "moving_time": 14400});
    let plan_event = json!({
        "name": "Marathon Race",
        "start_date_local": "2026-05-24T08:00:00",
        "category": "Workout"
    });
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "Marathon Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
            .with_upcoming_workouts(json!([plan_event])),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"compare_to_planned": true}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Comparison to Plan"));
    assert!(content_str.contains("Planned event"));
}

#[tokio::test]
async fn test_execute_compare_true_no_matching_plan() {
    let detail = json!({"distance": 42195.0, "moving_time": 14400});
    let plan_event = json!({
        "name": "Easy Run",
        "start_date_local": "2026-05-23T08:00:00",
        "category": "Workout"
    });
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "Marathon Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
            .with_upcoming_workouts(json!([plan_event])),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"compare_to_planned": true}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("No matching planned event found"));
}

#[tokio::test]
async fn test_execute_compare_true_plan_error() {
    let detail = json!({"distance": 42195.0, "moving_time": 14400});
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "Marathon Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
            .with_upcoming_workouts_error(IntervalsError::from_status(500, "server error")),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"compare_to_planned": true}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("No matching planned event found"));
}

#[tokio::test]
async fn test_execute_compare_false() {
    let detail = json!({"distance": 42195.0, "moving_time": 14400});
    let client = Arc::new(make_client_with_activity(
        "race-1",
        "2026-05-24",
        "Marathon",
        detail,
    ));
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"compare_to_planned": false}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(!content_str.contains("Comparison to Plan"));
}

#[tokio::test]
async fn test_execute_with_wellness_data() {
    let detail = json!({"distance": 10000.0, "moving_time": 2700, "average_heartrate": 165.0});
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "10K")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
            .with_wellness(json!([{"mood": 3, "sleep_hours": 7.5}]))
            .with_fitness_summary(json!({"ctl": 60.0, "atl": 55.0, "tsb": 5.0})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("10K"));
    assert!(content_str.contains("165 bpm"));
    assert!(content_str.contains("Race Readiness"));
    assert!(content_str.contains("CTL Drop: none (no detraining detected)"));
}

#[tokio::test]
async fn test_execute_race_readiness_shows_ctl_drop() {
    // Wellness data with CTL series showing a drop (peak 70 → current 50)
    let detail = json!({"distance": 10000.0, "moving_time": 2700});
    let wellness = json!([
        {"date": "2026-05-17", "ctl": 55.0},
        {"date": "2026-05-18", "ctl": 60.0},
        {"date": "2026-05-19", "ctl": 65.0},
        {"date": "2026-05-20", "ctl": 70.0},
        {"date": "2026-05-21", "ctl": 65.0},
        {"date": "2026-05-22", "ctl": 58.0},
        {"date": "2026-05-23", "ctl": 50.0}
    ]);
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity("race-1", "2026-05-24", "10K Race")])
            .with_activity_detail("race-1", detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
            .with_wellness(wellness)
            .with_fitness_summary(json!({"ctl": 50.0, "atl": 60.0, "tsb": -10.0})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Race Readiness"));
    // Peak CTL was 70, current is 50 → drop of 20
    assert!(content_str.contains("CTL Drop: 20.0"));
    assert!(content_str.contains("Score:"));
    assert!(content_str.contains("Tier:"));
}

#[tokio::test]
async fn test_execute_target_date_alias() {
    let client = Arc::new(make_minimal_client("race-1", "2026-05-24", "Race Day"));
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"target_date": "2026-05-24"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Race Day"));
}

#[tokio::test]
async fn test_execute_today_alias() {
    let today = chrono::Local::now().date_naive();
    let client = Arc::new(make_minimal_client(
        "race-1",
        &today.to_string(),
        "Today Race",
    ));
    let handler = AnalyzeRaceHandler::new();
    let result = handler
        .execute(json!({"date": "today"}), client, None)
        .await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Today Race"));
}

#[tokio::test]
async fn test_execute_detail_is_not_object_skips_table() {
    let detail = json!("not an object");
    let client = Arc::new(make_client_with_activity(
        "race-1",
        "2026-05-24",
        "Weird Race",
        detail,
    ));
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_without_race_name_fallback() {
    let activities = vec![ActivitySummary {
        id: "act-1".to_string(),
        name: None,
        start_date_local: "2026-05-24".to_string(),
        ..Default::default()
    }];
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(activities)
            .with_activity_detail("act-1", json!({}))
            .with_streams(json!({}))
            .with_intervals(json!({})),
    );
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Race"));
}

#[tokio::test]
async fn test_execute_suggestions_includes_nutrition_when_missing() {
    let client = Arc::new(make_minimal_client("race-1", "2026-05-24", "Race"));
    let handler = AnalyzeRaceHandler::new();
    let result = handler.execute(json!({}), client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.suggestions.iter().any(|s| s.contains("nutrition")));
}
