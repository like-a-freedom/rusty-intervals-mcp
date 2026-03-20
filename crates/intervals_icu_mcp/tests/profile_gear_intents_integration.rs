use async_trait::async_trait;
use intervals_icu_client::{
    ActivitySummary, AthleteProfile, BestEffortsOptions, DownloadProgress, Event, EventCategory,
    IntervalsClient, IntervalsError,
};
use intervals_icu_mcp::intents::IntentHandler;
use intervals_icu_mcp::intents::handlers::{ManageGearHandler, ManageProfileHandler};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct MutationTrackingClient {
    sport_settings: Value,
    gear_list: Value,
    updated_sport_settings: Arc<Mutex<Vec<(String, bool, Value)>>>,
    applied_sport_settings: Arc<Mutex<Vec<String>>>,
    created_gear: Arc<Mutex<Vec<Value>>>,
    updated_gear: Arc<Mutex<Vec<(String, Value)>>>,
}

impl MutationTrackingClient {
    fn new() -> Self {
        Self {
            sport_settings: json!([
                {
                    "id": 1783043,
                    "types": ["Run", "VirtualRun", "TrailRun"],
                    "threshold_aet_hr": 150,
                    "lthr": 170,
                    "hr_zones": [144, 160, 167, 173, 180]
                }
            ]),
            gear_list: json!([
                {
                    "id": "gear-1",
                    "name": "Nike Pegasus 40",
                    "type": "Shoes",
                    "distance": 850000.0,
                    "retired": "",
                    "reminders": []
                }
            ]),
            updated_sport_settings: Arc::new(Mutex::new(Vec::new())),
            applied_sport_settings: Arc::new(Mutex::new(Vec::new())),
            created_gear: Arc::new(Mutex::new(Vec::new())),
            updated_gear: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn markdown_text(output: &intervals_icu_mcp::intents::IntentOutput) -> String {
        output
            .content
            .iter()
            .map(|block| match block {
                intervals_icu_mcp::intents::ContentBlock::Text { text } => text.clone(),
                intervals_icu_mcp::intents::ContentBlock::Markdown { markdown } => markdown.clone(),
                intervals_icu_mcp::intents::ContentBlock::Table { headers, rows } => {
                    format!("{:?}{:?}", headers, rows)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[async_trait]
impl IntervalsClient for MutationTrackingClient {
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
        Ok(event)
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

    async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }

    async fn get_events(
        &self,
        _days_back: Option<i32>,
        _limit: Option<u32>,
    ) -> Result<Vec<Event>, IntervalsError> {
        Ok(vec![])
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
        Ok(self.gear_list.clone())
    }

    async fn get_sport_settings(&self) -> Result<Value, IntervalsError> {
        Ok(self.sport_settings.clone())
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
        Ok(json!([]))
    }

    async fn update_event(
        &self,
        _event_id: &str,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn bulk_delete_events(&self, _event_ids: Vec<String>) -> Result<(), IntervalsError> {
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

    async fn create_gear(&self, gear: &Value) -> Result<Value, IntervalsError> {
        self.created_gear.lock().unwrap().push(gear.clone());
        Ok(gear.clone())
    }

    async fn update_gear(&self, gear_id: &str, fields: &Value) -> Result<Value, IntervalsError> {
        self.updated_gear
            .lock()
            .unwrap()
            .push((gear_id.to_string(), fields.clone()));
        Ok(fields.clone())
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
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &Value,
    ) -> Result<Value, IntervalsError> {
        self.updated_sport_settings.lock().unwrap().push((
            sport_type.to_string(),
            recalc_hr_zones,
            fields.clone(),
        ));
        Ok(fields.clone())
    }

    async fn apply_sport_settings(&self, sport_type: &str) -> Result<Value, IntervalsError> {
        self.applied_sport_settings
            .lock()
            .unwrap()
            .push(sport_type.to_string());
        Ok(json!({"status": "ok"}))
    }

    async fn create_sport_settings(&self, _settings: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }

    async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> {
        Ok(())
    }
}

#[tokio::test]
async fn manage_profile_update_thresholds_applies_to_activities_when_requested() {
    let client = Arc::new(MutationTrackingClient::new());
    let updated = client.updated_sport_settings.clone();
    let applied = client.applied_sport_settings.clone();
    let handler = ManageProfileHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "update_thresholds",
                "new_aet_hr": 155,
                "new_lt_hr": 175,
                "thresholds_source": "manual",
                "apply_to_activities": true,
                "idempotency_token": "profile-update-apply"
            }),
            client,
            None,
        )
        .await
        .expect("profile threshold update should call settings endpoints");

    let updates = updated.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, "Run");
    assert!(updates[0].1);
    assert_eq!(applied.lock().unwrap().as_slice(), ["Run"]);
    assert!(MutationTrackingClient::markdown_text(&output).contains("Apply to history: Yes"));
}

#[tokio::test]
async fn manage_gear_add_calls_create_gear_endpoint() {
    let client = Arc::new(MutationTrackingClient::new());
    let created = client.created_gear.clone();
    let handler = ManageGearHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "add",
                "new_gear_name": "Hoka Clifton 10",
                "new_gear_type": "shoes",
                "idempotency_token": "gear-add-1"
            }),
            client,
            None,
        )
        .await
        .expect("gear add should call create_gear");

    let created = created.lock().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(
        created[0].get("name").and_then(Value::as_str),
        Some("Hoka Clifton 10")
    );
    assert_eq!(
        created[0].get("type").and_then(Value::as_str),
        Some("Shoes")
    );
    assert!(MutationTrackingClient::markdown_text(&output).contains("Created"));
}

#[tokio::test]
async fn manage_gear_retire_calls_update_gear_endpoint() {
    let client = Arc::new(MutationTrackingClient::new());
    let updated = client.updated_gear.clone();
    let handler = ManageGearHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "retire",
                "gear_name": "Nike Pegasus 40",
                "idempotency_token": "gear-retire-1"
            }),
            client,
            None,
        )
        .await
        .expect("gear retire should call update_gear");

    let updated = updated.lock().unwrap();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].0, "gear-1");
    assert!(
        updated[0]
            .1
            .get("retired")
            .and_then(Value::as_str)
            .is_some()
    );
    assert!(MutationTrackingClient::markdown_text(&output).contains("Retired"));
}
