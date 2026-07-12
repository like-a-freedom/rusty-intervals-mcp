use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Duration, NaiveDate};
use intervals_icu_client::{ActivityMessage, ActivitySummary, Event, IntervalsClient};
use serde_json::Value;

use super::fetch_error::FetchError;
use crate::domains::coach::AnalysisWindow;
use crate::domains::load::{ComparableLoadSeries, LoadObservation, LoadSource};
use crate::engines::shared::{parse_activity_date, parse_event_date};

/// Minimum wellness history to fetch for personal baseline calculation.
/// Requires 60 calendar days of observations to compute the reference window.
pub const PERSONAL_BASELINE_WINDOW_DAYS: i32 = 60;

#[derive(Debug, Clone)]
pub struct PeriodFetchRequest {
    pub window: AnalysisWindow,
    pub include_activity_details: bool,
    pub include_comparison_window: bool,
}

#[derive(Debug, Clone)]
pub struct SingleWorkoutFetchRequest {
    pub activity_id: String,
    pub include_intervals: bool,
    pub include_streams: bool,
    pub include_best_efforts: bool,
    pub include_hr_histogram: bool,
    pub include_power_histogram: bool,
    pub include_pace_histogram: bool,
}

#[derive(Debug, Clone)]
pub struct RecoveryFetchRequest {
    pub period_days: i32,
    pub include_wellness: bool,
}

#[derive(Debug, Clone)]
pub struct RaceFetchRequest {
    pub activity_id: String,
    pub include_intervals: bool,
    pub include_streams: bool,
}

/// State of a single upstream data source after a fetch attempt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SourceFetchState {
    #[default]
    NotRequested,
    Available,
    Empty,
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct FetchedAnalysisData {
    pub activities: Vec<ActivitySummary>,
    pub comparison_activities: Vec<ActivitySummary>,
    pub calendar_events: Vec<Event>,
    pub fetch_warnings: Vec<String>,
    pub activity_messages: Vec<ActivityMessage>,
    pub activity_details: HashMap<String, Value>,
    pub workout_detail: Option<Value>,
    pub fitness: Option<Value>,
    pub wellness: Option<Value>,
    pub intervals: Option<Value>,
    pub streams: Option<Value>,
    pub best_efforts: Option<Value>,
    pub hr_histogram: Option<Value>,
    pub power_histogram: Option<Value>,
    pub pace_histogram: Option<Value>,
    pub intervals_state: SourceFetchState,
    pub streams_state: SourceFetchState,
}

/// Returns true when a payload has no usable content (empty array/object or null).
fn value_is_empty(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Null => true,
        _ => false,
    }
}

pub fn build_previous_window(current: &AnalysisWindow) -> AnalysisWindow {
    let days = current.window_days();
    let previous_end = current.start_date - Duration::days(1);
    let previous_start = previous_end - Duration::days(days - 1);
    AnalysisWindow::new(previous_start, previous_end)
}

pub fn required_activity_window(request: &PeriodFetchRequest) -> (NaiveDate, NaiveDate) {
    let start = if request.include_comparison_window {
        build_previous_window(&request.window).start_date
    } else {
        request.window.start_date
    };
    (start, request.window.end_date)
}

pub fn activity_lookback_days(start: NaiveDate, today: NaiveDate) -> Result<i32, FetchError> {
    let days = (today - start).num_days().max(0);
    i32::try_from(days).map_err(|_| {
        FetchError::InvalidDateRange("Requested activity window is too large".to_string())
    })
}

pub fn extract_activity_load(detail: Option<&Value>) -> Option<LoadObservation> {
    let object = detail?.as_object()?;

    // Alias priority: icu_training_load → training_load/icuTrainingLoad → tss
    let (key, source) = object
        .get("icu_training_load")
        .map(|_| ("icu_training_load", LoadSource::IcuTrainingLoad))
        .or_else(|| {
            object
                .get("training_load")
                .map(|_| ("training_load", LoadSource::TrainingLoadAlias))
        })
        .or_else(|| {
            object
                .get("icuTrainingLoad")
                .map(|_| ("icuTrainingLoad", LoadSource::TrainingLoadAlias))
        })
        .or_else(|| object.get("tss").map(|_| ("tss", LoadSource::TssAlias)))?;

    let value = object.get(key).and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_i64().map(|n| n as f64))
            .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
    })?;

    LoadObservation::new(value, source)
}

/// Return the best available load for an activity without requiring its detail
/// endpoint. Activity summaries carry `training_load` for historical queries;
/// a loaded detail takes precedence when it exposes a more specific value.
pub fn activity_load(
    activity: &ActivitySummary,
    detail: Option<&Value>,
) -> Option<LoadObservation> {
    extract_activity_load(detail).or_else(|| {
        activity.training_load.map(|value| LoadObservation {
            value: f64::from(value),
            source: LoadSource::ActivitySummaryTrainingLoad,
        })
    })
}

pub fn build_daily_load_series(
    activities: &[&ActivitySummary],
    details: &HashMap<String, Value>,
    window: &AnalysisWindow,
) -> ComparableLoadSeries {
    let mut daily_totals = HashMap::<NaiveDate, f64>::new();
    let mut activities_total = 0usize;
    let mut activities_with_load = 0usize;
    let mut source_counts = BTreeMap::new();

    for activity in activities {
        if let Some(activity_date) = parse_activity_date(&activity.start_date_local) {
            activities_total += 1;
            if let Some(observation) = activity_load(activity, details.get(&activity.id)) {
                activities_with_load += 1;
                *source_counts.entry(observation.source).or_insert(0) += 1;
                *daily_totals.entry(activity_date).or_insert(0.0) += observation.value;
            }
        }
    }

    let mut current = window.start_date;
    let mut daily = Vec::with_capacity(window.window_days().max(0) as usize);
    while current <= window.end_date {
        daily.push((current, daily_totals.get(&current).copied().unwrap_or(0.0)));
        current += Duration::days(1);
    }

    ComparableLoadSeries {
        daily,
        activities_total,
        activities_with_load,
        source_counts,
    }
}

fn dedupe_and_sort_events(mut events: Vec<Event>) -> Vec<Event> {
    let mut seen = HashSet::new();
    events.retain(|event| {
        let dedupe_key = event.id.clone().unwrap_or_else(|| {
            format!(
                "{}:{}:{:?}",
                event.start_date_local, event.name, event.category
            )
        });
        seen.insert(dedupe_key)
    });

    events.sort_by(|a, b| {
        let a_date = parse_event_date(&a.start_date_local).unwrap_or(NaiveDate::MIN);
        let b_date = parse_event_date(&b.start_date_local).unwrap_or(NaiveDate::MIN);
        a_date
            .cmp(&b_date)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| format!("{:?}", a.category).cmp(&format!("{:?}", b.category)))
            .then_with(|| a.id.cmp(&b.id))
    });

    events
}

fn normalize_upcoming_events_payload(payload: Value) -> Value {
    let Some(items) = payload.as_array() else {
        return payload;
    };

    Value::Array(
        items
            .iter()
            .map(|event| {
                let Some(object) = event.as_object() else {
                    return event.clone();
                };

                let has_name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty());
                if has_name {
                    return event.clone();
                }

                let fallback_name = object
                    .get("description")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| {
                        object
                            .get("category")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                    })
                    .unwrap_or("Untitled event");

                let mut normalized = object.clone();
                normalized.insert("name".to_string(), Value::String(fallback_name.to_string()));
                Value::Object(normalized)
            })
            .collect(),
    )
}

fn upcoming_rate_limit_warning() -> String {
    "planned workouts unavailable due to Intervals.icu rate limiting; continuing with completed-activity history only".to_string()
}

