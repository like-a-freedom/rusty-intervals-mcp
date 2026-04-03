/// Shared utilities for intent handlers
///
/// Provides common functionality used across multiple intent handlers:
/// - Date parsing and validation
/// - Activity filtering
/// - Period calculations
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime};
use intervals_icu_client::{ActivitySummary, Event, IntervalsClient};
use serde_json::Value;

use crate::intents::{ContentBlock, IntentError};

fn resolve_relative_day_alias(date_str: &str) -> Option<NaiveDate> {
    let today = Local::now().date_naive();

    match date_str.to_ascii_lowercase().as_str() {
        "today" => Some(today),
        "tomorrow" => Some(today + Duration::days(1)),
        "yesterday" => Some(today - Duration::days(1)),
        _ => None,
    }
}

/// Parse and validate an ordinary date string.
///
/// Accepts:
/// - YYYY-MM-DD
/// - today
/// - tomorrow
/// - yesterday
///
/// # Errors
/// Returns [`IntentError`] if the input is not a valid date string.
pub fn parse_date(date_str: &str, field_name: &str) -> Result<NaiveDate, IntentError> {
    if let Some(relative_date) = resolve_relative_day_alias(date_str) {
        return Ok(relative_date);
    }

    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").map_err(|_| {
        IntentError::validation(format!(
            "Invalid date format for {}: '{}'. Use YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday'.",
            field_name, date_str
        ))
    })
}

/// Parse optional date string
///
/// # Errors
/// Returns [`IntentError`] if the input is Some and not a valid date string.
pub fn parse_optional_date(
    date_str: Option<&str>,
    field_name: &str,
) -> Result<Option<NaiveDate>, IntentError> {
    match date_str {
        Some(s) => Ok(Some(parse_date(s, field_name)?)),
        None => Ok(None),
    }
}

#[must_use]
/// Filter activities by date
/// API returns start_date_local in format "YYYY-MM-DDTHH:MM:SS"
pub fn filter_activities_by_date<'a>(
    activities: &'a [ActivitySummary],
    target_date: &NaiveDate,
) -> Vec<&'a ActivitySummary> {
    activities
        .iter()
        .filter(|a| parse_activity_date(&a.start_date_local) == Some(*target_date))
        .collect()
}

#[must_use]
/// Filter activities by date range
/// API returns start_date_local in format "YYYY-MM-DDTHH:MM:SS"
pub fn filter_activities_by_range<'a>(
    activities: &'a [ActivitySummary],
    start: &NaiveDate,
    end: &NaiveDate,
) -> Vec<&'a ActivitySummary> {
    activities
        .iter()
        .filter(|a| {
            parse_activity_date(&a.start_date_local)
                .is_some_and(|date| date >= *start && date <= *end)
        })
        .collect()
}

fn parse_activity_date(value: &str) -> Option<NaiveDate> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|dt| dt.date())
        .or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
}

/// Normalize a date string to `YYYY-MM-DD`.
#[must_use]
pub fn normalize_date_str(date_str: &str) -> Option<String> {
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .ok()
        .map(|date| date.format("%Y-%m-%d").to_string())
        .or_else(|| {
            NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|date_time| date_time.date().format("%Y-%m-%d").to_string())
        })
        .or_else(|| {
            DateTime::parse_from_rfc3339(date_str)
                .ok()
                .map(|date_time| {
                    date_time
                        .naive_local()
                        .date()
                        .format("%Y-%m-%d")
                        .to_string()
                })
        })
}

/// Normalize an event start timestamp to `YYYY-MM-DDTHH:MM:SS`.
#[must_use]
pub fn normalize_event_start(date_str: &str) -> Option<String> {
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .ok()
        .map(|date| format!("{}T00:00:00", date.format("%Y-%m-%d")))
        .or_else(|| {
            NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|date_time| date_time.format("%Y-%m-%dT%H:%M:%S").to_string())
        })
        .or_else(|| {
            DateTime::parse_from_rfc3339(date_str)
                .ok()
                .map(|date_time| {
                    date_time
                        .naive_local()
                        .format("%Y-%m-%dT%H:%M:%S")
                        .to_string()
                })
        })
}

#[must_use]
/// Filter activities by description (case-insensitive)
pub fn filter_activities_by_description<'a>(
    activities: &'a [ActivitySummary],
    description: &str,
) -> Vec<&'a ActivitySummary> {
    let search_term = description.to_lowercase();
    activities
        .iter()
        .filter(|a| {
            a.name
                .as_ref()
                .is_some_and(|name| name.to_lowercase().contains(&search_term))
        })
        .collect()
}

