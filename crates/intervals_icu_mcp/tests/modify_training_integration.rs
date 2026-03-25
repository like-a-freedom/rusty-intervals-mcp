use async_trait::async_trait;
use intervals_icu_client::ValidationError;
use intervals_icu_client::{
    ActivitySummary, AthleteProfile, BestEffortsOptions, DownloadProgress, Event, EventCategory,
    IntervalsClient, IntervalsError,
};
use intervals_icu_mcp::domains::events::validate_and_prepare_event;
use intervals_icu_mcp::intents::handlers::ModifyTrainingHandler;
use intervals_icu_mcp::intents::{ContentBlock, IntentHandler};
use intervals_icu_mcp::intents::{IdempotencyMiddleware, IntentError, IntentRouter};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct TrackingMockClient {
    events: Vec<Event>,
    upcoming_workouts: Value,
    updated_events: Arc<Mutex<Vec<(String, Value)>>>,
    created_events: Arc<Mutex<Vec<Event>>>,
    deleted_event_ids: Arc<Mutex<Vec<String>>>,
}

impl TrackingMockClient {
    fn with_future_strength_workout() -> Self {
        Self {
            events: vec![],
            upcoming_workouts: json!([
                {
                    "id": 94131981,
                    "start_date_local": "2026-03-26",
                    "name": "Gym — Legs (Recovery Week -30%)",
                    "category": "WORKOUT",
                    "description": "Strength session focused on neural stimulus and low soreness. (Recovery week -30% volume)",
                    "type": "WeightTraining",
                    "moving_time": 600,
                    "paired_activity_id": null
                }
            ]),
            updated_events: Arc::new(Mutex::new(Vec::new())),
            created_events: Arc::new(Mutex::new(Vec::new())),
            deleted_event_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_future_training_block() -> Self {
        Self {
            events: vec![],
            upcoming_workouts: json!([
                {
                    "id": 94131991,
                    "start_date_local": "2026-03-26",
                    "name": "Tempo Session",
                    "category": "WORKOUT",
                    "description": "Tempo build workout",
                    "type": "Run",
                    "moving_time": 3600,
                    "paired_activity_id": null
                },
                {
                    "id": 94131992,
                    "start_date_local": "2026-03-27",
                    "name": "Tempo Session",
                    "category": "WORKOUT",
                    "description": "Tempo build workout",
                    "type": "Run",
                    "moving_time": 4200,
                    "paired_activity_id": null
                },
                {
                    "id": 94131993,
                    "start_date_local": "2026-03-28",
                    "name": "Easy Run",
                    "category": "WORKOUT",
                    "description": "Recovery run",
                    "type": "Run",
                    "moving_time": 2700,
                    "paired_activity_id": null
                }
            ]),
            updated_events: Arc::new(Mutex::new(Vec::new())),
            created_events: Arc::new(Mutex::new(Vec::new())),
            deleted_event_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_future_calendar_events() -> Self {
        Self {
            events: vec![],
            upcoming_workouts: json!([
                {
                    "id": 94131994,
                    "start_date_local": "2026-03-26",
                    "name": "City Marathon",
                    "category": "RACE_A",
                    "description": "Race day",
                    "type": "Race",
                    "moving_time": 14400,
                    "paired_activity_id": null
                },
                {
                    "id": 94131995,
                    "start_date_local": "2026-03-27",
                    "name": "Sick day",
                    "category": "SICK",
                    "description": "Out sick, rest only",
                    "type": null,
                    "moving_time": 0,
                    "paired_activity_id": null
                }
            ]),
            updated_events: Arc::new(Mutex::new(Vec::new())),
            created_events: Arc::new(Mutex::new(Vec::new())),
            deleted_event_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

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
}

#[async_trait]
impl IntervalsClient for TrackingMockClient {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
        Ok(AthleteProfile {
            id: "athlete-1".into(),
            name: Some("Test Athlete".into()),
        })
    }

    async fn get_recent_activities(
        &self,
        _limit: Option<u32>,
        _days_back: Option<i32>,
    ) -> Result<Vec<ActivitySummary>, IntervalsError> {
        Ok(vec![])
    }

    async fn create_event(&self, event: Event) -> Result<Event, IntervalsError> {
        let prepared = validate_and_prepare_event(event).map_err(|err| {
            IntervalsError::Validation(ValidationError::InvalidFormat {
                field: "event".to_string(),
                value: err.to_string(),
            })
        })?;
        self.created_events.lock().unwrap().push(prepared.clone());
        Ok(prepared)
    }

    async fn get_event(&self, event_id: &str) -> Result<Event, IntervalsError> {
        Ok(Event {
            id: Some(event_id.to_string()),
            start_date_local: "2026-03-23".into(),
            name: "Mock event".into(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        })
    }

    async fn delete_event(&self, event_id: &str) -> Result<(), IntervalsError> {
        self.deleted_event_ids
            .lock()
            .unwrap()
            .push(event_id.to_string());
        Ok(())
    }

    async fn get_events(
        &self,
        _days_back: Option<i32>,
        _limit: Option<u32>,
    ) -> Result<Vec<Event>, IntervalsError> {
        Ok(self.events.clone())
    }

    async fn bulk_create_events(&self, _events: Vec<Event>) -> Result<Vec<Event>, IntervalsError> {
        Ok(vec![])
    }

    async fn get_activity_streams(
        &self,
        _activity_id: &str,
        _streams: Option<Vec<String>>,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn get_activity_intervals(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn get_best_efforts(
        &self,
        _activity_id: &str,
        _options: Option<BestEffortsOptions>,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_activity_details(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn search_activities(
        &self,
        _query: &str,
        _limit: Option<u32>,
    ) -> Result<Vec<ActivitySummary>, IntervalsError> {
        Ok(vec![])
    }

    async fn search_activities_full(
        &self,
        _query: &str,
        _limit: Option<u32>,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
        Ok(String::new())
    }

    async fn update_activity(
        &self,
        _activity_id: &str,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn download_activity_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        Ok(None)
    }

    async fn download_activity_file_with_progress(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
        _progress_tx: tokio::sync::mpsc::Sender<DownloadProgress>,
        _cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>, IntervalsError> {
        Ok(None)
    }

    async fn download_fit_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        Ok(None)
    }

    async fn download_gpx_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        Ok(None)
    }

    async fn get_gear_list(&self) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_sport_settings(&self) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_power_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_gap_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn delete_activity(&self, _activity_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }

    async fn get_activities_around(
        &self,
        _activity_id: &str,
        _limit: Option<u32>,
        _route_id: Option<i64>,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn search_intervals(
        &self,
        _min_secs: u32,
        _max_secs: u32,
        _min_intensity: u32,
        _max_intensity: u32,
        _interval_type: Option<String>,
        _min_reps: Option<u32>,
        _max_reps: Option<u32>,
        _limit: Option<u32>,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_power_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_hr_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_pace_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_fitness_summary(&self) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn get_wellness(&self, _days_back: Option<i32>) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_wellness_for_date(&self, _date: &str) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn update_wellness(&self, _date: &str, _data: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn get_upcoming_workouts(
        &self,
        _days_ahead: Option<u32>,
        _limit: Option<u32>,
        _category: Option<String>,
    ) -> Result<Value, IntervalsError> {
        Ok(self.upcoming_workouts.clone())
    }

    async fn update_event(&self, event_id: &str, fields: &Value) -> Result<Value, IntervalsError> {
        self.updated_events
            .lock()
            .unwrap()
            .push((event_id.to_string(), fields.clone()));
        Ok(json!({"id": event_id, "updated": true}))
    }

    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<(), IntervalsError> {
        self.deleted_event_ids.lock().unwrap().extend(event_ids);
        Ok(())
    }

    async fn duplicate_event(
        &self,
        _event_id: &str,
        _num_copies: Option<u32>,
        _weeks_between: Option<u32>,
    ) -> Result<Vec<Event>, IntervalsError> {
        Ok(vec![])
    }

    async fn get_hr_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_pace_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_workout_library(&self) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn get_workouts_in_folder(&self, _folder_id: &str) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }

    async fn create_folder(&self, _folder: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn update_folder(
        &self,
        _folder_id: &str,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn delete_folder(&self, _folder_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }

    async fn create_gear(&self, _gear: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn update_gear(&self, _gear_id: &str, _fields: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn delete_gear(&self, _gear_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }

    async fn create_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn update_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder_id: &str,
        _reset: bool,
        _snooze_days: u32,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn update_sport_settings(
        &self,
        _sport_type: &str,
        _recalc_hr_zones: bool,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn apply_sport_settings(&self, _sport_type: &str) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn create_sport_settings(&self, _settings: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> {
        Ok(())
    }
}

#[tokio::test]
async fn modify_training_dry_run_finds_future_workout_by_description() {
    let client = Arc::new(TrackingMockClient::with_future_strength_workout());
    let handler = ModifyTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "modify",
                "target_date": "2026-03-26",
                "target_description_contains": "neural stimulus",
                "new_duration": "1:00",
                "dry_run": true,
                "idempotency_token": "modify-future-dry-run"
            }),
            client,
            None,
        )
        .await
        .expect("dry run should find future workout");

    let markdown = TrackingMockClient::markdown_text(&output);
    assert!(markdown.contains("Modify Training - Preview (dry_run)"));
    assert!(markdown.contains("Gym — Legs (Recovery Week -30%)"));
    assert!(!markdown.contains("No events found"));
    assert_eq!(output.metadata.events_modified, Some(1));
}

#[tokio::test]
async fn modify_training_applies_duration_and_description_updates() {
    let client = Arc::new(TrackingMockClient::with_future_strength_workout());
    let updated_events = client.updated_events.clone();
    let handler = ModifyTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "modify",
                "target_date": "2026-03-26",
                "target_description_contains": "neural stimulus",
                "new_duration": "1:00",
                "new_description": "Strength session focused on neural stimulus. Duration 1h.",
                "idempotency_token": "modify-future-apply"
            }),
            client,
            None,
        )
        .await
        .expect("modify should apply updates");

    let updates = updated_events.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, "94131981");
    assert_eq!(
        updates[0].1.get("moving_time").and_then(Value::as_i64),
        Some(3600)
    );
    assert_eq!(
        updates[0].1.get("description").and_then(Value::as_str),
        Some("Strength session focused on neural stimulus. Duration 1h.")
    );
    assert_eq!(output.metadata.events_modified, Some(1));
}

#[tokio::test]
async fn create_training_accepts_target_date_as_alias_for_new_date() {
    let client = Arc::new(TrackingMockClient::with_future_strength_workout());
    let created_events = client.created_events.clone();
    let handler = ModifyTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "create",
                "target_date": "2026-04-10",
                "new_name": "Technical Downhill Session",
                "new_duration": "0:25",
                "new_description": "Purpose: spend 25 min practicing technical descents.",
                "dry_run": false,
                "idempotency_token": "create-alias-date"
            }),
            client,
            None,
        )
        .await
        .expect("create should accept target_date alias");

    let created = created_events.lock().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].start_date_local, "2026-04-10T00:00:00");
    assert_eq!(created[0].name, "Technical Downhill Session");
    assert_eq!(created[0].r#type.as_deref(), Some("Run"));
    assert_eq!(
        created[0].description.as_deref(),
        Some("Purpose: spend 25 min practicing technical descents.")
    );
    assert!(TrackingMockClient::markdown_text(&output).contains("2026-04-10"));
}

#[tokio::test]
async fn create_training_preserves_explicit_new_type() {
    let client = Arc::new(TrackingMockClient::with_future_strength_workout());
    let created_events = client.created_events.clone();
    let handler = ModifyTrainingHandler::new();

    handler
        .execute(
            json!({
                "action": "create",
                "target_date": "2026-04-11",
                "new_name": "Gym Session",
                "new_duration": "0:45",
                "new_category": "Workout",
                "new_type": "WeightTraining",
                "new_description": "Strength maintenance session.",
                "idempotency_token": "create-explicit-type"
            }),
            client,
            None,
        )
        .await
        .expect("create should preserve explicit workout type");

    let created = created_events.lock().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].start_date_local, "2026-04-11T00:00:00");
    assert_eq!(created[0].r#type.as_deref(), Some("WeightTraining"));
}

#[tokio::test]
async fn modify_training_normalizes_new_date_before_update() {
    let client = Arc::new(TrackingMockClient::with_future_strength_workout());
    let updated_events = client.updated_events.clone();
    let handler = ModifyTrainingHandler::new();

    handler
        .execute(
            json!({
                "action": "modify",
                "target_date": "2026-03-26",
                "target_description_contains": "neural stimulus",
                "new_date": "2026-03-27",
                "idempotency_token": "modify-normalized-date"
            }),
            client,
            None,
        )
        .await
        .expect("modify should normalize date before update");

    let updates = updated_events.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0].1.get("start_date_local").and_then(Value::as_str),
        Some("2026-03-27T00:00:00")
    );
}

#[tokio::test]
async fn modify_training_reports_filter_miss_when_date_has_events() {
    let client = Arc::new(TrackingMockClient::with_future_strength_workout());
    let handler = ModifyTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "modify",
                "target_date": "2026-03-26",
                "target_description_contains": "tempo",
                "new_duration": "1:00",
                "dry_run": true,
                "idempotency_token": "filter-miss-guidance"
            }),
            client,
            None,
        )
        .await
        .expect("filter miss should return guidance, not an error");

