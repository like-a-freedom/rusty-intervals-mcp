/// Normalize a date string to YYYY-MM-DD format.
///
/// Accepts:
/// - YYYY-MM-DD (returns as-is)
/// - RFC3339 datetime (extracts date)
/// - Naive datetime YYYY-MM-DDTHH:MM:SS (extracts date)
pub fn normalize_date_str(s: &str) -> Option<String> {
    if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
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
    if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
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
