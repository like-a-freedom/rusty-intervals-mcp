//! Trait-based HTTP client for the Intervals.icu API.
//!
//! Provides the [`IntervalsClient`] async trait and a `ReqwestIntervalsClient`
//! implementation with circuit breaking, structured error reporting, and typed
//! domain models. See `http_client` for the implementation, `error` for the
//! error taxonomy, and `circuit_breaker` for upstream fail-fast semantics.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

pub mod circuit_breaker;
pub mod config;
pub mod domains;
pub mod error;
pub mod http_client;
pub mod utils;

pub use error::{ApiError, ConfigError, IntervalsError, Result, TransportError, ValidationError};

/// Options for finding best efforts in an activity.
///
/// If `options` is `Some`, at least `stream` must be provided along with
/// either `duration` or `distance` per API contract.
#[derive(Clone, Debug)]
pub struct BestEffortsOptions {
    pub stream: Option<String>,
    pub duration: Option<i32>,
    pub distance: Option<f64>,
    pub count: Option<i32>,
    pub min_value: Option<f64>,
    pub exclude_intervals: Option<bool>,
    pub start_index: Option<i32>,
    pub end_index: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AthleteProfile {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ActivitySummary {
    pub id: String,
    pub name: Option<String>,
    pub start_date_local: String,
    #[serde(default)]
    pub moving_time: Option<i32>,
    #[serde(default)]
    pub elapsed_time: Option<i32>,
    #[serde(default)]
    pub distance: Option<f64>,
    #[serde(default, rename = "icu_training_load")]
    pub training_load: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ActivityMessage {
    pub id: i64,
    pub athlete_id: Option<String>,
    pub name: Option<String>,
    pub created: Option<String>,
    #[serde(rename = "type")]
    pub message_type: Option<String>,
    pub content: Option<String>,
    pub activity_id: Option<String>,
    pub start_index: Option<i32>,
    pub end_index: Option<i32>,
    pub attachment_url: Option<String>,
    pub attachment_mime_type: Option<String>,
    pub deleted: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventCategory {
    Workout,
    RaceA,
    RaceB,
    RaceC,
    Note,
    Plan,
    Holiday,
    Sick,
    Injured,
    SetEftp,
    FitnessDays,
    SeasonStart,
    Target,
    SetFitness,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Event {
    #[serde(default, deserialize_with = "deserialize_opt_string")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "start_date_local")]
    pub start_date_local: String, // YYYY-MM-DD
    pub name: String,
    pub category: EventCategory,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub r#type: Option<String>,
}

fn deserialize_opt_string<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        Some(serde_json::Value::Number(n)) => Ok(n.to_string().into()),
        Some(other) => Err(D::Error::custom(format!(
            "expected string or number, got {other}"
        ))),
    }
}

#[async_trait::async_trait]
#[allow(clippy::too_many_arguments)]
pub trait IntervalsClient: Send + Sync + 'static {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile>;
    async fn get_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<i32>,
    ) -> Result<Vec<ActivitySummary>>;
    async fn create_event(&self, event: Event) -> Result<Event>;
    async fn get_event(&self, event_id: &str) -> Result<Event>;
    async fn delete_event(&self, event_id: &str) -> Result<()>;
    async fn get_events(&self, days_back: Option<i32>, limit: Option<u32>) -> Result<Vec<Event>>;
    async fn bulk_create_events(&self, events: Vec<Event>) -> Result<Vec<Event>>;
    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value>;
    async fn get_activity_intervals(&self, activity_id: &str) -> Result<serde_json::Value>;
    async fn get_best_efforts(
        &self,
        activity_id: &str,
        options: Option<BestEffortsOptions>,
    ) -> Result<serde_json::Value>;
    async fn get_activity_details(&self, activity_id: &str) -> Result<serde_json::Value>;
    async fn get_activity_messages(&self, _activity_id: &str) -> Result<Vec<ActivityMessage>> {
        Err(IntervalsError::Config(ConfigError::Unsupported {
            method: "get_activity_messages",
        }))
    }
    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<crate::ActivitySummary>>;
    async fn search_activities_full(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<serde_json::Value>;
    async fn get_activities_csv(&self) -> Result<String>;
    async fn update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    async fn download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>>;
    async fn download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<DownloadProgress>,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>>;
    async fn download_fit_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>>;
    async fn download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>>;
    async fn get_gear_list(&self) -> Result<serde_json::Value>;
    async fn get_sport_settings(&self) -> Result<domains::workout::SportSettings>;
    async fn get_power_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value>;
    async fn get_gap_histogram(&self, activity_id: &str) -> Result<serde_json::Value>;
    async fn delete_activity(&self, activity_id: &str) -> Result<()>;
    async fn get_activities_around(
        &self,
        activity_id: &str,
        limit: Option<u32>,
        route_id: Option<i64>,
    ) -> Result<serde_json::Value>;
    async fn search_intervals(
        &self,
        min_secs: u32,
        max_secs: u32,
        min_intensity: u32,
        max_intensity: u32,
        interval_type: Option<String>,
        min_reps: Option<u32>,
        max_reps: Option<u32>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value>;
    async fn get_power_histogram(&self, activity_id: &str) -> Result<serde_json::Value>;
    async fn get_hr_histogram(&self, activity_id: &str) -> Result<serde_json::Value>;
    async fn get_pace_histogram(&self, activity_id: &str) -> Result<serde_json::Value>;
    async fn get_fitness_summary(&self) -> Result<serde_json::Value>;
    async fn get_wellness(&self, days_back: Option<i32>) -> Result<serde_json::Value>;
    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value>;
    async fn update_wellness(
        &self,
        date: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    /// Update wellness entries in bulk (PUT `/athlete/{id}/wellness-bulk`).
    ///
    /// Production callers: none today. A real implementation lives in
    /// `ReqwestIntervalsClient::update_wellness_bulk` (`http_client.rs`)
    /// with a contract test in `tests/http_client_contract.rs`.
    ///
    /// Per ADR-0005, this is a **YAGNI candidate**: hidden from rendered
    /// docs until a production caller appears. Re-promote by removing the
    /// `#[doc(hidden)]` annotation when that happens.
    #[doc(hidden)]
    async fn update_wellness_bulk(&self, _entries: &[serde_json::Value]) -> Result<()> {
        Err(IntervalsError::Config(ConfigError::Unsupported {
            method: "update_wellness_bulk",
        }))
    }
    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
        limit: Option<u32>,
        category: Option<String>,
    ) -> Result<serde_json::Value>;
    async fn update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<()>;
    async fn duplicate_event(
        &self,
        event_id: &str,
        num_copies: Option<u32>,
        weeks_between: Option<u32>,
    ) -> Result<Vec<Event>>;
    async fn get_hr_curves(&self, days_back: Option<i32>, sport: &str)
    -> Result<serde_json::Value>;
    async fn get_pace_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value>;
    async fn get_workout_library(&self) -> Result<Vec<domains::workout::WorkoutItem>>;
    async fn get_workouts_in_folder(
        &self,
        folder_id: &str,
    ) -> Result<Vec<domains::workout::WorkoutItem>>;
    async fn create_folder(&self, folder: &serde_json::Value) -> Result<domains::workout::Folder>;
    async fn update_folder(
        &self,
        folder_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    async fn delete_folder(&self, folder_id: &str) -> Result<()>;
    async fn create_gear(&self, gear: &serde_json::Value) -> Result<serde_json::Value>;
    async fn update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    async fn delete_gear(&self, gear_id: &str) -> Result<()>;
    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    async fn update_gear_reminder(
        &self,
        gear_id: &str,
        reminder_id: &str,
        reset: bool,
        snooze_days: u32,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    async fn update_sport_settings(
        &self,
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    async fn apply_sport_settings(&self, sport_type: &str) -> Result<serde_json::Value>;
    async fn create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    async fn delete_sport_settings(&self, sport_type: &str) -> Result<()>;
    /// Get the athlete's weather configuration (GET
    /// `/athlete/{id}/weather-config`).
    ///
    /// Production callers: none today. A real implementation lives in
    /// `ReqwestIntervalsClient::get_weather_config` (`http_client.rs`).
    ///
    /// Per ADR-0005, this is a **YAGNI candidate**: hidden from rendered
    /// docs until a production caller appears.
    #[doc(hidden)]
    async fn get_weather_config(&self) -> Result<serde_json::Value> {
        Err(IntervalsError::Config(ConfigError::Unsupported {
            method: "get_weather_config",
        }))
    }
    /// Update the athlete's weather configuration (PUT
    /// `/athlete/{id}/weather-config`).
    ///
    /// Per ADR-0005, this is a **YAGNI candidate**: hidden from rendered
    /// docs until a production caller appears.
    #[doc(hidden)]
    async fn update_weather_config(
        &self,
        _config: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        Err(IntervalsError::Config(ConfigError::Unsupported {
            method: "update_weather_config",
        }))
    }
    /// List the athlete's routes (GET `/athlete/{id}/routes`).
    ///
    /// The `router.rs` smoke block in `intents/router.rs` exercises this
    /// path with a discarded `_ =` binding.
    ///
    /// Per ADR-0005, this is a **YAGNI candidate**: hidden from rendered
    /// docs until a production caller appears.
    #[doc(hidden)]
    async fn list_routes(&self) -> Result<serde_json::Value> {
        Err(IntervalsError::Config(ConfigError::Unsupported {
            method: "list_routes",
        }))
    }
    /// Fetch a single route by ID (GET `/athlete/{id}/routes/{route_id}`).
    ///
    /// Per ADR-0005, this is a **YAGNI candidate**: hidden from rendered
    /// docs until a production caller appears.
    #[doc(hidden)]
    async fn get_route(&self, _route_id: i64, _include_path: bool) -> Result<serde_json::Value> {
        Err(IntervalsError::Config(ConfigError::Unsupported {
            method: "get_route",
        }))
    }
    /// Update a route (PUT `/athlete/{id}/routes/{route_id}`).
    ///
    /// Per ADR-0005, this is a **YAGNI candidate**: hidden from rendered
    /// docs until a production caller appears.
    #[doc(hidden)]
    async fn update_route(
        &self,
        _route_id: i64,
        _route: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        Err(IntervalsError::Config(ConfigError::Unsupported {
            method: "update_route",
        }))
    }
    /// Compare route similarity between two routes (GET
    /// `/athlete/{id}/routes/{route_id}/similarity/{other_id}`).
    ///
    /// Per ADR-0005, this is a **YAGNI candidate**: hidden from rendered
    /// docs until a production caller appears.
    #[doc(hidden)]
    async fn get_route_similarity(
        &self,
        _route_id: i64,
        _other_id: i64,
    ) -> Result<serde_json::Value> {
        Err(IntervalsError::Config(ConfigError::Unsupported {
            method: "get_route_similarity",
        }))
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, JsonSchema)]
pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn deserialize_opt_string_from_number() {
        let payload =
            json!({"id": 123, "start_date_local": "2025-12-15", "name": "x", "category": "NOTE"});
        let e: super::Event = serde_json::from_value(payload).expect("deserialize number id");
        assert_eq!(e.id, Some("123".to_string()));
    }

    #[test]
    fn deserialize_opt_string_invalid_type_errors() {
        let payload = json!({"id": {"nested": true}, "start_date_local": "2025-12-15", "name": "x", "category": "NOTE"});
        let res: Result<super::Event, _> = serde_json::from_value(payload);
        assert!(res.is_err());
    }

    #[test]
    fn deserialize_event_category_unknown_maps_to_unknown() {
        // Unknown enum variants should deserialize to `EventCategory::Unknown` due to `#[serde(other)]`.
        let payload = json!({"id": "1", "start_date_local": "2025-12-15", "name": "x", "category": "NOT_A_KIND"});
        let ev: super::Event = serde_json::from_value(payload).expect("deserialize event");
        assert_eq!(ev.category, super::EventCategory::Unknown);
    }
}
