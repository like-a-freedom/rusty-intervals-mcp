use super::*;
use crate::test_support::content_text;
use crate::test_support::mock::MockIntervalsClient;

// ========================================================================
// Constructor Tests
// ========================================================================

#[test]
fn test_new_handler() {
    let handler = ManageProfileHandler::new();
    assert_eq!(handler.name(), "manage_profile");
}

#[test]
fn test_default_handler() {
    let _handler = ManageProfileHandler;
}

// ========================================================================
// IntentHandler Trait Implementation Tests
// ========================================================================

#[test]
fn test_name() {
    let handler = ManageProfileHandler::new();
    assert_eq!(IntentHandler::name(&handler), "manage_profile");
}

#[test]
fn test_description() {
    let handler = ManageProfileHandler::new();
    let desc = IntentHandler::description(&handler);
    assert!(desc.contains("Manages athlete profile"));
    assert!(desc.contains("zones"));
    assert!(desc.contains("thresholds"));
}

#[test]
fn test_input_schema_structure() {
    let handler = ManageProfileHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(props.contains_key("action"));
    assert!(props.contains_key("sections"));
    assert!(props.contains_key("new_aet_hr"));
    assert!(props.contains_key("new_lt_hr"));
    assert!(props.contains_key("thresholds_source"));
    assert!(props.contains_key("apply_to_activities"));

    // Check action enum values
    let action = props.get("action").unwrap();
    let action_enum = action.get("enum").unwrap().as_array().unwrap();
    assert!(action_enum.contains(&json!("get")));
    assert!(action_enum.contains(&json!("update_thresholds")));
}

#[test]
fn test_requires_idempotency_token() {
    let handler = ManageProfileHandler::new();
    assert!(!IntentHandler::requires_idempotency_token(&handler));
}

// ========================================================================
// sport_settings_entries() Tests
// ========================================================================

