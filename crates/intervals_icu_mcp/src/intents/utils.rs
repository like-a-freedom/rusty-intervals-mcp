/// Shared utilities for intent handlers
///
/// Provides common functionality used across multiple intent handlers:
/// - Date parsing and validation
/// - Activity filtering
/// - Period calculations
use chrono::{NaiveDate, NaiveDateTime};
use intervals_icu_client::{ActivitySummary, Event, IntervalsClient};
use serde_json::Value;

use crate::intents::{ContentBlock, IntentError};

/// Parse and validate a date string in YYYY-MM-DD format
pub fn parse_date(date_str: &str, field_name: &str) -> Result<NaiveDate, IntentError> {
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").map_err(|_| {
        IntentError::validation(format!(
            "Invalid date format for {}: '{}'. Use YYYY-MM-DD.",
            field_name, date_str
        ))
    })
}

/// Parse optional date string
pub fn parse_optional_date(
    date_str: Option<&str>,
    field_name: &str,
) -> Result<Option<NaiveDate>, IntentError> {
    match date_str {
        Some(s) => Ok(Some(parse_date(s, field_name)?)),
        None => Ok(None),
    }
}

/// Filter activities by date
/// API returns start_date_local in format "YYYY-MM-DDTHH:MM:SS"
pub fn filter_activities_by_date<'a>(
    activities: &'a [ActivitySummary],
    target_date: &NaiveDate,
) -> Vec<&'a ActivitySummary> {
    activities
        .iter()
        .filter(|a| {
            parse_activity_date(&a.start_date_local)
                .map(|date| date == *target_date)
                .unwrap_or(false)
        })
        .collect()
}

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
                .map(|d| d >= *start && d <= *end)
                .unwrap_or(false)
        })
        .collect()
}

fn parse_activity_date(value: &str) -> Option<NaiveDate> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|dt| dt.date())
        .or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
}

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
                .map(|n| n.to_lowercase().contains(&search_term))
                .unwrap_or(false)
        })
        .collect()
}

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
                .map(|d| d == *target_date)
                .unwrap_or(false)
        })
        .collect()
}

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
                .map(|d| d >= *start && d <= *end)
                .unwrap_or(false)
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
        return Some(ContentBlock::markdown(format!(
            "### Data Availability\n\n- {}",
            degraded_mode_reasons.join("\n- ")
        )));
    }

    if all_available {
        return Some(ContentBlock::markdown(
            "### Data Availability\n\n✅ All data sources available".to_string(),
        ));
    }

    None
}

// ============================================================================
// Date/Time Transformations (from transforms.rs for backward compatibility)
// ============================================================================

/// Normalize a date string to YYYY-MM-DD format.
///
/// Accepts:
/// - YYYY-MM-DD (returns as-is)
/// - RFC3339 datetime (extracts date)
/// - Naive datetime YYYY-MM-DDTHH:MM:SS (extracts date)
pub fn normalize_date_str(s: &str) -> Option<String> {
    if NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
        return Some(s.to_string());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.date_naive().format("%Y-%m-%d").to_string());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(ndt.date().format("%Y-%m-%d").to_string());
    }
    None
}