pub async fn fetch_calendar_events_between(
    client: &dyn IntervalsClient,
    start_date: &NaiveDate,
    end_date: &NaiveDate,
    limit: u32,
) -> Result<Vec<Event>, FetchError> {
    let today = chrono::Utc::now().date_naive();
    let mut events = Vec::new();

    if *start_date <= today {
        let days_back = (today - *start_date).num_days() as i32;
        let mut historical = client
            .get_events(Some(days_back), Some(limit))
            .await
            .map_err(|e| FetchError::ClientError(format!("Failed to fetch events: {}", e)))?;
        events.append(&mut historical);
    }

    if *end_date >= today {
        let days_ahead = (*end_date - today).num_days().max(0) as u32;
        let upcoming = client
            .get_upcoming_workouts(Some(days_ahead), Some(limit), None)
            .await
            .map_err(|e| {
                FetchError::ClientError(format!("Failed to fetch upcoming events: {}", e))
            })?;

        let normalized_upcoming = normalize_upcoming_events_payload(upcoming);
        let mut parsed: Vec<Event> = serde_json::from_value(normalized_upcoming).map_err(|e| {
            FetchError::ClientError(format!("Failed to decode upcoming events: {}", e))
        })?;
        events.append(&mut parsed);
    }

    Ok(dedupe_and_sort_events(events))
}

fn parse_planned_workout(
    event: &Value,
    known_activity_ids: &std::collections::HashSet<String>,
) -> Option<(ActivitySummary, Value)> {
    let object = event.as_object()?;

    if object
        .get("paired_activity_id")
        .and_then(Value::as_str)
        .is_some_and(|activity_id| known_activity_ids.contains(activity_id))
    {
        return None;
    }

    let event_id = object.get("id").and_then(|value| {
        value
            .as_i64()
            .map(|id| id.to_string())
            .or_else(|| value.as_str().map(str::to_owned))
    })?;
    let start_date_local = object.get("start_date_local").and_then(Value::as_str)?;
    let name = object
        .get("description")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| object.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .or_else(|| Some("Planned workout".to_string()));

    Some((
        ActivitySummary {
            id: format!("event:{event_id}"),
            name,
            start_date_local: start_date_local.to_string(),
            ..Default::default()
        },
        event.clone(),
    ))
}

fn normalize_intervals_payload(payload: Value) -> Value {
    if payload.is_array() {
        return payload;
    }

    let Some(object) = payload.as_object() else {
        return payload;
    };

    if let Some(intervals) = object.get("icu_intervals").and_then(Value::as_array) {
        return Value::Array(intervals.clone());
    }

    if let Some(groups) = object.get("icu_groups").and_then(Value::as_array) {
        return Value::Array(groups.clone());
    }

    payload
}

fn normalize_stream_descriptor_array(items: &[Value]) -> Option<Value> {
    let mut normalized = serde_json::Map::new();

    for item in items.iter().filter_map(Value::as_object) {
        let key = item
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                item.get("type")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
            })?;
        let data = item.get("data").filter(|value| value.is_array())?;
        normalized.insert(key.to_string(), data.clone());
    }

    if normalized.is_empty() {
        None
    } else {
        Some(Value::Object(normalized))
    }
}

fn normalize_streams_payload(payload: Value) -> Value {
    if let Some(items) = payload.as_array() {
        return normalize_stream_descriptor_array(items).unwrap_or(payload);
    }

    let Some(object) = payload.as_object() else {
        return payload;
    };

    if let Some(streams) = object.get("streams") {
        if let Some(stream_map) = streams.as_object() {
            return Value::Object(stream_map.clone());
        }

        if let Some(stream_items) = streams.as_array() {
            return normalize_stream_descriptor_array(stream_items).unwrap_or(payload);
        }
    }

    payload
}

pub async fn fetch_period_data(
    client: &dyn IntervalsClient,
    request: &PeriodFetchRequest,
) -> Result<FetchedAnalysisData, FetchError> {
    let (required_start, required_end) = required_activity_window(request);
    let today = chrono::Utc::now().date_naive();
    let days_back = activity_lookback_days(required_start, today)?;
    let mut activities = client
        .get_recent_activities(None, Some(days_back))
        .await
        .map_err(|error| FetchError::ClientError(format!("Failed to fetch activities: {error}")))?;

    activities.retain(|activity| {
        parse_activity_date(&activity.start_date_local)
            .is_some_and(|date| date >= required_start && date <= required_end)
    });

    let known_activity_ids = activities
        .iter()
        .map(|activity| activity.id.clone())
        .collect::<std::collections::HashSet<_>>();

    let mut fetched = FetchedAnalysisData {
        activities,
        ..Default::default()
    };

    let today = chrono::Utc::now().date_naive();
    let mut calendar_events = Vec::new();
    let mut upcoming_workouts_payload: Option<Value> = None;

    if request.window.start_date <= today {
        let days_back = (today - request.window.start_date).num_days() as i32;
        let mut historical = client
            .get_events(Some(days_back), Some(500))
            .await
            .map_err(|e| FetchError::ClientError(format!("Failed to fetch events: {}", e)))?;
        calendar_events.append(&mut historical);
    }

    if request.window.end_date >= today {
        let days_ahead = (request.window.end_date - today).num_days().max(0) as u32;
        match client
            .get_upcoming_workouts(Some(days_ahead), Some(500), None)
            .await
        {
            Ok(upcoming) => {
                let normalized_upcoming = normalize_upcoming_events_payload(upcoming.clone());
                let mut parsed: Vec<Event> =
                    serde_json::from_value(normalized_upcoming).map_err(|e| {
                        FetchError::ClientError(format!("Failed to decode upcoming events: {}", e))
                    })?;
                calendar_events.append(&mut parsed);
                upcoming_workouts_payload = Some(upcoming);
            }
            Err(e) if e.is_rate_limited() => {
                fetched.fetch_warnings.push(upcoming_rate_limit_warning());
            }
            Err(e) => {
                return Err(FetchError::ClientError(format!(
                    "Failed to fetch upcoming events: {}",
                    e
                )));
            }
        }
    }

    fetched.calendar_events = dedupe_and_sort_events(calendar_events);

    if request.include_activity_details {
        let requested_detail_count = fetched.activities.len();
        let mut failed_detail_count = 0usize;
        for activity in &fetched.activities {
            match client.get_activity_details(&activity.id).await {
                Ok(details) => {
                    fetched
                        .activity_details
                        .insert(activity.id.clone(), details);
                }
                Err(_) => failed_detail_count += 1,
            }
        }
        if failed_detail_count > 0 {
            fetched.fetch_warnings.push(format!(
                "{failed_detail_count} of {requested_detail_count} activity details unavailable; period totals remain available, but HR, zones, TSS, and load-derived metrics may be partial"
            ));
        }
    }

    if let Some(upcoming_workouts) = upcoming_workouts_payload.as_ref()
        && let Some(events) = upcoming_workouts.as_array()
    {
        for event in events {
            if event.get("category").and_then(Value::as_str) != Some("WORKOUT") {
                continue;
            }
            if let Some((activity, detail)) = parse_planned_workout(event, &known_activity_ids) {
                if request.include_activity_details {
                    fetched.activity_details.insert(activity.id.clone(), detail);
                }
                fetched.activities.push(activity);
            }
        }
    }

    fetched.activities.sort_by_key(|activity| {
        parse_activity_date(&activity.start_date_local).unwrap_or(request.window.start_date)
    });

    if request.include_comparison_window {
        fetched.comparison_activities = fetched.activities.clone();
    }

    Ok(fetched)
}

pub async fn fetch_recovery_data(
    client: &dyn IntervalsClient,
    request: &RecoveryFetchRequest,
) -> Result<FetchedAnalysisData, FetchError> {
    let wellness = if request.include_wellness {
        let wellness_lookback_days = request.period_days.max(PERSONAL_BASELINE_WINDOW_DAYS);
        Some(
            client
                .get_wellness(Some(wellness_lookback_days))
                .await
                .map_err(|e| FetchError::ClientError(format!("Failed to fetch wellness: {}", e)))?,
        )
    } else {
        None
    };

    let fitness = Some(
        client
            .get_fitness_summary()
            .await
            .map_err(|e| FetchError::ClientError(format!("Failed to fetch fitness: {}", e)))?,
    );

    let activities = client
        .get_recent_activities(Some(20), Some(request.period_days))
        .await
        .map_err(|e| FetchError::ClientError(format!("Failed to fetch activities: {}", e)))?;

    Ok(FetchedAnalysisData {
        activities,
        fitness,
        wellness,
        ..Default::default()
    })
}

