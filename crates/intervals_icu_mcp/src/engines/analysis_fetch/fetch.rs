use std::collections::HashSet;

use chrono::NaiveDate;
use intervals_icu_client::{ActivitySummary, Event, IntervalsClient};
use serde_json::Value;

use super::decode::{
    normalize_intervals_payload, normalize_streams_payload, normalize_upcoming_events_payload,
    upcoming_rate_limit_warning, value_is_empty,
};
use super::load::{activity_lookback_days, required_activity_window};
use super::{
    FetchedAnalysisData, PeriodFetchRequest, RaceFetchRequest, RecoveryFetchRequest,
    SingleWorkoutFetchRequest, SourceFetchState,
};
use crate::engines::dedupe::dedupe_and_sort_events;
use crate::engines::fetch_error::FetchError;
use crate::engines::shared::parse_activity_date;

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

pub(crate) fn parse_planned_workout(
    event: &Value,
    known_activity_ids: &HashSet<String>,
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
        .collect::<HashSet<_>>();

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

    if request.include_endurance_evidence {
        super::collect_endurance_evidence(client, request, &mut fetched).await;
    }

    Ok(fetched)
}

pub async fn fetch_recovery_data(
    client: &dyn IntervalsClient,
    request: &RecoveryFetchRequest,
) -> Result<FetchedAnalysisData, FetchError> {
    let wellness = if request.include_wellness {
        let wellness_lookback_days = request
            .period_days
            .max(super::PERSONAL_BASELINE_WINDOW_DAYS);
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
