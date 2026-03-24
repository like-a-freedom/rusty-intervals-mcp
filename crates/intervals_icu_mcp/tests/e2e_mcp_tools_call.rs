//! E2E integration tests for MCP intent handlers.
//!
//! These tests verify the full pipeline:
//! Intent handler → MockClient → CoachContext → IntentOutput
//!
//! Tests use a mock IntervalsClient with realistic data
//! and verify that intent handlers produce correct, non-empty output.

use serde_json::json;
use std::sync::Arc;

use intervals_icu_client::AthleteProfile;
use intervals_icu_client::IntervalsClient;
use intervals_icu_mcp::intents::{IdempotencyMiddleware, IntentHandler, IntentRouter};

/// Mock client returning realistic data for E2E testing.
struct E2eMockClient;

#[async_trait::async_trait]
impl IntervalsClient for E2eMockClient {
    async fn get_athlete_profile(
        &self,
    ) -> Result<AthleteProfile, intervals_icu_client::IntervalsError> {
        Ok(AthleteProfile {
            id: "e2e-athlete".into(),
            name: Some("E2E Test Athlete".into()),
        })
    }

    async fn get_recent_activities(
        &self,
        _limit: Option<u32>,
        _days_back: Option<i32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
    {
        Ok(vec![
            intervals_icu_client::ActivitySummary {
                id: "act-1".to_string(),
                name: Some("Morning Run".to_string()),
                start_date_local: "2026-03-15".to_string(),
                ..Default::default()
            },
            intervals_icu_client::ActivitySummary {
                id: "act-2".to_string(),
                name: Some("Interval Session".to_string()),
                start_date_local: "2026-03-17".to_string(),
                ..Default::default()
            },
        ])
    }

    async fn get_fitness_summary(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({"ctl": 45.0, "atl": 52.0, "rampRate": 2.1}))
    }

