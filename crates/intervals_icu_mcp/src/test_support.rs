#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::LazyLock;
#[cfg(test)]
use tokio::sync::{Mutex, MutexGuard};

#[cfg(test)]
static ENV_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[cfg(test)]
pub(crate) const DYNAMIC_RUNTIME_ENV_VARS: &[&str] = &[
    "INTERVALS_ICU_BASE_URL",
    "INTERVALS_ICU_ATHLETE_ID",
    "INTERVALS_ICU_API_KEY",
    "INTERVALS_ICU_OPENAPI_SPEC",
    "INTERVALS_ICU_SPEC_REFRESH_SECS",
];

#[cfg(test)]
pub(crate) struct EnvVarGuard {
    _guard: MutexGuard<'static, ()>,
    saved: HashMap<&'static str, Option<String>>,
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
fn snapshot_env(keys: &'static [&'static str]) -> HashMap<&'static str, Option<String>> {
    keys.iter()
        .copied()
        .map(|key| (key, std::env::var(key).ok()))
        .collect()
}

fn clone_intervals_error(
    err: &intervals_icu_client::IntervalsError,
) -> intervals_icu_client::IntervalsError {
    use intervals_icu_client::{ApiError, ConfigError, IntervalsError, ValidationError};

    match err {
        IntervalsError::Transport(e) => IntervalsError::from_status(
            // Transport errors don't carry an HTTP status; classify them as 500
            // (server/proxy-style) so upstream observability surfaces are still
            // discriminable. Tests can introspect the message via `err.to_string()`.
            500,
            e.to_string(),
        ),
        IntervalsError::Config(e) => IntervalsError::Config(match e {
            ConfigError::MissingEnvVar(value) => ConfigError::MissingEnvVar(value.clone()),
            ConfigError::InvalidValue { key, message } => ConfigError::InvalidValue {
                key: key.clone(),
                message: message.clone(),
            },
            ConfigError::Unsupported { method } => ConfigError::Unsupported { method },
            ConfigError::Other(message) => ConfigError::Other(message.clone()),
        }),
        IntervalsError::Api(api) => IntervalsError::Api(ApiError {
            status: api.status,
            message: api.message.clone(),
            raw_body: api.raw_body.clone(),
        }),
        IntervalsError::JsonDecode(_) => IntervalsError::from_status(500, "json decode"),
        IntervalsError::Validation(validation) => IntervalsError::Validation(match validation {
            ValidationError::EmptyField { field } => ValidationError::EmptyField {
                field: field.clone(),
            },
            ValidationError::InvalidFormat { field, value } => ValidationError::InvalidFormat {
                field: field.clone(),
                value: value.clone(),
            },
            ValidationError::UnknownVariant { field, value } => ValidationError::UnknownVariant {
                field: field.clone(),
                value: value.clone(),
            },
            ValidationError::MissingParameter(value) => {
                ValidationError::MissingParameter(value.clone())
            }
            ValidationError::InvalidParameterCombination(value) => {
                ValidationError::InvalidParameterCombination(value.clone())
            }
        }),
        IntervalsError::NotFound(message) => IntervalsError::NotFound(message.clone()),
        IntervalsError::Auth(message) => IntervalsError::Auth(message.clone()),
        IntervalsError::Io(e) => IntervalsError::Io(std::io::Error::new(e.kind(), e.to_string())),
        IntervalsError::Cancelled { reason } => IntervalsError::Cancelled {
            reason: reason.clone(),
        },
        IntervalsError::Decode { message, snippet } => IntervalsError::Decode {
            message: message.clone(),
            snippet: snippet.clone(),
        },
    }
}

// ============================================================================
// Shared MockIntervalsClient for tests
// ============================================================================

pub mod mock {
    use async_trait::async_trait;
    use intervals_icu_client::domains::workout::{Folder, SportSettings, WorkoutItem};
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
    pub struct MockObservations {
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
    pub struct MockIntervalsClient {
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
        pub sport_settings: Option<SportSettings>,
        pub gear_list: Option<Value>,
        pub update_error: Option<String>,
        pub upcoming_workouts: Option<Value>,
        pub upcoming_workouts_error: Option<IntervalsError>,
        pub upcoming_workouts_calls: Arc<AtomicUsize>,
        /// Observations shared with the test. `Arc` so the test can keep its own
        /// reference after the mock is wrapped in a trait object.
        pub observations: Arc<MockObservations>,
        /// Per-activity-id stream payload overrides. Falls back to `streams` when
        /// an id is missing.
        pub streams_map: HashMap<String, Value>,
        /// Per-activity-id detail payload overrides. When non-empty and an id is
        /// not found, returns `NotFound` instead of falling back.
        pub activity_details_map: HashMap<String, Value>,
        /// Injectable error for `get_activity_intervals`.
        pub intervals_error: Option<String>,
        /// Injectable error for `get_activity_streams`.
        pub streams_error: Option<String>,
        /// Wellness payload returned by `get_wellness_for_date`. Falls back to
        /// `wellness` when `None`.
        pub wellness_for_date_data: Option<Value>,
        /// Records `(limit, days_back)` arguments passed to `get_recent_activities`.
        #[allow(clippy::type_complexity)]
        pub activity_calls: Arc<Mutex<Vec<(Option<u32>, Option<i32>)>>>,
        /// Counts `get_activity_streams` calls for `ride-` prefixed ids
        /// (endurance evidence observation).
        pub profile_stream_calls: Arc<Mutex<usize>>,
        /// Records every `activity_id` passed to `get_activity_details`.
        /// Lets tests assert which activities were re-fetched in order.
        pub activity_details_calls: Arc<Mutex<Vec<String>>>,
        /// Activity ids whose `get_activity_details` calls should fail with
        /// `IntervalsError::NotFound`. Driven by `with_failing_activity_details`.
        pub failing_activity_detail_ids: std::collections::HashSet<String>,
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

        /// Builder ergonomic completeness: lets test authors set the pace
        /// histogram even when their assertion path does not read it back.
        /// Per ADR-0002 builder-completeness exemption.
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

        pub fn with_sport_settings(mut self, settings: SportSettings) -> Self {
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

        pub fn with_intervals_error(mut self, error: impl Into<String>) -> Self {
            self.intervals_error = Some(error.into());
            self
        }

        pub fn with_streams_error(mut self, error: impl Into<String>) -> Self {
            self.streams_error = Some(error.into());
            self
        }

        pub fn with_wellness_for_date(mut self, data: Value) -> Self {
            self.wellness_for_date_data = Some(data);
            self
        }

        pub fn with_stream_for_id(mut self, id: &str, streams: Value) -> Self {
            self.streams_map.insert(id.to_string(), streams);
            self
        }

        pub fn with_activity_details_map(mut self, map: HashMap<String, Value>) -> Self {
            self.activity_details_map = map;
            self
        }

        /// Mark the given activity ids as failing for `get_activity_details`.
        /// Mirrors the `with_failing_details` knob on the now-retired inline
        /// `DetailRecordingClient` mock.
        pub fn with_failing_activity_details(mut self, ids: Vec<&str>) -> Self {
            self.failing_activity_detail_ids = ids.into_iter().map(str::to_owned).collect();
            self
        }

        /// Snapshot of every `activity_id` passed to `get_activity_details`,
        /// in call order. Mirrors `requested_detail_ids()` on the now-retired
        /// inline mocks.
        pub fn requested_activity_detail_ids(&self) -> Vec<String> {
            self.activity_details_calls.lock().unwrap().clone()
        }

        pub fn activity_call_count(&self) -> usize {
            self.activity_calls.lock().unwrap().len()
        }

        pub fn activity_calls_snapshot(&self) -> Vec<(Option<u32>, Option<i32>)> {
            self.activity_calls.lock().unwrap().clone()
        }

        pub fn endurance_stream_calls(&self) -> usize {
            *self.profile_stream_calls.lock().unwrap()
        }

        pub fn observations_wellness_days(&self) -> Vec<Option<i32>> {
            self.observations
                .wellness_last_days_back
                .lock()
                .map(|opt| opt.map_or_else(Vec::new, |d| vec![Some(d)]))
                .unwrap_or_default()
        }
    }

    // ========================================================================
    // Scenario constructors (absorbed from MockCoachClient)
    // ========================================================================

    impl MockIntervalsClient {
        pub fn adaptive_wellness_series(
            baseline_sleep_secs: f64,
            baseline_resting_hr: f64,
            baseline_hrv: f64,
            recent_sleep_secs: f64,
            recent_resting_hr: f64,
            recent_hrv: f64,
        ) -> Value {
            let mut entries = Vec::new();
            entries.extend((0..28).map(|_| {
                json!({
                    "sleepSecs": baseline_sleep_secs,
                    "restingHR": baseline_resting_hr,
                    "hrv": baseline_hrv
                })
            }));
            entries.extend((0..7).map(|_| {
                json!({
                    "sleepSecs": recent_sleep_secs,
                    "restingHR": recent_resting_hr,
                    "hrv": recent_hrv
                })
            }));
            Value::Array(entries)
        }