#[test]
fn test_sport_settings_entries_from_array() {
    let settings = json!([
        {"name": "Run", "types": ["Run"]},
        {"name": "Bike", "types": ["Ride"]}
    ]);

    let entries = sport_settings_entries(&settings);
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_sport_settings_entries_from_object_with_sports() {
    let settings = json!({
        "sports": [
            {"name": "Run", "types": ["Run"]},
            {"name": "Bike", "types": ["Ride"]}
        ]
    });

    let entries = sport_settings_entries(&settings);
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_sport_settings_entries_from_single_object() {
    let settings = json!({"name": "Run", "types": ["Run"]});

    let entries = sport_settings_entries(&settings);
    assert_eq!(entries.len(), 1);
}

#[test]
fn test_sport_settings_entries_empty_array() {
    let settings = json!([]);

    let entries = sport_settings_entries(&settings);
    assert_eq!(entries.len(), 0);
}

#[test]
fn test_sport_settings_entries_null_value() {
    let settings = json!(null);

    let entries = sport_settings_entries(&settings);
    assert_eq!(entries.len(), 0);
}

// ========================================================================
// sport_display_name() Tests
// ========================================================================

#[test]
fn test_sport_display_name_from_name_field() {
    let setting = serde_json::Map::new();
    let mut setting_with_name = setting.clone();
    setting_with_name.insert("name".to_string(), json!("Running"));

    assert_eq!(sport_display_name(&setting_with_name), "Running");
}

#[test]
fn test_sport_display_name_from_types_array() {
    let mut setting = serde_json::Map::new();
    setting.insert("types".to_string(), json!(["Run", "TrailRun"]));

    assert_eq!(sport_display_name(&setting), "Run");
}

#[test]
fn test_sport_display_name_falls_back_to_unknown() {
    let setting = serde_json::Map::new();

    assert_eq!(sport_display_name(&setting), "Unknown");
}

#[test]
fn test_sport_display_name_priority_name_over_types() {
    let mut setting = serde_json::Map::new();
    setting.insert("name".to_string(), json!("Custom Run"));
    setting.insert("types".to_string(), json!(["Run"]));

    assert_eq!(sport_display_name(&setting), "Custom Run");
}

// ========================================================================
// primary_sport_setting() Tests
// ========================================================================

#[test]
fn test_primary_sport_setting_prefers_run() {
    let run_setting = serde_json::Map::new();
    let mut run_with_types = run_setting.clone();
    run_with_types.insert("types".to_string(), json!(["Run"]));

    let bike_setting = serde_json::Map::new();
    let mut bike_with_types = bike_setting.clone();
    bike_with_types.insert("types".to_string(), json!(["Ride"]));

    let settings = vec![&bike_with_types, &run_with_types];
    let primary = primary_sport_setting(&settings);

    assert!(primary.is_some());
    assert_eq!(
        primary
            .unwrap()
            .get("types")
            .and_then(|v| v.as_array())
            .unwrap()[0]
            .as_str(),
        Some("Run")
    );
}

#[test]
fn test_primary_sport_setting_prefers_trail_run() {
    let trail_setting = serde_json::Map::new();
    let mut trail_with_types = trail_setting.clone();
    trail_with_types.insert("types".to_string(), json!(["TrailRun"]));

    let settings = vec![&trail_with_types];
    let primary = primary_sport_setting(&settings);

    assert!(primary.is_some());
}

#[test]
fn test_primary_sport_setting_prefers_virtual_run() {
    let virtual_setting = serde_json::Map::new();
    let mut virtual_with_types = virtual_setting.clone();
    virtual_with_types.insert("types".to_string(), json!(["VirtualRun"]));

    let settings = vec![&virtual_with_types];
    let primary = primary_sport_setting(&settings);

    assert!(primary.is_some());
}

#[test]
fn test_primary_sport_setting_falls_back_to_first() {
    let swim_setting = serde_json::Map::new();
    let mut swim_with_types = swim_setting.clone();
    swim_with_types.insert("types".to_string(), json!(["Swim"]));

    let settings = vec![&swim_with_types];
    let primary = primary_sport_setting(&settings);

    assert!(primary.is_some());
}

#[test]
fn test_primary_sport_setting_empty_input() {
    let settings: Vec<&serde_json::Map<String, Value>> = vec![];
    let primary = primary_sport_setting(&settings);

    assert!(primary.is_none());
}

// ========================================================================
// get_number() Tests
// ========================================================================

#[test]
fn test_get_number_from_f64() {
    let mut setting = serde_json::Map::new();
    setting.insert("ftp".to_string(), json!(250.5));

    assert_eq!(get_number(&setting, &["ftp"]), Some(250.5));
}

#[test]
fn test_get_number_from_i64() {
    let mut setting = serde_json::Map::new();
    setting.insert("ftp".to_string(), json!(250));

    assert_eq!(get_number(&setting, &["ftp"]), Some(250.0));
}

#[test]
fn test_get_number_tries_multiple_keys() {
    let mut setting = serde_json::Map::new();
    setting.insert("threshold_lt_hr".to_string(), json!(170));

    assert_eq!(
        get_number(&setting, &["lthr", "threshold_lt_hr"]),
        Some(170.0)
    );
}

#[test]
fn test_get_number_returns_none_when_keys_not_found() {
    let setting = serde_json::Map::new();

    assert_eq!(get_number(&setting, &["ftp", "threshold"]), None);
}

#[test]
fn test_get_number_returns_none_for_non_numeric() {
    let mut setting = serde_json::Map::new();
    setting.insert("name".to_string(), json!("Run"));

    assert_eq!(get_number(&setting, &["name"]), None);
}

// ========================================================================
// format_hr_zone_ranges() Tests
// ========================================================================

#[test]
fn test_format_hr_zone_ranges_basic() {
    let zones: Vec<Value> = vec![json!(140), json!(160), json!(180)];

    let ranges = format_hr_zone_ranges(&zones);
    assert_eq!(ranges.len(), 3);
    assert_eq!(ranges[0], "≤ 140 bpm");
    assert_eq!(ranges[1], "141-160 bpm");
    assert_eq!(ranges[2], "161-180 bpm");
}

#[test]
fn test_format_hr_zone_ranges_empty() {
    let zones: Vec<Value> = vec![];

    let ranges = format_hr_zone_ranges(&zones);
    assert!(ranges.is_empty());
}

#[test]
fn test_format_hr_zone_ranges_with_floats() {
    let zones: Vec<Value> = vec![json!(140.5), json!(160.7), json!(180.2)];

    let ranges = format_hr_zone_ranges(&zones);
    assert_eq!(ranges.len(), 3);
    assert_eq!(ranges[0], "≤ 141 bpm");
    assert_eq!(ranges[1], "142-161 bpm");
    assert_eq!(ranges[2], "162-180 bpm");
}

#[test]
fn test_format_hr_zone_ranges_single_zone() {
    let zones: Vec<Value> = vec![json!(150)];

    let ranges = format_hr_zone_ranges(&zones);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0], "≤ 150 bpm");
}