    async fn get_wellness(
        &self,
        _days_back: Option<i32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!([
            {"id": "2026-03-15", "sleepSecs": 28800, "restingHR": 48, "hrv": 72.0},
            {"id": "2026-03-16", "sleepSecs": 27000, "restingHR": 50, "hrv": 68.0}
        ]))
    }

    async fn get_events(
        &self,
        _days_back: Option<i32>,
        _limit: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![
            intervals_icu_client::Event {
                id: Some("ev-1".to_string()),
                start_date_local: "2026-03-15".to_string(),
                name: "Planned Run".to_string(),
                category: intervals_icu_client::EventCategory::Workout,
                description: None,
                r#type: None,
            },
            intervals_icu_client::Event {
                id: Some("ev-2".to_string()),
                start_date_local: "2026-03-17".to_string(),
                name: "Planned Intervals".to_string(),
                category: intervals_icu_client::EventCategory::Workout,
                description: None,
                r#type: None,
            },
        ])
    }

    // Stubs for remaining trait methods
    async fn create_event(
        &self,
        event: intervals_icu_client::Event,
    ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
        Ok(event)
    }
    async fn get_event(
        &self,
        event_id: &str,
    ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
        Ok(intervals_icu_client::Event {
            id: Some(event_id.to_string()),
            start_date_local: "2026-03-15".to_string(),
            name: "Mock".to_string(),
            category: intervals_icu_client::EventCategory::Workout,
            description: None,
            r#type: None,
        })
    }
    async fn delete_event(&self, _: &str) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
    async fn bulk_create_events(
        &self,
        _: Vec<intervals_icu_client::Event>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![])
    }
    async fn get_activity_streams(
        &self,
        _: &str,
        _: Option<Vec<String>>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_activity_intervals(
        &self,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_activity_details(
        &self,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_best_efforts(
        &self,
        _: &str,
        _: Option<intervals_icu_client::BestEffortsOptions>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn download_activity_file(
        &self,
        _: &str,
        _: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }
    async fn download_activity_file_with_progress(
        &self,
        _: &str,
        _: Option<std::path::PathBuf>,
        _: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
        _: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }
    async fn delete_activity(&self, _: &str) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
    async fn get_activities_around(
        &self,
        _: &str,
        _: Option<u32>,
        _: Option<i64>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_sport_settings(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!([]))
    }
    async fn create_folder(
        &self,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn update_folder(
        &self,
        _: &str,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn delete_folder(&self, _: &str) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
    async fn get_gear_list(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!([]))
    }
    async fn create_gear(
        &self,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn update_gear(
        &self,
        _: &str,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn delete_gear(&self, _: &str) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
    async fn create_gear_reminder(
        &self,
        _: &str,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn update_gear_reminder(
        &self,
        _: &str,
        _: &str,
        _: bool,
        _: u32,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn update_sport_settings(
        &self,
        _: &str,
        _: bool,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn apply_sport_settings(
        &self,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn create_sport_settings(
        &self,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn delete_sport_settings(
        &self,
        _: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
    async fn get_power_curves(
        &self,
        _: Option<i32>,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_hr_curves(
        &self,
        _: Option<i32>,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_pace_curves(
        &self,
        _: Option<i32>,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_gap_histogram(
        &self,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn search_intervals(
        &self,
        _: u32,
        _: u32,
        _: u32,
        _: u32,
        _: Option<String>,
        _: Option<u32>,
        _: Option<u32>,
        _: Option<u32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_power_histogram(
        &self,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_hr_histogram(
        &self,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_pace_histogram(
        &self,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_wellness_for_date(
        &self,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn update_wellness(
        &self,
        _: &str,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_upcoming_workouts(
        &self,
        _: Option<u32>,
        _: Option<u32>,
        _: Option<String>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn update_event(
        &self,
        _: &str,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn bulk_delete_events(
        &self,
        _: Vec<String>,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
    async fn duplicate_event(
        &self,
        _: &str,
        _: Option<u32>,
        _: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![])
    }
    async fn download_fit_file(
        &self,
        _: &str,
        _: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }
    async fn download_gpx_file(
        &self,
        _: &str,
        _: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }
    async fn search_activities(
        &self,
        _: &str,
        _: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
    {
        Ok(vec![])
    }
    async fn search_activities_full(
        &self,
        _: &str,
        _: Option<u32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_activities_csv(&self) -> Result<String, intervals_icu_client::IntervalsError> {
        Ok(String::new())
    }
    async fn update_activity(
        &self,
        _: &str,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_activity_messages(
        &self,
        _: &str,
    ) -> Result<Vec<intervals_icu_client::ActivityMessage>, intervals_icu_client::IntervalsError>
    {
        Ok(vec![])
    }
    async fn get_workout_library(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!([]))
    }
    async fn get_workouts_in_folder(
        &self,
        _: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!([]))
    }
}

/// Build an IntentRouter with the mock client.
fn build_test_router() -> IntentRouter {
    let client: Arc<dyn IntervalsClient> = Arc::new(E2eMockClient);
    let idempotency = Arc::new(IdempotencyMiddleware::new());
    let handlers: Vec<Box<dyn IntentHandler>> = vec![
        Box::new(intervals_icu_mcp::intents::handlers::PlanTrainingHandler::new()),
        Box::new(intervals_icu_mcp::intents::handlers::AnalyzeTrainingHandler::new()),
        Box::new(intervals_icu_mcp::intents::handlers::ModifyTrainingHandler::new()),
        Box::new(intervals_icu_mcp::intents::handlers::ComparePeriodsHandler::new()),
        Box::new(intervals_icu_mcp::intents::handlers::AssessRecoveryHandler::new()),
        Box::new(intervals_icu_mcp::intents::handlers::ManageProfileHandler::new()),
        Box::new(intervals_icu_mcp::intents::handlers::ManageGearHandler::new()),
        Box::new(intervals_icu_mcp::intents::handlers::AnalyzeRaceHandler::new()),
    ];
    IntentRouter::new(handlers, client, idempotency)
}

#[tokio::test]
async fn e2e_analyze_training_period_returns_content() {
    let router = build_test_router();

    let result = router
        .route(
            "analyze_training",
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-20"
            }),
            Some("e2e-athlete"),
        )
        .await;

    assert!(
        result.is_ok(),
        "analyze_training should succeed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(!output.content.is_empty(), "content should not be empty");
}

#[tokio::test]
async fn e2e_assess_recovery_returns_content() {
    let router = build_test_router();

    let result = router
        .route(
            "assess_recovery",
            json!({"days_back": 7}),
            Some("e2e-athlete"),
        )
        .await;

    assert!(
        result.is_ok(),
        "assess_recovery should succeed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(!output.content.is_empty(), "content should not be empty");
}

#[tokio::test]
async fn e2e_compare_periods_returns_content() {
    let router = build_test_router();

    let result = router
        .route(
            "compare_periods",
            json!({
                "period_a_start": "2026-03-01",
                "period_a_end": "2026-03-10",
                "period_b_start": "2026-03-11",
                "period_b_end": "2026-03-20"
            }),
            Some("e2e-athlete"),
        )
        .await;

    assert!(
        result.is_ok(),
        "compare_periods should succeed: {:?}",
        result.err()
    );
    let output = result.unwrap();
    assert!(!output.content.is_empty(), "content should not be empty");
}

#[tokio::test]
async fn e2e_unknown_intent_returns_error() {
    let router = build_test_router();

    let result = router
        .route("intervals_nonexistent_tool", json!({}), None)
        .await;

    assert!(result.is_err(), "unknown intent should return error");
}
