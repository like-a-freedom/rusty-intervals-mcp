//! Minimal `IntervalsClient` trait and basic reqwest-based skeleton.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

pub mod config;
pub mod http_client;
pub mod observability;
pub mod retry;

#[derive(Debug, Error)]
pub enum IntervalsError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("configuration error: {0}")]
    Config(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AthleteProfile {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ActivitySummary {
    pub id: String,
    pub name: Option<String>,
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
    pub id: Option<String>,
    #[serde(rename = "start_date_local")]
    pub start_date_local: String, // YYYY-MM-DD
    pub name: String,
    pub category: EventCategory,
    pub description: Option<String>,
}

fn deserialize_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
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


#[async_trait]
pub trait IntervalsClient: Send + Sync + 'static {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError>;
    async fn get_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<u32>,
    ) -> Result<Vec<ActivitySummary>, IntervalsError>;
    async fn create_event(&self, event: Event) -> Result<Event, IntervalsError>;
    async fn get_event(&self, event_id: &str) -> Result<Event, IntervalsError>;
    async fn delete_event(&self, event_id: &str) -> Result<(), IntervalsError>;
    async fn get_events(
        &self,
        days_back: Option<u32>,
        limit: Option<u32>,
    ) -> Result<Vec<Event>, IntervalsError>;
    async fn bulk_create_events(&self, events: Vec<Event>) -> Result<Vec<Event>, IntervalsError>;
    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value, IntervalsError>;
    async fn get_activity_intervals(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError>;
    async fn get_best_efforts(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError>;
    async fn get_activity_details(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError>;
    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<crate::ActivitySummary>, IntervalsError>;
    async fn search_activities_full(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, IntervalsError>;
    async fn update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError>;
    async fn download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError>;
    /// Download activity file with progress notifications.
    ///
    /// The implementor should send `DownloadProgress` messages on `progress_tx` while
    /// downloading and should respect `cancel_rx` to abort early.
    async fn download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<DownloadProgress>,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>, IntervalsError>;
    async fn download_fit_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError>;
    async fn download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError>;
    async fn get_gear_list(&self) -> Result<serde_json::Value, IntervalsError>;
    async fn get_sport_settings(&self) -> Result<serde_json::Value, IntervalsError>;
    async fn get_power_curves(
        &self,
        days_back: Option<u32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError>;
    async fn get_gap_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError>;

    // === Activities: Missing Methods ===

    /// Delete an activity by ID
    async fn delete_activity(&self, activity_id: &str) -> Result<(), IntervalsError>;

    /// Get activities around a specific activity (for context)
    async fn get_activities_around(
        &self,
        activity_id: &str,
        limit: Option<u32>,
        route_id: Option<i64>,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Search intervals within activities
    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Get power histogram for an activity
    async fn get_power_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Get heart rate histogram for an activity
    async fn get_hr_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Get pace histogram for an activity
    async fn get_pace_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError>;

    // === Fitness Summary ===

    /// Get athlete fitness summary (CTL, ATL, TSB)
    async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError>;

    // === Wellness ===

    /// Get wellness data for recent days
    async fn get_wellness(
        &self,
        days_back: Option<u32>,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Get wellness data for a specific date
    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value, IntervalsError>;

    /// Update wellness data for a specific date
    async fn update_wellness(
        &self,
        date: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError>;

    // === Events/Calendar: Missing Methods ===

    /// Get upcoming workouts and events
    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Update an existing event
    async fn update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Bulk delete events by IDs
    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<(), IntervalsError>;

    /// Duplicate an event
    async fn duplicate_event(
        &self,
        event_id: &str,
        num_copies: Option<u32>,
        weeks_between: Option<u32>,
    ) -> Result<Vec<Event>, IntervalsError>;

    // === Performance Curves ===

    /// Get heart rate curves
    async fn get_hr_curves(
        &self,
        days_back: Option<u32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Get pace curves
    async fn get_pace_curves(
        &self,
        days_back: Option<u32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError>;

    // === Workout Library ===

    /// Get workout library folders and plans
    async fn get_workout_library(&self) -> Result<serde_json::Value, IntervalsError>;

    /// Get workouts in a specific folder
    async fn get_workouts_in_folder(
        &self,
        folder_id: &str,
    ) -> Result<serde_json::Value, IntervalsError>;

    // === Gear Management ===

    /// Create a new gear item
    async fn create_gear(
        &self,
        gear: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Update an existing gear item
    async fn update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Delete a gear item
    async fn delete_gear(&self, gear_id: &str) -> Result<(), IntervalsError>;

    /// Create a gear maintenance reminder
    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Update a gear maintenance reminder
    async fn update_gear_reminder(
        &self,
        gear_id: &str,
        reminder_id: &str,
        reset: bool,
        snooze_days: u32,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError>;

    // === Sport Settings Management ===

    /// Update sport settings
    async fn update_sport_settings(
        &self,
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Apply sport settings to historical activities
    async fn apply_sport_settings(
        &self,
        sport_type: &str,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Create new sport settings
    async fn create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError>;

    /// Delete sport settings
    async fn delete_sport_settings(&self, sport_type: &str) -> Result<(), IntervalsError>;
}

#[derive(Clone, Debug, Serialize, PartialEq, JsonSchema)]
pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use crate::http_client::ReqwestIntervalsClient;
    use serde_json::json;

    #[tokio::test]
    async fn client_new_and_basic() {
        let client = ReqwestIntervalsClient::new(
            "http://localhost",
            "ath",
            secrecy::SecretString::new("key".into()),
        );
        let _ = client;
    }

    #[test]
    fn deserialize_opt_string_from_number() {
        let payload = json!({"id": 123, "start_date_local": "2025-12-15", "name": "x", "category": "NOTE"});
        let e: super::Event = serde_json::from_value(payload).expect("deserialize number id");
        assert_eq!(e.id.unwrap(), "123");
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