        pub fn relative_date(days_from_today: i64) -> String {
            (chrono::Utc::now().date_naive() + chrono::Duration::days(days_from_today))
                .format("%Y-%m-%d")
                .to_string()
        }

        pub fn mock_event(event_id: Option<&str>) -> Event {
            Event {
                id: event_id.map(str::to_owned),
                start_date_local: "2026-03-04".to_string(),
                name: "Mock event".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            }
        }

        pub fn fitness_snapshot(fitness: f64, fatigue: f64, form: f64) -> Value {
            json!([{ "fitness": fitness, "fatigue": fatigue, "form": form }])
        }

        pub fn activity(activity_id: &str, name: &str, start_date_local: &str) -> ActivitySummary {
            ActivitySummary {
                id: activity_id.to_string(),
                name: Some(name.to_string()),
                start_date_local: start_date_local.to_string(),
                ..Default::default()
            }
        }

        pub fn with_tsb(tsb: f64) -> Self {
            Self {
                activities: vec![Self::activity("activity-1", "Hard Session", "2026-03-04")],
                fitness_summary: Some(Self::fitness_snapshot(50.0, 75.0, tsb)),
                activity_details: HashMap::from([(
                    "activity-1".to_string(),
                    json!({
                        "distance": 10000.0,
                        "moving_time": 3600,
                        "average_heartrate": 150.0,
                        "average_watts": 220.0,
                        "total_elevation_gain": 200.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_race_activity() -> Self {
            Self {
                activities: vec![Self::activity("race-1", "Mountain 50K", "2026-03-01")],
                events: vec![Event {
                    id: Some("event-race-1".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    name: "Mountain 50K Plan".to_string(),
                    category: intervals_icu_client::EventCategory::RaceA,
                    description: Some("Planned race target".to_string()),
                    r#type: Some("Race".to_string()),
                }],
                fitness_summary: Some(Self::fitness_snapshot(42.0, 68.0, -18.0)),
                wellness: Some(json!([
                    {"sleepSecs": 21600.0, "restingHR": 58.0, "hrv": 45.0},
                    {"sleepSecs": 21000.0, "restingHR": 60.0, "hrv": 42.0}
                ])),
                activity_details: HashMap::from([(
                    "race-1".to_string(),
                    json!({
                        "distance": 50000.0,
                        "moving_time": 18000,
                        "average_heartrate": 148.0,
                        "total_elevation_gain": 1800.0
                    }),
                )]),
                intervals: Some(json!([
                    {"moving_time": 1800, "average_heartrate": 145.0, "average_watts": 210.0},
                    {"moving_time": 1800, "average_heartrate": 152.0, "average_watts": 205.0}
                ])),
                streams: Some(json!({
                    "velocity_smooth": [3.0, 3.0, 3.0, 3.0, 3.0, 3.0],
                    "heartrate": [140.0, 141.0, 142.0, 150.0, 151.0, 152.0],
                    "watts": [220.0, 220.0, 220.0, 220.0, 220.0, 220.0]
                })),
                ..Default::default()
            }
        }

        pub fn with_period_blocks() -> Self {
            Self {
                activities: vec![
                    Self::activity("a1", "Run 1", "2026-03-01"),
                    Self::activity("a2", "Run 2", "2026-03-03"),
                    Self::activity("a3", "Run 3", "2026-02-25"),
                ],
                fitness_summary: Some(Self::fitness_snapshot(55.0, 45.0, 10.0)),
                activity_details: HashMap::from([(
                    "a1".to_string(),
                    json!({
                        "distance": 15000.0,
                        "moving_time": 5400,
                        "average_heartrate": 145.0,
                        "average_watts": 210.0,
                        "total_elevation_gain": 300.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_single_workout_degraded_streams() -> Self {
            Self {
                activities: vec![Self::activity("single-1", "Track Intervals", "2026-03-04")],
                fitness_summary: Some(json!({})),
                activity_details: HashMap::from([(
                    "single-1".to_string(),
                    json!({
                        "distance": 12000.0,
                        "moving_time": 4200,
                        "average_heartrate": 158.0,
                        "average_watts": 245.0,
                        "total_elevation_gain": 90.0
                    }),
                )]),
                intervals: Some(json!([
                    {"moving_time": 300, "average_heartrate": 162.0, "average_watts": 265.0},
                    {"moving_time": 300, "average_heartrate": 164.0, "average_watts": 268.0}
                ])),
                ..Default::default()
            }
        }

        pub fn with_race_degraded_context() -> Self {
            Self {
                activities: vec![Self::activity(
                    "race-degraded-1",
                    "Spring Marathon",
                    "2026-03-02",
                )],
                fitness_summary: Some(json!({})),
                activity_details: HashMap::from([(
                    "race-degraded-1".to_string(),
                    json!({
                        "distance": 42195.0,
                        "moving_time": 12600,
                        "average_heartrate": 151.0,
                        "total_elevation_gain": 180.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_positive_tsb_and_low_sleep() -> Self {
            Self {
                activities: vec![Self::activity(
                    "activity-1",
                    "Sharpening Session",
                    "2026-03-04",
                )],
                fitness_summary: Some(Self::fitness_snapshot(62.0, 48.0, 14.0)),
                wellness: Some(json!([
                    {"sleepSecs": 19800.0, "restingHR": 58.0, "hrv": 30.0},
                    {"sleepSecs": 20700.0, "restingHR": 60.0, "hrv": 34.0}
                ])),
                activity_details: HashMap::from([(
                    "activity-1".to_string(),
                    json!({
                        "distance": 12000.0,
                        "moving_time": 4300,
                        "average_heartrate": 150.0,
                        "average_watts": 230.0,
                        "total_elevation_gain": 120.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_supportive_recovery_metrics() -> Self {
            Self {
                activities: vec![Self::activity(
                    "supportive-1",
                    "Pre-race tune-up",
                    "2026-03-04",
                )],
                fitness_summary: Some(Self::fitness_snapshot(64.0, 46.0, 15.0)),
                wellness: Some(json!([
                    {"sleepSecs": 28800.0, "restingHR": 48.0, "hrv": 74.0},
                    {"sleepSecs": 28200.0, "restingHR": 49.0, "hrv": 71.0}
                ])),
                activity_details: HashMap::from([(
                    "supportive-1".to_string(),
                    json!({
                        "distance": 10000.0,
                        "moving_time": 3300,
                        "average_heartrate": 142.0,
                        "average_watts": 225.0,
                        "total_elevation_gain": 80.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_personal_hrv_drop_profile() -> Self {
            Self {
                activities: vec![Self::activity(
                    "adaptive-drop-1",
                    "Quality Session",
                    "2026-03-04",
                )],
                fitness_summary: Some(Self::fitness_snapshot(62.0, 48.0, 14.0)),
                wellness: Some(Self::adaptive_wellness_series(
                    28_800.0, 50.0, 60.0, 28_800.0, 50.0, 45.0,
                )),
                activity_details: HashMap::from([(
                    "adaptive-drop-1".to_string(),
                    json!({
                        "distance": 12000.0,
                        "moving_time": 4300,
                        "average_heartrate": 150.0,
                        "average_watts": 230.0,
                        "total_elevation_gain": 120.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_personal_hrv_norm_profile() -> Self {
            Self {
                activities: vec![Self::activity(
                    "adaptive-norm-1",
                    "Quality Session",
                    "2026-03-04",
                )],
                fitness_summary: Some(Self::fitness_snapshot(62.0, 48.0, 14.0)),
                wellness: Some(Self::adaptive_wellness_series(
                    28_800.0, 50.0, 44.0, 28_800.0, 50.0, 45.0,
                )),
                activity_details: HashMap::from([(
                    "adaptive-norm-1".to_string(),
                    json!({
                        "distance": 12000.0,
                        "moving_time": 4300,
                        "average_heartrate": 150.0,
                        "average_watts": 230.0,
                        "total_elevation_gain": 120.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_load_ramp_block() -> Self {
            let activities: Vec<ActivitySummary> = (1..=28)
                .map(|day| ActivitySummary {
                    id: format!("load-{day}"),
                    name: Some(format!("Run {day}")),
                    start_date_local: format!("2026-03-{day:02}"),
                    ..Default::default()
                })
                .collect();

            Self {
                activities,
                fitness_summary: Some(Self::fitness_snapshot(58.0, 50.0, 8.0)),
                activity_details: HashMap::from([(
                    "load-1".to_string(),
                    json!({
                        "distance": 12000.0,
                        "moving_time": 3600,
                        "average_heartrate": 145.0,
                        "average_watts": 215.0,
                        "total_elevation_gain": 120.0,
                        "icu_training_load": 55.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_stream_supported_workout() -> Self {
            Self {
                activities: vec![Self::activity("stream-1", "Tempo Session", "2026-03-04")],
                fitness_summary: Some(Self::fitness_snapshot(55.0, 47.0, 8.0)),
                activity_details: HashMap::from([(
                    "stream-1".to_string(),
                    json!({
                        "distance": 14000.0,
                        "moving_time": 3600,
                        "average_heartrate": 145.0,
                        "average_watts": 220.0,
                        "total_elevation_gain": 80.0
                    }),
                )]),
                intervals: Some(json!([])),
                streams: Some(json!({
                    "heartrate": [140.0, 141.0, 142.0, 144.0, 145.0, 146.0],
                    "watts": [220.0, 221.0, 222.0, 224.0, 225.0, 226.0]
                })),
                pace_histogram: Some(json!({
                    "zones": {
                        "z1": 600,
                        "z2": 1200,
                        "z3": 300
                    }
                })),
                ..Default::default()
            }
        }

        pub fn with_api_load_snapshot() -> Self {
            let mut client = Self::with_load_ramp_block();
            client.wellness_for_date_data = Some(json!({"atlLoad": 444.0, "ctlLoad": 333.0}));
            client
        }

        pub fn with_profile_metrics() -> Self {
            Self {
                fitness_summary: Some(json!([{
                    "fitness": 61.0,
                    "fatigue": 47.0,
                    "form": 14.0
                }])),
                sport_settings: Some(intervals_icu_client::domains::workout::SportSettings {
                    sports: vec![intervals_icu_client::domains::workout::SportSetting {
                        id: Some(1783043),
                        types: Some(vec!["Run".into(), "VirtualRun".into(), "TrailRun".into()]),
                        lthr: Some(171.0),
                        max_hr: Some(180.0),
                        hr_zones: vec![json!(144), json!(160), json!(167), json!(173), json!(180)],
                        threshold_pace: Some(3.7037036),
                        pace_units: Some("MINS_KM".into()),
                        load_order: Some("HR_PACE_POWER".into()),
                        ..Default::default()
                    }],
                    age: None,
                    weight: None,
                }),
                ..Default::default()
            }
        }

        pub fn with_mode_collapse_single_workout() -> Self {
            Self {
                activities: vec![Self::activity(
                    "mode-collapse-1",
                    "Uphill intervals",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "mode-collapse-1".to_string(),
                    json!({
                        "distance": 12240.0,
                        "moving_time": 4740,
                        "average_heartrate": 145.0,
                        "total_elevation_gain": 0.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_future_workouts_only() -> Self {
            let first_date = Self::relative_date(1);
            let second_date = Self::relative_date(2);

            Self {
                fitness_summary: Some(Self::fitness_snapshot(57.0, 43.0, 14.0)),
                upcoming_workouts: Some(json!([
                    {
                        "id": 94131802,
                        "category": "WORKOUT",
                        "start_date_local": format!("{first_date}T00:00:00"),
                        "description": "Recovery Run Z1",
                        "moving_time": 2700,
                        "icu_training_load": 30.0,
                        "paired_activity_id": null
                    },
                    {
                        "id": 94131803,
                        "category": "WORKOUT",
                        "start_date_local": format!("{second_date}T00:00:00"),
                        "description": "Endurance Run Z2 — Pre-Trip",
                        "moving_time": 6300,
                        "icu_training_load": 82.0,
                        "paired_activity_id": null
                    }
                ])),
                ..Default::default()
            }
        }

        pub fn with_future_calendar_events_only() -> Self {
            let race_date = Self::relative_date(1);
            let sick_date = Self::relative_date(2);

            Self {
                upcoming_workouts: Some(json!([
                    {
                        "id": 99131991,
                        "category": "RACE_A",
                        "start_date_local": format!("{race_date}T00:00:00"),
                        "description": "Race day",
                        "name": "City Marathon",
                        "type": "Race",
                        "moving_time": 14400,
                        "paired_activity_id": null
                    },
                    {
                        "id": 99131992,
                        "category": "SICK",
                        "start_date_local": format!("{sick_date}T00:00:00"),
                        "description": "Out sick, rest only",
                        "name": "Sick day",
                        "type": null,
                        "moving_time": 0,
                        "paired_activity_id": null
                    }
                ])),
                ..Default::default()
            }
        }

        pub fn with_paired_activity_and_calendar_duplicate() -> Self {
            let planned_date = Self::relative_date(0);

            Self {
                activities: vec![Self::activity(
                    "i130349092",
                    "Completed Endurance Run",
                    &format!("{planned_date}T07:00:00"),
                )],
                fitness_summary: Some(Self::fitness_snapshot(57.0, 43.0, 14.0)),
                upcoming_workouts: Some(json!([
                    {
                        "id": 94131804,
                        "category": "WORKOUT",
                        "start_date_local": format!("{planned_date}T00:00:00"),
                        "description": "Endurance Run Z2 — Key Workout",
                        "moving_time": 6300,
                        "icu_training_load": 82.0,
                        "paired_activity_id": "i130349092"
                    }
                ])),
                activity_details: HashMap::from([(
                    "i130349092".to_string(),
                    json!({
                        "distance": 18000.0,
                        "moving_time": 6300,
                        "icu_training_load": 82.0,
                        "total_elevation_gain": 220.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_profile_metrics_and_wellness_weight() -> Self {
            let mut client = Self::with_profile_metrics();
            client.wellness_for_date_data = Some(json!({
                "weight": 86.0,
                "restingHR": 54,
                "ctl": 40.13324,
                "atl": 40.22276
            }));
            client
        }

        pub fn with_recent_non_race_then_race_activity() -> Self {
            Self {
                activities: vec![
                    Self::activity("activity-regular-1", "Easy Run", "2026-03-06"),
                    Self::activity("race-2", "City Marathon Race", "2026-03-01"),
                ],
                events: vec![Event {
                    id: Some("planned-race-2".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    name: "City Marathon Race Plan".to_string(),
                    category: intervals_icu_client::EventCategory::RaceA,
                    description: Some("Goal marathon plan".to_string()),
                    r#type: Some("Race".to_string()),
                }],
                fitness_summary: Some(Self::fitness_snapshot(45.0, 60.0, -8.0)),
                activity_details: HashMap::from([(
                    "race-2".to_string(),
                    json!({
                        "distance": 42195.0,
                        "moving_time": 13200,
                        "average_heartrate": 149.0,
                        "total_elevation_gain": 120.0
                    }),
                )]),
                intervals: Some(json!([
                    {"moving_time": 1800, "average_heartrate": 145.0, "average_watts": 210.0},
                    {"moving_time": 1800, "average_heartrate": 152.0, "average_watts": 205.0}
                ])),
                streams: Some(json!({
                    "velocity_smooth": [3.1, 3.0, 2.9, 2.8],
                    "heartrate": [145.0, 148.0, 151.0, 154.0],
                    "watts": [220.0, 218.0, 210.0, 205.0]
                })),
                ..Default::default()
            }
        }

        pub fn with_mixed_period_workouts() -> Self {
            Self {
                activities: vec![
                    Self::activity("tempo-1", "Tempo Builder", "2026-03-01"),
                    Self::activity("long-1", "Long Run", "2026-03-03"),
                    Self::activity("tempo-2", "Tempo Cruise Intervals", "2026-02-25"),
                ],
                fitness_summary: Some(Self::fitness_snapshot(55.0, 45.0, 10.0)),
                activity_details: HashMap::from([(
                    "tempo-1".to_string(),
                    json!({
                        "distance": 15000.0,
                        "moving_time": 5400,
                        "average_heartrate": 145.0,
                        "average_watts": 210.0,
                        "total_elevation_gain": 300.0,
                        "tss": 77.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_mode_sensitive_single_workout() -> Self {
            Self {
                activities: vec![Self::activity("mode-1", "Progression Run", "2026-03-08")],
                fitness_summary: Some(Self::fitness_snapshot(58.0, 46.0, 12.0)),
                activity_details: HashMap::from([(
                    "mode-1".to_string(),
                    json!({
                        "distance": 7040.0,
                        "moving_time": 2880,
                        "average_heartrate": 127.0,
                        "average_watts": 188.0,
                        "total_elevation_gain": 66.0,
                        "decoupling": 2.8,
                        "icu_efficiency_factor": 1.74
                    }),
                )]),
                intervals: Some(json!([
                    {"moving_time": 600, "average_heartrate": 122.0, "average_watts": 175.0},
                    {"moving_time": 600, "average_heartrate": 129.0, "average_watts": 192.0}
                ])),
                streams: Some(json!({
                    "heartrate": [120.0, 122.0, 124.0, 126.0, 128.0, 130.0],
                    "watts": [170.0, 176.0, 182.0, 188.0, 194.0, 200.0],
                    "velocity_smooth": [2.9, 3.0, 3.1, 3.1, 3.0, 2.9]
                })),
                ..Default::default()
            }
        }

        pub fn with_object_shaped_interval_payload() -> Self {
            Self {
                activities: vec![Self::activity(
                    "i126027814",
                    "Uphill intervals",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "i126027814".to_string(),
                    json!({
                        "distance": 12240.0,
                        "moving_time": 4740,
                        "average_heartrate": 145.0,
                        "total_elevation_gain": 0.0
                    }),
                )]),
                intervals: Some(json!({
                    "id": "i126027814",
                    "icu_intervals": [
                        {"moving_time": 601, "average_heartrate": 126, "average_watts": null, "type": "WORK"},
                        {"moving_time": 300, "average_heartrate": 142, "average_watts": null, "type": "WORK"},
                        {"moving_time": 360, "average_heartrate": 158, "average_watts": null, "type": "WORK"}
                    ],
                    "icu_groups": [{"moving_time": 300, "average_heartrate": 138, "count": 6}]
                })),
                ..Default::default()
            }
        }

        pub fn with_interval_power_only_in_streams() -> Self {
            Self {
                activities: vec![Self::activity(
                    "power-fallback-1",
                    "Hill reps",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "power-fallback-1".to_string(),
                    json!({
                        "distance": 6400.0,
                        "moving_time": 1800,
                        "average_heartrate": 152.0,
                        "total_elevation_gain": 120.0,
                        "average_cadence": 86.0,
                        "icu_training_load": 74.0
                    }),
                )]),
                intervals: Some(json!({
                    "id": "power-fallback-1",
                    "icu_intervals": [
                        {"start_index": 0, "end_index": 4, "moving_time": 240, "average_heartrate": 150.0, "average_watts": null, "type": "WORK"},
                        {"start_index": 4, "end_index": 8, "moving_time": 240, "average_heartrate": 162.0, "average_watts": null, "type": "WORK"}
                    ]
                })),
                streams: Some(json!({
                    "watts": [210.0, 220.0, 230.0, 240.0, 280.0, 290.0, 300.0, 310.0],
                    "heartrate": [148.0, 149.0, 150.0, 151.0, 158.0, 160.0, 162.0, 164.0]
                })),
                ..Default::default()
            }
        }

        pub fn with_noncanonical_stream_payload() -> Self {
            Self {
                activities: vec![Self::activity(
                    "streams-weird-1",
                    "Tempo with sensors",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "streams-weird-1".to_string(),
                    json!({
                        "distance": 10000.0,
                        "moving_time": 2700,
                        "average_heartrate": 148.0,
                        "average_watts": 225.0,
                        "average_cadence": 85.0,
                        "total_elevation_gain": 40.0,
                        "icu_training_load": 63.0
                    }),
                )]),
                intervals: Some(json!([
                    {"moving_time": 300, "average_heartrate": 150.0, "average_watts": 240.0}
                ])),
                streams: Some(json!({
                    "streams": [
                        {"type": "heartrate", "data": [138.0, 142.0, 147.0, 151.0]},
                        {"type": "watts", "data": [205.0, 218.0, 231.0, 244.0]},
                        {"type": "cadence", "data": [82.0, 84.0, 86.0, 88.0]}
                    ]
                })),
                ..Default::default()
            }
        }

        pub fn with_rich_detailed_workout() -> Self {
            Self {
                activities: vec![Self::activity(
                    "detail-rich-1",
                    "Steady aerobic run",
                    "2026-03-08",
                )],
                fitness_summary: Some(Self::fitness_snapshot(58.0, 46.0, 12.0)),
                activity_details: HashMap::from([(
                    "detail-rich-1".to_string(),
                    json!({
                        "distance": 12000.0,
                        "moving_time": 3600,
                        "average_heartrate": 141.0,
                        "average_watts": 212.0,
                        "average_cadence": 84.5,
                        "average_speed": 3.3333333,
                        "average_temp": 19.4,
                        "total_elevation_gain": 95.0,
                        "tss": 78.5,
                        "icu_training_load": 81.0
                    }),
                )]),
                ..Default::default()
            }
        }

        pub fn with_interval_power_stream_alias() -> Self {
            Self {
                activities: vec![Self::activity(
                    "power-alias-1",
                    "Threshold reps",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "power-alias-1".to_string(),
                    json!({
                        "distance": 9000.0,
                        "moving_time": 2700,
                        "average_heartrate": 151.0,
                        "average_speed": 3.2,
                        "total_elevation_gain": 70.0
                    }),
                )]),
                intervals: Some(json!({
                    "id": "power-alias-1",
                    "icu_intervals": [
                        {"start_index": 0, "end_index": 3, "moving_time": 180, "average_heartrate": 148.0, "average_watts": null, "type": "WORK"},
                        {"start_index": 3, "end_index": 6, "moving_time": 180, "average_heartrate": 156.0, "average_watts": null, "type": "WORK"}
                    ]
                })),
                streams: Some(json!({
                    "power": [250.0, 255.0, 260.0, 300.0, 305.0, 310.0],
                    "heartrate": [145.0, 148.0, 151.0, 153.0, 156.0, 159.0],
                    "velocity_smooth": [2.9, 3.0, 3.1, 3.2, 3.3, 3.4]
                })),
                ..Default::default()
            }
        }

        pub fn with_interval_output_only_in_speed_streams() -> Self {
            Self {
                activities: vec![Self::activity(
                    "speed-fallback-1",
                    "Run intervals from pace stream",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "speed-fallback-1".to_string(),
                    json!({
                        "distance": 11000.0,
                        "moving_time": 3600,
                        "average_heartrate": 148.0,
                        "total_elevation_gain": 0.0
                    }),
                )]),
                intervals: Some(json!({
                    "id": "speed-fallback-1",
                    "icu_intervals": [
                        {"start_index": 0, "end_index": 4, "moving_time": 240, "average_heartrate": 146.0, "average_watts": null, "average_speed": 3.0, "type": "WORK"},
                        {"start_index": 4, "end_index": 8, "moving_time": 240, "average_heartrate": 156.0, "average_watts": null, "average_speed": 3.2, "type": "WORK"}
                    ]
                })),
                streams: Some(json!({
                    "velocity_smooth": [3.0, 3.0, 3.0, 3.0, 3.2, 3.2, 3.2, 3.2],
                    "heartrate": [144.0, 145.0, 146.0, 147.0, 153.0, 155.0, 156.0, 158.0]
                })),
                ..Default::default()
            }
        }

        pub fn with_many_intervals() -> Self {
            let intervals_data: Vec<Value> = (0..15)
                .map(|idx| {
                    let start = idx * 4;
                    json!({
                        "start_index": start,
                        "end_index": start + 4,
                        "moving_time": if idx % 2 == 0 { 300 } else { 360 },
                        "average_heartrate": 140.0 + idx as f64,
                        "average_watts": 220.0 + idx as f64,
                        "type": "WORK"
                    })
                })
                .collect();

            Self {
                activities: vec![Self::activity(
                    "many-intervals-1",
                    "Big interval session",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "many-intervals-1".to_string(),
                    json!({
                        "distance": 16000.0,
                        "moving_time": 5400,
                        "average_heartrate": 149.0,
                        "total_elevation_gain": 120.0
                    }),
                )]),
                intervals: Some(json!({
                    "id": "many-intervals-1",
                    "icu_intervals": intervals_data
                })),
                streams: Some(json!({
                    "watts": (0..60).map(|idx| 220.0 + idx as f64).collect::<Vec<_>>(),
                    "heartrate": (0..60).map(|idx| 135.0 + idx as f64 * 0.5).collect::<Vec<_>>()
                })),
                ..Default::default()
            }
        }

        pub fn with_priority_streams_without_power() -> Self {
            Self {
                activities: vec![Self::activity(
                    "priority-streams-1",
                    "Uphill intervals",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "priority-streams-1".to_string(),
                    json!({
                        "distance": 12240.0,
                        "moving_time": 4740,
                        "average_heartrate": 145.0,
                        "average_speed": 2.5822785,
                        "average_cadence": 82.0,
                        "total_elevation_gain": 0.0
                    }),
                )]),
                intervals: Some(json!([])),
                streams: Some(json!([
                    {"type": "time", "data": [0, 1, 2, 3]},
                    {"type": "cadence", "data": [80.0, 81.0, 82.0, 83.0]},
                    {"type": "heartrate", "data": [138.0, 142.0, 147.0, 151.0]},
                    {"type": "distance", "data": [0.0, 100.0, 200.0, 300.0]},
                    {"type": "altitude", "data": [152.2, 152.2, 152.2, 152.2]},
                    {"type": "velocity_smooth", "data": [2.50, 2.55, 2.60, 2.68]},
                    {"type": "temp", "data": [26.0, 26.2, 26.4, 26.5]},
                    {"type": "GroundContactTime", "data": [250.0, 255.0, 260.0, 265.0]},
                    {"type": "VerticalOscillation", "data": [70.0, 72.0, 74.0, 76.0]}
                ])),
                ..Default::default()
            }
        }

        pub fn with_best_efforts_and_bucket_histograms() -> Self {
            Self {
                activities: vec![Self::activity(
                    "payload-1",
                    "Structured Long Run",
                    "2026-03-08",
                )],
                fitness_summary: Some(Self::fitness_snapshot(60.0, 44.0, 16.0)),
                activity_details: HashMap::from([(
                    "payload-1".to_string(),
                    json!({
                        "distance": 18000.0,
                        "moving_time": 5400,
                        "average_heartrate": 138.0,
                        "average_watts": 215.0,
                        "total_elevation_gain": 110.0
                    }),
                )]),
                best_efforts: Some(json!({
                    "best_efforts": [
                        {"seconds": 60, "watts": 310.0, "heartrate": 171.0},
                        {"seconds": 300, "watts": 282.0, "heartrate": 165.0}
                    ]
                })),
                hr_histogram: Some(json!([
                    {"min": 120, "max": 124, "secs": 469},
                    {"min": 125, "max": 129, "secs": 1150}
                ])),
                power_histogram: Some(json!([
                    {"min": 200, "max": 224, "secs": 1525},
                    {"min": 225, "max": 249, "secs": 1021}
                ])),
                pace_histogram: Some(json!([
                    {"min": 2.2593105, "max": 2.354023, "secs": 295},
                    {"min": 2.354023, "max": 2.4487357, "secs": 353}
                ])),
                ..Default::default()
            }
        }

        pub fn with_full_histogram_ranges() -> Self {
            Self {
                activities: vec![Self::activity(
                    "hist-full-1",
                    "Recovery Run Z1",
                    "2026-03-08",
                )],
                fitness_summary: Some(Self::fitness_snapshot(58.0, 46.0, 12.0)),
                activity_details: HashMap::from([(
                    "hist-full-1".to_string(),
                    json!({
                        "distance": 7040.0,
                        "moving_time": 2880,
                        "average_heartrate": 127.0,
                        "average_watts": 219.0,
                        "total_elevation_gain": 66.0
                    }),
                )]),
                hr_histogram: Some(json!([
                    {"min": 80, "max": 84, "secs": 1},
                    {"min": 85, "max": 89, "secs": 5},
                    {"min": 90, "max": 94, "secs": 8},
                    {"min": 95, "max": 99, "secs": 17},
                    {"min": 100, "max": 104, "secs": 29},
                    {"min": 105, "max": 109, "secs": 40},
                    {"min": 110, "max": 114, "secs": 54},
                    {"min": 115, "max": 119, "secs": 190},
                    {"min": 120, "max": 124, "secs": 469},
                    {"min": 125, "max": 129, "secs": 1150},
                    {"min": 130, "max": 134, "secs": 720},
                    {"min": 135, "max": 139, "secs": 151},
                    {"min": 140, "max": 144, "secs": 22},
                    {"min": 145, "max": 149, "secs": 9},
                    {"min": 150, "max": 154, "secs": 3}
                ])),
                power_histogram: Some(json!([
                    {"min": 0, "max": 24, "secs": 71},
                    {"min": 25, "max": 49, "secs": 9},
                    {"min": 50, "max": 74, "secs": 8},
                    {"min": 75, "max": 99, "secs": 7},
                    {"min": 100, "max": 124, "secs": 6},
                    {"min": 125, "max": 149, "secs": 5},
                    {"min": 150, "max": 174, "secs": 4},
                    {"min": 175, "max": 199, "secs": 84},
                    {"min": 200, "max": 224, "secs": 1525},
                    {"min": 225, "max": 249, "secs": 1021},
                    {"min": 250, "max": 274, "secs": 91},
                    {"min": 275, "max": 299, "secs": 24},
                    {"min": 300, "max": 324, "secs": 3},
                    {"min": 325, "max": 349, "secs": 1}
                ])),
                pace_histogram: Some(json!([
                    {"min": 0.93333334, "max": 1.028046, "secs": 3},
                    {"min": 1.028046, "max": 1.1227586, "secs": 12},
                    {"min": 1.1227586, "max": 1.2174712, "secs": 18},
                    {"min": 1.2174712, "max": 1.3121839, "secs": 22},
                    {"min": 1.3121839, "max": 1.4068965, "secs": 27},
                    {"min": 1.4068965, "max": 1.5016091, "secs": 31},
                    {"min": 1.5016091, "max": 1.5963217, "secs": 36},
                    {"min": 1.5963217, "max": 1.6910343, "secs": 41},
                    {"min": 1.6910343, "max": 1.785747, "secs": 48},
                    {"min": 1.785747, "max": 1.8804595, "secs": 55},
                    {"min": 1.8804595, "max": 1.9751722, "secs": 58},
                    {"min": 1.9751722, "max": 2.0698848, "secs": 59},
                    {"min": 2.0698848, "max": 2.1645975, "secs": 61},
                    {"min": 2.1645975, "max": 2.2593105, "secs": 74},
                    {"min": 2.2593105, "max": 2.354023, "secs": 295},
                    {"min": 2.354023, "max": 2.4487357, "secs": 353},
                    {"min": 2.4487357, "max": 2.5434482, "secs": 166},
                    {"min": 2.5434482, "max": 2.6381607, "secs": 117},
                    {"min": 2.6381607, "max": 2.7328734, "secs": 89},
                    {"min": 2.7328734, "max": 2.8275862, "secs": 61},
                    {"min": 2.8275862, "max": 2.922299, "secs": 44},
                    {"min": 2.922299, "max": 3.0170114, "secs": 29},
                    {"min": 3.0170114, "max": 3.1117241, "secs": 17},
                    {"min": 3.1117241, "max": 3.2064366, "secs": 8},
                    {"min": 3.2064366, "max": 3.3011494, "secs": 4},
                    {"min": 3.3011494, "max": 3.395862, "secs": 2},
                    {"min": 3.395862, "max": 3.4905746, "secs": 1},
                    {"min": 3.4905746, "max": 3.5852873, "secs": 1},
                    {"min": 3.5852873, "max": 3.68, "secs": 1},
                    {"min": 3.68, "max": 3.77, "secs": 1}
                ])),
                ..Default::default()
            }
        }

        pub fn with_live_best_efforts_shape() -> Self {
            Self {
                activities: vec![Self::activity(
                    "live-efforts-1",
                    "Recovery Run Z1",
                    "2026-03-08",
                )],
                fitness_summary: Some(Self::fitness_snapshot(58.0, 46.0, 12.0)),
                activity_details: HashMap::from([(
                    "live-efforts-1".to_string(),
                    json!({
                        "distance": 7040.0,
                        "moving_time": 2880,
                        "average_heartrate": 127.0,
                        "average_watts": 219.0,
                        "total_elevation_gain": 66.0
                    }),
                )]),
                best_efforts: Some(json!({
                    "stream": "watts",
                    "efforts": [
                        {"start_index": 2723, "end_index": 2783, "average": 303.51666, "duration": 60, "distance": null},
                        {"start_index": 1320, "end_index": 1380, "average": 241.98334, "duration": 60, "distance": null}
                    ]
                })),
                ..Default::default()
            }
        }

        pub fn with_streams_and_interval_error() -> Self {
            let mut time_s = Vec::new();
            let mut speed = Vec::new();
            let mut heartrate = Vec::new();
            let mut power = Vec::new();
            let mut t = 0.0f64;
            for rep in 0..4 {
                for _ in 0..180 {
                    time_s.push(t);
                    speed.push(6.0);
                    heartrate.push(175.0);
                    power.push(300.0);
                    t += 1.0;
                }
                if rep < 3 {
                    for _ in 0..120 {
                        time_s.push(t);
                        speed.push(2.5);
                        heartrate.push(140.0);
                        power.push(100.0);
                        t += 1.0;
                    }
                }
            }
            Self {
                activities: vec![Self::activity(
                    "stream-err-1",
                    "Workout with stream fallback",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "stream-err-1".to_string(),
                    json!({
                        "distance": 12000.0,
                        "moving_time": 1200,
                        "average_heartrate": 160.0,
                        "average_watts": 250.0,
                        "total_elevation_gain": 80.0
                    }),
                )]),
                streams: Some(json!({
                    "time": time_s,
                    "velocity_smooth": speed,
                    "heartrate": heartrate,
                    "watts": power
                })),
                intervals_error: Some("upstream interval endpoint unavailable".to_string()),
                ..Default::default()
            }
        }

        pub fn with_stream_error() -> Self {
            Self {
                activities: vec![Self::activity(
                    "stream-err-2",
                    "Workout with missing streams",
                    "2026-02-18",
                )],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "stream-err-2".to_string(),
                    json!({
                        "distance": 12000.0,
                        "moving_time": 1200,
                        "average_heartrate": 160.0,
                        "average_watts": 250.0,
                        "total_elevation_gain": 80.0
                    }),
                )]),
                streams_error: Some("streams endpoint 504".to_string()),
                ..Default::default()
            }
        }

        pub fn with_fartlek_streams_and_upstream_intervals() -> Self {
            let mut time_s = Vec::new();
            let mut speed = Vec::new();
            let mut heartrate = Vec::new();
            let mut power = Vec::new();
            let mut t = 0.0f64;
            for block in 0..6 {
                let is_work = block % 2 == 0;
                let dur = if is_work { 180 } else { 120 };
                let spd = if is_work { 5.5 } else { 2.5 };
                let hr = if is_work { 170.0 } else { 135.0 };
                let w = if is_work { 280.0 } else { 100.0 };
                for _ in 0..dur {
                    time_s.push(t);
                    speed.push(spd);
                    heartrate.push(hr);
                    power.push(w);
                    t += 1.0;
                }
            }
            Self {
                activities: vec![Self::activity("fartlek-1", "Fartlek session", "2026-02-18")],
                fitness_summary: Some(Self::fitness_snapshot(54.0, 47.0, 7.0)),
                activity_details: HashMap::from([(
                    "fartlek-1".to_string(),
                    json!({
                        "distance": 10000.0,
                        "moving_time": 1800,
                        "average_heartrate": 155.0,
                        "average_watts": 200.0,
                        "total_elevation_gain": 50.0
                    }),
                )]),
                streams: Some(json!({
                    "time": time_s,
                    "velocity_smooth": speed,
                    "heartrate": heartrate,
                    "watts": power
                })),
                intervals: Some(json!([
                    {"moving_time": 180, "average_heartrate": 170.0, "average_watts": 280.0}
                ])),
                ..Default::default()
            }
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
            limit: Option<u32>,
            days_back: Option<i32>,
        ) -> Result<Vec<ActivitySummary>, IntervalsError> {
            self.activity_calls.lock().unwrap().push((limit, days_back));
            Ok(self.activities.clone())
        }

        async fn get_fitness_summary(&self) -> Result<Value, IntervalsError> {
            self.fitness_summary
                .clone()
                .ok_or_else(|| IntervalsError::NotFound("No fitness summary".to_string()))
        }

        async fn get_activity_details(&self, activity_id: &str) -> Result<Value, IntervalsError> {
            self.activity_details_calls
                .lock()
                .unwrap()
                .push(activity_id.to_string());
            if self.failing_activity_detail_ids.contains(activity_id) {
                return Err(IntervalsError::NotFound(format!(
                    "Activity {activity_id} not found"
                )));
            }
            if !self.activity_details_map.is_empty() {
                return self
                    .activity_details_map
                    .get(activity_id)
                    .cloned()
                    .ok_or_else(|| {
                        IntervalsError::NotFound(format!("Activity {activity_id} not found"))
                    });
            }
            Ok(self
                .activity_details
                .get(activity_id)
                .cloned()
                .or_else(|| self.workout_detail.clone())
                .unwrap_or_else(|| json!({})))
        }

        async fn get_activity_streams(
            &self,
            activity_id: &str,
            _streams: Option<Vec<String>>,
        ) -> Result<Value, IntervalsError> {
            if let Some(reason) = &self.streams_error {
                return Err(IntervalsError::Config(
                    intervals_icu_client::ConfigError::Other(reason.clone()),
                ));
            }
            if activity_id.starts_with("ride-") {
                *self.profile_stream_calls.lock().unwrap() += 1;
            }
            Ok(self
                .streams_map
                .get(activity_id)
                .cloned()
                .or_else(|| self.streams.clone())
                .unwrap_or_else(|| json!({})))
        }

        async fn get_activity_intervals(
            &self,
            _activity_id: &str,
        ) -> Result<Value, IntervalsError> {
            if let Some(reason) = &self.intervals_error {
                return Err(IntervalsError::Config(
                    intervals_icu_client::ConfigError::Other(reason.clone()),
                ));
            }
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
            if let Some(msgs) = self.activity_details.get("__activity_messages") {
                return serde_json::from_value(msgs.clone()).map_err(|e| {
                    IntervalsError::Config(intervals_icu_client::ConfigError::Other(e.to_string()))
                });
            }
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
            if let Some(data) = &self.wellness_for_date_data {
                return Ok(data.clone());
            }
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

        async fn get_sport_settings(&self) -> Result<SportSettings, IntervalsError> {
            Ok(self.sport_settings.clone().unwrap_or_default())
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
                return Err(super::clone_intervals_error(err));
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

        async fn get_workout_library(&self) -> Result<Vec<WorkoutItem>, IntervalsError> {
            Ok(vec![])
        }

        async fn get_workouts_in_folder(
            &self,
            _folder_id: &str,
        ) -> Result<Vec<WorkoutItem>, IntervalsError> {
            Ok(vec![])
        }

        async fn create_folder(&self, _folder: &Value) -> Result<Folder, IntervalsError> {
            Ok(Folder {
                id: 0,
                name: String::new(),
                description: None,
                parent_id: None,
                children: vec![],
            })
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

/// Test helper: flatten `ContentBlock` items into a single String for assertions.
pub fn content_text(content: &[crate::intents::ContentBlock]) -> String {
    content
        .iter()
        .flat_map(|b| match b {
            crate::intents::ContentBlock::Text { text } => vec![text.clone()],
            crate::intents::ContentBlock::Markdown { markdown } => vec![markdown.clone()],
            crate::intents::ContentBlock::Table { headers, rows } => {
                let mut parts: Vec<String> = headers.clone();
                for row in rows {
                    parts.extend(row.clone());
                }
                parts
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::mock::MockIntervalsClient;
    use super::*;
    use intervals_icu_client::domains::workout::SportSettings;
    use intervals_icu_client::{
        ActivityMessage, ActivitySummary, ApiError, AthleteProfile, ConfigError, Event,
        EventCategory, IntervalsClient, IntervalsError, ValidationError,
    };
    use serde_json::json;

    #[test]
    fn test_content_text_with_text_blocks() {
        use crate::intents::ContentBlock;
        let content = vec![
            ContentBlock::Text {
                text: "hello".to_string(),
            },
            ContentBlock::Text {
                text: "world".to_string(),
            },
        ];
        let result = content_text(&content);
        assert_eq!(result, "hello\nworld");
    }

    #[test]
    fn test_content_text_with_markdown_blocks() {
        use crate::intents::ContentBlock;
        let content = vec![ContentBlock::Markdown {
            markdown: "# Title".to_string(),
        }];
        let result = content_text(&content);
        assert_eq!(result, "# Title");
    }

    #[test]
    fn test_content_text_with_table_blocks() {
        use crate::intents::ContentBlock;
        let content = vec![ContentBlock::Table {
            headers: vec!["Name".to_string(), "Value".to_string()],
            rows: vec![vec!["a".to_string(), "1".to_string()]],
        }];
        let result = content_text(&content);
        assert!(result.contains("Name"));
        assert!(result.contains("Value"));
        assert!(result.contains("a"));
        assert!(result.contains("1"));
    }

    #[test]
    fn test_content_text_empty() {
        use crate::intents::ContentBlock;
        let content: Vec<ContentBlock> = vec![];
        let result = content_text(&content);
        assert!(result.is_empty());
    }

    #[test]
    fn test_mock_scenario_adaptive_wellness_series() {
        let value =
            MockIntervalsClient::adaptive_wellness_series(28800.0, 50.0, 60.0, 25200.0, 55.0, 45.0);
        assert!(value.is_array());
        assert_eq!(value.as_array().unwrap().len(), 35);
    }

    #[test]
    fn test_mock_scenario_fitness_snapshot() {
        let value = MockIntervalsClient::fitness_snapshot(50.0, 70.0, -20.0);
        assert!(value.is_array());
        let arr = value.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["fitness"], 50.0);
        assert_eq!(arr[0]["fatigue"], 70.0);
        assert_eq!(arr[0]["form"], -20.0);
    }

    #[test]
    fn test_mock_scenario_activity() {
        let act = MockIntervalsClient::activity("a1", "Run 1", "2026-03-01");
        assert_eq!(act.id, "a1");
        assert_eq!(act.name.as_deref(), Some("Run 1"));
        assert_eq!(act.start_date_local, "2026-03-01");
    }

    #[test]
    fn test_mock_scenario_mock_event() {
        let event = MockIntervalsClient::mock_event(Some("e1"));
        assert_eq!(event.id.as_deref(), Some("e1"));
        assert_eq!(event.name, "Mock event");
    }

    #[test]
    fn test_mock_scenario_mock_event_no_id() {
        let event = MockIntervalsClient::mock_event(None);
        assert!(event.id.is_none());
    }

    #[test]
    fn test_mock_scenario_relative_date() {
        let date = MockIntervalsClient::relative_date(0);
        assert!(!date.is_empty());
        assert!(date.contains('-'));
    }

    #[test]
    fn test_mock_scenario_with_tsb() {
        let client = MockIntervalsClient::with_tsb(10.0);
        assert_eq!(client.activities.len(), 1);
        assert!(client.fitness_summary.is_some());
    }

    #[test]
    fn test_mock_scenario_with_race_activity() {
        let client = MockIntervalsClient::with_race_activity();
        assert_eq!(client.activities.len(), 1);
        assert_eq!(client.events.len(), 1);
        assert!(client.fitness_summary.is_some());
    }

    #[test]
    fn test_mock_scenario_with_period_blocks() {
        let client = MockIntervalsClient::with_period_blocks();
        assert_eq!(client.activities.len(), 3);
        assert!(client.fitness_summary.is_some());
    }

    #[test]
    fn test_mock_scenario_with_single_workout_degraded_streams() {
        let client = MockIntervalsClient::with_single_workout_degraded_streams();
        assert_eq!(client.activities.len(), 1);
        assert!(client.fitness_summary.is_some());
        assert!(client.intervals.is_some());
    }

    #[test]
    fn test_mock_scenario_with_race_degraded_context() {
        let client = MockIntervalsClient::with_race_degraded_context();
        assert_eq!(client.activities.len(), 1);
        assert!(client.fitness_summary.is_some());
    }

    #[test]
    fn test_mock_scenario_with_positive_tsb_and_low_sleep() {
        let client = MockIntervalsClient::with_positive_tsb_and_low_sleep();
        assert_eq!(client.activities.len(), 1);
        assert!(client.fitness_summary.is_some());
        assert!(client.wellness.is_some());
    }

    #[test]
    fn test_mock_scenario_with_supportive_recovery_metrics() {
        let client = MockIntervalsClient::with_supportive_recovery_metrics();
        assert_eq!(client.activities.len(), 1);
        assert!(client.fitness_summary.is_some());
        assert!(client.wellness.is_some());
    }

    #[test]
    fn test_mock_scenario_with_personal_hrv_drop_profile() {
        let client = MockIntervalsClient::with_personal_hrv_drop_profile();
        assert_eq!(client.activities.len(), 1);
        assert!(client.wellness.is_some());
        let wellness = client.wellness.as_ref().unwrap();
        assert!(wellness.is_array());
        assert_eq!(wellness.as_array().unwrap().len(), 35);
    }

    #[test]
    fn test_mock_scenario_with_personal_hrv_norm_profile() {
        let client = MockIntervalsClient::with_personal_hrv_norm_profile();
        assert_eq!(client.activities.len(), 1);
        assert!(client.wellness.is_some());
    }

    #[test]
    fn test_mock_scenario_with_load_ramp_block() {
        let client = MockIntervalsClient::with_load_ramp_block();
        assert_eq!(client.activities.len(), 28);
        assert!(client.fitness_summary.is_some());
    }

    #[test]
    fn test_mock_scenario_with_stream_supported_workout() {
        let client = MockIntervalsClient::with_stream_supported_workout();
        assert_eq!(client.activities.len(), 1);
        assert!(client.intervals.is_some());
        assert!(client.streams.is_some());
        assert!(client.pace_histogram.is_some());
    }

    #[test]
    fn test_mock_scenario_with_api_load_snapshot() {
        let client = MockIntervalsClient::with_api_load_snapshot();
        assert!(client.wellness_for_date_data.is_some());
    }

    #[test]
    fn test_mock_scenario_with_profile_metrics() {
        let client = MockIntervalsClient::with_profile_metrics();
        assert!(client.fitness_summary.is_some());
        assert!(client.sport_settings.is_some());
    }

    #[test]
    fn test_mock_scenario_with_mode_collapse_single_workout() {
        let client = MockIntervalsClient::with_mode_collapse_single_workout();
        assert_eq!(client.activities.len(), 1);
        assert!(client.fitness_summary.is_some());
    }

    #[test]
    fn test_mock_scenario_with_future_workouts_only() {
        let client = MockIntervalsClient::with_future_workouts_only();
        assert!(client.upcoming_workouts.is_some());
    }

    #[test]
    fn test_mock_scenario_with_future_calendar_events_only() {
        let client = MockIntervalsClient::with_future_calendar_events_only();
        assert!(client.upcoming_workouts.is_some());
    }

    #[test]
    fn test_mock_scenario_with_paired_activity_and_calendar_duplicate() {
        let client = MockIntervalsClient::with_paired_activity_and_calendar_duplicate();
        assert_eq!(client.activities.len(), 1);
        assert!(client.upcoming_workouts.is_some());
    }

    #[test]
    fn test_mock_scenario_with_profile_metrics_and_wellness_weight() {
        let client = MockIntervalsClient::with_profile_metrics_and_wellness_weight();
        assert!(client.wellness_for_date_data.is_some());
    }

    #[test]
    fn test_mock_scenario_with_recent_non_race_then_race_activity() {
        let client = MockIntervalsClient::with_recent_non_race_then_race_activity();
        assert_eq!(client.activities.len(), 2);
        assert!(client.events.len() == 1);
    }

    #[test]
    fn test_mock_scenario_with_mixed_period_workouts() {
        let client = MockIntervalsClient::with_mixed_period_workouts();
        assert_eq!(client.activities.len(), 3);
    }

    #[test]
    fn test_mock_scenario_with_mode_sensitive_single_workout() {
        let client = MockIntervalsClient::with_mode_sensitive_single_workout();
        assert_eq!(client.activities.len(), 1);
        assert!(client.intervals.is_some());
        assert!(client.streams.is_some());
    }

    #[test]
    fn test_mock_scenario_with_object_shaped_interval_payload() {
        let client = MockIntervalsClient::with_object_shaped_interval_payload();
        assert!(client.intervals.is_some());
    }

    #[test]
    fn test_mock_scenario_with_interval_power_only_in_streams() {
        let client = MockIntervalsClient::with_interval_power_only_in_streams();
        assert!(client.streams.is_some());
    }

    #[test]
    fn test_mock_scenario_with_noncanonical_stream_payload() {
        let client = MockIntervalsClient::with_noncanonical_stream_payload();
        assert!(client.streams.is_some());
    }

    #[test]
    fn test_mock_scenario_with_rich_detailed_workout() {
        let client = MockIntervalsClient::with_rich_detailed_workout();
        assert_eq!(client.activities.len(), 1);
    }

    #[test]
    fn test_mock_scenario_with_interval_power_stream_alias() {
        let client = MockIntervalsClient::with_interval_power_stream_alias();
        assert!(client.streams.is_some());
    }

    #[test]
    fn test_mock_scenario_with_interval_output_only_in_speed_streams() {
        let client = MockIntervalsClient::with_interval_output_only_in_speed_streams();
        assert!(client.streams.is_some());
    }

    #[test]
    fn test_mock_scenario_with_many_intervals() {
        let client = MockIntervalsClient::with_many_intervals();
        assert!(client.intervals.is_some());
    }

    #[test]
    fn test_mock_scenario_with_priority_streams_without_power() {
        let client = MockIntervalsClient::with_priority_streams_without_power();
        assert!(client.streams.is_some());
    }

    #[test]
    fn test_mock_scenario_with_best_efforts_and_bucket_histograms() {
        let client = MockIntervalsClient::with_best_efforts_and_bucket_histograms();
        assert!(client.best_efforts.is_some());
        assert!(client.hr_histogram.is_some());
        assert!(client.power_histogram.is_some());
        assert!(client.pace_histogram.is_some());
    }

    #[test]
    fn test_mock_scenario_with_full_histogram_ranges() {
        let client = MockIntervalsClient::with_full_histogram_ranges();
        assert!(client.hr_histogram.is_some());
        assert!(client.power_histogram.is_some());
        assert!(client.pace_histogram.is_some());
    }

    #[test]
    fn test_mock_scenario_with_live_best_efforts_shape() {
        let client = MockIntervalsClient::with_live_best_efforts_shape();
        assert!(client.best_efforts.is_some());
    }

    #[test]
    fn test_mock_scenario_with_streams_and_interval_error() {
        let client = MockIntervalsClient::with_streams_and_interval_error();
        assert!(client.streams.is_some());
        assert!(client.intervals_error.is_some());
    }

    #[test]
    fn test_mock_scenario_with_stream_error() {
        let client = MockIntervalsClient::with_stream_error();
        assert!(client.streams_error.is_some());
    }

    #[test]
    fn test_mock_scenario_with_fartlek_streams_and_upstream_intervals() {
        let client = MockIntervalsClient::with_fartlek_streams_and_upstream_intervals();
        assert!(client.streams.is_some());
        assert!(client.intervals.is_some());
    }

    #[test]
    fn test_mock_observation_wellness_call_count() {
        let client = MockIntervalsClient::default();
        let observations = client.observations();
        assert_eq!(observations.wellness_call_count(), 0);
    }

    #[test]
    fn test_mock_observation_wellness_last_days_back_none() {
        let client = MockIntervalsClient::default();
        let observations = client.observations();
        assert!(observations.wellness_last_days_back().is_none());
    }

    #[test]
    fn test_mock_with_stream_for_id() {
        let client =
            MockIntervalsClient::default().with_stream_for_id("a1", json!({"watts": [100, 200]}));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(client.get_activity_streams("a1", None))
            .unwrap();
        assert_eq!(result, json!({"watts": [100, 200]}));
    }

    #[test]
    fn test_mock_with_activity_details_map() {
        let mut map = std::collections::HashMap::new();
        map.insert("a1".to_string(), json!({"distance": 5000}));
        let client = MockIntervalsClient::default().with_activity_details_map(map);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(client.get_activity_details("a1")).unwrap();
        assert_eq!(result, json!({"distance": 5000}));
    }

    #[test]
    fn test_mock_with_activity_details_map_not_found() {
        let mut map = std::collections::HashMap::new();
        map.insert("a1".to_string(), json!({"distance": 5000}));
        let client = MockIntervalsClient::default().with_activity_details_map(map);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(client.get_activity_details("missing"));
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_with_failing_activity_details() {
        let client = MockIntervalsClient::default().with_failing_activity_details(vec!["a1"]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(client.get_activity_details("a1"));
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_requested_activity_detail_ids() {
        let client = MockIntervalsClient::default().with_activity_detail("a1", json!({}));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(client.get_activity_details("a1"));
        assert_eq!(client.requested_activity_detail_ids(), vec!["a1"]);
    }

    #[test]
    fn test_mock_activity_call_count() {
        let client = MockIntervalsClient::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(client.get_recent_activities(None, None));
        assert_eq!(client.activity_call_count(), 1);
    }

    #[test]
    fn test_mock_activity_calls_snapshot() {
        let client = MockIntervalsClient::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(client.get_recent_activities(Some(10), Some(7)));
        let calls = client.activity_calls_snapshot();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], (Some(10), Some(7)));
    }

    #[test]
    fn test_mock_with_intervals_error() {
        let client = MockIntervalsClient::default().with_intervals_error("test error");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(client.get_activity_intervals("a1"));
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_with_streams_error() {
        let client = MockIntervalsClient::default().with_streams_error("test error");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(client.get_activity_streams("a1", None));
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_with_wellness_for_date() {
        let client = MockIntervalsClient::default().with_wellness_for_date(json!({"sleep": 8}));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(client.get_wellness_for_date("2026-03-21"))
            .unwrap();
        assert_eq!(result, json!({"sleep": 8}));
    }

    #[test]
    fn test_mock_endurance_stream_calls() {
        let client = MockIntervalsClient::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(client.get_activity_streams("ride-1", None));
        assert_eq!(client.endurance_stream_calls(), 1);
    }

    #[test]
    fn test_mock_observations_wellness_days() {
        let client = MockIntervalsClient::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(client.get_wellness(Some(7)));
        let days = client.observations_wellness_days();
        assert_eq!(days, vec![Some(7)]);
    }

    #[test]
    fn test_mock_get_activity_messages_from_details() {
        let mut details = std::collections::HashMap::new();
        details.insert(
            "__activity_messages".to_string(),
            json!([{"id": 1, "name": "Msg"}]),
        );
        let client = MockIntervalsClient::default()
            .with_activity_detail("__activity_messages", json!([{"id": 1, "name": "Msg"}]));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(client.get_activity_messages("a1")).unwrap();
        assert_eq!(result.len(), 1);
    }

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
    fn test_clone_intervals_error_preserves_api_error() {
        let original = IntervalsError::Api(ApiError::new(503, "upstream unavailable", "raw"));
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Api(api) => {
                assert_eq!(api.status, 503);
                assert_eq!(api.message, "upstream unavailable");
                assert_eq!(api.raw_body, "raw");
            }
            other => panic!("expected API error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_validation_error() {
        let original = IntervalsError::Validation(ValidationError::InvalidFormat {
            field: "target_date".into(),
            value: "bad-date".into(),
        });
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Validation(ValidationError::InvalidFormat { field, value }) => {
                assert_eq!(field, "target_date");
                assert_eq!(value, "bad-date");
            }
            other => panic!("expected validation error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_not_found_and_auth() {
        let missing = clone_intervals_error(&IntervalsError::NotFound("missing".into()));
        let auth = clone_intervals_error(&IntervalsError::Auth("forbidden".into()));

        assert!(matches!(missing, IntervalsError::NotFound(message) if message == "missing"));
        assert!(matches!(auth, IntervalsError::Auth(message) if message == "forbidden"));
    }

    #[test]
    fn test_clone_intervals_error_maps_json_decode_to_generic_server_error() {
        let json_err = serde_json::from_str::<()>("invalid").unwrap_err();
        let cloned = clone_intervals_error(&IntervalsError::JsonDecode(json_err));

        match cloned {
            IntervalsError::Api(api) => {
                assert_eq!(api.status, 500);
                assert_eq!(api.message, "json decode");
            }
            other => panic!("expected API fallback clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_transport() {
        let original = IntervalsError::Transport(intervals_icu_client::TransportError {
            message: "conn refused".to_string(),
            is_timeout: false,
            is_connect: true,
        });
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Api(api) => {
                assert_eq!(api.status, 500);
                assert!(api.message.contains("conn refused"));
            }
            other => panic!("expected API fallback for transport clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_config_missing_env_var() {
        let original = IntervalsError::Config(ConfigError::MissingEnvVar("API_KEY".into()));
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Config(ConfigError::MissingEnvVar(key)) => {
                assert_eq!(key, "API_KEY");
            }
            other => panic!("expected MissingEnvVar config error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_config_invalid_value() {
        let original = IntervalsError::Config(ConfigError::InvalidValue {
            key: "TIMEOUT".into(),
            message: "must be positive".into(),
        });
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Config(ConfigError::InvalidValue { key, message }) => {
                assert_eq!(key, "TIMEOUT");
                assert_eq!(message, "must be positive");
            }
            other => panic!("expected InvalidValue config error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_config_unsupported() {
        let original = IntervalsError::Config(ConfigError::Unsupported { method: "PATCH" });
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Config(ConfigError::Unsupported { method }) => {
                assert_eq!(method, "PATCH");
            }
            other => panic!("expected Unsupported config error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_config_other() {
        let original = IntervalsError::Config(ConfigError::Other("custom error".into()));
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Config(ConfigError::Other(msg)) => {
                assert_eq!(msg, "custom error");
            }
            other => panic!("expected Other config error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_validation_empty_field() {
        let original = IntervalsError::Validation(ValidationError::EmptyField {
            field: "name".into(),
        });
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Validation(ValidationError::EmptyField { field }) => {
                assert_eq!(field, "name");
            }
            other => panic!("expected EmptyField validation error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_validation_unknown_variant() {
        let original = IntervalsError::Validation(ValidationError::UnknownVariant {
            field: "sport".into(),
            value: "swimming".into(),
        });
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Validation(ValidationError::UnknownVariant { field, value }) => {
                assert_eq!(field, "sport");
                assert_eq!(value, "swimming");
            }
            other => panic!("expected UnknownVariant validation error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_validation_missing_parameter() {
        let original =
            IntervalsError::Validation(ValidationError::MissingParameter("target_date".into()));
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Validation(ValidationError::MissingParameter(param)) => {
                assert_eq!(param, "target_date");
            }
            other => {
                panic!("expected MissingParameter validation error clone, got {other:?}")
            }
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_validation_invalid_param_comb() {
        let original = IntervalsError::Validation(ValidationError::InvalidParameterCombination(
            "cannot combine x and y".into(),
        ));
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Validation(ValidationError::InvalidParameterCombination(msg)) => {
                assert_eq!(msg, "cannot combine x and y");
            }
            other => {
                panic!("expected InvalidParameterCombination validation error clone, got {other:?}")
            }
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_io() {
        let original = IntervalsError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "request timed out",
        ));
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Io(io_err) => {
                assert_eq!(io_err.kind(), std::io::ErrorKind::TimedOut);
            }
            other => panic!("expected IO error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_cancelled() {
        let original = IntervalsError::Cancelled {
            reason: "user abort".to_string(),
        };
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Cancelled { reason } => {
                assert_eq!(reason, "user abort");
            }
            other => panic!("expected Cancelled error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_cancelled_empty_reason() {
        let original = IntervalsError::Cancelled {
            reason: String::new(),
        };
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Cancelled { reason } => {
                assert!(reason.is_empty());
            }
            other => panic!("expected Cancelled error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_decode() {
        let original = IntervalsError::Decode {
            message: "unexpected token".to_string(),
            snippet: "<<< raw >>>".to_string(),
        };
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Decode { message, snippet } => {
                assert_eq!(message, "unexpected token");
                assert_eq!(snippet, "<<< raw >>>");
            }
            other => panic!("expected Decode error clone, got {other:?}"),
        }
    }

    #[test]
    fn test_clone_intervals_error_preserves_decode_empty_snippet() {
        let original = IntervalsError::Decode {
            message: "parse error".to_string(),
            snippet: String::new(),
        };
        let cloned = clone_intervals_error(&original);

        match cloned {
            IntervalsError::Decode { message, snippet } => {
                assert_eq!(message, "parse error");
                assert!(snippet.is_empty());
            }
            other => panic!("expected Decode error clone, got {other:?}"),
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
        let settings = SportSettings {
            sports: vec![intervals_icu_client::domains::workout::SportSetting {
                name: Some("cycling".into()),
                ..Default::default()
            }],
            age: None,
            weight: None,
        };
        let client = MockIntervalsClient::builder().with_sport_settings(settings);
        assert!(client.sport_settings.is_some());
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
        let settings = SportSettings {
            sports: vec![intervals_icu_client::domains::workout::SportSetting {
                name: Some("run".into()),
                ..Default::default()
            }],
            age: None,
            weight: None,
        };
        let client = MockIntervalsClient::builder().with_sport_settings(settings);
        let result = client.get_sport_settings().await.unwrap();
        assert_eq!(result.sports.len(), 1);
        assert_eq!(result.sports[0].name.as_deref(), Some("run"));
    }

    #[tokio::test]
    async fn test_mock_get_sport_settings_none() {
        let client = MockIntervalsClient::default();
        let result = client.get_sport_settings().await.unwrap();
        assert!(result.sports.is_empty());
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
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_mock_get_workouts_in_folder() {
        let client = MockIntervalsClient::default();
        let result = client.get_workouts_in_folder("folder-1").await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_mock_create_folder() {
        let client = MockIntervalsClient::default();
        let result = client.create_folder(&json!({})).await.unwrap();
        assert_eq!(result.id, 0);
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
