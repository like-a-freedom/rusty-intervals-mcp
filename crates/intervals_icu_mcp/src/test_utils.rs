//! Shared test utilities and mock `IntervalsClient` implementations used by unit tests.
//!
//! Keep this module `#[cfg(test)]`-only and ensure behaviour matches existing inline mocks
//! so tests don't change their expectations.
#![cfg(test)]

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use serde_json::json;

/// A generic lightweight mock used by many unit tests.
/// Behaviour mirrors the previous in‑file `MockClient` implementations.
pub struct MockClient;

#[async_trait]
impl intervals_icu_client::IntervalsClient for MockClient {
    async fn get_athlete_profile(
        &self,
    ) -> Result<intervals_icu_client::AthleteProfile, intervals_icu_client::IntervalsError> {
        Ok(intervals_icu_client::AthleteProfile {
            id: "test_athlete".to_string(),
            name: Some("Test Athlete".to_string()),
        })
    }

    async fn get_recent_activities(
        &self,
        _limit: Option<u32>,
        _days_back: Option<i32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
    {
        Ok(vec![])
    }

    async fn create_event(
        &self,
        event: intervals_icu_client::Event,
    ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
        Ok(event)
    }

    async fn get_event(
        &self,
        _event_id: &str,
    ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
        unimplemented!()
    }

    async fn delete_event(
        &self,
        _event_id: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }

    async fn get_events(
        &self,
        _days_back: Option<i32>,
        _limit: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![])
    }

    async fn bulk_create_events(
        &self,
        events: Vec<intervals_icu_client::Event>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(events)
    }

    async fn get_activity_streams(
        &self,
        _activity_id: &str,
        _streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({ "streams": { "time": [1,2,3] } }))
    }

    async fn get_activity_intervals(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({ "intervals": [{ "start": 0, "end": 60 }] }))
    }

    async fn get_best_efforts(
        &self,
        _activity_id: &str,
        _options: Option<intervals_icu_client::BestEffortsOptions>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({ "best": [{ "duration": 600 }] }))
    }

    async fn get_activity_details(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({ "id": "a1" }))
    }

    async fn search_activities(
        &self,
        _q: &str,
        _limit: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
    {
        Ok(vec![])
    }

    async fn search_activities_full(
        &self,
        _q: &str,
        _limit: Option<u32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!([]))
    }

    async fn get_activities_csv(&self) -> Result<String, intervals_icu_client::IntervalsError> {
        Ok("id,start_date_local,name\n1,2025-10-18,Run".into())
    }

    async fn update_activity(
        &self,
        _activity_id: &str,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn download_activity_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }

    async fn download_activity_file_with_progress(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
        _cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        // send a few progress updates and complete
        let _ = progress_tx.try_send(intervals_icu_client::DownloadProgress {
            bytes_downloaded: 10,
            total_bytes: Some(100),
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let _ = progress_tx.try_send(intervals_icu_client::DownloadProgress {
            bytes_downloaded: 100,
            total_bytes: Some(100),
        });
        Ok(Some("/tmp/a1.fit".into()))
    }

    async fn download_fit_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }

    async fn download_gpx_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }

    async fn get_gear_list(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_sport_settings(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_power_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_gap_histogram(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_power_histogram(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_hr_histogram(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_pace_histogram(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_fitness_summary(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_wellness(
        &self,
        _days_back: Option<i32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_wellness_for_date(
        &self,
        _date: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn update_wellness(
        &self,
        _date: &str,
        _data: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn get_upcoming_workouts(
        &self,
        _days_ahead: Option<u32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }
    async fn delete_activity(
        &self,
        _activity_id: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
    async fn get_activities_around(
        &self,
        _activity_id: &str,
        _limit: Option<u32>,
        _route_id: Option<i64>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
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
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn update_event(
        &self,
        _event_id: &str,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn bulk_delete_events(
        &self,
        _event_ids: Vec<String>,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }

    async fn duplicate_event(
        &self,
        _event_id: &str,
        _num_copies: Option<u32>,
        _weeks_between: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![])
    }

    async fn get_hr_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn get_pace_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn get_workout_library(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!([]))
    }

    async fn get_workouts_in_folder(
        &self,
        _folder_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!([]))
    }

    async fn create_gear(
        &self,
        _gear: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn update_gear(
        &self,
        _gear_id: &str,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn delete_gear(
        &self,
        _gear_id: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }

    async fn create_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn update_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder_id: &str,
        _reset: bool,
        _snooze_days: u32,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn update_sport_settings(
        &self,
        _sport_type: &str,
        _recalc_hr_zones: bool,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn apply_sport_settings(
        &self,
        _sport_type: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn create_sport_settings(
        &self,
        _settings: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(json!({}))
    }

    async fn delete_sport_settings(
        &self,
        _sport_type: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
}
