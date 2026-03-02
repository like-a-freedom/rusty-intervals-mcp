//! Common test helpers for MCP integration tests.
//!
//! This module provides shared utilities for testing dynamic OpenAPI tools:
//! - MockClient: Mock implementation of IntervalsClient trait
//! - setup_mock_server: Helper to create wiremock server with common endpoints
//! - Test fixtures and builders

use async_trait::async_trait;
use intervals_icu_client::{
    ActivitySummary, AthleteProfile, BestEffortsOptions, DownloadProgress, Event, EventCategory,
    IntervalsClient, IntervalsError,
};
use serde_json::Value;
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Mock implementation of IntervalsClient for unit testing.
///
/// Returns stubbed responses for all trait methods.
/// Useful for testing MCP handler logic without real API calls.
pub struct MockClient;

#[async_trait]
impl IntervalsClient for MockClient {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
        Ok(AthleteProfile {
            id: "test_athlete".to_string(),
            name: Some("Test Athlete".to_string()),
        })
    }

    async fn get_recent_activities(
        &self,
        _limit: Option<u32>,
        _days_back: Option<i32>,
    ) -> Result<Vec<ActivitySummary>, IntervalsError> {
        Ok(vec![ActivitySummary {
            id: "test_activity".to_string(),
            name: Some("Test Activity".to_string()),
        }])
    }

    async fn create_event(&self, _event: Event) -> Result<Event, IntervalsError> {
        Ok(Event {
            id: Some("test_event".to_string()),
            start_date_local: "2025-01-01".to_string(),
            name: "Test Event".to_string(),
            category: EventCategory::Note,
            description: None,
            r#type: None,
        })
    }

    async fn get_event(&self, _event_id: &str) -> Result<Event, IntervalsError> {
        Ok(Event {
            id: Some("test_event".to_string()),
            start_date_local: "2025-01-01".to_string(),
            name: "Test Event".to_string(),
            category: EventCategory::Note,
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
        Ok(vec![Event {
            id: Some("test_event".to_string()),
            start_date_local: "2025-01-01".to_string(),
            name: "Test Event".to_string(),
            category: EventCategory::Note,
            description: None,
            r#type: None,
        }])
    }

    async fn bulk_create_events(&self, _events: Vec<Event>) -> Result<Vec<Event>, IntervalsError> {
        Ok(vec![])
    }

    async fn get_activity_streams(
        &self,
        _activity_id: &str,
        _streams: Option<Vec<String>>,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "power": [100, 200, 150],
            "heartrate": [120, 130, 125]
        }))
    }

    async fn get_activity_intervals(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "intervals": []
        }))
    }

    async fn get_best_efforts(
        &self,
        _activity_id: &str,
        _options: Option<BestEffortsOptions>,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "best_efforts": []
        }))
    }

    async fn get_activity_details(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "id": "test_activity",
            "name": "Test Activity"
        }))
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
        Ok(serde_json::json!([]))
    }

    async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
        Ok("id,name\n1,Test".to_string())
    }

    async fn update_activity(
        &self,
        _activity_id: &str,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "updated": true
        }))
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
        Ok(serde_json::json!([]))
    }

    async fn get_sport_settings(&self) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn get_power_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn get_gap_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
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
        Ok(serde_json::json!({
            "before": [],
            "after": []
        }))
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
        Ok(serde_json::json!([]))
    }

    async fn get_power_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn get_hr_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn get_pace_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn get_fitness_summary(&self) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([{
            "fitness": 50,
            "fatigue": 30,
            "form": 20
        }]))
    }

    async fn get_wellness(&self, _days_back: Option<i32>) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn get_wellness_for_date(&self, _date: &str) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn update_wellness(&self, _date: &str, _data: &Value) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "updated": true
        }))
    }

    async fn get_upcoming_workouts(
        &self,
        _days_ahead: Option<u32>,
        _limit: Option<u32>,
        _category: Option<String>,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn update_event(
        &self,
        _event_id: &str,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "updated": true
        }))
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
        Ok(serde_json::json!([]))
    }

    async fn get_pace_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn get_workout_library(&self) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn get_workouts_in_folder(&self, _folder_id: &str) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn create_folder(&self, _folder: &Value) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "id": "test_folder",
            "name": "Test Folder"
        }))
    }

    async fn update_folder(
        &self,
        _folder_id: &str,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "updated": true
        }))
    }

    async fn delete_folder(&self, _folder_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }

    async fn create_gear(&self, _gear: &Value) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "id": "test_gear",
            "name": "Test Gear"
        }))
    }

    async fn update_gear(&self, _gear_id: &str, _fields: &Value) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "updated": true
        }))
    }

    async fn delete_gear(&self, _gear_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }

    async fn create_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "id": "test_reminder"
        }))
    }

    async fn update_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder_id: &str,
        _reset: bool,
        _snooze_days: u32,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "updated": true
        }))
    }

    async fn update_sport_settings(
        &self,
        _sport_type: &str,
        _recalc_hr_zones: bool,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "updated": true
        }))
    }

    async fn apply_sport_settings(&self, _sport_type: &str) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "applied": true
        }))
    }

    async fn create_sport_settings(&self, _settings: &Value) -> Result<Value, IntervalsError> {
        Ok(serde_json::json!({
            "id": "test_settings"
        }))
    }

    async fn delete_sport_settings(&self, _id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }
}