pub async fn fetch_single_workout_data(
    client: &dyn IntervalsClient,
    request: &SingleWorkoutFetchRequest,
) -> Result<FetchedAnalysisData, FetchError> {
    let workout_detail = Some(
        client
            .get_activity_details(&request.activity_id)
            .await
            .map_err(|e| {
                FetchError::ClientError(format!("Failed to fetch activity details: {}", e))
            })?,
    );

    let mut intervals_state = SourceFetchState::NotRequested;
    let intervals = if request.include_intervals {
        match client.get_activity_intervals(&request.activity_id).await {
            Ok(value) => {
                let normalized = normalize_intervals_payload(value);
                if value_is_empty(&normalized) {
                    intervals_state = SourceFetchState::Empty;
                } else {
                    intervals_state = SourceFetchState::Available;
                }
                Some(normalized)
            }
            Err(_) => {
                intervals_state = SourceFetchState::Failed {
                    reason: "upstream interval endpoint unavailable".to_string(),
                };
                None
            }
        }
    } else {
        None
    };

    let mut streams_state = SourceFetchState::NotRequested;
    let streams = if request.include_streams {
        match client
            .get_activity_streams(&request.activity_id, None)
            .await
        {
            Ok(value) => {
                let normalized = normalize_streams_payload(value);
                if value_is_empty(&normalized) {
                    streams_state = SourceFetchState::Empty;
                } else {
                    streams_state = SourceFetchState::Available;
                }
                Some(normalized)
            }
            Err(_) => {
                streams_state = SourceFetchState::Failed {
                    reason: "upstream stream endpoint unavailable".to_string(),
                };
                None
            }
        }
    } else {
        None
    };

    let best_efforts = if request.include_best_efforts {
        client
            .get_best_efforts(&request.activity_id, None)
            .await
            .ok()
    } else {
        None
    };

    let hr_histogram = if request.include_hr_histogram {
        match client.get_hr_histogram(&request.activity_id).await {
            Ok(hist) => {
                tracing::debug!(
                    "HR histogram fetched for activity {}: {} buckets",
                    request.activity_id,
                    hist.as_array().map(|a| a.len()).unwrap_or(0)
                );
                Some(hist)
            }
            Err(e) => {
                tracing::info!(
                    "HR histogram not available for activity {}: {}",
                    request.activity_id,
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let power_histogram = if request.include_power_histogram {
        match client.get_power_histogram(&request.activity_id).await {
            Ok(hist) => {
                let bucket_count = hist.as_array().map(|a| a.len()).unwrap_or(0);
                if bucket_count == 0 {
                    tracing::debug!(
                        "Power histogram returned empty array for activity {}",
                        request.activity_id
                    );
                } else {
                    tracing::debug!(
                        "Power histogram fetched for activity {}: {} buckets",
                        request.activity_id,
                        bucket_count
                    );
                }
                Some(hist)
            }
            Err(e) => {
                tracing::info!(
                    "Power histogram not available for activity {}: {}",
                    request.activity_id,
                    e
                );
                None
            }
        }
    } else {
        None
    };

    let pace_histogram = if request.include_pace_histogram {
        match client.get_pace_histogram(&request.activity_id).await {
            Ok(hist) => {
                tracing::debug!(
                    "Pace histogram fetched for activity {}: {} buckets",
                    request.activity_id,
                    hist.as_array().map(|a| a.len()).unwrap_or(0)
                );
                Some(hist)
            }
            Err(e) => {
                tracing::info!(
                    "Pace histogram not available for activity {}: {}",
                    request.activity_id,
                    e
                );
                None
            }
        }
    } else {
        None
    };

    Ok(FetchedAnalysisData {
        activity_messages: client
            .get_activity_messages(&request.activity_id)
            .await
            .unwrap_or_default(),
        workout_detail,
        intervals,
        streams,
        best_efforts,
        hr_histogram,
        power_histogram,
        pace_histogram,
        intervals_state,
        streams_state,
        ..Default::default()
    })
}

pub async fn fetch_race_data(
    client: &dyn IntervalsClient,
    request: &RaceFetchRequest,
) -> Result<FetchedAnalysisData, FetchError> {
    let single_request = SingleWorkoutFetchRequest {
        activity_id: request.activity_id.clone(),
        include_intervals: request.include_intervals,
        include_streams: request.include_streams,
        include_best_efforts: false,
        include_hr_histogram: false,
        include_power_histogram: false,
        include_pace_histogram: false,
    };

    fetch_single_workout_data(client, &single_request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use chrono::NaiveDate;
    use intervals_icu_client::{EventCategory, IntervalsClient, IntervalsError};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    type ActivityCallRecord = Vec<(Option<u32>, Option<i32>)>;

    /// Recording mock that captures `get_recent_activities` arguments for assertions.
    struct RecordingPeriodClient {
        activities: Vec<ActivitySummary>,
        activity_calls: Arc<Mutex<ActivityCallRecord>>,
    }

    impl RecordingPeriodClient {
        fn with_activities(activities: Vec<ActivitySummary>) -> Self {
            Self {
                activities,
                activity_calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn activity_calls(&self) -> ActivityCallRecord {
            self.activity_calls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl IntervalsClient for RecordingPeriodClient {
        async fn get_recent_activities(
            &self,
            limit: Option<u32>,
            days_back: Option<i32>,
        ) -> Result<Vec<ActivitySummary>, IntervalsError> {
            self.activity_calls.lock().unwrap().push((limit, days_back));
            Ok(self.activities.clone())
        }

        // Stub all other methods with empty defaults
        async fn get_athlete_profile(
            &self,
        ) -> Result<intervals_icu_client::AthleteProfile, IntervalsError> {
            Ok(intervals_icu_client::AthleteProfile {
                id: "test".into(),
                name: None,
            })
        }
        async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError> {
            Err(IntervalsError::NotFound("not needed".into()))
        }
        async fn get_activity_details(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_activity_streams(
            &self,
            _: &str,
            _: Option<Vec<String>>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_activity_intervals(
            &self,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_best_efforts(
            &self,
            _: &str,
            _: Option<intervals_icu_client::BestEffortsOptions>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_hr_histogram(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_power_histogram(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_pace_histogram(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_activity_messages(
            &self,
            _: &str,
        ) -> Result<Vec<ActivityMessage>, IntervalsError> {
            Ok(vec![])
        }
        async fn get_events(
            &self,
            _: Option<i32>,
            _: Option<u32>,
        ) -> Result<Vec<Event>, IntervalsError> {
            Ok(vec![])
        }
        async fn get_wellness_for_date(
            &self,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Err(IntervalsError::NotFound("not needed".into()))
        }
        async fn create_event(&self, _: Event) -> Result<Event, IntervalsError> {
            Err(IntervalsError::NotFound("not needed".into()))
        }
        async fn get_event(&self, _: &str) -> Result<Event, IntervalsError> {
            Err(IntervalsError::NotFound("not needed".into()))
        }
        async fn delete_event(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn bulk_create_events(&self, _: Vec<Event>) -> Result<Vec<Event>, IntervalsError> {
            Ok(vec![])
        }
        async fn search_activities(
            &self,
            _: &str,
            _: Option<u32>,
        ) -> Result<Vec<ActivitySummary>, IntervalsError> {
            Ok(vec![])
        }
        async fn search_activities_full(
            &self,
            _: &str,
            _: Option<u32>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
            Ok(String::new())
        }
        async fn update_activity(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn download_activity_file(
            &self,
            _: &str,
            _: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn download_activity_file_with_progress(
            &self,
            _: &str,
            _: Option<std::path::PathBuf>,
            _: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
            _: tokio::sync::watch::Receiver<bool>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn download_fit_file(
            &self,
            _: &str,
            _: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn download_gpx_file(
            &self,
            _: &str,
            _: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn get_gear_list(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_sport_settings(
            &self,
        ) -> Result<intervals_icu_client::domains::workout::SportSettings, IntervalsError> {
            Ok(Default::default())
        }
        async fn get_power_curves(
            &self,
            _: Option<i32>,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_gap_histogram(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn delete_activity(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn get_activities_around(
            &self,
            _: &str,
            _: Option<u32>,
            _: Option<i64>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
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
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_wellness(&self, _: Option<i32>) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn update_wellness(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_upcoming_workouts(
            &self,
            _: Option<u32>,
            _: Option<u32>,
            _: Option<String>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn update_event(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn bulk_delete_events(&self, _: Vec<String>) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn duplicate_event(
            &self,
            _: &str,
            _: Option<u32>,
            _: Option<u32>,
        ) -> Result<Vec<Event>, IntervalsError> {
            Ok(vec![])
        }
        async fn get_hr_curves(
            &self,
            _: Option<i32>,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_pace_curves(
            &self,
            _: Option<i32>,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_workout_library(
            &self,
        ) -> Result<Vec<intervals_icu_client::domains::workout::WorkoutItem>, IntervalsError>
        {
            Ok(vec![])
        }
        async fn get_workouts_in_folder(
            &self,
            _: &str,
        ) -> Result<Vec<intervals_icu_client::domains::workout::WorkoutItem>, IntervalsError>
        {
            Ok(vec![])
        }
        async fn create_folder(
            &self,
            _: &serde_json::Value,
        ) -> Result<intervals_icu_client::domains::workout::Folder, IntervalsError> {
            Ok(intervals_icu_client::domains::workout::Folder {
                id: 0,
                name: String::new(),
                description: None,
                parent_id: None,
                children: vec![],
            })
        }
        async fn update_folder(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_folder(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn create_gear(
            &self,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_gear(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_gear(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn create_gear_reminder(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_gear_reminder(
            &self,
            _: &str,
            _: &str,
            _: bool,
            _: u32,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_sport_settings(
            &self,
            _: &str,
            _: bool,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn apply_sport_settings(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn create_sport_settings(
            &self,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_sport_settings(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn update_wellness_bulk(
            &self,
            _: &[serde_json::Value],
        ) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn get_weather_config(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_weather_config(
            &self,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn list_routes(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_route(&self, _: i64, _: bool) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_route(
            &self,
            _: i64,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_route_similarity(
            &self,
            _: i64,
            _: i64,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
    }

    #[test]
    fn previous_window_matches_current_window_length() {
        let current = AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
        );
        let previous = build_previous_window(&current);

        assert_eq!(previous.window_days(), current.window_days());
        assert_eq!(previous.end_date, current.start_date.pred_opt().unwrap());
    }

    fn activity(id: &str, date: &str) -> ActivitySummary {
        ActivitySummary {
            id: id.to_string(),
            name: Some(format!("Activity {}", id)),
            start_date_local: date.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn daily_load_series_fills_missing_days_with_zero() {
        let window = AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 4).unwrap(),
        );
        let activities = [activity("a1", "2026-03-01"), activity("a2", "2026-03-03")];
        let refs = activities.iter().collect::<Vec<_>>();
        let details = HashMap::from([
            ("a1".to_string(), json!({"icu_training_load": 50.0})),
            ("a2".to_string(), json!({"icu_training_load": 70.0})),
        ]);

        let series = build_daily_load_series(&refs, &details, &window);

        assert_eq!(series.daily.len(), 4);
        assert_eq!(series.daily[0].1, 50.0);
        assert_eq!(series.daily[1].1, 0.0);
        assert_eq!(series.daily[2].1, 70.0);
        assert_eq!(series.daily[3].1, 0.0);
        assert_eq!(series.activities_total, 2);
        assert_eq!(series.activities_with_load, 2);
    }

    #[test]
    fn load_extraction_never_uses_moving_time_as_load() {
        let detail = json!({"moving_time": 5400});
        assert_eq!(extract_activity_load(Some(&detail)), None);
    }

    #[test]
    fn load_extraction_records_exact_alias_provenance() {
        assert_eq!(
            extract_activity_load(Some(&json!({"tss": 73.0}))),
            Some(LoadObservation {
                value: 73.0,
                source: LoadSource::TssAlias
            })
        );
    }

    #[test]
    fn load_extraction_records_icu_training_load_provenance() {
        assert_eq!(
            extract_activity_load(Some(&json!({"icu_training_load": 88.0}))),
            Some(LoadObservation {
                value: 88.0,
                source: LoadSource::IcuTrainingLoad
            })
        );
    }

    #[test]
    fn load_extraction_records_training_load_alias_provenance() {
        assert_eq!(
            extract_activity_load(Some(&json!({"training_load": 65.0}))),
            Some(LoadObservation {
                value: 65.0,
                source: LoadSource::TrainingLoadAlias
            })
        );
    }

    #[test]
    fn load_extraction_records_icu_training_load_camel_case_provenance() {
        assert_eq!(
            extract_activity_load(Some(&json!({"icuTrainingLoad": 72.0}))),
            Some(LoadObservation {
                value: 72.0,
                source: LoadSource::TrainingLoadAlias
            })
        );
    }

    #[test]
    fn load_extraction_prefers_canonical_over_aliases() {
        let detail = json!({
            "icu_training_load": 88.0,
            "training_load": 65.0,
            "tss": 73.0
        });
        assert_eq!(
            extract_activity_load(Some(&detail)),
            Some(LoadObservation {
                value: 88.0,
                source: LoadSource::IcuTrainingLoad
            })
        );
    }

    #[test]
    fn activity_load_records_summary_training_load_provenance() {
        let activity = ActivitySummary {
            id: "a1".to_string(),
            training_load: Some(42),
            ..Default::default()
        };
        assert_eq!(
            activity_load(&activity, None),
            Some(LoadObservation {
                value: 42.0,
                source: LoadSource::ActivitySummaryTrainingLoad
            })
        );
    }

    #[test]
    fn daily_series_separates_rest_days_from_missing_load_coverage() {
        // One loaded activity, one activity without load, and one rest day.
        // Daily values include the rest-day zero, while coverage remains 1/2.
        let window = AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
        );
        let activities = [activity("a1", "2026-03-01"), activity("a2", "2026-03-02")];
        let refs = activities.iter().collect::<Vec<_>>();
        let details = HashMap::from([
            ("a1".to_string(), json!({"icu_training_load": 50.0})),
            // a2 has no load data
        ]);

        let series = build_daily_load_series(&refs, &details, &window);

        assert_eq!(series.daily.len(), 3);
        assert_eq!(series.daily[0].1, 50.0); // loaded
        assert_eq!(series.daily[1].1, 0.0); // activity without load
        assert_eq!(series.daily[2].1, 0.0); // rest day
        assert_eq!(series.activities_total, 2);
        assert_eq!(series.activities_with_load, 1);
        assert_eq!(series.coverage_ratio(), Some(0.5));
    }

    #[test]
    fn daily_load_series_aggregates_multiple_activities_on_same_day() {
        let window = AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 2).unwrap(),
        );
        let activities = [
            activity("a1", "2026-03-01T07:00:00"),
            activity("a2", "2026-03-01T18:00:00"),
        ];
        let refs = activities.iter().collect::<Vec<_>>();
        let details = HashMap::from([
            ("a1".to_string(), json!({"icu_training_load": 35.0})),
            ("a2".to_string(), json!({"icu_training_load": 40.0})),
        ]);

        let series = build_daily_load_series(&refs, &details, &window);

        assert_eq!(series.daily[0].1, 75.0);
        assert_eq!(series.daily[1].1, 0.0);
    }

    #[test]
    fn dedupe_and_sort_events_prefers_unique_calendar_entries() {
        let events = vec![
            Event {
                id: Some("e2".into()),
                start_date_local: "2026-03-03".into(),
                name: "Injury note".into(),
                category: intervals_icu_client::EventCategory::Injured,
                description: None,
                r#type: None,
            },
            Event {
                id: Some("e1".into()),
                start_date_local: "2026-03-01".into(),
                name: "Race day".into(),
                category: intervals_icu_client::EventCategory::RaceA,
                description: None,
                r#type: None,
            },
            Event {
                id: Some("e1".into()),
                start_date_local: "2026-03-01".into(),
                name: "Race day".into(),
                category: intervals_icu_client::EventCategory::RaceA,
                description: None,
                r#type: None,
            },
        ];

        let normalized = dedupe_and_sort_events(events);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].name, "Race day");
        assert_eq!(normalized[1].name, "Injury note");
    }

    #[test]
    fn normalize_upcoming_events_payload_backfills_missing_name() {
        let payload = json!([
            {
                "id": 94131802,
                "category": "WORKOUT",
                "start_date_local": "2026-03-21T00:00:00",
                "description": "Recovery Run Z1"
            },
            {
                "id": 94131803,
                "category": "SICK",
                "start_date_local": "2026-03-22T00:00:00"
            }
        ]);

        let normalized = normalize_upcoming_events_payload(payload);
        let events: Vec<Event> =
            serde_json::from_value(normalized).expect("normalized payload should decode");

        assert_eq!(events[0].name, "Recovery Run Z1");
        assert_eq!(events[1].name, "SICK");
    }

    #[test]
    fn normalize_intervals_payload_extracts_icu_intervals_array() {
        let payload = json!({
            "id": "i126027814",
            "icu_intervals": [
                {"moving_time": 300, "average_heartrate": 142},
                {"moving_time": 360, "average_heartrate": 158}
            ],
            "icu_groups": [
                {"moving_time": 300, "count": 6}
            ]
        });

        let normalized = normalize_intervals_payload(payload);
        let intervals = normalized
            .as_array()
            .expect("interval payload should normalize to array");

        assert_eq!(intervals.len(), 2);
        assert_eq!(
            intervals[0].get("moving_time").and_then(Value::as_i64),
            Some(300)
        );
    }

    #[test]
    fn normalize_intervals_payload_falls_back_to_icu_groups_when_needed() {
        let payload = json!({
            "id": "i126027814",
            "icu_groups": [
                {"moving_time": 300, "count": 6}
            ]
        });

        let normalized = normalize_intervals_payload(payload);
        let groups = normalized
            .as_array()
            .expect("group payload should normalize to array");

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].get("count").and_then(Value::as_i64), Some(6));
    }

    #[test]
    fn normalize_streams_payload_extracts_nested_stream_map() {
        let payload = json!({
            "streams": {
                "heartrate": [140, 145, 150],
                "watts": [220, 230, 240]
            }
        });

        let normalized = normalize_streams_payload(payload);
        let object = normalized
            .as_object()
            .expect("stream payload should normalize to object map");

        assert!(object.contains_key("heartrate"));
        assert!(object.contains_key("watts"));
    }

    #[test]
    fn normalize_streams_payload_extracts_descriptor_array() {
        let payload = json!({
            "streams": [
                {"type": "heartrate", "data": [140, 145, 150]},
                {"type": "watts", "data": [220, 230, 240]}
            ]
        });

        let normalized = normalize_streams_payload(payload);
        let object = normalized
            .as_object()
            .expect("descriptor array should normalize to object map");

        assert_eq!(
            object
                .get("heartrate")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(3)
        );
        assert_eq!(
            object.get("watts").and_then(Value::as_array).map(Vec::len),
            Some(3)
        );
    }

    // ========================================================================
    // required_activity_window / activity_lookback_days Tests
    // ========================================================================

    #[test]
    fn historical_period_lookback_is_anchored_to_today() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap();
        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(
                NaiveDate::from_ymd_opt(2025, 4, 1).unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
            ),
            include_activity_details: false,
            include_comparison_window: false,
        };

        let (start, end) = required_activity_window(&request);
        assert_eq!(start, NaiveDate::from_ymd_opt(2025, 4, 1).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2025, 6, 30).unwrap());
        assert_eq!(activity_lookback_days(start, today).unwrap(), 459);
    }

    #[test]
    fn comparison_fetch_includes_the_full_previous_window() {
        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(
                NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            ),
            include_activity_details: false,
            include_comparison_window: true,
        };

        let (start, end) = required_activity_window(&request);
        assert_eq!(start, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 6, 30).unwrap());
    }

    // ========================================================================
    // PeriodFetchRequest Tests
    // ========================================================================

    #[test]
    fn period_fetch_request_clone() {
        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
            ),
            include_activity_details: true,
            include_comparison_window: false,
        };
        let _cloned = request.clone();
    }

    #[test]
    fn period_fetch_request_debug() {
        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
            ),
            include_activity_details: true,
            include_comparison_window: false,
        };
        let debug_str = format!("{:?}", request);
        assert!(debug_str.contains("PeriodFetchRequest"));
    }

    // ========================================================================
    // SingleWorkoutFetchRequest Tests
    // ========================================================================

    #[test]
    fn single_workout_fetch_request_clone() {
        let request = SingleWorkoutFetchRequest {
            activity_id: "a123".to_string(),
            include_intervals: true,
            include_streams: true,
            include_best_efforts: false,
            include_hr_histogram: true,
            include_power_histogram: true,
            include_pace_histogram: false,
        };
        let _cloned = request.clone();
    }

    #[test]
    fn single_workout_fetch_request_debug() {
        let request = SingleWorkoutFetchRequest {
            activity_id: "a123".to_string(),
            include_intervals: true,
            include_streams: false,
            include_best_efforts: false,
            include_hr_histogram: false,
            include_power_histogram: false,
            include_pace_histogram: false,
        };
        let debug_str = format!("{:?}", request);
        assert!(debug_str.contains("SingleWorkoutFetchRequest"));
    }

    // ========================================================================
    // RecoveryFetchRequest Tests
    // ========================================================================

    #[test]
    fn recovery_fetch_request_clone() {
        let request = RecoveryFetchRequest {
            period_days: 7,
            include_wellness: true,
        };
        let _cloned = request.clone();
    }

    #[test]
    fn recovery_fetch_request_debug() {
        let request = RecoveryFetchRequest {
            period_days: 14,
            include_wellness: false,
        };
        let debug_str = format!("{:?}", request);
        assert!(debug_str.contains("RecoveryFetchRequest"));
    }

    // ========================================================================
    // RaceFetchRequest Tests
    // ========================================================================

    #[test]
    fn race_fetch_request_clone() {
        let request = RaceFetchRequest {
            activity_id: "r123".to_string(),
            include_intervals: true,
            include_streams: false,
        };
        let _cloned = request.clone();
    }

    #[test]
    fn race_fetch_request_debug() {
        let request = RaceFetchRequest {
            activity_id: "r123".to_string(),
            include_intervals: false,
            include_streams: true,
        };
        let debug_str = format!("{:?}", request);
        assert!(debug_str.contains("RaceFetchRequest"));
    }

    // ========================================================================
    // FetchedAnalysisData Tests
    // ========================================================================

    #[test]
    fn fetched_analysis_data_default() {
        let data = FetchedAnalysisData::default();
        assert!(data.activities.is_empty());
        assert!(data.comparison_activities.is_empty());
        assert!(data.calendar_events.is_empty());
        assert!(data.activity_messages.is_empty());
        assert!(data.activity_details.is_empty());
        assert!(data.workout_detail.is_none());
        assert!(data.fitness.is_none());
        assert!(data.wellness.is_none());
        assert!(data.intervals.is_none());
        assert!(data.streams.is_none());
        assert!(data.best_efforts.is_none());
        assert!(data.hr_histogram.is_none());
        assert!(data.power_histogram.is_none());
        assert!(data.pace_histogram.is_none());
    }

    #[test]
    fn fetched_analysis_data_clone() {
        let data = FetchedAnalysisData {
            activities: vec![ActivitySummary {
                id: "a1".to_string(),
                name: Some("Test".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let _cloned = data.clone();
    }

    #[test]
    fn fetched_analysis_data_debug() {
        let data = FetchedAnalysisData::default();
        let debug_str = format!("{:?}", data);
        assert!(debug_str.contains("FetchedAnalysisData"));
    }

    // ========================================================================
    // Parse Activity Date Tests
    // ========================================================================

    #[test]
    fn parse_activity_date_with_time() {
        let result = parse_activity_date("2026-03-01T10:30:00");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        );
    }

    #[test]
    fn parse_activity_date_without_time() {
        let result = parse_activity_date("2026-03-01");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        );
    }

    #[test]
    fn parse_activity_date_invalid_format() {
        assert!(parse_activity_date("invalid").is_none());
        assert!(parse_activity_date("01-03-2026").is_none());
        assert!(parse_activity_date("").is_none());
    }

    // ========================================================================
    // Parse Event Date Tests
    // ========================================================================

    #[test]
    fn parse_event_date_with_time() {
        let result = parse_event_date("2026-03-01T10:30:00");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        );
    }

    #[test]
    fn parse_event_date_without_time() {
        let result = parse_event_date("2026-03-01");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        );
    }

    #[test]
    fn parse_event_date_invalid() {
        assert!(parse_event_date("not-a-date").is_none());
    }

    // ========================================================================
    // Dedupe and Sort Events Tests
    // ========================================================================

    #[test]
    fn dedupe_and_sort_events_empty_list() {
        let events: Vec<Event> = vec![];
        let result = dedupe_and_sort_events(events);
        assert!(result.is_empty());
    }

    #[test]
    fn dedupe_and_sort_events_removes_duplicates_by_id() {
        let events = vec![
            Event {
                id: Some("e1".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Event 1".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
            Event {
                id: Some("e1".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Event 1 Duplicate".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
        ];
        let result = dedupe_and_sort_events(events);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, Some("e1".to_string()));
    }

    #[test]
    fn dedupe_and_sort_events_sorts_by_date() {
        let events = vec![
            Event {
                id: Some("e2".to_string()),
                start_date_local: "2026-03-02".to_string(),
                name: "Event 2".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
            Event {
                id: Some("e1".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Event 1".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
        ];
        let result = dedupe_and_sort_events(events);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, Some("e1".to_string()));
        assert_eq!(result[1].id, Some("e2".to_string()));
    }

    #[test]
    fn dedupe_and_sort_events_without_id_uses_fallback_key() {
        let events = vec![
            Event {
                id: None,
                start_date_local: "2026-03-01".to_string(),
                name: "Same Event".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
            Event {
                id: None,
                start_date_local: "2026-03-01".to_string(),
                name: "Same Event".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            },
        ];
        let result = dedupe_and_sort_events(events);
        assert_eq!(result.len(), 1);
    }

    // ========================================================================
    // Normalize Upcoming Events Payload Tests
    // ========================================================================

    #[test]
    fn normalize_upcoming_events_payload_non_array_passthrough() {
        let payload = json!({"not": "an array"});
        let result = normalize_upcoming_events_payload(payload.clone());
        assert_eq!(result, payload);
    }

    #[test]
    fn normalize_upcoming_events_payload_with_name_unchanged() {
        let payload = json!([
            {
                "id": 1,
                "name": "Has Name",
                "start_date_local": "2026-03-01"
            }
        ]);
        let result = normalize_upcoming_events_payload(payload.clone());
        assert_eq!(result, payload);
    }

    #[test]
    fn normalize_upcoming_events_payload_empty_name_uses_description() {
        let payload = json!([
            {
                "id": 1,
                "name": "",
                "description": "Fallback Description",
                "category": "WORKOUT",
                "start_date_local": "2026-03-01"
            }
        ]);
        let result = normalize_upcoming_events_payload(payload);
        let events: Vec<Event> = serde_json::from_value(result).unwrap();
        assert_eq!(events[0].name, "Fallback Description");
    }

    #[test]
    fn normalize_upcoming_events_payload_empty_name_uses_category() {
        let payload = json!([
            {
                "id": 1,
                "name": "  ",
                "category": "WORKOUT",
                "start_date_local": "2026-03-01"
            }
        ]);
        let result = normalize_upcoming_events_payload(payload);
        let events: Vec<Event> = serde_json::from_value(result).unwrap();
        assert_eq!(events[0].name, "WORKOUT");
    }

    #[test]
    fn normalize_upcoming_events_payload_no_name_fallback_to_default() {
        let payload = json!([
            {
                "id": 1,
                "name": "",
                "description": "",
                "category": "",
                "start_date_local": "2026-03-01"
            }
        ]);
        let result = normalize_upcoming_events_payload(payload);
        let events: Vec<Event> = serde_json::from_value(result).unwrap();
        assert_eq!(events[0].name, "Untitled event");
    }

    // ========================================================================
    // Normalize Intervals Payload Tests
    // ========================================================================

    #[test]
    fn normalize_intervals_payload_array_passthrough() {
        let payload = json!([
            {"moving_time": 300},
            {"moving_time": 600}
        ]);
        let result = normalize_intervals_payload(payload.clone());
        assert_eq!(result, payload);
    }

    #[test]
    fn normalize_intervals_payload_non_object_passthrough() {
        let payload = json!("not an object");
        let result = normalize_intervals_payload(payload.clone());
        assert_eq!(result, payload);
    }

    #[test]
    fn normalize_intervals_payload_no_intervals_or_groups_passthrough() {
        let payload = json!({
            "id": "i123",
            "other_field": "value"
        });
        let result = normalize_intervals_payload(payload.clone());
        assert_eq!(result, payload);
    }

    // ========================================================================
    // Normalize Streams Payload Tests
    // ========================================================================

    #[test]
    fn normalize_streams_payload_array_passthrough() {
        // Array input gets normalized to object by normalize_stream_descriptor_array
        let payload = json!([
            {"name": "heartrate", "data": [140, 145]}
        ]);
        let result = normalize_streams_payload(payload);
        // Should normalize to object map
        assert!(result.as_object().is_some());
        assert!(result.as_object().unwrap().contains_key("heartrate"));
    }

    #[test]
    fn normalize_streams_payload_non_object_passthrough() {
        let payload = json!("not an object");
        let result = normalize_streams_payload(payload.clone());
        assert_eq!(result, payload);
    }

    #[test]
    fn normalize_streams_payload_no_streams_key_passthrough() {
        let payload = json!({
            "other": "field"
        });
        let result = normalize_streams_payload(payload.clone());
        assert_eq!(result, payload);
    }

    #[test]
    fn normalize_streams_payload_streams_non_array_passthrough() {
        let payload = json!({
            "streams": "not an array"
        });
        let result = normalize_streams_payload(payload.clone());
        assert_eq!(result, payload);
    }

    // ========================================================================
    // Extract Activity Load Tests
    // ========================================================================

    #[test]
    fn extract_activity_load_none_input() {
        assert_eq!(extract_activity_load(None), None);
    }

    #[test]
    fn extract_activity_load_non_object() {
        let detail = json!("not an object");
        assert_eq!(extract_activity_load(Some(&detail)), None);
    }

    #[test]
    fn extract_activity_load_no_load_or_time() {
        let detail = json!({"distance": 10000});
        assert_eq!(extract_activity_load(Some(&detail)), None);
    }

    #[test]
    fn extract_activity_load_alternate_load_field_names() {
        let detail = json!({"training_load": 75.0});
        assert_eq!(
            extract_activity_load(Some(&detail)),
            Some(LoadObservation {
                value: 75.0,
                source: LoadSource::TrainingLoadAlias
            })
        );
    }

    #[test]
    fn extract_activity_load_camelcase_load_field() {
        let detail = json!({"icuTrainingLoad": 88.0});
        assert_eq!(
            extract_activity_load(Some(&detail)),
            Some(LoadObservation {
                value: 88.0,
                source: LoadSource::TrainingLoadAlias
            })
        );
    }

    #[test]
    fn extract_activity_load_integer_load() {
        let detail = json!({"icu_training_load": 90});
        assert_eq!(
            extract_activity_load(Some(&detail)),
            Some(LoadObservation {
                value: 90.0,
                source: LoadSource::IcuTrainingLoad
            })
        );
    }

    #[test]
    fn extract_activity_load_moving_time_returns_none() {
        let detail = json!({"moving_time": 7200});
        assert_eq!(extract_activity_load(Some(&detail)), None);
    }

    // ========================================================================
    // Build Previous Window Tests
    // ========================================================================

    #[test]
    fn build_previous_window_correct_length() {
        let current = AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 14).unwrap(),
        );
        let previous = build_previous_window(&current);
        assert_eq!(previous.window_days(), current.window_days());
    }

    #[test]
    fn build_previous_window_adjacent_to_current() {
        let current = AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 14).unwrap(),
        );
        let previous = build_previous_window(&current);
        assert_eq!(previous.end_date, current.start_date.pred_opt().unwrap());
    }

    #[test]
    fn build_previous_window_7_day_example() {
        let current = AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 14).unwrap(),
        );
        let previous = build_previous_window(&current);
        assert_eq!(
            previous.start_date,
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        );
        assert_eq!(
            previous.end_date,
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap()
        );
    }

    // ========================================================================
    // Parse Planned Workout Tests
    // ========================================================================

    #[test]
    fn parse_planned_workout_with_paired_activity_id_excluded() {
        let known_ids = std::collections::HashSet::from(["a123".to_string()]);
        let event = json!({
            "id": 1,
            "start_date_local": "2026-03-01",
            "description": "Planned",
            "paired_activity_id": "a123"
        });
        assert!(parse_planned_workout(&event, &known_ids).is_none());
    }

    #[test]
    fn parse_planned_workout_missing_id() {
        let known_ids = std::collections::HashSet::new();
        let event = json!({
            "start_date_local": "2026-03-01",
            "description": "Planned"
        });
        assert!(parse_planned_workout(&event, &known_ids).is_none());
    }

    #[test]
    fn parse_planned_workout_missing_start_date() {
        let known_ids = std::collections::HashSet::new();
        let event = json!({
            "id": 1,
            "description": "Planned"
        });
        assert!(parse_planned_workout(&event, &known_ids).is_none());
    }

    #[test]
    fn parse_planned_workout_no_description_uses_fallback() {
        let known_ids = std::collections::HashSet::new();
        let event = json!({
            "id": 1,
            "start_date_local": "2026-03-01"
        });
        let result = parse_planned_workout(&event, &known_ids);
        assert!(result.is_some());
        let (activity, _) = result.unwrap();
        assert_eq!(activity.name, Some("Planned workout".to_string()));
    }

    #[test]
    fn parse_planned_workout_uses_name_if_description_missing() {
        let known_ids = std::collections::HashSet::new();
        let event = json!({
            "id": 1,
            "start_date_local": "2026-03-01",
            "name": "Named Workout"
        });
        let result = parse_planned_workout(&event, &known_ids);
        assert!(result.is_some());
        let (activity, _) = result.unwrap();
        assert_eq!(activity.name, Some("Named Workout".to_string()));
    }

    #[test]
    fn parse_planned_workout_string_id() {
        let known_ids = std::collections::HashSet::new();
        let event = json!({
            "id": "event_1",
            "start_date_local": "2026-03-01",
            "description": "Planned"
        });
        let result = parse_planned_workout(&event, &known_ids);
        assert!(result.is_some());
        let (activity, _) = result.unwrap();
        assert_eq!(activity.id, "event:event_1");
    }

    #[tokio::test]
    async fn fetch_period_data_reuses_single_upcoming_fetch_for_calendar_and_planned_workouts() {
        let today = chrono::Utc::now().date_naive();
        let client = MockIntervalsClient::builder()
            .with_activities(vec![activity("a1", &today.to_string())])
            .with_upcoming_workouts(json!([
                {
                    "id": 11,
                    "category": "WORKOUT",
                    "start_date_local": (today + Duration::days(1)).to_string(),
                    "description": "Planned threshold session"
                },
                {
                    "id": 12,
                    "category": "NOTE",
                    "start_date_local": (today + Duration::days(1)).to_string(),
                    "description": "Coach note"
                }
            ]));

        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(today - Duration::days(2), today + Duration::days(2)),
            include_activity_details: false,
            include_comparison_window: false,
        };

        let fetched = fetch_period_data(&client as &dyn IntervalsClient, &request)
            .await
            .expect("period fetch succeeds");

        assert_eq!(client.upcoming_workouts_call_count(), 1);
        assert_eq!(fetched.calendar_events.len(), 2);
        assert!(
            fetched
                .activities
                .iter()
                .any(|activity| activity.id == "event:11")
        );
    }

    #[tokio::test]
    async fn period_fetch_retains_all_in_window_activities() {
        let today = chrono::Utc::now().date_naive();
        let required_start = today - Duration::days(459);
        let required_end = required_start + Duration::days(90);

        // 250 activities inside the 90-day window (multiple per day), 10 outside
        let mut activities_in_window = (0..250)
            .map(|i| {
                let day_offset = (i as i64) % 91;
                let date = required_start + Duration::days(day_offset);
                activity(&format!("in_{i}"), &date.to_string())
            })
            .collect::<Vec<_>>();
        let activities_outside = (0..10)
            .map(|i| {
                let date = required_start - Duration::days(i + 1);
                activity(&format!("out_{i}"), &date.to_string())
            })
            .collect::<Vec<_>>();
        activities_in_window.extend(activities_outside);

        let client = RecordingPeriodClient::with_activities(activities_in_window);
        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(required_start, required_end),
            include_activity_details: false,
            include_comparison_window: false,
        };

        let fetched = fetch_period_data(&client, &request).await.unwrap();

        assert_eq!(fetched.activities.len(), 250);
        assert!(fetched.activities.iter().all(|a| {
            parse_activity_date(&a.start_date_local)
                .is_some_and(|date| date >= required_start && date <= required_end)
        }));
    }

    #[tokio::test]
    async fn period_fetch_requests_complete_historical_activity_list() {
        let today = chrono::Utc::now().date_naive();
        let start = today - Duration::days(459);
        let client = RecordingPeriodClient::with_activities(Vec::new());
        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(start, start + Duration::days(90)),
            include_activity_details: false,
            include_comparison_window: false,
        };

        fetch_period_data(&client, &request).await.unwrap();

        assert_eq!(client.activity_calls(), vec![(None, Some(459))]);
    }

    /// Recording mock that captures `get_activity_details` IDs for assertions.
    struct DetailRecordingClient {
        activities: Vec<ActivitySummary>,
        detail_ids: Arc<Mutex<Vec<String>>>,
        failing_detail_ids: std::collections::HashSet<String>,
    }

    impl DetailRecordingClient {
        fn with_activities(activities: Vec<ActivitySummary>) -> Self {
            Self {
                activities,
                detail_ids: Arc::new(Mutex::new(Vec::new())),
                failing_detail_ids: std::collections::HashSet::new(),
            }
        }

        fn with_failing_details(mut self, ids: Vec<&str>) -> Self {
            self.failing_detail_ids = ids.into_iter().map(str::to_owned).collect();
            self
        }

        fn requested_detail_ids(&self) -> Vec<String> {
            self.detail_ids.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl IntervalsClient for DetailRecordingClient {
        async fn get_recent_activities(
            &self,
            _limit: Option<u32>,
            _days_back: Option<i32>,
        ) -> Result<Vec<ActivitySummary>, IntervalsError> {
            Ok(self.activities.clone())
        }

        async fn get_activity_details(
            &self,
            activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            self.detail_ids
                .lock()
                .unwrap()
                .push(activity_id.to_string());
            if self.failing_detail_ids.contains(activity_id) {
                Err(IntervalsError::NotFound(format!(
                    "detail not available for {activity_id}"
                )))
            } else {
                Ok(serde_json::json!({"id": activity_id}))
            }
        }

        async fn get_athlete_profile(
            &self,
        ) -> Result<intervals_icu_client::AthleteProfile, IntervalsError> {
            Ok(intervals_icu_client::AthleteProfile {
                id: "test".into(),
                name: None,
            })
        }
        async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError> {
            Err(IntervalsError::NotFound("not needed".into()))
        }
        async fn get_activity_streams(
            &self,
            _: &str,
            _: Option<Vec<String>>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_activity_intervals(
            &self,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_best_efforts(
            &self,
            _: &str,
            _: Option<intervals_icu_client::BestEffortsOptions>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_hr_histogram(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_power_histogram(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_pace_histogram(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_activity_messages(
            &self,
            _: &str,
        ) -> Result<Vec<ActivityMessage>, IntervalsError> {
            Ok(vec![])
        }
        async fn get_events(
            &self,
            _: Option<i32>,
            _: Option<u32>,
        ) -> Result<Vec<Event>, IntervalsError> {
            Ok(vec![])
        }
        async fn get_wellness_for_date(
            &self,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Err(IntervalsError::NotFound("not needed".into()))
        }
        async fn create_event(&self, _: Event) -> Result<Event, IntervalsError> {
            Err(IntervalsError::NotFound("not needed".into()))
        }
        async fn get_event(&self, _: &str) -> Result<Event, IntervalsError> {
            Err(IntervalsError::NotFound("not needed".into()))
        }
        async fn delete_event(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn bulk_create_events(&self, _: Vec<Event>) -> Result<Vec<Event>, IntervalsError> {
            Ok(vec![])
        }
        async fn search_activities(
            &self,
            _: &str,
            _: Option<u32>,
        ) -> Result<Vec<ActivitySummary>, IntervalsError> {
            Ok(vec![])
        }
        async fn search_activities_full(
            &self,
            _: &str,
            _: Option<u32>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
            Ok(String::new())
        }
        async fn update_activity(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn download_activity_file(
            &self,
            _: &str,
            _: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn download_activity_file_with_progress(
            &self,
            _: &str,
            _: Option<std::path::PathBuf>,
            _: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
            _: tokio::sync::watch::Receiver<bool>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn download_fit_file(
            &self,
            _: &str,
            _: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn download_gpx_file(
            &self,
            _: &str,
            _: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn get_gear_list(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_sport_settings(
            &self,
        ) -> Result<intervals_icu_client::domains::workout::SportSettings, IntervalsError> {
            Ok(Default::default())
        }
        async fn get_power_curves(
            &self,
            _: Option<i32>,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_gap_histogram(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn delete_activity(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn get_activities_around(
            &self,
            _: &str,
            _: Option<u32>,
            _: Option<i64>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
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
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_wellness(&self, _: Option<i32>) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn update_wellness(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_upcoming_workouts(
            &self,
            _: Option<u32>,
            _: Option<u32>,
            _: Option<String>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn update_event(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn bulk_delete_events(&self, _: Vec<String>) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn duplicate_event(
            &self,
            _: &str,
            _: Option<u32>,
            _: Option<u32>,
        ) -> Result<Vec<Event>, IntervalsError> {
            Ok(vec![])
        }
        async fn get_hr_curves(
            &self,
            _: Option<i32>,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_pace_curves(
            &self,
            _: Option<i32>,
            _: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_workout_library(
            &self,
        ) -> Result<Vec<intervals_icu_client::domains::workout::WorkoutItem>, IntervalsError>
        {
            Ok(vec![])
        }
        async fn get_workouts_in_folder(
            &self,
            _: &str,
        ) -> Result<Vec<intervals_icu_client::domains::workout::WorkoutItem>, IntervalsError>
        {
            Ok(vec![])
        }
        async fn create_folder(
            &self,
            _: &serde_json::Value,
        ) -> Result<intervals_icu_client::domains::workout::Folder, IntervalsError> {
            Ok(intervals_icu_client::domains::workout::Folder {
                id: 0,
                name: String::new(),
                description: None,
                parent_id: None,
                children: vec![],
            })
        }
        async fn update_folder(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_folder(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn create_gear(
            &self,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_gear(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_gear(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn create_gear_reminder(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_gear_reminder(
            &self,
            _: &str,
            _: &str,
            _: bool,
            _: u32,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_sport_settings(
            &self,
            _: &str,
            _: bool,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn apply_sport_settings(&self, _: &str) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn create_sport_settings(
            &self,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_sport_settings(&self, _: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn update_wellness_bulk(
            &self,
            _: &[serde_json::Value],
        ) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn get_weather_config(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_weather_config(
            &self,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn list_routes(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_route(&self, _: i64, _: bool) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_route(
            &self,
            _: i64,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_route_similarity(
            &self,
            _: i64,
            _: i64,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
    }

    #[tokio::test]
    async fn period_fetch_limits_details_to_required_window() {
        let today = chrono::Utc::now().date_naive();
        let window_start = today - Duration::days(30);
        let window_end = today - Duration::days(1);

        let activities = vec![
            activity("before", &(window_start - Duration::days(5)).to_string()),
            activity("inside", &window_start.to_string()),
            activity("after", &(window_end + Duration::days(5)).to_string()),
        ];

        let client = DetailRecordingClient::with_activities(activities);
        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(window_start, window_end),
            include_activity_details: true,
            include_comparison_window: false,
        };

        let fetched = fetch_period_data(&client as &dyn IntervalsClient, &request)
            .await
            .expect("fetch succeeds");

        // After window filtering, only "inside" remains
        assert_eq!(fetched.activities.len(), 1);
        assert_eq!(fetched.activities[0].id, "inside");
        assert_eq!(client.requested_detail_ids(), vec!["inside"]);
    }

    #[tokio::test]
    async fn period_fetch_reports_partial_detail_coverage() {
        let today = chrono::Utc::now().date_naive();
        let window_start = today - Duration::days(30);
        let window_end = today - Duration::days(1);

        let activities = vec![
            activity("ok", &window_start.to_string()),
            activity("fail", &(window_start + Duration::days(1)).to_string()),
        ];

        let client =
            DetailRecordingClient::with_activities(activities).with_failing_details(vec!["fail"]);
        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(window_start, window_end),
            include_activity_details: true,
            include_comparison_window: false,
        };

        let fetched = fetch_period_data(&client as &dyn IntervalsClient, &request)
            .await
            .expect("fetch succeeds");

        assert_eq!(fetched.activities.len(), 2);
        assert!(fetched.activity_details.contains_key("ok"));
        assert!(!fetched.activity_details.contains_key("fail"));
        assert!(fetched.fetch_warnings.iter().any(|warning| {
            warning.contains("1 of 2 activity details unavailable")
                && warning.contains("period totals remain available")
        }));
    }

    #[tokio::test]
    async fn fetch_period_data_degrades_gracefully_when_upcoming_workouts_are_rate_limited() {
        let today = chrono::Utc::now().date_naive();
        let client = MockIntervalsClient::builder()
            .with_activities(vec![activity("a1", &today.to_string())])
            .with_upcoming_workouts_error(IntervalsError::from_status(429, "rate limited"));

        let request = PeriodFetchRequest {
            window: AnalysisWindow::new(today - Duration::days(2), today + Duration::days(2)),
            include_activity_details: false,
            include_comparison_window: false,
        };

        let fetched = fetch_period_data(&client as &dyn IntervalsClient, &request)
            .await
            .expect("period fetch degrades");

        assert_eq!(client.upcoming_workouts_call_count(), 1);
        assert!(
            fetched
                .fetch_warnings
                .iter()
                .any(|warning| warning.contains("rate limiting"))
        );
        assert_eq!(fetched.activities.len(), 1);
    }
}
