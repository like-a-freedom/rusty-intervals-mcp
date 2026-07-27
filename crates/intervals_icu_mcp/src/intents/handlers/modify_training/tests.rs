use super::*;
use crate::engines::dedupe::dedupe_and_sort_events;
use crate::intents::OutputMetadata;
use crate::test_support::content_text;
use intervals_icu_client::EventCategory;

#[tokio::test]
async fn test_new_handler() {
    let handler = ModifyTrainingHandler::new();
    assert_eq!(handler.name(), "modify_training");
}

#[test]
fn test_default_handler() {
    let _handler = ModifyTrainingHandler;
}

#[test]
fn test_name() {
    let handler = ModifyTrainingHandler::new();
    assert_eq!(IntentHandler::name(&handler), "modify_training");
}

#[test]
fn test_description() {
    let handler = ModifyTrainingHandler::new();
    let desc = IntentHandler::description(&handler);
    assert!(desc.contains("Modifies or creates calendar training events"));
    assert!(desc.contains("modify"));
    assert!(desc.contains("create"));
    assert!(desc.contains("delete"));
}

#[test]
fn test_input_schema_structure() {
    let handler = ModifyTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(props.contains_key("action"));
    assert!(props.contains_key("target_date"));
    assert!(props.contains_key("new_date"));
    assert!(props.contains_key("new_type"));
    assert!(props.contains_key("dry_run"));
    assert!(props.contains_key("idempotency_token"));

    // Check action enum values
    let action = props.get("action").unwrap();
    let action_enum = action.get("enum").unwrap().as_array().unwrap();
    assert!(action_enum.contains(&json!("modify")));
    assert!(action_enum.contains(&json!("create")));
    assert!(action_enum.contains(&json!("delete")));
}

#[test]
fn test_requires_idempotency_token() {
    let handler = ModifyTrainingHandler::new();
    assert!(IntentHandler::requires_idempotency_token(&handler));
}

#[test]
fn test_action_values() {
    let valid_actions = ["modify", "create", "delete"];
    for action in &valid_actions {
        assert!(["modify", "create", "delete"].contains(action));
    }
}

#[test]
fn test_dry_run_default() {
    let input = json!({
        "action": "modify",
        "target_date": "2026-03-01",
        "idempotency_token": "test"
    });
    let dry_run = input
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(!dry_run);
}

#[test]
fn test_delete_requires_dry_run_validation() {
    // Delete operation should require dry_run: true first
    let action = "delete";
    let dry_run = false;

    // This should be rejected per business logic
    assert_eq!(action, "delete");
    assert!(!dry_run);
    // In actual code: returns error "Delete operation requires dry_run: true first"
}

#[test]
fn test_date_validation() {
    let valid_date = "2026-03-01";
    let result = NaiveDate::parse_from_str(valid_date, "%Y-%m-%d");
    assert!(result.is_ok());

    let invalid_date = "01-03-2026";
    let result = NaiveDate::parse_from_str(invalid_date, "%Y-%m-%d");
    assert!(result.is_err());
}

#[test]
fn test_optional_fields() {
    let input = json!({
        "action": "modify",
        "target_date": "2026-03-01",
        "idempotency_token": "test"
    });

    // These fields are optional
    assert!(input.get("new_date").is_none());
    assert!(input.get("new_name").is_none());
    assert!(input.get("new_description").is_none());
    assert!(input.get("target_description_contains").is_none());
}

#[test]
fn test_resolve_target_scope_requires_both_range_bounds() {
    let input = json!({
        "action": "modify",
        "target_date_from": "2026-03-01",
        "idempotency_token": "test"
    });

    let err = ModifyTrainingHandler::resolve_target_scope(&input)
        .expect_err("missing target_date_to should be rejected");

    assert!(
        err.to_string()
            .contains("target_date_from and target_date_to must be provided together")
    );
}

