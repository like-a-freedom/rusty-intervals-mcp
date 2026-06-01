use once_cell::sync::Lazy;
use std::collections::HashMap;
use tokio::sync::{Mutex, MutexGuard};

static ENV_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub(crate) const DYNAMIC_RUNTIME_ENV_VARS: &[&str] = &[
    "INTERVALS_ICU_BASE_URL",
    "INTERVALS_ICU_ATHLETE_ID",
    "INTERVALS_ICU_API_KEY",
    "INTERVALS_ICU_OPENAPI_SPEC",
    "INTERVALS_ICU_SPEC_REFRESH_SECS",
];

pub(crate) struct EnvVarGuard {
    _guard: MutexGuard<'static, ()>,
    saved: HashMap<&'static str, Option<String>>,
}

impl EnvVarGuard {
    pub(crate) fn acquire_blocking(keys: &'static [&'static str]) -> Self {
        let guard = ENV_MUTEX.blocking_lock();
        let saved = snapshot_env(keys);
        Self {
            _guard: guard,
            saved,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => {
                    #[allow(unsafe_code)]
                    unsafe {
                        std::env::set_var(key, value);
                    }
                }
                None => {
                    #[allow(unsafe_code)]
                    unsafe {
                        std::env::remove_var(key);
                    }
                }
            }
        }
    }
}

fn snapshot_env(keys: &'static [&'static str]) -> HashMap<&'static str, Option<String>> {
    keys.iter()
        .copied()
        .map(|key| (key, std::env::var(key).ok()))
        .collect()
}

// ============================================================================
// Shared MockIntervalsClient for tests
// ============================================================================

#[cfg(test)]
pub(crate) mod mock {
    use async_trait::async_trait;
    use intervals_icu_client::{
        ActivityMessage, ActivitySummary, AthleteProfile, BestEffortsOptions, DownloadProgress,
        Event, EventCategory, IntervalsClient, IntervalsError,
    };
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Shared mock for `IntervalsClient`. Covers both single-activity and period test scenarios.
    ///
    /// Builder pattern — all fields default to empty/None:
    /// ```ignore
    /// let client = MockIntervalsClient::builder()
    ///     .with_activities(vec![...])
    ///     .with_workout_detail(json!({...}))
    ///     .build();
    /// ```
    /// Shared observation state for `MockIntervalsClient`. Cloned into both the mock
    /// and the test, so tests can inspect call counts even after the mock has been
    /// moved into a `dyn IntervalsClient` trait object.
    #[derive(Default, Debug)]
    pub(crate) struct MockObservations {
        pub wellness_last_days_back: Mutex<Option<i32>>,
        pub wellness_calls: AtomicUsize,
    }

    impl MockObservations {
        pub fn wellness_call_count(&self) -> usize {
            self.wellness_calls.load(Ordering::SeqCst)
        }

        pub fn wellness_last_days_back(&self) -> Option<i32> {
            *self
                .wellness_last_days_back
                .lock()
                .expect("wellness_last_days_back mutex poisoned")
        }
    }

    #[derive(Default)]
    pub(crate) struct MockIntervalsClient {
        pub activities: Vec<ActivitySummary>,
        pub events: Vec<Event>,
        pub fitness_summary: Option<Value>,
        pub workout_detail: Option<Value>,
        pub streams: Option<Value>,
        pub intervals: Option<Value>,
        pub best_efforts: Option<Value>,
        pub hr_histogram: Option<Value>,
        pub power_histogram: Option<Value>,
        pub pace_histogram: Option<Value>,
        pub activity_messages: Vec<ActivityMessage>,
        pub wellness: Option<Value>,
        pub activity_details: HashMap<String, Value>,
        pub athlete_profile: Option<AthleteProfile>,
        pub sport_settings: Option<Value>,
        pub gear_list: Option<Value>,
        pub update_error: Option<String>,
        pub upcoming_workouts: Option<Value>,
        pub upcoming_workouts_error: Option<IntervalsError>,
        pub upcoming_workouts_calls: Arc<AtomicUsize>,
        /// Observations shared with the test. `Arc` so the test can keep its own
        /// reference after the mock is wrapped in a trait object.
        pub observations: Arc<MockObservations>,
    }

    impl MockIntervalsClient {
        pub fn builder() -> Self {
            Self::default()
        }