#[must_use]
/// Filter events by date
/// Event start_date_local may be in format "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM:SS"
pub fn filter_events_by_date<'a>(events: &'a [Event], target_date: &NaiveDate) -> Vec<&'a Event> {
    events
        .iter()
        .filter(|e| {
            NaiveDateTime::parse_from_str(&e.start_date_local, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|dt| dt.date())
                .or_else(|| NaiveDate::parse_from_str(&e.start_date_local, "%Y-%m-%d").ok())
                == Some(*target_date)
        })
        .collect()
}

#[must_use]
/// Filter events by date range.
/// Event start_date_local may be in format "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM:SS"
pub fn filter_events_by_range<'a>(
    events: &'a [Event],
    start: &NaiveDate,
    end: &NaiveDate,
) -> Vec<&'a Event> {
    events
        .iter()
        .filter(|e| {
            NaiveDateTime::parse_from_str(&e.start_date_local, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|dt| dt.date())
                .or_else(|| NaiveDate::parse_from_str(&e.start_date_local, "%Y-%m-%d").ok())
                .is_some_and(|date| date >= *start && date <= *end)
        })
        .collect()
}

/// Fetch activities for a specific date
pub async fn fetch_activities_for_date(
    client: &dyn IntervalsClient,
    date: &NaiveDate,
    limit: u32,
    days_back: i32,
) -> Result<Vec<ActivitySummary>, IntentError> {
    let activities = client
        .get_recent_activities(Some(limit), Some(days_back))
        .await
        .map_err(|e| IntentError::api(format!("Failed to fetch activities: {}", e)))?;

    Ok(filter_activities_by_date(&activities, date)
        .into_iter()
        .cloned()
        .collect())
}

/// Fetch activities for a date range
pub async fn fetch_activities_for_range(
    client: &dyn IntervalsClient,
    start: &NaiveDate,
    end: &NaiveDate,
    limit: u32,
) -> Result<Vec<ActivitySummary>, IntentError> {
    let days = (*end - *start).num_days() as i32 + 30; // Buffer
    let activities = client
        .get_recent_activities(Some(limit), Some(days))
        .await
        .map_err(|e| IntentError::api(format!("Failed to fetch activities: {}", e)))?;

    Ok(filter_activities_by_range(&activities, start, end)
        .into_iter()
        .cloned()
        .collect())
}

/// Validate that a date range is valid (start <= end, reasonable range)
pub fn validate_date_range(
    start: &NaiveDate,
    end: &NaiveDate,
    max_days: i32,
) -> Result<(), IntentError> {
    if start > end {
        return Err(IntentError::validation(format!(
            "Start date ({}) must be before end date ({})",
            start, end
        )));
    }

    let days = (*end - *start).num_days();
    if days > max_days as i64 {
        return Err(IntentError::validation(format!(
            "Date range too large: {} days (max: {} days)",
            days, max_days
        )));
    }

    Ok(())
}

/// Extract idempotency token from input
pub fn extract_idempotency_token(input: &Value) -> Option<String> {
    input
        .get("idempotency_token")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// Validate idempotency token is present
pub fn validate_idempotency_token(input: &Value) -> Result<String, IntentError> {
    extract_idempotency_token(input).ok_or_else(|| {
        IntentError::validation(
            "Idempotency token is required for this operation. Please generate a deterministic token from your request parameters."
                .to_string(),
        )
    })
}

/// Calculate weekly average from total and date range
pub fn calculate_weekly_average(total: f32, start: &NaiveDate, end: &NaiveDate) -> f32 {
    let weeks = ((*end - *start).num_days() as f32 / 7.0).max(1.0);
    total / weeks
}

/// Format duration in minutes to HH:MM string
pub fn format_duration_minutes(minutes: u32) -> String {
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

/// Format duration in seconds to HH:MM string
pub fn format_duration_seconds(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 3600, (seconds % 3600) / 60)
}

/// Calculate percentage change
pub fn calculate_percent_change(old_value: f32, new_value: f32) -> f32 {
    if old_value == 0.0 {
        0.0
    } else {
        ((new_value - old_value) / old_value) * 100.0
    }
}

/// Format delta with sign
pub fn format_delta(value: f32, suffix: &str) -> String {
    if value >= 0.0 {
        format!("+{}{}", value, suffix)
    } else {
        format!("{}{}", value, suffix)
    }
}

/// Render a standard data-availability section for intent outputs.
pub fn data_availability_block(
    degraded_mode_reasons: &[String],
    all_available: bool,
) -> Option<ContentBlock> {
    if !degraded_mode_reasons.is_empty() {
        let mut text = "Data availability".to_string();
        for reason in degraded_mode_reasons {
            text.push('\n');
            text.push_str(&format!("  {reason}"));
        }
        return Some(ContentBlock::markdown(text));
    }

    if all_available {
        return Some(ContentBlock::markdown(
            "Data availability: all sources available".to_string(),
        ));
    }

    None
}

// ============================================================================
// Compact Markdown Helpers
// ============================================================================