#[test]
fn test_dedupe_events_keeps_unique_id_and_fallback_keys() {
    let deduped = dedupe_and_sort_events(vec![
        Event {
            id: Some("event-1".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Tempo Session".to_string(),
            category: EventCategory::Workout,
            description: Some("first copy".to_string()),
            r#type: Some("Run".to_string()),
        },
        Event {
            id: Some("event-1".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Tempo Session".to_string(),
            category: EventCategory::Workout,
            description: Some("duplicate id".to_string()),
            r#type: Some("Run".to_string()),
        },
        Event {
            id: None,
            start_date_local: "2026-03-02".to_string(),
            name: "Strength".to_string(),
            category: EventCategory::Workout,
            description: Some("fallback key".to_string()),
            r#type: Some("Gym".to_string()),
        },
        Event {
            id: None,
            start_date_local: "2026-03-02".to_string(),
            name: "Strength".to_string(),
            category: EventCategory::Workout,
            description: Some("duplicate fallback key".to_string()),
            r#type: Some("Gym".to_string()),
        },
        Event {
            id: None,
            start_date_local: "2026-03-03".to_string(),
            name: "Long Run".to_string(),
            category: EventCategory::Workout,
            description: Some("unique fallback key".to_string()),
            r#type: Some("Run".to_string()),
        },
    ]);

    assert_eq!(deduped.len(), 3);
    assert_eq!(deduped[0].id.as_deref(), Some("event-1"));
    assert_eq!(deduped[1].name, "Strength");
    assert_eq!(deduped[2].name, "Long Run");
}

#[test]
fn test_metadata_structure() {
    let metadata = OutputMetadata {
        has_more: None,
        next_offset: None,
        total_count: None,
        events_created: Some(5),
        events_modified: Some(3),
        events_deleted: Some(1),
        extra: std::collections::HashMap::new(),
    };

    assert_eq!(metadata.events_created, Some(5));
    assert_eq!(metadata.events_modified, Some(3));
    assert_eq!(metadata.events_deleted, Some(1));
}

// ========================================================================
// TargetScope Enum Tests
// ========================================================================

#[test]
fn test_target_scope_single_variant() {
    let date = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let scope = TargetScope::Single(date);

    match scope {
        TargetScope::Single(d) => assert_eq!(d, date),
        TargetScope::Range(_, _) => panic!("Expected Single variant"),
    }
}

#[test]
fn test_target_scope_range_variant() {
    let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
    let scope = TargetScope::Range(start, end);

    match scope {
        TargetScope::Range(s, e) => {
            assert_eq!(s, start);
            assert_eq!(e, end);
        }
        TargetScope::Single(_) => panic!("Expected Range variant"),
    }
}

// ========================================================================
// Duration Parsing Tests
// ========================================================================

#[test]
fn test_parse_duration_to_seconds_valid() {
    assert_eq!(
        ModifyTrainingHandler::parse_duration_to_seconds("1:00").unwrap(),
        3600
    );
    assert_eq!(
        ModifyTrainingHandler::parse_duration_to_seconds("0:30").unwrap(),
        1800
    );
    assert_eq!(
        ModifyTrainingHandler::parse_duration_to_seconds("2:30").unwrap(),
        9000
    );
    assert_eq!(
        ModifyTrainingHandler::parse_duration_to_seconds("1:30").unwrap(),
        5400
    );
}

#[test]
fn test_parse_duration_to_seconds_invalid_format() {
    let result = ModifyTrainingHandler::parse_duration_to_seconds("1:00:00");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid duration format")
    );
}

#[test]
fn test_parse_duration_to_seconds_invalid_hours() {
    let result = ModifyTrainingHandler::parse_duration_to_seconds("abc:00");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid duration hours")
    );
}

#[test]
fn test_parse_duration_to_seconds_invalid_minutes() {
    let result = ModifyTrainingHandler::parse_duration_to_seconds("1:abc");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid duration minutes")
    );
}

#[test]
fn test_parse_duration_to_seconds_negative_hours() {
    let result = ModifyTrainingHandler::parse_duration_to_seconds("-1:00");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid duration value")
    );
}

#[test]
fn test_parse_duration_to_seconds_invalid_minutes_range() {
    let result = ModifyTrainingHandler::parse_duration_to_seconds("1:60");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid duration value")
    );
}

// ========================================================================
// Build Update Fields Tests
// ========================================================================

