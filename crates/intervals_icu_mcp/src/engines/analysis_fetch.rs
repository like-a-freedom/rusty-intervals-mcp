use std::{collections::HashMap, collections::HashSet};

use chrono::{Duration, NaiveDate, NaiveDateTime};
use intervals_icu_client::{ActivityMessage, ActivitySummary, Event, IntervalsClient};
use serde_json::Value;

use crate::domains::coach::AnalysisWindow;
use crate::intents::IntentError;

const ADAPTIVE_HRV_LOOKBACK_DAYS: i32 = 35;

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

#[derive(Debug, Clone, Default)]
pub struct FetchedAnalysisData {
    pub activities: Vec<ActivitySummary>,
    pub comparison_activities: Vec<ActivitySummary>,
    pub calendar_events: Vec<Event>,
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
}

pub fn build_previous_window(current: &AnalysisWindow) -> AnalysisWindow {
    let days = current.window_days();
    let previous_end = current.start_date - Duration::days(1);
    let previous_start = previous_end - Duration::days(days - 1);
    AnalysisWindow::new(previous_start, previous_end)
}

pub fn extract_activity_load(detail: Option<&Value>) -> Option<f64> {
    let object = detail?.as_object()?;

    ["icu_training_load", "training_load", "icuTrainingLoad"]
        .iter()
        .find_map(|key| {
            object
                .get(*key)
                .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
        })
        .or_else(|| {
            object
                .get("moving_time")
                .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
                .map(|seconds| seconds / 60.0)
        })
}

pub fn build_daily_load_series(
    activities: &[&ActivitySummary],
    details: &HashMap<String, Value>,
    window: &AnalysisWindow,
) -> Vec<(NaiveDate, f64)> {
    let mut totals = HashMap::<NaiveDate, f64>::new();

    for activity in activities {
        if let Some(activity_date) = parse_activity_date(&activity.start_date_local) {
            let load = extract_activity_load(details.get(&activity.id)).unwrap_or(0.0);
            totals
                .entry(activity_date)
                .and_modify(|total| *total += load)
                .or_insert(load);
        }
    }

    let mut current = window.start_date;
    let mut series = Vec::with_capacity(window.window_days().max(0) as usize);
    while current <= window.end_date {
        series.push((current, totals.get(&current).copied().unwrap_or(0.0)));
        current += Duration::days(1);
    }

    series
}

fn parse_activity_date(value: &str) -> Option<NaiveDate> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|dt| dt.date())
        .or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
}

fn parse_event_date(value: &str) -> Option<NaiveDate> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|dt| dt.date())
        .or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
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

pub async fn fetch_calendar_events_between(
    client: &dyn IntervalsClient,
    start_date: &NaiveDate,
    end_date: &NaiveDate,
    limit: u32,
) -> Result<Vec<Event>, IntentError> {
    let today = chrono::Utc::now().date_naive();
    let mut events = Vec::new();

    if *start_date <= today {
        let days_back = (today - *start_date).num_days() as i32;
        let mut historical = client
            .get_events(Some(days_back), Some(limit))
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch events: {}", e)))?;
        events.append(&mut historical);
    }

    if *end_date >= today {
        let days_ahead = (*end_date - today).num_days().max(0) as u32;
        let upcoming = client
            .get_upcoming_workouts(Some(days_ahead), Some(limit), None)
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch upcoming events: {}", e)))?;

        let normalized_upcoming = normalize_upcoming_events_payload(upcoming);
        let mut parsed: Vec<Event> = serde_json::from_value(normalized_upcoming)
            .map_err(|e| IntentError::api(format!("Failed to decode upcoming events: {}", e)))?;
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
) -> Result<FetchedAnalysisData, IntentError> {
    let days_back = ((request.window.end_date - request.window.start_date).num_days() as i32) + 30;
    let activities = client
        .get_recent_activities(Some(200), Some(days_back))
        .await
        .map_err(|e| IntentError::api(format!("Failed to fetch activities: {}", e)))?;

    let known_activity_ids = activities
        .iter()
        .map(|activity| activity.id.clone())
        .collect::<std::collections::HashSet<_>>();

    let mut fetched = FetchedAnalysisData {
        activities,
        ..Default::default()
    };

    fetched.calendar_events = fetch_calendar_events_between(
        client,
        &request.window.start_date,
        &request.window.end_date,
        500,
    )
    .await?;

    if request.include_activity_details {
        for activity in &fetched.activities {
            if let Ok(details) = client.get_activity_details(&activity.id).await {
                fetched
                    .activity_details
                    .insert(activity.id.clone(), details);
            }
        }
    }

    let today = chrono::Utc::now().date_naive();
    if request.window.end_date >= today {
        let days_ahead = (request.window.end_date - today).num_days().max(0) as u32;
        let upcoming_workouts = client
            .get_upcoming_workouts(Some(days_ahead), Some(200), Some("WORKOUT".to_string()))
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch upcoming workouts: {}", e)))?;

        if let Some(events) = upcoming_workouts.as_array() {
            for event in events {
                if let Some((activity, detail)) = parse_planned_workout(event, &known_activity_ids)
                {
                    if request.include_activity_details {
                        fetched.activity_details.insert(activity.id.clone(), detail);
                    }
                    fetched.activities.push(activity);
                }
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
) -> Result<FetchedAnalysisData, IntentError> {
    let wellness = if request.include_wellness {
        let wellness_lookback_days = request.period_days.max(ADAPTIVE_HRV_LOOKBACK_DAYS);
        Some(
            client
                .get_wellness(Some(wellness_lookback_days))
                .await
                .map_err(|e| IntentError::api(format!("Failed to fetch wellness: {}", e)))?,
        )
    } else {
        None
    };

    let fitness = Some(
        client
            .get_fitness_summary()
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch fitness: {}", e)))?,
    );

    let activities = client
        .get_recent_activities(Some(20), Some(request.period_days))
        .await
        .map_err(|e| IntentError::api(format!("Failed to fetch activities: {}", e)))?;

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
) -> Result<FetchedAnalysisData, IntentError> {
    let workout_detail = Some(
        client
            .get_activity_details(&request.activity_id)
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch activity details: {}", e)))?,
    );

    let intervals = if request.include_intervals {
        client
            .get_activity_intervals(&request.activity_id)
            .await
            .map(normalize_intervals_payload)
            .ok()
    } else {
        None
    };

    let streams = if request.include_streams {
        client
            .get_activity_streams(&request.activity_id, None)
            .await
            .map(normalize_streams_payload)
            .ok()
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
        ..Default::default()
    })
}

pub async fn fetch_race_data(
    client: &dyn IntervalsClient,
    request: &RaceFetchRequest,
) -> Result<FetchedAnalysisData, IntentError> {
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
    use chrono::NaiveDate;
    use serde_json::json;

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

        assert_eq!(series.len(), 4);
        assert_eq!(series[0].1, 50.0);
        assert_eq!(series[1].1, 0.0);
        assert_eq!(series[2].1, 70.0);
        assert_eq!(series[3].1, 0.0);
    }

    #[test]
    fn extract_activity_load_prefers_canonical_load_over_moving_time_proxy() {
        let detail = json!({
            "icu_training_load": 88.0,
            "moving_time": 5400
        });

        assert_eq!(extract_activity_load(Some(&detail)), Some(88.0));
    }

    #[test]
    fn extract_activity_load_falls_back_to_moving_time_minutes() {
        let detail = json!({"moving_time": 5400});

        assert_eq!(extract_activity_load(Some(&detail)), Some(90.0));
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

        assert_eq!(series[0].1, 75.0);
        assert_eq!(series[1].1, 0.0);
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
}