/// Normalize start_date_local for events: preserve time when provided;
/// if only date is given, set time to 00:00:00.
///
/// Accepts:
/// - YYYY-MM-DD -> YYYY-MM-DDT00:00:00
/// - RFC3339 datetime -> YYYY-MM-DDTHH:MM:SS
/// - Naive datetime YYYY-MM-DDTHH:MM:SS -> YYYY-MM-DDTHH:MM:SS
pub fn normalize_event_start(s: &str) -> Option<String> {
    if NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
        return Some(format!("{}T00:00:00", s));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_local().format("%Y-%m-%dT%H:%M:%S").to_string());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(ndt.format("%Y-%m-%dT%H:%M:%S").to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use serde_json::json;

    #[test]
    fn test_parse_date_valid() {
        let date = parse_date("2026-03-01", "test").unwrap();
        assert_eq!(date.day(), 1);
        assert_eq!(date.month(), 3);
        assert_eq!(date.year(), 2026);
    }

    #[test]
    fn test_parse_date_invalid() {
        let result = parse_date("invalid", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_date_range_valid() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        assert!(validate_date_range(&start, &end, 365).is_ok());
    }

    #[test]
    fn test_validate_date_range_invalid() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        assert!(validate_date_range(&start, &end, 365).is_err());
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration_minutes(90), "1:30");
        assert_eq!(format_duration_minutes(60), "1:00");
        assert_eq!(format_duration_minutes(150), "2:30");
    }

    #[test]
    fn test_calculate_percent_change() {
        assert_eq!(calculate_percent_change(100.0, 110.0), 10.0);
        assert_eq!(calculate_percent_change(100.0, 90.0), -10.0);
        assert_eq!(calculate_percent_change(0.0, 100.0), 0.0);
    }

    #[test]
    fn test_format_delta() {
        assert_eq!(format_delta(5.0, "%"), "+5%");
        assert_eq!(format_delta(-5.0, "%"), "-5%");
        assert_eq!(format_delta(0.0, "%"), "+0%");
    }

    #[test]
    fn test_extract_idempotency_token() {
        let input = json!({"idempotency_token": "test-token-123"});
        let token = extract_idempotency_token(&input);
        assert_eq!(token, Some("test-token-123".to_string()));

        let input_no_token = json!({"other_field": "value"});
        let token = extract_idempotency_token(&input_no_token);
        assert_eq!(token, None);
    }

    #[test]
    fn test_validate_idempotency_token() {
        let input = json!({"idempotency_token": "test-token"});
        let result = validate_idempotency_token(&input);
        assert!(result.is_ok());

        let input_no_token = json!({"other_field": "value"});
        let result = validate_idempotency_token(&input_no_token);
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_date_str() {
        // YYYY-MM-DD format
        assert_eq!(
            normalize_date_str("2026-03-04"),
            Some("2026-03-04".to_string())
        );

        // RFC3339 datetime
        assert_eq!(
            normalize_date_str("2026-03-04T09:00:00Z"),
            Some("2026-03-04".to_string())
        );

        // Naive datetime
        assert_eq!(
            normalize_date_str("2026-03-04T09:00:00"),
            Some("2026-03-04".to_string())
        );

        // Invalid format
        assert_eq!(normalize_date_str("invalid"), None);
    }

    #[test]
    fn test_normalize_event_start() {
        // YYYY-MM-DD -> YYYY-MM-DDT00:00:00
        assert_eq!(
            normalize_event_start("2026-03-04"),
            Some("2026-03-04T00:00:00".to_string())
        );

        // RFC3339 datetime
        let result = normalize_event_start("2026-03-04T09:00:00Z");
        assert!(result.is_some());

        // Naive datetime
        assert_eq!(
            normalize_event_start("2026-03-04T09:00:00"),
            Some("2026-03-04T09:00:00".to_string())
        );
    }

    #[test]
    fn test_calculate_weekly_average() {
        let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();

        let weekly = calculate_weekly_average(28.0, &start, &end);
        assert!((weekly - 14.0).abs() < 0.1);

        // Single day should return full value
        let weekly = calculate_weekly_average(10.0, &start, &start);
        assert!((weekly - 10.0).abs() < 0.1);
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration_seconds(3661), "1:01");
        assert_eq!(format_duration_seconds(7200), "2:00");
        assert_eq!(format_duration_seconds(90), "0:01");
    }

    #[test]
    fn test_parse_optional_date() {
        let result = parse_optional_date(Some("2026-03-04"), "test");
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());

        let result = parse_optional_date(None, "test");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_data_availability_block_for_degraded_mode() {
        let block = data_availability_block(&["stream data unavailable".to_string()], false)
            .expect("degraded mode should render a block");

        match block {
            crate::intents::ContentBlock::Markdown { markdown } => {
                assert!(markdown.contains("Data Availability"));
                assert!(markdown.contains("stream data unavailable"));
            }
            other => panic!("unexpected block: {:?}", other),
        }
    }

    #[test]
    fn test_data_availability_block_for_all_sources_available() {
        let block =
            data_availability_block(&[], true).expect("all-available state should render a block");

        match block {
            crate::intents::ContentBlock::Markdown { markdown } => {
                assert!(markdown.contains("All data sources available"));
            }
            other => panic!("unexpected block: {:?}", other),
        }
    }
}