        /// Convenience: create with a single activity shortcut.
        pub fn with_activity(activity_id: &str, date: &str, name: &str) -> Self {
            Self {
                activities: vec![ActivitySummary {
                    id: activity_id.to_string(),
                    name: Some(name.to_string()),
                    start_date_local: date.to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }
        }

        pub fn with_activities(mut self, activities: Vec<ActivitySummary>) -> Self {
            self.activities = activities;
            self
        }

        pub fn with_events(mut self, events: Vec<Event>) -> Self {
            self.events = events;
            self
        }

        pub fn with_fitness_summary(mut self, summary: Value) -> Self {
            self.fitness_summary = Some(summary);
            self
        }

        pub fn with_workout_detail(mut self, detail: Value) -> Self {
            self.workout_detail = Some(detail);
            self
        }

        pub fn with_streams(mut self, streams: Value) -> Self {
            self.streams = Some(streams);
            self
        }

        pub fn with_intervals(mut self, intervals: Value) -> Self {
            self.intervals = Some(intervals);
            self
        }

        pub fn with_best_efforts(mut self, best_efforts: Value) -> Self {
            self.best_efforts = Some(best_efforts);
            self
        }

        pub fn with_hr_histogram(mut self, histogram: Value) -> Self {
            self.hr_histogram = Some(histogram);
            self
        }

        pub fn with_power_histogram(mut self, histogram: Value) -> Self {
            self.power_histogram = Some(histogram);
            self
        }

        #[allow(dead_code)]
        pub fn with_pace_histogram(mut self, histogram: Value) -> Self {
            self.pace_histogram = Some(histogram);
            self
        }

        pub fn with_activity_messages(mut self, messages: Vec<ActivityMessage>) -> Self {
            self.activity_messages = messages;
            self
        }

        pub fn with_wellness(mut self, wellness: Value) -> Self {
            self.wellness = Some(wellness);
            self
        }

        pub fn with_activity_detail(mut self, id: &str, detail: Value) -> Self {
            self.activity_details.insert(id.to_string(), detail);
            self
        }

        pub fn with_athlete_profile(mut self, profile: AthleteProfile) -> Self {
            self.athlete_profile = Some(profile);
            self
        }

        pub fn with_sport_settings(mut self, settings: Value) -> Self {
            self.sport_settings = Some(settings);
            self
        }

        pub fn with_gear_list(mut self, list: Value) -> Self {
            self.gear_list = Some(list);
            self
        }

        pub fn with_update_error(mut self, error: impl Into<String>) -> Self {
            self.update_error = Some(error.into());
            self
        }

        pub fn with_upcoming_workouts(mut self, workouts: Value) -> Self {
            self.upcoming_workouts = Some(workouts);
            self
        }

        /// Returns the `MockObservations` shared with this mock. Tests can keep this
        /// handle and inspect call counts even after the mock is wrapped in a
        /// `dyn IntervalsClient` trait object.
        pub fn observations(&self) -> Arc<MockObservations> {
            self.observations.clone()
        }

        pub fn with_upcoming_workouts_error(mut self, error: IntervalsError) -> Self {
            self.upcoming_workouts_error = Some(error);
            self
        }

        pub fn upcoming_workouts_call_count(&self) -> usize {
            self.upcoming_workouts_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl IntervalsClient for MockIntervalsClient {
        async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
            Ok(self
                .athlete_profile
                .clone()
                .unwrap_or_else(|| AthleteProfile {
                    id: "test_athlete".to_string(),
                    name: Some("Test Athlete".to_string()),
                }))
        }

        async fn get_recent_activities(
            &self,
            _limit: Option<u32>,
            _days_back: Option<i32>,
        ) -> Result<Vec<ActivitySummary>, IntervalsError> {
            Ok(self.activities.clone())
        }

        async fn get_fitness_summary(&self) -> Result<Value, IntervalsError> {
            self.fitness_summary
                .clone()
                .ok_or_else(|| IntervalsError::NotFound("No fitness summary".to_string()))
        }

        async fn get_activity_details(&self, activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(self
                .activity_details
                .get(activity_id)
                .cloned()
                .or_else(|| self.workout_detail.clone())
                .unwrap_or_else(|| json!({})))
        }

        async fn get_activity_streams(
            &self,
            _activity_id: &str,
            _streams: Option<Vec<String>>,
        ) -> Result<Value, IntervalsError> {
            Ok(self.streams.clone().unwrap_or_else(|| json!({})))
        }

        async fn get_activity_intervals(
            &self,
            _activity_id: &str,
        ) -> Result<Value, IntervalsError> {
            Ok(self.intervals.clone().unwrap_or_else(|| json!({})))
        }

        async fn get_best_efforts(
            &self,
            _activity_id: &str,
            _options: Option<BestEffortsOptions>,
        ) -> Result<Value, IntervalsError> {
            Ok(self.best_efforts.clone().unwrap_or_else(|| json!({})))
        }

        async fn get_hr_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(self.hr_histogram.clone().unwrap_or_else(|| json!({})))
        }

        async fn get_power_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(self.power_histogram.clone().unwrap_or_else(|| json!({})))
        }

        async fn get_pace_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(self.pace_histogram.clone().unwrap_or_else(|| json!({})))
        }

        async fn get_activity_messages(
            &self,
            _activity_id: &str,
        ) -> Result<Vec<ActivityMessage>, IntervalsError> {
            Ok(self.activity_messages.clone())
        }

        async fn get_events(
            &self,
            _days_back: Option<i32>,
            _limit: Option<u32>,
        ) -> Result<Vec<Event>, IntervalsError> {
            Ok(self.events.clone())
        }

        async fn get_wellness_for_date(&self, _date: &str) -> Result<Value, IntervalsError> {
            self.wellness
                .clone()
                .ok_or_else(|| IntervalsError::NotFound("No wellness data".to_string()))
        }

        // -- Stubs (all return empty defaults) --

        async fn create_event(&self, _event: Event) -> Result<Event, IntervalsError> {
            Ok(Event {
                id: Some("test".to_string()),
                start_date_local: "2026-01-01".to_string(),
                name: "Test".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            })
        }

        async fn get_event(&self, _event_id: &str) -> Result<Event, IntervalsError> {
            Err(IntervalsError::NotFound("event not found".to_string()))
        }

