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

        async fn get_wellness(&self, _days_back: Option<i32>) -> Result<Value, IntervalsError> {
            Ok(json!([]))
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
