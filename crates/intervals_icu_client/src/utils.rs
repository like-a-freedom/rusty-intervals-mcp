//! Utility functions for date/time normalization and other common operations.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_event_start_accepts_date_only() {
        let result = normalize_event_start("2025-12-15");
        assert_eq!(result.unwrap(), "2025-12-15T00:00:00");
    }

    #[test]
    fn normalize_event_start_preserves_datetime() {
        let result = normalize_event_start("2025-12-15T10:30:00");
        assert_eq!(result.unwrap(), "2025-12-15T10:30:00");
    }

    #[test]
    fn normalize_event_start_preserves_rfc3339() {
        let result = normalize_event_start("2025-12-15T10:30:00Z");
        assert_eq!(result.unwrap(), "2025-12-15T10:30:00");
    }

    #[test]
    fn normalize_event_start_rejects_invalid() {
        let result = normalize_event_start("not-a-date");
        assert!(result.is_none());
    }
}
