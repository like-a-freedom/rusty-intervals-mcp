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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use chrono::{Duration, Local, NaiveDate};
    use intervals_icu_client::{ActivitySummary, Event, EventCategory};
    use serde_json::json;

    #[test]
    fn test_resolve_relative_day_today() {
        assert_eq!(
            resolve_relative_day_alias("today"),
            Some(Local::now().date_naive())
        );
    }

    #[test]
    fn test_resolve_relative_day_tomorrow() {
        assert_eq!(
            resolve_relative_day_alias("tomorrow"),
            Some(Local::now().date_naive() + Duration::days(1))
        );
    }

    #[test]
    fn test_resolve_relative_day_yesterday() {
        assert_eq!(
            resolve_relative_day_alias("yesterday"),
            Some(Local::now().date_naive() - Duration::days(1))
        );
    }

    #[test]
    fn test_resolve_relative_day_invalid() {
        assert_eq!(resolve_relative_day_alias("invalid"), None);
    }

    #[test]
    fn test_resolve_relative_day_case_insensitive() {
        assert_eq!(
            resolve_relative_day_alias("TODAY"),
            Some(Local::now().date_naive())
        );
        assert_eq!(
            resolve_relative_day_alias("Tomorrow"),
            Some(Local::now().date_naive() + Duration::days(1))
        );
        assert_eq!(
            resolve_relative_day_alias("YESTERDAY"),
            Some(Local::now().date_naive() - Duration::days(1))
        );
    }

    #[test]
    fn test_parse_date_valid_yyyy_mm_dd() {
        let result = parse_date("2026-03-21", "test_date").unwrap();
        assert_eq!(result, NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
    }

    #[test]
    fn test_parse_date_relative() {
        assert_eq!(parse_date("today", "d").unwrap(), Local::now().date_naive());
    }

    #[test]
    fn test_parse_date_invalid_format() {
        let err = parse_date("21-03-2026", "my_field").unwrap_err();
        assert!(err.to_string().contains("my_field"));
        assert!(err.to_string().contains("21-03-2026"));
    }

    #[test]
    fn test_parse_date_garbage() {
        let err = parse_date("not-a-date", "x").unwrap_err();
        assert!(matches!(err, IntentError::ValidationError(_)));
    }

    #[test]
    fn test_parse_optional_date_some() {
        let result = parse_optional_date(Some("2026-03-21"), "d").unwrap();
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2026, 3, 21).unwrap()));
    }

    #[test]
    fn test_parse_optional_date_none() {
        assert_eq!(parse_optional_date(None, "d").unwrap(), None);
    }

    #[test]
    fn test_parse_optional_date_invalid() {
        let err = parse_optional_date(Some("bad"), "x").unwrap_err();
        assert!(matches!(err, IntentError::ValidationError(_)));
    }

    fn act(id: &str, date: &str) -> ActivitySummary {
        ActivitySummary {
            id: id.into(),
            start_date_local: date.into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_filter_activities_by_date_matches() {
        let activities = vec![
            act("1", "2026-03-21T10:00:00"),
            act("2", "2026-03-22T10:00:00"),
        ];
        let target = NaiveDate::from_ymd_opt(2026, 3, 21).unwrap();
        let result = filter_activities_by_date(&activities, &target);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "1");
    }

    #[test]
    fn test_filter_activities_by_date_no_match() {
        let activities = vec![act("1", "2026-03-22T10:00:00")];
        let result =
            filter_activities_by_date(&activities, &NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_activities_by_date_empty() {
        let result = filter_activities_by_date(&[], &NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_activities_by_date_date_only_format() {
        let activities = vec![act("1", "2026-03-21")];
        let result =
            filter_activities_by_date(&activities, &NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_filter_activities_by_range_within() {
        let activities = vec![
            act("1", "2026-03-21T10:00:00"),
            act("2", "2026-03-23T10:00:00"),
            act("3", "2026-03-25T10:00:00"),
        ];
        let start = NaiveDate::from_ymd_opt(2026, 3, 22).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 24).unwrap();
        let result = filter_activities_by_range(&activities, &start, &end);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "2");
    }

    #[test]
    fn test_filter_activities_by_range_all_outside() {
        let activities = vec![act("1", "2026-03-21T10:00:00")];
        let start = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 4, 30).unwrap();
        let result = filter_activities_by_range(&activities, &start, &end);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_activities_by_range_empty() {
        let result = filter_activities_by_range(
            &[],
            &NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            &NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_activity_date_datetime() {
        assert_eq!(
            parse_activity_date("2026-03-21T10:30:00"),
            Some(NaiveDate::from_ymd_opt(2026, 3, 21).unwrap())
        );
    }

    #[test]
    fn test_parse_activity_date_date_only() {
        assert_eq!(
            parse_activity_date("2026-03-21"),
            Some(NaiveDate::from_ymd_opt(2026, 3, 21).unwrap())
        );
    }

    #[test]
    fn test_parse_activity_date_invalid() {
        assert_eq!(parse_activity_date("garbage"), None);
    }

    #[test]
    fn test_normalize_date_str_yyyy_mm_dd() {
        assert_eq!(normalize_date_str("2026-03-21"), Some("2026-03-21".into()));
    }

    #[test]
    fn test_normalize_date_str_datetime_format() {
        assert_eq!(
            normalize_date_str("2026-03-21T10:30:00"),
            Some("2026-03-21".into())
        );
    }

    #[test]
    fn test_normalize_date_str_rfc3339() {
        assert_eq!(
            normalize_date_str("2026-03-21T10:30:00Z"),
            Some("2026-03-21".into())
        );
    }

    #[test]
    fn test_normalize_date_str_invalid() {
        assert_eq!(normalize_date_str("garbage"), None);
    }

    #[test]
    fn test_normalize_date_str_empty() {
        assert_eq!(normalize_date_str(""), None);
    }

    #[test]
    fn test_normalize_event_start_yyyy_mm_dd() {
        assert_eq!(
            normalize_event_start("2026-03-21"),
            Some("2026-03-21T00:00:00".into())
        );
    }

    #[test]
    fn test_normalize_event_start_datetime() {
        assert_eq!(
            normalize_event_start("2026-03-21T10:30:00"),
            Some("2026-03-21T10:30:00".into())
        );
    }

    #[test]
    fn test_normalize_event_start_rfc3339_utc() {
        assert_eq!(
            normalize_event_start("2026-03-21T10:30:00Z"),
            Some("2026-03-21T10:30:00".into())
        );
    }

    #[test]
    fn test_normalize_event_start_rfc3339_offset() {
        assert_eq!(
            normalize_event_start("2026-03-21T10:30:00+05:00"),
            Some("2026-03-21T10:30:00".into())
        );
    }

    #[test]
    fn test_normalize_event_start_invalid() {
        assert_eq!(normalize_event_start("garbage"), None);
    }

    fn act_with_name(id: &str, name: Option<&str>) -> ActivitySummary {
        ActivitySummary {
            id: id.into(),
            name: name.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_filter_activities_by_description_matches() {
        let activities = vec![
            act_with_name("1", Some("Morning Run")),
            act_with_name("2", Some("Evening Ride")),
        ];
        let result = filter_activities_by_description(&activities, "run");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "1");
    }

    #[test]
    fn test_filter_activities_by_description_case_insensitive() {
        let activities = vec![act_with_name("1", Some("MORNING RUN"))];
        assert_eq!(
            filter_activities_by_description(&activities, "morning").len(),
            1
        );
    }

    #[test]
    fn test_filter_activities_by_description_no_match() {
        let activities = vec![act_with_name("1", Some("Swim Session"))];
        assert!(filter_activities_by_description(&activities, "run").is_empty());
    }

    #[test]
    fn test_filter_activities_by_description_none_name() {
        let activities = vec![act_with_name("1", None)];
        assert!(filter_activities_by_description(&activities, "anything").is_empty());
    }

    #[test]
    fn test_filter_activities_by_description_empty_search() {
        let activities = vec![
            act_with_name("1", Some("Run")),
            act_with_name("2", Some("Ride")),
        ];
        assert_eq!(filter_activities_by_description(&activities, "").len(), 2);
    }

    #[test]
    fn test_filter_activities_by_description_empty_list() {
        assert!(filter_activities_by_description(&[], "run").is_empty());
    }

    fn evt(id: &str, date: &str) -> Event {
        Event {
            id: Some(id.into()),
            start_date_local: date.into(),
            name: "Test Event".into(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        }
    }

    #[test]
    fn test_filter_events_by_date_matches_date_only() {
        let events = vec![evt("1", "2026-03-21"), evt("2", "2026-03-22")];
        let target = NaiveDate::from_ymd_opt(2026, 3, 21).unwrap();
        let result = filter_events_by_date(&events, &target);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, Some("1".into()));
    }

    #[test]
    fn test_filter_events_by_date_matches_datetime() {
        let events = vec![evt("1", "2026-03-21T10:00:00")];
        let result = filter_events_by_date(&events, &NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_filter_events_by_date_no_match() {
        let events = vec![evt("1", "2026-03-22")];
        let result = filter_events_by_date(&events, &NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_events_by_date_empty() {
        let result = filter_events_by_date(&[], &NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_events_by_date_invalid_date_string() {
        let events = vec![evt("1", "not-a-date")];
        let result = filter_events_by_date(&events, &NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_events_by_range_within() {
        let events = vec![
            evt("1", "2026-03-21"),
            evt("2", "2026-03-25"),
            evt("3", "2026-03-30"),
        ];
        let start = NaiveDate::from_ymd_opt(2026, 3, 22).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 28).unwrap();
        let result = filter_events_by_range(&events, &start, &end);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, Some("2".into()));
    }

    #[test]
    fn test_filter_events_by_range_all_outside() {
        let events = vec![evt("1", "2026-03-21")];
        let start = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 4, 30).unwrap();
        assert!(filter_events_by_range(&events, &start, &end).is_empty());
    }

    #[test]
    fn test_filter_events_by_range_empty() {
        let result = filter_events_by_range(
            &[],
            &NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            &NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_validate_date_range_valid() {
        let s = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        assert!(validate_date_range(&s, &e, 30).is_ok());
    }

    #[test]
    fn test_validate_date_range_same_day() {
        let d = NaiveDate::from_ymd_opt(2026, 3, 21).unwrap();
        assert!(validate_date_range(&d, &d, 30).is_ok());
    }

    #[test]
    fn test_validate_date_range_start_after_end() {
        let s = NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        let err = validate_date_range(&s, &e, 30).unwrap_err();
        assert!(err.to_string().contains("must be before"));
    }

    #[test]
    fn test_validate_date_range_exceeds_max_days() {
        let s = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        let err = validate_date_range(&s, &e, 30).unwrap_err();
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn test_validate_date_range_exact_max() {
        let s = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        assert!(validate_date_range(&s, &e, 30).is_ok());
    }

    #[test]
    fn test_extract_idempotency_token_present() {
        assert_eq!(
            extract_idempotency_token(&json!({"idempotency_token": "abc-123"})),
            Some("abc-123".into())
        );
    }

    #[test]
    fn test_extract_idempotency_token_missing_key() {
        assert_eq!(extract_idempotency_token(&json!({"other": "value"})), None);
    }

    #[test]
    fn test_extract_idempotency_token_not_string() {
        assert_eq!(
            extract_idempotency_token(&json!({"idempotency_token": 42})),
            None
        );
    }

    #[test]
    fn test_extract_idempotency_token_null_value() {
        assert_eq!(
            extract_idempotency_token(&json!({"idempotency_token": null})),
            None
        );
    }

    #[test]
    fn test_extract_idempotency_token_empty_object() {
        assert_eq!(extract_idempotency_token(&json!({})), None);
    }

    #[test]
    fn test_extract_idempotency_token_empty_string() {
        assert_eq!(
            extract_idempotency_token(&json!({"idempotency_token": ""})),
            Some("".into())
        );
    }

    #[test]
    fn test_validate_idempotency_token_ok() {
        assert_eq!(
            validate_idempotency_token(&json!({"idempotency_token": "tok-1"})).unwrap(),
            "tok-1"
        );
    }

    #[test]
    fn test_validate_idempotency_token_missing() {
        let err = validate_idempotency_token(&json!({})).unwrap_err();
        assert!(matches!(err, IntentError::ValidationError(_)));
    }

    #[test]
    fn test_validate_idempotency_token_not_string() {
        let err = validate_idempotency_token(&json!({"idempotency_token": false})).unwrap_err();
        assert!(matches!(err, IntentError::ValidationError(_)));
    }

    #[test]
    fn test_calculate_weekly_average_normal() {
        let s = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        assert!((calculate_weekly_average(100.0, &s, &e) - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_weekly_average_less_than_one_week() {
        let s = NaiveDate::from_ymd_opt(2026, 3, 21).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 3, 23).unwrap();
        assert!((calculate_weekly_average(50.0, &s, &e) - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_weekly_average_zero_total() {
        let s = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 1, 8).unwrap();
        assert!((calculate_weekly_average(0.0, &s, &e) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_weekly_average_exact_one_week() {
        let s = NaiveDate::from_ymd_opt(2026, 3, 21).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 3, 28).unwrap();
        assert!((calculate_weekly_average(70.0, &s, &e) - 70.0).abs() < 0.01);
    }

    #[test]
    fn test_format_duration_minutes_zero() {
        assert_eq!(format_duration_minutes(0), "0:00");
    }

    #[test]
    fn test_format_duration_minutes_under_hour() {
        assert_eq!(format_duration_minutes(45), "0:45");
    }

    #[test]
    fn test_format_duration_minutes_exact_hour() {
        assert_eq!(format_duration_minutes(60), "1:00");
    }

    #[test]
    fn test_format_duration_minutes_hour_half() {
        assert_eq!(format_duration_minutes(90), "1:30");
    }

    #[test]
    fn test_format_duration_minutes_multi_hour() {
        assert_eq!(format_duration_minutes(150), "2:30");
    }

    #[test]
    fn test_format_duration_minutes_single_digit_minutes() {
        assert_eq!(format_duration_minutes(61), "1:01");
    }

    #[test]
    fn test_format_duration_seconds_zero() {
        assert_eq!(format_duration_seconds(0), "0:00");
    }

    #[test]
    fn test_format_duration_seconds_under_hour() {
        assert_eq!(format_duration_seconds(1800), "0:30");
    }

    #[test]
    fn test_format_duration_seconds_exact_hour() {
        assert_eq!(format_duration_seconds(3600), "1:00");
    }

    #[test]
    fn test_format_duration_seconds_hour_plus() {
        assert_eq!(format_duration_seconds(3660), "1:01");
    }

    #[test]
    fn test_format_duration_seconds_large() {
        assert_eq!(format_duration_seconds(7500), "2:05");
    }

    #[test]
    fn test_calculate_percent_change_old_zero() {
        assert!((calculate_percent_change(0.0, 100.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_percent_change_increase() {
        assert!((calculate_percent_change(50.0, 75.0) - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_percent_change_decrease() {
        assert!((calculate_percent_change(100.0, 75.0) + 25.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_percent_change_no_change() {
        assert!((calculate_percent_change(50.0, 50.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_percent_change_both_zero() {
        assert!((calculate_percent_change(0.0, 0.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_format_delta_positive() {
        assert_eq!(format_delta(10.5, "%"), "+10.5%");
    }

    #[test]
    fn test_format_delta_negative() {
        assert_eq!(format_delta(-5.0, "%"), "-5%");
    }

    #[test]
    fn test_format_delta_zero() {
        assert_eq!(format_delta(0.0, " hrs"), "+0 hrs");
    }

    #[test]
    fn test_format_delta_positive_int() {
        assert_eq!(format_delta(42.0, "km"), "+42km");
    }

    #[test]
    fn test_format_delta_negative_int() {
        assert_eq!(format_delta(-8.0, " pts"), "-8 pts");
    }

    #[test]
    fn test_data_availability_block_with_reasons() {
        let reasons = vec!["No HR data".into(), "No power".into()];
        let block = data_availability_block(&reasons, true).unwrap();
        assert_eq!(
            block,
            ContentBlock::Markdown {
                markdown: "Data availability\n  No HR data\n  No power".into()
            }
        );
    }

    #[test]
    fn test_data_availability_block_all_available() {
        let block = data_availability_block(&[], true).unwrap();
        assert_eq!(
            block,
            ContentBlock::Markdown {
                markdown: "Data availability: all sources available".into()
            }
        );
    }

    #[test]
    fn test_data_availability_block_not_available_no_reasons() {
        assert!(data_availability_block(&[], false).is_none());
    }

    #[test]
    fn test_data_availability_block_reasons_not_all_available() {
        let reasons = vec!["Missing data".into()];
        let block = data_availability_block(&reasons, false).unwrap();
        assert_eq!(
            block,
            ContentBlock::Markdown {
                markdown: "Data availability\n  Missing data".into()
            }
        );
    }

    #[tokio::test]
    async fn test_fetch_activities_for_date_found() {
        let client = MockIntervalsClient::builder().with_activities(vec![
            act("1", "2026-03-21T10:00:00"),
            act("2", "2026-03-22T10:00:00"),
        ]);
        let date = NaiveDate::from_ymd_opt(2026, 3, 21).unwrap();
        let result = fetch_activities_for_date(&client, &date, 10, 7)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "1");
    }

    #[tokio::test]
    async fn test_fetch_activities_for_date_not_found() {
        let client =
            MockIntervalsClient::builder().with_activities(vec![act("1", "2026-03-22T10:00:00")]);
        let date = NaiveDate::from_ymd_opt(2026, 3, 21).unwrap();
        let result = fetch_activities_for_date(&client, &date, 10, 7)
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_activities_for_range_found() {
        let client = MockIntervalsClient::builder().with_activities(vec![
            act("1", "2026-03-21T10:00:00"),
            act("2", "2026-04-01T10:00:00"),
        ]);
        let s = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        let result = fetch_activities_for_range(&client, &s, &e, 10)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "1");
    }

    #[tokio::test]
    async fn test_fetch_activities_for_range_not_found() {
        let client =
            MockIntervalsClient::builder().with_activities(vec![act("1", "2026-04-01T10:00:00")]);
        let s = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        let result = fetch_activities_for_range(&client, &s, &e, 10)
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_activities_for_range_empty_client() {
        let client = MockIntervalsClient::default();
        let s = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        let result = fetch_activities_for_range(&client, &s, &e, 10)
            .await
            .unwrap();
        assert!(result.is_empty());
    }
}