    let markdown = TrackingMockClient::markdown_text(&output);
    assert!(markdown.contains("No events matched the provided description filter"));
    assert!(markdown.contains("The date has scheduled training, but none matched 'tempo'"));
}

#[tokio::test]
async fn modify_training_finds_future_race_events() {
    let client = Arc::new(TrackingMockClient::with_future_calendar_events());
    let updated_events = client.updated_events.clone();
    let handler = ModifyTrainingHandler::new();

    handler
        .execute(
            json!({
                "action": "modify",
                "target_date": "2026-03-26",
                "target_description_contains": "marathon",
                "new_name": "City Marathon — Confirmed",
                "idempotency_token": "future-race-event"
            }),
            client,
            None,
        )
        .await
        .expect("modify should find race events in future calendar fetches");

    let updates = updated_events.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0].1.get("name").and_then(Value::as_str),
        Some("City Marathon — Confirmed")
    );
}

#[tokio::test]
async fn modify_training_finds_future_sick_events() {
    let client = Arc::new(TrackingMockClient::with_future_calendar_events());
    let deleted_event_ids = client.deleted_event_ids.clone();
    let handler = ModifyTrainingHandler::new();

    handler
        .execute(
            json!({
                "action": "delete",
                "target_date": "2026-03-27",
                "target_description_contains": "sick",
                "idempotency_token": "future-sick-event"
            }),
            client,
            None,
        )
        .await
        .expect("delete should find sick/absence events in future calendar fetches");

    let deleted = deleted_event_ids.lock().unwrap();
    assert_eq!(deleted.len(), 1);
}