/// Setup a mock HTTP server with common Intervals.icu API endpoints.
///
/// This helper creates a wiremock server and mounts common mocks:
/// - OpenAPI spec endpoint (/api/v1/docs)
/// - Athlete profile endpoint
/// - Activities endpoint
///
/// # Example
/// ```rust,ignore
/// let mock_server = setup_mock_server().await;
/// let client = create_client(mock_server.uri());
/// ```
pub async fn setup_mock_server() -> MockServer {
    let mock_server = MockServer::start().await;

    // Mock OpenAPI spec endpoint
    let openapi_spec = serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Intervals.icu API",
            "version": "1.0.0"
        },
        "paths": {
            "/api/v1/athlete/{id}/profile": {
                "get": {
                    "operationId": "getAthleteProfile",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"}
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "id": {"type": "string"},
                                            "name": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/api/v1/athlete/{id}/activities": {
                "get": {
                    "operationId": "listActivities",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"}
                        },
                        {
                            "name": "limit",
                            "in": "query",
                            "schema": {"type": "integer"}
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success"
                        }
                    }
                }
            }
        }
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/docs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openapi_spec))
        .mount(&mock_server)
        .await;

    // Mock athlete profile endpoint
    let profile_body = serde_json::json!({
        "id": "test_athlete",
        "name": "Test Athlete"
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(profile_body))
        .mount(&mock_server)
        .await;

    // Mock activities endpoint
    let activities_body = serde_json::json!([
        {
            "id": "act1",
            "name": "Test Activity 1"
        },
        {
            "id": "act2",
            "name": "Test Activity 2"
        }
    ]);

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/activities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(activities_body))
        .mount(&mock_server)
        .await;

    mock_server
}

/// Create a test client pointing to the mock server.
pub fn create_client(base_url: String) -> Arc<impl IntervalsClient> {
    use intervals_icu_client::http_client::ReqwestIntervalsClient;
    use secrecy::SecretString;

    Arc::new(ReqwestIntervalsClient::new(
        &base_url,
        "test_athlete".to_string(),
        SecretString::new("test_api_key".to_string().into()),
    ))
}

/// Builder for creating test OpenAPI specs with custom endpoints.
pub struct OpenApiSpecBuilder {
    spec: Value,
}

impl OpenApiSpecBuilder {
    pub fn new() -> Self {
        Self {
            spec: serde_json::json!({
                "openapi": "3.0.0",
                "info": {
                    "title": "Test API",
                    "version": "1.0.0"
                },
                "paths": {}
            }),
        }
    }

    pub fn add_endpoint(
        mut self,
        path: &str,
        method: &str,
        operation_id: &str,
        description: &str,
    ) -> Self {
        let paths = self.spec.get_mut("paths").unwrap().as_object_mut().unwrap();

        let path_entry = paths.entry(path).or_insert_with(|| serde_json::json!({}));
        let _path_obj = path_entry.as_object_mut().unwrap();

        let method_lower = method.to_lowercase();
        path_entry[&method_lower] = serde_json::json!({
            "operationId": operation_id,
            "summary": description,
            "parameters": [],
            "responses": {
                "200": {
                    "description": "Success"
                }
            }
        });

        self
    }

    pub fn build(self) -> Value {
        self.spec
    }
}

impl Default for OpenApiSpecBuilder {
    fn default() -> Self {
        Self::new()
    }
}