// ========================================================================
// format_threshold_pace() Tests
// ========================================================================

#[test]
fn test_format_threshold_pace_mins_km() {
    let pace = 5.0; // 5:00 /km
    assert_eq!(format_threshold_pace(pace, Some("MINS_KM")), "5:00 /km");
}

#[test]
fn test_format_threshold_pace_mins_km_with_seconds() {
    let pace = 5.5; // 5:30 /km
    assert_eq!(format_threshold_pace(pace, Some("MINS_KM")), "5:30 /km");
}

#[test]
fn test_format_threshold_pace_secs_100m() {
    let pace = 3.0; // 3 min/100m = 180 sec/100m
    assert_eq!(
        format_threshold_pace(pace, Some("SECS_100M")),
        "180.0 sec/100m"
    );
}

#[test]
fn test_format_threshold_pace_unknown_unit() {
    let pace = 10.0;
    assert_eq!(
        format_threshold_pace(pace, Some("UNKNOWN")),
        "10.00 UNKNOWN"
    );
}

#[test]
fn test_format_threshold_pace_no_unit() {
    let pace = 5.5;
    assert_eq!(format_threshold_pace(pace, None), "5.50");
}

// ========================================================================
// Input Validation and Default Value Tests
// ========================================================================

#[test]
fn test_action_values() {
    let valid_actions = ["get", "update_thresholds"];
    for action in &valid_actions {
        assert!(["get", "update_thresholds"].contains(action));
    }
}

#[test]
fn test_default_sections() {
    let input = json!({
        "action": "get"
    });

    let sections = input
        .get("sections")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["overview", "zones", "thresholds"]);

    assert_eq!(sections.len(), 3);
    assert!(sections.contains(&"overview"));
    assert!(sections.contains(&"zones"));
    assert!(sections.contains(&"thresholds"));
}

#[test]
fn test_threshold_source_values() {
    let valid_sources = ["manual", "lab_test"];
    for source in &valid_sources {
        assert!(["manual", "lab_test"].contains(source));
    }
}

#[test]
fn test_apply_to_activities_default() {
    let input = json!({
        "action": "update_thresholds",
        "new_aet_hr": 155,
        "new_lt_hr": 171
    });

    let apply = input
        .get("apply_to_activities")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    assert!(apply);
}

#[test]
fn test_gap_change_calculation() {
    let old_aet: i64 = 150;
    let old_lt: i64 = 170;
    let new_aet: i64 = 155;
    let new_lt: i64 = 171;

    let old_gap = (old_lt - old_aet) as f64 * 100.0 / old_aet as f64;
    let new_gap = (new_lt - new_aet) as f64 * 100.0 / new_aet as f64;
    let gap_change = new_gap - old_gap;

    assert!((old_gap - 13.33).abs() < 0.1);
    assert!((new_gap - 10.32).abs() < 0.1);
    assert!(gap_change < 0.0);
}

#[test]
fn test_threshold_update_schema() {
    let handler = ManageProfileHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    let props = schema.get("properties").unwrap().as_object().unwrap();

    let aet_hr = props.get("new_aet_hr").unwrap();
    assert_eq!(aet_hr.get("type").and_then(|v| v.as_str()), Some("number"));

    let lt_hr = props.get("new_lt_hr").unwrap();
    assert_eq!(lt_hr.get("type").and_then(|v| v.as_str()), Some("number"));
}