#[tokio::test]
async fn router_allows_apply_after_dry_run_with_same_token() {
    let client = Arc::new(TrackingMockClient::with_future_strength_workout());
    let updated_events = client.updated_events.clone();
    let router = IntentRouter::new(
        vec![Box::new(ModifyTrainingHandler::new())],
        client,
        Arc::new(IdempotencyMiddleware::new()),
    );

    let preview = router
        .route(
            "modify_training",
            json!({
                "action": "modify",
                "target_date": "2026-03-26",
                "target_description_contains": "neural stimulus",
                "new_duration": "1:00",
                "dry_run": true,
                "idempotency_token": "shared-preview-apply-token"
            }),
            None,
        )
        .await
        .expect("dry_run should succeed");
    assert!(TrackingMockClient::markdown_text(&preview).contains("Preview (dry_run)"));

    let applied = router
        .route(
            "modify_training",
            json!({
                "action": "modify",
                "target_date": "2026-03-26",
                "target_description_contains": "neural stimulus",
                "new_duration": "1:00",
                "idempotency_token": "shared-preview-apply-token"
            }),
            None,
        )
        .await
        .expect("apply call should not be served from cached dry_run preview");

    assert!(TrackingMockClient::markdown_text(&applied).contains("Changes Applied"));
    assert_eq!(updated_events.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn router_rejects_same_token_for_different_mutation_payload() {
    let client = Arc::new(TrackingMockClient::with_future_strength_workout());
    let router = IntentRouter::new(
        vec![Box::new(ModifyTrainingHandler::new())],
        client,
        Arc::new(IdempotencyMiddleware::new()),
    );

    router
        .route(
            "modify_training",
            json!({
                "action": "modify",
                "target_date": "2026-03-23",
                "target_description_contains": "neural stimulus",
                "new_duration": "1:00",
                "idempotency_token": "reused-mutation-token"
            }),
            None,
        )
        .await
        .expect("first mutation should succeed");

    let err = router
        .route(
            "modify_training",
            json!({
                "action": "create",
                "new_date": "2026-04-10",
                "new_name": "Technical Downhill Session",
                "new_duration": "0:25",
                "idempotency_token": "reused-mutation-token"
            }),
            None,
        )
        .await
        .expect_err("reusing the same idempotency token for a different request should fail");

    assert!(matches!(err, IntentError::IdempotencyConflict(_)));
}

#[tokio::test]
async fn modify_training_range_updates_all_matching_events() {
    let client = Arc::new(TrackingMockClient::with_future_training_block());
    let updated_events = client.updated_events.clone();
    let handler = ModifyTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "modify",
                "target_date_from": "2026-03-26",
                "target_date_to": "2026-03-27",
                "target_description_contains": "tempo",
                "new_duration": "1:00",
                "new_description": "Adjusted tempo block",
                "idempotency_token": "modify-range-block"
            }),
            client,
            None,
        )
        .await
        .expect("range modify should update all matching workouts");

    let updates = updated_events.lock().unwrap();
    assert_eq!(updates.len(), 2);
    assert_eq!(output.metadata.events_modified, Some(2));
    assert!(TrackingMockClient::markdown_text(&output).contains("Affected: 2 event(s)"));
}

#[tokio::test]
async fn delete_training_range_bulk_deletes_all_matching_events() {
    let client = Arc::new(TrackingMockClient::with_future_training_block());
    let deleted_event_ids = client.deleted_event_ids.clone();
    let handler = ModifyTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "delete",
                "target_date_from": "2026-03-26",
                "target_date_to": "2026-03-27",
                "target_description_contains": "tempo",
                "idempotency_token": "delete-range-block"
            }),
            client,
            None,
        )
        .await
        .expect("range delete should delete all matching workouts");

    let deleted = deleted_event_ids.lock().unwrap();
    assert_eq!(deleted.len(), 2);
    assert!(deleted.iter().any(|id| id == "94131991"));
    assert!(deleted.iter().any(|id| id == "94131992"));
    assert_eq!(output.metadata.events_deleted, Some(2));
}