        async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn bulk_create_events(
            &self,
            _events: Vec<Event>,
        ) -> Result<Vec<Event>, IntervalsError> {
            Ok(vec![])
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
            Ok("id,name\n1,Test".to_string())
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
            Ok(self.gear_list.clone().unwrap_or_else(|| json!([])))
        }

        async fn get_sport_settings(&self) -> Result<Value, IntervalsError> {
            Ok(self.sport_settings.clone().unwrap_or_else(|| json!([])))
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

        async fn get_wellness(&self, days_back: Option<i32>) -> Result<Value, IntervalsError> {
            self.observations
                .wellness_calls
                .fetch_add(1, Ordering::SeqCst);
            *self
                .observations
                .wellness_last_days_back
                .lock()
                .expect("wellness_last_days_back mutex poisoned") = days_back;
            Ok(self.wellness.clone().unwrap_or_else(|| json!([])))
        }

        async fn update_wellness(
            &self,
            _date: &str,
            _data: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_upcoming_workouts(
            &self,
            _days_ahead: Option<u32>,
            _limit: Option<u32>,
            _category: Option<String>,
        ) -> Result<Value, IntervalsError> {
            self.upcoming_workouts_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(err) = &self.upcoming_workouts_error {
                return Err(match err {
                    IntervalsError::Http(e) => IntervalsError::from_status(
                        e.status().map(|status| status.as_u16()).unwrap_or(500),
                        e.to_string(),
                    ),
                    IntervalsError::Config(e) => IntervalsError::Config(match e {
                        intervals_icu_client::ConfigError::MissingEnvVar(value) => {
                            intervals_icu_client::ConfigError::MissingEnvVar(value.clone())
                        }
                        intervals_icu_client::ConfigError::InvalidValue { key, message } => {
                            intervals_icu_client::ConfigError::InvalidValue {
                                key: key.clone(),
                                message: message.clone(),
                            }
                        }
                        intervals_icu_client::ConfigError::Other(message) => {
                            intervals_icu_client::ConfigError::Other(message.clone())
                        }
                    }),
                    IntervalsError::Api(api) => {
                        IntervalsError::Api(intervals_icu_client::ApiError {
                            status: api.status,
                            message: api.message.clone(),
                            raw_body: api.raw_body.clone(),
                        })
                    }
                    IntervalsError::JsonDecode(_) => {
                        IntervalsError::from_status(500, "json decode")
                    }
                    IntervalsError::Validation(validation) => {
                        IntervalsError::Validation(match validation {
                            intervals_icu_client::ValidationError::EmptyField { field } => {
                                intervals_icu_client::ValidationError::EmptyField {
                                    field: field.clone(),
                                }
                            }
                            intervals_icu_client::ValidationError::InvalidFormat {
                                field,
                                value,
                            } => intervals_icu_client::ValidationError::InvalidFormat {
                                field: field.clone(),
                                value: value.clone(),
                            },
                            intervals_icu_client::ValidationError::UnknownVariant {
                                field,
                                value,
                            } => intervals_icu_client::ValidationError::UnknownVariant {
                                field: field.clone(),
                                value: value.clone(),
                            },
                            intervals_icu_client::ValidationError::MissingParameter(value) => {
                                intervals_icu_client::ValidationError::MissingParameter(
                                    value.clone(),
                                )
                            }
                            intervals_icu_client::ValidationError::InvalidParameterCombination(
                                value,
                            ) => {
                                intervals_icu_client::ValidationError::InvalidParameterCombination(
                                    value.clone(),
                                )
                            }
                        })
                    }
                    IntervalsError::NotFound(message) => IntervalsError::NotFound(message.clone()),
                    IntervalsError::Auth(message) => IntervalsError::Auth(message.clone()),
                });
            }

            Ok(self.upcoming_workouts.clone().unwrap_or_else(|| json!([])))
        }

        async fn update_event(
            &self,
            _event_id: &str,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
            if let Some(ref err) = self.update_error {
                Err(IntervalsError::from_status(500, err.clone()))
            } else {
                Ok(json!({"updated": true}))
            }
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

        async fn create_gear(&self, _gear: &Value) -> Result<Value, IntervalsError> {
            Ok(json!({"id": "new_gear_id", "name": "New Gear"}))
        }

        async fn update_gear(
            &self,
            _gear_id: &str,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({"updated": true}))
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
            Ok(json!({"updated": true}))
        }

        async fn apply_sport_settings(&self, _sport_type: &str) -> Result<Value, IntervalsError> {
            Ok(json!({"applied": true}))
        }

        async fn create_sport_settings(&self, _settings: &Value) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn update_wellness_bulk(&self, _entries: &[Value]) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn get_weather_config(&self) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_weather_config(&self, _config: &Value) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn list_routes(&self) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_route(
            &self,
            _route_id: i64,
            _include_path: bool,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_route(
            &self,
            _route_id: i64,
            _route: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_route_similarity(
            &self,
            _route_id: i64,
            _other_id: i64,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockIntervalsClient;
    use super::*;
    use intervals_icu_client::{
        ActivityMessage, ActivitySummary, ApiError, AthleteProfile, ConfigError, Event,
        EventCategory, IntervalsClient, IntervalsError, ValidationError,
    };
    use serde_json::json;

    #[test]
    fn test_snapshot_env_captures_vars() {
        let keys: &[&str] = &["PATH", "HOME"];
        let snap = snapshot_env(keys);
        assert!(snap.contains_key("PATH"));
        assert!(snap.contains_key("HOME"));
        assert!(snap.get("PATH").unwrap().is_some());
    }

    #[test]
    fn test_snapshot_env_unset_var() {
        let snap = snapshot_env(&["_UNLIKELY_ENV_VAR_THAT_DOES_NOT_EXIST_XYZ_"]);
        assert!(snap.contains_key("_UNLIKELY_ENV_VAR_THAT_DOES_NOT_EXIST_XYZ_"));
        assert!(
            snap.get("_UNLIKELY_ENV_VAR_THAT_DOES_NOT_EXIST_XYZ_")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_snapshot_env_multiple_keys() {
        let snap = snapshot_env(&["PATH", "_MISSING_ENV_VAR_123_"]);
        assert_eq!(snap.len(), 2);
        assert!(snap.get("PATH").unwrap().is_some());
        assert!(snap.get("_MISSING_ENV_VAR_123_").unwrap().is_none());
    }

    #[test]
    fn test_env_var_guard_acquire_and_restore() {
        let _guard = EnvVarGuard::acquire_blocking(DYNAMIC_RUNTIME_ENV_VARS);
    }

    #[test]
    fn test_env_var_guard_restores_original() {
        const KEY: &str = "INTERVALS_ICU_BASE_URL";
        let original = std::env::var(KEY).ok();

        unsafe {
            std::env::set_var(KEY, "http://test-env-guard.local");
        }

        {
            let _guard = EnvVarGuard::acquire_blocking(DYNAMIC_RUNTIME_ENV_VARS);
            unsafe {
                std::env::set_var(KEY, "http://modified.local");
            }
        }

        let after = std::env::var(KEY).unwrap();
        assert_eq!(
            after, "http://test-env-guard.local",
            "EnvVarGuard should restore the env var to value at acquire time"
        );

        match original {
            Some(v) => unsafe {
                std::env::set_var(KEY, &v);
            },
            None => unsafe {
                std::env::remove_var(KEY);
            },
        }
    }

    #[test]
    fn test_env_var_guard_restores_removed_var() {
        let original = std::env::var("_TEST_ENV_GUARD_REMOVED_").ok();

        unsafe {
            std::env::remove_var("_TEST_ENV_GUARD_REMOVED_");
        }

        {
            let guard = EnvVarGuard::acquire_blocking(&["_TEST_ENV_GUARD_REMOVED_"]);
            unsafe {
                std::env::set_var("_TEST_ENV_GUARD_REMOVED_", "temporary");
            }
            drop(guard);
        }

        assert!(
            std::env::var("_TEST_ENV_GUARD_REMOVED_").is_err(),
            "EnvVarGuard should remove vars that were missing at acquire time"
        );

        if let Some(v) = original {
            unsafe {
                std::env::set_var("_TEST_ENV_GUARD_REMOVED_", &v);
            }
        }
    }

    #[test]
    fn test_env_var_guard_acquire_with_custom_keys() {
        unsafe {
            std::env::set_var("_CUSTOM_TEST_KEY_1_", "val1");
        }
        unsafe {
            std::env::remove_var("_CUSTOM_TEST_KEY_2_");
        }

        let guard = EnvVarGuard::acquire_blocking(&["_CUSTOM_TEST_KEY_1_", "_CUSTOM_TEST_KEY_2_"]);
        assert_eq!(
            guard.saved.get("_CUSTOM_TEST_KEY_1_").unwrap(),
            &Some("val1".into())
        );
        assert_eq!(guard.saved.get("_CUSTOM_TEST_KEY_2_").unwrap(), &None);

        unsafe {
            std::env::remove_var("_CUSTOM_TEST_KEY_1_");
        }
        unsafe {
            std::env::remove_var("_CUSTOM_TEST_KEY_2_");
        }
    }

    #[test]
    fn test_mock_with_activity() {
        let client = MockIntervalsClient::with_activity("act-1", "2026-03-21", "Morning Run");
        assert_eq!(client.activities.len(), 1);
        assert_eq!(client.activities[0].id, "act-1");
        assert_eq!(client.activities[0].name.as_deref(), Some("Morning Run"));
        assert_eq!(client.activities[0].start_date_local, "2026-03-21");
    }

    #[test]
    fn test_mock_with_activities() {
        let a1 = ActivitySummary {
            id: "1".into(),
            ..Default::default()
        };
        let a2 = ActivitySummary {
            id: "2".into(),
            ..Default::default()
        };
        let client = MockIntervalsClient::builder().with_activities(vec![a1, a2]);
        assert_eq!(client.activities.len(), 2);
    }

    #[test]
    fn test_mock_with_events() {
        let e = Event {
            id: Some("evt-1".into()),
            start_date_local: "2026-03-21".into(),
            name: "Test".into(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        };
        let client = MockIntervalsClient::builder().with_events(vec![e]);
        assert_eq!(client.events.len(), 1);
    }

    #[test]
    fn test_mock_with_fitness_summary() {
        let client = MockIntervalsClient::builder().with_fitness_summary(json!({"ctl": 100}));
        assert_eq!(client.fitness_summary, Some(json!({"ctl": 100})));
    }

    #[test]
    fn test_mock_with_workout_detail() {
        let client = MockIntervalsClient::builder().with_workout_detail(json!({"id": "w1"}));
        assert_eq!(client.workout_detail, Some(json!({"id": "w1"})));
    }

    #[test]
    fn test_mock_with_streams() {
        let client = MockIntervalsClient::builder().with_streams(json!({"watts": [1, 2, 3]}));
        assert_eq!(client.streams, Some(json!({"watts": [1, 2, 3]})));
    }

    #[test]
    fn test_mock_with_intervals() {
        let client = MockIntervalsClient::builder().with_intervals(json!([{"start": 0}]));
        assert_eq!(client.intervals, Some(json!([{"start": 0}])));
    }

    #[test]
    fn test_mock_with_best_efforts() {
        let client = MockIntervalsClient::builder().with_best_efforts(json!([{"distance": 5000}]));
        assert_eq!(client.best_efforts, Some(json!([{"distance": 5000}])));
    }

    #[test]
    fn test_mock_with_hr_histogram() {
        let client = MockIntervalsClient::builder().with_hr_histogram(json!({"zones": []}));
        assert_eq!(client.hr_histogram, Some(json!({"zones": []})));
    }

    #[test]
    fn test_mock_with_power_histogram() {
        let client = MockIntervalsClient::builder().with_power_histogram(json!({"zones": []}));
        assert_eq!(client.power_histogram, Some(json!({"zones": []})));
    }

    #[test]
    fn test_mock_with_pace_histogram() {
        let client = MockIntervalsClient::builder().with_pace_histogram(json!({"zones": []}));
        assert_eq!(client.pace_histogram, Some(json!({"zones": []})));
    }

    #[test]
    fn test_mock_with_activity_messages() {
        let msg = ActivityMessage {
            id: 1,
            athlete_id: None,
            name: Some("Test".into()),
            created: None,
            message_type: None,
            content: None,
            activity_id: None,
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        };
        let client = MockIntervalsClient::builder().with_activity_messages(vec![msg]);
        assert_eq!(client.activity_messages.len(), 1);
    }

    #[test]
    fn test_mock_with_wellness() {
        let client = MockIntervalsClient::builder().with_wellness(json!({"mood": 3}));
        assert_eq!(client.wellness, Some(json!({"mood": 3})));
    }

    #[test]
    fn test_mock_with_activity_detail() {
        let client =
            MockIntervalsClient::builder().with_activity_detail("act-1", json!({"hr": 150}));
        assert_eq!(
            client.activity_details.get("act-1"),
            Some(&json!({"hr": 150}))
        );
    }

    #[test]
    fn test_mock_with_athlete_profile() {
        let profile = AthleteProfile {
            id: "athlete-1".into(),
            name: Some("Test Athlete".into()),
        };
        let client = MockIntervalsClient::builder().with_athlete_profile(profile);
        assert!(client.athlete_profile.is_some());
    }

    #[test]
    fn test_mock_with_sport_settings() {
        let client =
            MockIntervalsClient::builder().with_sport_settings(json!([{"sport": "cycling"}]));
        assert_eq!(client.sport_settings, Some(json!([{"sport": "cycling"}])));
    }

    #[test]
    fn test_mock_with_gear_list() {
        let client = MockIntervalsClient::builder().with_gear_list(json!([{"name": "Bike"}]));
        assert_eq!(client.gear_list, Some(json!([{"name": "Bike"}])));
    }

    #[test]
    fn test_mock_with_upcoming_workouts() {
        let client =
            MockIntervalsClient::builder().with_upcoming_workouts(json!([{"name": "Workout"}]));
        assert!(client.upcoming_workouts.is_some());
    }

    #[test]
    fn test_mock_with_update_error() {
        let client = MockIntervalsClient::builder().with_update_error("something went wrong");
        assert_eq!(client.update_error, Some("something went wrong".into()));
    }

    #[test]
    fn test_mock_with_upcoming_workouts_error() {
        let err = IntervalsError::from_status(500, "server error");
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(client.upcoming_workouts_error.is_some());
    }

    #[test]
    fn test_mock_with_upcoming_workouts_and_call_count() {
        let client =
            MockIntervalsClient::builder().with_upcoming_workouts(json!([{"name": "Test"}]));
        assert_eq!(client.upcoming_workouts_call_count(), 0);
    }

    #[test]
    fn test_mock_default_empty() {
        let client = MockIntervalsClient::default();
        assert!(client.activities.is_empty());
        assert!(client.events.is_empty());
        assert!(client.fitness_summary.is_none());
        assert!(client.upcoming_workouts.is_none());
        assert_eq!(client.upcoming_workouts_call_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_get_recent_activities() {
        let client = MockIntervalsClient::with_activity("a1", "2026-03-21", "Run");
        let result = client.get_recent_activities(None, None).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "a1");
    }

    #[tokio::test]
    async fn test_mock_get_recent_activities_empty() {
        let client = MockIntervalsClient::default();
        let result = client.get_recent_activities(None, None).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_mock_get_fitness_summary_some() {
        let client = MockIntervalsClient::builder().with_fitness_summary(json!({"ctl": 80}));
        let result = client.get_fitness_summary().await.unwrap();
        assert_eq!(result, json!({"ctl": 80}));
    }

    #[tokio::test]
    async fn test_mock_get_fitness_summary_none() {
        let client = MockIntervalsClient::default();
        let err = client.get_fitness_summary().await.unwrap_err();
        assert!(matches!(err, IntervalsError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_mock_get_activity_details_from_map() {
        let client =
            MockIntervalsClient::builder().with_activity_detail("act-1", json!({"key": "value"}));
        let result = client.get_activity_details("act-1").await.unwrap();
        assert_eq!(result, json!({"key": "value"}));
    }

    #[tokio::test]
    async fn test_mock_get_activity_details_from_workout_detail() {
        let client = MockIntervalsClient::builder().with_workout_detail(json!({"fallback": true}));
        let result = client.get_activity_details("unknown").await.unwrap();
        assert_eq!(result, json!({"fallback": true}));
    }

    #[tokio::test]
    async fn test_mock_get_activity_details_empty() {
        let client = MockIntervalsClient::default();
        let result = client.get_activity_details("any").await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_get_events() {
        let e = Event {
            id: Some("evt-1".into()),
            start_date_local: "2026-03-21".into(),
            name: "Test".into(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        };
        let client = MockIntervalsClient::builder().with_events(vec![e]);
        let result = client.get_events(None, None).await.unwrap();
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_get_events_empty() {
        let client = MockIntervalsClient::default();
        let result = client.get_events(None, None).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_mock_get_wellness_for_date_some() {
        let client = MockIntervalsClient::builder().with_wellness(json!({"sleep": 8}));
        let result = client.get_wellness_for_date("2026-03-21").await.unwrap();
        assert_eq!(result, json!({"sleep": 8}));
    }

    #[tokio::test]
    async fn test_mock_get_wellness_for_date_none() {
        let client = MockIntervalsClient::default();
        let err = client
            .get_wellness_for_date("2026-03-21")
            .await
            .unwrap_err();
        assert!(matches!(err, IntervalsError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_mock_get_gear_list_some() {
        let client = MockIntervalsClient::builder().with_gear_list(json!([{"name": "Bike"}]));
        let result = client.get_gear_list().await.unwrap();
        assert_eq!(result, json!([{"name": "Bike"}]));
    }

    #[tokio::test]
    async fn test_mock_get_gear_list_none() {
        let client = MockIntervalsClient::default();
        let result = client.get_gear_list().await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_get_sport_settings_some() {
        let client = MockIntervalsClient::builder().with_sport_settings(json!([{"sport": "run"}]));
        let result = client.get_sport_settings().await.unwrap();
        assert_eq!(result, json!([{"sport": "run"}]));
    }

    #[tokio::test]
    async fn test_mock_get_sport_settings_none() {
        let client = MockIntervalsClient::default();
        let result = client.get_sport_settings().await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_update_event_success() {
        let client = MockIntervalsClient::default();
        let result = client
            .update_event("evt-1", &json!({"name": "New"}))
            .await
            .unwrap();
        assert_eq!(result, json!({"updated": true}));
    }

    #[tokio::test]
    async fn test_mock_update_event_error() {
        let client = MockIntervalsClient::builder().with_update_error("update failed");
        let err = client.update_event("evt-1", &json!({})).await.unwrap_err();
        assert!(err.to_string().contains("update failed"));
    }

    #[tokio::test]
    async fn test_mock_create_event() {
        let client = MockIntervalsClient::default();
        let e = Event {
            id: None,
            start_date_local: "2026-03-21".into(),
            name: "New".into(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        };
        let result = client.create_event(e).await.unwrap();
        assert_eq!(result.id, Some("test".into()));
    }

    #[tokio::test]
    async fn test_mock_get_event_not_found() {
        let client = MockIntervalsClient::default();
        let err = client.get_event("evt-1").await.unwrap_err();
        assert!(matches!(err, IntervalsError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_mock_delete_event() {
        let client = MockIntervalsClient::default();
        client.delete_event("evt-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_bulk_create_events() {
        let client = MockIntervalsClient::default();
        let result = client.bulk_create_events(vec![]).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_mock_get_athlete_profile_default() {
        let client = MockIntervalsClient::default();
        let profile = client.get_athlete_profile().await.unwrap();
        assert_eq!(profile.id, "test_athlete");
        assert_eq!(profile.name, Some("Test Athlete".into()));
    }

    #[tokio::test]
    async fn test_mock_get_athlete_profile_custom() {
        let profile = AthleteProfile {
            id: "custom".into(),
            name: Some("Custom".into()),
        };
        let client = MockIntervalsClient::builder().with_athlete_profile(profile);
        let result = client.get_athlete_profile().await.unwrap();
        assert_eq!(result.id, "custom");
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_success() {
        let client = MockIntervalsClient::builder().with_upcoming_workouts(json!([{"name": "W1"}]));
        let result = client
            .get_upcoming_workouts(None, None, None)
            .await
            .unwrap();
        assert_eq!(result, json!([{"name": "W1"}]));
        assert_eq!(client.upcoming_workouts_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_success_default() {
        let client = MockIntervalsClient::default();
        let result = client
            .get_upcoming_workouts(None, None, None)
            .await
            .unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_config_missing_env() {
        let err = IntervalsError::Config(ConfigError::MissingEnvVar("KEY".into()));
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_config_invalid_value() {
        let err = IntervalsError::Config(ConfigError::InvalidValue {
            key: "KEY".into(),
            message: "bad value".into(),
        });
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_config_other() {
        let err = IntervalsError::Config(ConfigError::Other("config error".into()));
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_api() {
        let err = IntervalsError::Api(ApiError::new(500, "server error", "body"));
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_json_decode() {
        let json_err = serde_json::from_str::<()>("invalid").unwrap_err();
        let err = IntervalsError::JsonDecode(json_err);
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_validation_empty_field() {
        let err = IntervalsError::Validation(ValidationError::EmptyField {
            field: "name".into(),
        });
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_validation_invalid_format() {
        let err = IntervalsError::Validation(ValidationError::InvalidFormat {
            field: "date".into(),
            value: "bad".into(),
        });
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_validation_unknown_variant() {
        let err = IntervalsError::Validation(ValidationError::UnknownVariant {
            field: "type".into(),
            value: "weird".into(),
        });
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_validation_missing_param() {
        let err =
            IntervalsError::Validation(ValidationError::MissingParameter("target_date".into()));
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_validation_invalid_param_comb() {
        let err = IntervalsError::Validation(ValidationError::InvalidParameterCombination(
            "cannot combine x and y".into(),
        ));
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_not_found() {
        let err = IntervalsError::NotFound("resource not found".into());
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_upcoming_workouts_error_auth() {
        let err = IntervalsError::Auth("unauthorized".into());
        let client = MockIntervalsClient::builder().with_upcoming_workouts_error(err);
        assert!(
            client
                .get_upcoming_workouts(None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_mock_search_activities() {
        let client = MockIntervalsClient::default();
        let result = client.search_activities("test", None).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_mock_search_activities_full() {
        let client = MockIntervalsClient::default();
        let result = client.search_activities_full("test", None).await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_get_activities_csv() {
        let client = MockIntervalsClient::default();
        let result = client.get_activities_csv().await.unwrap();
        assert_eq!(result, "id,name\n1,Test");
    }

    #[tokio::test]
    async fn test_mock_download_activity_file() {
        let client = MockIntervalsClient::default();
        let result = client.download_activity_file("a1", None).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_mock_download_activity_file_with_progress() {
        let client = MockIntervalsClient::default();
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let result = client
            .download_activity_file_with_progress("a1", None, tx, cancel_rx)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_mock_download_fit_file() {
        let client = MockIntervalsClient::default();
        let result = client.download_fit_file("a1", None).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_mock_download_gpx_file() {
        let client = MockIntervalsClient::default();
        let result = client.download_gpx_file("a1", None).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_mock_get_power_curves() {
        let client = MockIntervalsClient::default();
        let result = client.get_power_curves(None, "cycling").await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_get_gap_histogram() {
        let client = MockIntervalsClient::default();
        let result = client.get_gap_histogram("a1").await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_delete_activity() {
        let client = MockIntervalsClient::default();
        client.delete_activity("a1").await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_get_activities_around() {
        let client = MockIntervalsClient::default();
        let result = client
            .get_activities_around("a1", None, None)
            .await
            .unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_search_intervals() {
        let client = MockIntervalsClient::default();
        let result = client
            .search_intervals(0, 300, 80, 100, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_get_wellness() {
        let client = MockIntervalsClient::default();
        let result = client.get_wellness(None).await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_update_wellness() {
        let client = MockIntervalsClient::default();
        let result = client
            .update_wellness("2026-03-21", &json!({}))
            .await
            .unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_get_hr_curves() {
        let client = MockIntervalsClient::default();
        let result = client.get_hr_curves(None, "running").await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_get_pace_curves() {
        let client = MockIntervalsClient::default();
        let result = client.get_pace_curves(None, "running").await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_get_workout_library() {
        let client = MockIntervalsClient::default();
        let result = client.get_workout_library().await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_get_workouts_in_folder() {
        let client = MockIntervalsClient::default();
        let result = client.get_workouts_in_folder("folder-1").await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_create_folder() {
        let client = MockIntervalsClient::default();
        let result = client.create_folder(&json!({})).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_update_folder() {
        let client = MockIntervalsClient::default();
        let result = client.update_folder("folder-1", &json!({})).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_delete_folder() {
        let client = MockIntervalsClient::default();
        client.delete_folder("folder-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_create_gear() {
        let client = MockIntervalsClient::default();
        let result = client
            .create_gear(&json!({"name": "New Bike"}))
            .await
            .unwrap();
        assert_eq!(result, json!({"id": "new_gear_id", "name": "New Gear"}));
    }

    #[tokio::test]
    async fn test_mock_update_gear() {
        let client = MockIntervalsClient::default();
        let result = client.update_gear("gear-1", &json!({})).await.unwrap();
        assert_eq!(result, json!({"updated": true}));
    }

    #[tokio::test]
    async fn test_mock_delete_gear() {
        let client = MockIntervalsClient::default();
        client.delete_gear("gear-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_create_gear_reminder() {
        let client = MockIntervalsClient::default();
        let result = client
            .create_gear_reminder("gear-1", &json!({}))
            .await
            .unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_update_gear_reminder() {
        let client = MockIntervalsClient::default();
        let result = client
            .update_gear_reminder("gear-1", "rem-1", true, 7, &json!({}))
            .await
            .unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_update_sport_settings() {
        let client = MockIntervalsClient::default();
        let result = client
            .update_sport_settings("cycling", true, &json!({}))
            .await
            .unwrap();
        assert_eq!(result, json!({"updated": true}));
    }

    #[tokio::test]
    async fn test_mock_apply_sport_settings() {
        let client = MockIntervalsClient::default();
        let result = client.apply_sport_settings("running").await.unwrap();
        assert_eq!(result, json!({"applied": true}));
    }

    #[tokio::test]
    async fn test_mock_create_sport_settings() {
        let client = MockIntervalsClient::default();
        let result = client.create_sport_settings(&json!({})).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_delete_sport_settings() {
        let client = MockIntervalsClient::default();
        client.delete_sport_settings("cycling").await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_update_wellness_bulk() {
        let client = MockIntervalsClient::default();
        client.update_wellness_bulk(&[]).await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_get_weather_config() {
        let client = MockIntervalsClient::default();
        let result = client.get_weather_config().await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_update_weather_config() {
        let client = MockIntervalsClient::default();
        let result = client.update_weather_config(&json!({})).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_list_routes() {
        let client = MockIntervalsClient::default();
        let result = client.list_routes().await.unwrap();
        assert_eq!(result, json!([]));
    }

    #[tokio::test]
    async fn test_mock_get_route() {
        let client = MockIntervalsClient::default();
        let result = client.get_route(1, false).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_update_route() {
        let client = MockIntervalsClient::default();
        let result = client.update_route(1, &json!({})).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_get_route_similarity() {
        let client = MockIntervalsClient::default();
        let result = client.get_route_similarity(1, 2).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_bulk_delete_events() {
        let client = MockIntervalsClient::default();
        client
            .bulk_delete_events(vec!["evt-1".into()])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_mock_duplicate_event() {
        let client = MockIntervalsClient::default();
        let result = client
            .duplicate_event("evt-1", Some(3), Some(1))
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_mock_get_activity_streams_some() {
        let client = MockIntervalsClient::builder().with_streams(json!({"watts": [100, 200, 300]}));
        let result = client.get_activity_streams("a1", None).await.unwrap();
        assert_eq!(result, json!({"watts": [100, 200, 300]}));
    }

    #[tokio::test]
    async fn test_mock_get_activity_streams_none() {
        let client = MockIntervalsClient::default();
        let result = client.get_activity_streams("a1", None).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_get_activity_intervals_some() {
        let client = MockIntervalsClient::builder().with_intervals(json!([{"start": 0}]));
        let result = client.get_activity_intervals("a1").await.unwrap();
        assert_eq!(result, json!([{"start": 0}]));
    }

    #[tokio::test]
    async fn test_mock_get_activity_intervals_none() {
        let client = MockIntervalsClient::default();
        let result = client.get_activity_intervals("a1").await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_get_best_efforts_some() {
        let client = MockIntervalsClient::builder().with_best_efforts(json!([{"distance": 5000}]));
        let result = client.get_best_efforts("a1", None).await.unwrap();
        assert_eq!(result, json!([{"distance": 5000}]));
    }

    #[tokio::test]
    async fn test_mock_get_best_efforts_none() {
        let client = MockIntervalsClient::default();
        let result = client.get_best_efforts("a1", None).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_get_hr_histogram_some() {
        let client = MockIntervalsClient::builder().with_hr_histogram(json!({"zones": [1, 2]}));
        let result = client.get_hr_histogram("a1").await.unwrap();
        assert_eq!(result, json!({"zones": [1, 2]}));
    }

    #[tokio::test]
    async fn test_mock_get_hr_histogram_none() {
        let client = MockIntervalsClient::default();
        let result = client.get_hr_histogram("a1").await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_get_power_histogram_some() {
        let client =
            MockIntervalsClient::builder().with_power_histogram(json!({"zones": [100, 200]}));
        let result = client.get_power_histogram("a1").await.unwrap();
        assert_eq!(result, json!({"zones": [100, 200]}));
    }

    #[tokio::test]
    async fn test_mock_get_power_histogram_none() {
        let client = MockIntervalsClient::default();
        let result = client.get_power_histogram("a1").await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_get_pace_histogram_some() {
        let client =
            MockIntervalsClient::builder().with_pace_histogram(json!({"zones": [4.0, 5.0]}));
        let result = client.get_pace_histogram("a1").await.unwrap();
        assert_eq!(result, json!({"zones": [4.0, 5.0]}));
    }

    #[tokio::test]
    async fn test_mock_get_pace_histogram_none() {
        let client = MockIntervalsClient::default();
        let result = client.get_pace_histogram("a1").await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn test_mock_get_activity_messages() {
        let msg = ActivityMessage {
            id: 1,
            athlete_id: None,
            name: Some("Msg".into()),
            created: None,
            message_type: None,
            content: None,
            activity_id: None,
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        };
        let client = MockIntervalsClient::builder().with_activity_messages(vec![msg]);
        let result = client.get_activity_messages("a1").await.unwrap();
        assert_eq!(result.len(), 1);
    }
}