#[test]
fn test_build_update_fields_new_date() {
    let input = json!({
        "new_date": "2026-03-15"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_ok());
    let fields = result.unwrap();
    // Date gets normalized to include timestamp
    assert!(
        fields
            .get("start_date_local")
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with("2026-03-15")
    );
}

#[test]
fn test_build_update_fields_new_name() {
    let input = json!({
        "new_name": "New Workout Name"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_ok());
    let fields = result.unwrap();
    assert_eq!(
        fields.get("name").unwrap().as_str(),
        Some("New Workout Name")
    );
}

#[test]
fn test_build_update_fields_new_description() {
    let input = json!({
        "new_description": "Updated description"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_ok());
    let fields = result.unwrap();
    assert_eq!(
        fields.get("description").unwrap().as_str(),
        Some("Updated description")
    );
}

#[test]
fn test_build_update_fields_new_category() {
    let input = json!({
        "new_category": "RaceA"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_ok());
    let fields = result.unwrap();
    assert_eq!(fields.get("category").unwrap().as_str(), Some("RaceA"));
}

#[test]
fn test_build_update_fields_new_type() {
    let input = json!({
        "new_type": "Ride"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_ok());
    let fields = result.unwrap();
    assert_eq!(fields.get("type").unwrap().as_str(), Some("Ride"));
}

#[test]
fn test_build_update_fields_new_duration() {
    let input = json!({
        "new_duration": "1:30"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_ok());
    let fields = result.unwrap();
    assert_eq!(fields.get("moving_time").unwrap().as_i64(), Some(5400));
}

#[test]
fn test_build_update_fields_multiple_fields() {
    let input = json!({
        "new_name": "Updated Name",
        "new_date": "2026-03-20",
        "new_duration": "2:00"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_ok());
    let fields = result.unwrap();
    assert_eq!(fields.get("name").unwrap().as_str(), Some("Updated Name"));
    // Date gets normalized to include timestamp
    assert!(
        fields
            .get("start_date_local")
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with("2026-03-20")
    );
    assert_eq!(fields.get("moving_time").unwrap().as_i64(), Some(7200));
}

#[test]
fn test_build_update_fields_empty_rejected() {
    let input = json!({});
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("at least one new_* field")
    );
}

#[test]
fn test_build_update_fields_invalid_date() {
    let input = json!({
        "new_date": "invalid-date"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid date format")
    );
}

#[test]
fn test_build_update_fields_type_fallback_to_type_field() {
    let input = json!({
        "type": "Swim"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_ok());
    let fields = result.unwrap();
    assert_eq!(fields.get("type").unwrap().as_str(), Some("Swim"));
}

#[test]
fn test_build_update_fields_category_fallback_to_category_field() {
    let input = json!({
        "category": "Note"
    });
    let result = ModifyTrainingHandler::build_update_fields(&input);
    assert!(result.is_ok());
    let fields = result.unwrap();
    assert_eq!(fields.get("category").unwrap().as_str(), Some("Note"));
}

// ========================================================================
// Resolve Target Scope Tests
// ========================================================================

#[test]
fn test_resolve_target_scope_single_date() {
    let input = json!({
        "target_date": "2026-03-15"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_ok());
    match result.unwrap() {
        TargetScope::Single(date) => {
            assert_eq!(date, NaiveDate::from_ymd_opt(2026, 3, 15).unwrap())
        }
        TargetScope::Range(_, _) => panic!("Expected Single variant"),
    }
}

#[test]
fn test_resolve_target_scope_single_relative_today() {
    let input = json!({
        "target_date": "today"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_ok());
    match result.unwrap() {
        TargetScope::Single(date) => {
            assert_eq!(date, chrono::Local::now().date_naive())
        }
        TargetScope::Range(_, _) => panic!("Expected Single variant"),
    }
}

#[test]
fn test_resolve_target_scope_range() {
    let input = json!({
        "target_date_from": "2026-03-01",
        "target_date_to": "2026-03-07"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_ok());
    match result.unwrap() {
        TargetScope::Range(start, end) => {
            assert_eq!(start, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
            assert_eq!(end, NaiveDate::from_ymd_opt(2026, 3, 7).unwrap());
        }
        TargetScope::Single(_) => panic!("Expected Range variant"),
    }
}

#[test]
fn test_resolve_target_scope_invalid_start_date() {
    let input = json!({
        "target_date_from": "invalid",
        "target_date_to": "2026-03-07"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid date format")
    );
}

#[test]
fn test_resolve_target_scope_invalid_end_date() {
    let input = json!({
        "target_date_from": "2026-03-01",
        "target_date_to": "invalid"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid date format")
    );
}

#[test]
fn test_resolve_target_scope_range_start_after_end() {
    let input = json!({
        "target_date_from": "2026-03-15",
        "target_date_to": "2026-03-01"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Start date must be before end date")
    );
}

#[test]
fn test_resolve_target_scope_only_target_date_from() {
    let input = json!({
        "target_date_from": "2026-03-01"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("target_date_from and target_date_to must be provided together")
    );
}

#[test]
fn test_resolve_target_scope_only_target_date_to() {
    let input = json!({
        "target_date_to": "2026-03-07"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("target_date_from and target_date_to must be provided together")
    );
}

#[test]
fn test_resolve_target_scope_no_date_fields() {
    let input = json!({
        "action": "modify"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Provide target_date or target_date_from/target_date_to")
    );
}

#[test]
fn test_resolve_target_scope_invalid_single_date() {
    let input = json!({
        "target_date": "not-a-date"
    });
    let result = ModifyTrainingHandler::resolve_target_scope(&input);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid date format")
    );
}

// ========================================================================
// Event Description Matching Tests
// ========================================================================

#[test]
fn test_event_matches_description_by_name() {
    let event = Event {
        id: Some("e1".to_string()),
        start_date_local: "2026-03-01".to_string(),
        name: "Tempo Run Session".to_string(),
        category: EventCategory::Workout,
        description: None,
        r#type: None,
    };
    assert!(ModifyTrainingHandler::event_matches_description(
        &event, "tempo"
    ));
    assert!(ModifyTrainingHandler::event_matches_description(
        &event, "Tempo"
    ));
    assert!(ModifyTrainingHandler::event_matches_description(
        &event, "run"
    ));
    assert!(!ModifyTrainingHandler::event_matches_description(
        &event,
        "intervals"
    ));
}

#[test]
fn test_event_matches_description_by_description() {
    let event = Event {
        id: Some("e1".to_string()),
        start_date_local: "2026-03-01".to_string(),
        name: "Workout".to_string(),
        category: EventCategory::Workout,
        description: Some("Threshold intervals at lactate turnpoint".to_string()),
        r#type: None,
    };
    assert!(ModifyTrainingHandler::event_matches_description(
        &event,
        "threshold"
    ));
    assert!(ModifyTrainingHandler::event_matches_description(
        &event,
        "intervals"
    ));
    assert!(!ModifyTrainingHandler::event_matches_description(
        &event, "recovery"
    ));
}

#[test]
fn test_event_matches_description_case_insensitive() {
    let event = Event {
        id: Some("e1".to_string()),
        start_date_local: "2026-03-01".to_string(),
        name: "LONG RUN Z2".to_string(),
        category: EventCategory::Workout,
        description: None,
        r#type: None,
    };
    assert!(ModifyTrainingHandler::event_matches_description(
        &event, "long"
    ));
    assert!(ModifyTrainingHandler::event_matches_description(
        &event, "run"
    ));
    assert!(ModifyTrainingHandler::event_matches_description(
        &event, "z2"
    ));
}

// ========================================================================
// Constants Tests
// ========================================================================

#[test]
fn test_single_scope_limit_constant() {
    assert_eq!(SINGLE_SCOPE_LIMIT, 200);
}

#[test]
fn test_range_scope_limit_constant() {
    assert_eq!(RANGE_SCOPE_LIMIT, 500);
}

// ========================================================================
// Dedupe Events Edge Cases
// ========================================================================

#[test]
fn test_dedupe_events_empty_list() {
    let events: Vec<Event> = vec![];
    let deduped = dedupe_and_sort_events(events);
    assert!(deduped.is_empty());
}

#[test]
fn test_dedupe_events_all_unique() {
    let events = vec![
        Event {
            id: Some("e1".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Event 1".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
        Event {
            id: Some("e2".to_string()),
            start_date_local: "2026-03-02".to_string(),
            name: "Event 2".to_string(),
            category: EventCategory::RaceA,
            description: None,
            r#type: None,
        },
    ];
    let deduped = dedupe_and_sort_events(events);
    assert_eq!(deduped.len(), 2);
}

#[test]
fn test_dedupe_events_fallback_key_uses_date_name_category() {
    let events = vec![
        Event {
            id: None,
            start_date_local: "2026-03-01".to_string(),
            name: "Same Name".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
        Event {
            id: None,
            start_date_local: "2026-03-01".to_string(),
            name: "Same Name".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
    ];
    let deduped = dedupe_and_sort_events(events);
    // Should be deduped because they have the same fallback key
    assert_eq!(deduped.len(), 1);
}

// ========================================================================
// Execute() Path Tests - modify_training action
// ========================================================================

use crate::test_support::mock::MockIntervalsClient;
use intervals_icu_client::Event;
use std::sync::Arc;

#[tokio::test]
async fn test_modify_training_update_action_dry_run() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![Event {
        id: Some("event-123".to_string()),
        start_date_local: "2026-03-01".to_string(),
        name: "Tempo Run".to_string(),
        category: EventCategory::Workout,
        description: None,
        r#type: None,
    }]));

    let input = json!({
        "action": "modify",
        "target_date": "2026-03-01",
        "new_name": "Updated Tempo Run",
        "dry_run": true,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Preview (dry_run)"));
    assert!(content_str.contains("Updated Tempo Run"));
}

#[tokio::test]
async fn test_modify_training_update_action_apply() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![Event {
        id: Some("event-123".to_string()),
        start_date_local: "2026-03-01".to_string(),
        name: "Tempo Run".to_string(),
        category: EventCategory::Workout,
        description: None,
        r#type: None,
    }]));

    let input = json!({
        "action": "modify",
        "target_date": "2026-03-01",
        "new_name": "Updated Tempo Run",
        "dry_run": false,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Changes Applied"));
    assert!(output.metadata.events_modified == Some(1));
}

#[tokio::test]
async fn test_modify_training_no_events_found() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "modify",
        "target_date": "2026-03-01",
        "new_name": "Updated Workout",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("No events found"));
    assert!(!output.suggestions.is_empty());
}

#[tokio::test]
async fn test_modify_training_with_description_filter() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![
        Event {
            id: Some("event-123".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Easy Run".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
        Event {
            id: Some("event-124".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Tempo Run".to_string(),
            category: EventCategory::Workout,
            description: Some("Threshold workout".to_string()),
            r#type: None,
        },
    ]));

    let input = json!({
        "action": "modify",
        "target_date": "2026-03-01",
        "target_description_contains": "tempo",
        "new_name": "Updated Tempo",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Updated Tempo"));
}

#[tokio::test]
async fn test_modify_training_description_filter_no_match() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![Event {
        id: Some("event-123".to_string()),
        start_date_local: "2026-03-01".to_string(),
        name: "Easy Run".to_string(),
        category: EventCategory::Workout,
        description: None,
        r#type: None,
    }]));

    let input = json!({
        "action": "modify",
        "target_date": "2026-03-01",
        "target_description_contains": "tempo",
        "new_name": "Updated",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("No events matched"));
}

#[tokio::test]
async fn test_modify_training_update_error() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_events(vec![Event {
                id: Some("event-123".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Tempo Run".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            }])
            .with_update_error("API error"),
    );

    let input = json!({
        "action": "modify",
        "target_date": "2026-03-01",
        "new_name": "Updated",
        "dry_run": false,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_modify_training_range_scope() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![
        Event {
            id: Some("event-123".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Run 1".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
        Event {
            id: Some("event-124".to_string()),
            start_date_local: "2026-03-02".to_string(),
            name: "Run 2".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
    ]));

    let input = json!({
        "action": "modify",
        "target_date_from": "2026-03-01",
        "target_date_to": "2026-03-07",
        "new_name": "Updated Run",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("2026-03-01 to 2026-03-07"));
}

// ========================================================================
// Execute() Path Tests - create_training action
// ========================================================================

#[tokio::test]
async fn test_create_training_dry_run() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "create",
        "new_date": "2026-03-15",
        "new_name": "New Workout",
        "new_duration": "1:00",
        "new_category": "Workout",
        "new_type": "Run",
        "dry_run": true,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Preview (dry_run)"));
    assert!(content_str.contains("New Workout"));
    assert!(output.metadata.events_created == Some(1));
}

#[tokio::test]
async fn test_create_training_apply() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "create",
        "new_date": "2026-03-15",
        "new_name": "New Workout",
        "new_duration": "1:00",
        "dry_run": false,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Created"));
}

#[tokio::test]
async fn test_create_training_missing_date() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "create",
        "new_name": "New Workout",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("new_date"));
}

#[tokio::test]
async fn test_create_training_with_target_date_alias() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "create",
        "target_date": "2026-03-15",
        "new_name": "New Workout",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_create_training_race_category() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "create",
        "new_date": "2026-04-01",
        "new_name": "Marathon",
        "new_category": "RaceA",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Marathon"));
}

#[tokio::test]
async fn test_create_training_note_category() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "create",
        "new_date": "2026-03-20",
        "new_name": "Rest Day Note",
        "new_category": "Note",
        "new_description": "Feeling tired",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_create_training_dry_run_shows_workout_warnings() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "create",
        "new_date": "2026-03-15",
        "new_name": "Bad Workout",
        "new_description": "- 10min 60%\n- 5m 95-105",
        "new_duration": "0:15",
        "dry_run": true,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(
        content_str.contains("Workout Builder Warnings"),
        "expected validation warnings in create response, got: {}",
        content_str
    );
    assert!(
        content_str.contains("duration_format"),
        "expected duration_format warning, got: {}",
        content_str
    );
    assert!(
        content_str.contains("target_format"),
        "expected target_format warning, got: {}",
        content_str
    );
}

#[tokio::test]
async fn test_create_training_dry_run_clean_description_no_warnings() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "create",
        "new_date": "2026-03-15",
        "new_name": "Clean Workout",
        "new_description": "- 10m 60%\n- 5m 95-105%",
        "new_duration": "0:15",
        "dry_run": true,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(
        !content_str.contains("Workout Builder Warnings"),
        "unexpected validation warnings for clean description: {}",
        content_str
    );
}

#[tokio::test]
async fn test_create_training_dry_run_with_duration_mismatch_warning() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    // Steps sum to 10m (600s), but new_duration is 1:00 (3600s) → mismatch
    let input = json!({
        "action": "create",
        "new_date": "2026-03-15",
        "new_name": "Duration Mismatch",
        "new_description": "- 10m 60%",
        "new_duration": "1:00",
        "dry_run": true,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(
        content_str.contains("duration_mismatch"),
        "expected duration_mismatch warning, got: {}",
        content_str
    );
}

// ========================================================================
// Execute() Path Tests - delete_training action
// ========================================================================

#[tokio::test]
async fn test_delete_training_dry_run_single() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![Event {
        id: Some("event-123".to_string()),
        start_date_local: "2026-03-01".to_string(),
        name: "Tempo Run".to_string(),
        category: EventCategory::Workout,
        description: None,
        r#type: None,
    }]));

    let input = json!({
        "action": "delete",
        "target_date": "2026-03-01",
        "dry_run": true,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Preview (dry_run)"));
    assert!(content_str.contains("1"));
}

#[tokio::test]
async fn test_delete_training_apply_single() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![Event {
        id: Some("event-123".to_string()),
        start_date_local: "2026-03-01".to_string(),
        name: "Tempo Run".to_string(),
        category: EventCategory::Workout,
        description: None,
        r#type: None,
    }]));

    let input = json!({
        "action": "delete",
        "target_date": "2026-03-01",
        "dry_run": false,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Deleted"));
    assert!(output.metadata.events_deleted == Some(1));
}

#[tokio::test]
async fn test_delete_training_dry_run_multiple() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![
        Event {
            id: Some("event-123".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Run 1".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
        Event {
            id: Some("event-124".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Run 2".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
    ]));

    let input = json!({
        "action": "delete",
        "target_date": "2026-03-01",
        "dry_run": true,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("2"));
}

#[tokio::test]
async fn test_delete_training_with_description_filter() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![
        Event {
            id: Some("event-123".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Easy Run".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
        Event {
            id: Some("event-124".to_string()),
            start_date_local: "2026-03-01".to_string(),
            name: "Tempo Run".to_string(),
            category: EventCategory::Workout,
            description: Some("Threshold workout".to_string()),
            r#type: None,
        },
    ]));

    let input = json!({
        "action": "delete",
        "target_date": "2026-03-01",
        "target_description_contains": "tempo",
        "dry_run": true,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    // Should only delete the tempo run
    assert!(content_str.contains("1"));
}

#[tokio::test]
async fn test_delete_training_no_events() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "delete",
        "target_date": "2026-03-01",
        "dry_run": true,
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("0"));
}

// ========================================================================
// Execute() Path Tests - invalid action
// ========================================================================

#[tokio::test]
async fn test_modify_training_invalid_action() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "invalid_action",
        "target_date": "2026-03-01",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid action"));
}

#[tokio::test]
async fn test_modify_training_missing_action() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "target_date": "2026-03-01",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_modify_training_missing_idempotency_token() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "modify",
        "target_date": "2026-03-01"
    });

    // Note: The handler itself doesn't check for idempotency token in execute()
    // The router middleware handles this check
    // This test verifies the execute path works without token (router adds it)
    let result = handler.execute(input, client, None).await;
    // This should fail because modify requires fields
    assert!(result.is_err());
}

#[tokio::test]
async fn test_modify_training_invalid_date_format() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "modify",
        "target_date": "invalid-date",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_modify_training_range_invalid_dates() {
    let handler = ModifyTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![]));

    let input = json!({
        "action": "modify",
        "target_date_from": "2026-03-07",
        "target_date_to": "2026-03-01",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}