#[test]
fn test_sections_field_type() {
    let handler = ManageProfileHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    let props = schema.get("properties").unwrap().as_object().unwrap();
    let sections = props.get("sections").unwrap();

    assert_eq!(sections.get("type").and_then(|v| v.as_str()), Some("array"));
}

#[test]
fn test_apply_to_activities_field_default() {
    let handler = ManageProfileHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    let props = schema.get("properties").unwrap().as_object().unwrap();
    let apply = props.get("apply_to_activities").unwrap();

    assert_eq!(apply.get("default").and_then(|v| v.as_bool()), Some(true));
}

// ========================================================================
// Handler Execution Tests
// ========================================================================

fn profile_mock_client() -> MockIntervalsClient {
    MockIntervalsClient::builder()
        .with_athlete_profile(intervals_icu_client::AthleteProfile {
            id: "ath1".to_string(),
            name: Some("Test Athlete".to_string()),
        })
        .with_sport_settings(intervals_icu_client::domains::workout::SportSettings {
            sports: vec![intervals_icu_client::domains::workout::SportSetting {
                name: Some("Run".into()),
                types: Some(vec!["Run".into()]),
                hr_zones: vec![
                    serde_json::json!(140),
                    serde_json::json!(160),
                    serde_json::json!(180),
                ],
                lthr: Some(170.0),
                max_hr: Some(190.0),
                ftp: Some(250.0),
                threshold_pace: Some(5.0),
                pace_units: Some("MINS_KM".into()),
                load_order: Some("heart_rate".into()),
                ..Default::default()
            }],
            age: None,
            weight: None,
        })
        .with_fitness_summary(json!({
            "ctl": 45.0,
            "atl": 30.0,
            "tsb": 15.0
        }))
        .with_wellness(json!({
            "weight": 75.5
        }))
}

#[tokio::test]
async fn test_execute_get_profile_action() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "get"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.content.is_empty());
}

#[tokio::test]
async fn test_execute_get_profile_with_all_sections() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "get",
        "sections": ["overview", "zones", "thresholds", "metrics"]
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.content.is_empty());
}

#[tokio::test]
async fn test_execute_get_profile_with_specific_sections() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "get",
        "sections": ["thresholds"]
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.content.is_empty());
}

#[tokio::test]
async fn test_execute_update_thresholds_action() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "update_thresholds",
        "new_aet_hr": 155,
        "new_lt_hr": 171,
        "thresholds_source": "lab_test",
        "apply_to_activities": true,
        "idempotency_token": "test-token-123"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.content.is_empty());

    // Check that suggestions are present
    assert!(!output.suggestions.is_empty());
}

#[tokio::test]
async fn test_execute_update_thresholds_without_apply() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "update_thresholds",
        "new_aet_hr": 150,
        "new_lt_hr": 170,
        "apply_to_activities": false,
        "idempotency_token": "test-token-456"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Historical activity recalculation was skipped"));
}

#[tokio::test]
async fn test_execute_invalid_action() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "invalid_action"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_missing_action() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({});

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_update_thresholds_missing_token() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "update_thresholds",
        "new_aet_hr": 155,
        "new_lt_hr": 171
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_update_thresholds_missing_new_aet_hr() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "update_thresholds",
        "new_lt_hr": 171,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_update_thresholds_missing_new_lt_hr() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "update_thresholds",
        "new_aet_hr": 155,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_update_thresholds_default_source() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "update_thresholds",
        "new_aet_hr": 155,
        "new_lt_hr": 171,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("manual")); // Default source
}

#[tokio::test]
async fn test_execute_get_profile_suggestions() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "get"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.suggestions.is_empty());
    assert!(!output.next_actions.is_empty());
}

#[tokio::test]
async fn test_execute_update_thresholds_next_actions() {
    let handler = ManageProfileHandler::new();
    let client = Arc::new(profile_mock_client());
    let input = json!({
        "action": "update_thresholds",
        "new_aet_hr": 155,
        "new_lt_hr": 171,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.next_actions.is_empty());
    assert!(
        output
            .next_actions
            .iter()
            .any(|a| a.contains("assess_recovery"))
    );
}
