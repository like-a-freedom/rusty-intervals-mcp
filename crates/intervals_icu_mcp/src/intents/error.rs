use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorGuidance {
    pub cause: String,
    pub correction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_intent: Option<String>,
}

impl ErrorGuidance {
    pub fn new(cause: String, correction: String, suggested_intent: Option<String>) -> Self {
        Self {
            cause,
            correction,
            suggested_intent,
        }
    }
    pub fn unknown_intent(name: &str) -> Self {
        Self::new(
            format!("Unknown intent '{}'", name),
            "Check the intent name.".to_string(),
            Some("Use list_tools to see available intents.".to_string()),
        )
    }
    pub fn validation_error(field: &str, expected: &str, actual: &str) -> Self {
        Self::new(
            format!("Invalid value for '{}': {}", field, actual),
            format!("Expected: {}", expected),
            None,
        )
    }
    pub fn missing_field(field: &str) -> Self {
        Self::new(
            format!("Missing required field: {}", field),
            format!("Please provide '{}'", field),
            None,
        )
    }
    pub fn not_found(resource: &str, id: &str) -> Self {
        Self::new(
            format!("{} not found: {}", resource, id),
            "Try a different identifier.".to_string(),
            Some("analyze_training".to_string()),
        )
    }
    pub fn empty_period(label: &str) -> Self {
        Self::new(
            format!("Period '{}' contains no activities", label),
            "Check the date range.".to_string(),
            Some("analyze_training".to_string()),
        )
    }
    pub fn idempotency_conflict(token: &str) -> Self {
        Self::new(
            format!("Token already used: {}", token),
            "Returning cached result.".to_string(),
            None,
        )
    }
    pub fn api_error(op: &str, details: &str) -> Self {
        Self::new(
            format!("API operation failed: {}", op),
            format!("Details: {}. Check connection.", details),
            None,
        )
    }
    pub fn destructive_requires_dry_run() -> Self {
        Self::new(
            "Delete requires dry_run confirmation".to_string(),
            "Call with dry_run=true first.".to_string(),
            None,
        )
    }
    pub fn nested_dict_not_allowed(field: &str) -> Self {
        Self::new(
            format!("Nested structure not allowed: {}", field),
            "Use flat parameters only.".to_string(),
            None,
        )
    }
    pub fn invalid_date(value: &str) -> Self {
        Self::new(
            format!("Invalid date: {}", value),
            "Use YYYY-MM-DD format.".to_string(),
            None,
        )
    }
    pub fn invalid_date_range(start: &str, end: &str) -> Self {
        Self::new(
            format!("Invalid range: {} to {}", start, end),
            "Ensure start < end.".to_string(),
            None,
        )
    }
    pub fn multiple_found(criteria: &str) -> Self {
        Self::new(
            format!("Multiple results for: {}", criteria),
            "Refine search criteria.".to_string(),
            None,
        )
    }
    pub fn rate_limit(retry_secs: u64) -> Self {
        Self::new(
            "Rate limit exceeded".to_string(),
            format!("Wait {} seconds", retry_secs),
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_guidance_new() {
        let g = ErrorGuidance::new("Cause".into(), "Fix".into(), Some("intent".into()));
        assert_eq!(g.cause, "Cause");
        assert_eq!(g.correction, "Fix");
        assert_eq!(g.suggested_intent, Some("intent".to_string()));
    }

    #[test]
    fn test_error_guidance_new_without_suggested_intent() {
        let g = ErrorGuidance::new("Cause".into(), "Fix".into(), None);
        assert_eq!(g.cause, "Cause");
        assert_eq!(g.correction, "Fix");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_unknown_intent() {
        let g = ErrorGuidance::unknown_intent("my_intent");
        assert_eq!(g.cause, "Unknown intent 'my_intent'");
        assert_eq!(g.correction, "Check the intent name.");
        assert_eq!(
            g.suggested_intent,
            Some("Use list_tools to see available intents.".to_string())
        );
    }

    #[test]
    fn test_error_guidance_validation_error() {
        let g = ErrorGuidance::validation_error("action", "get or update", "delete");
        assert_eq!(g.cause, "Invalid value for 'action': delete");
        assert_eq!(g.correction, "Expected: get or update");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_missing_field() {
        let g = ErrorGuidance::missing_field("target_date");
        assert_eq!(g.cause, "Missing required field: target_date");
        assert_eq!(g.correction, "Please provide 'target_date'");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_not_found() {
        let g = ErrorGuidance::not_found("Activity", "12345");
        assert_eq!(g.cause, "Activity not found: 12345");
        assert_eq!(g.correction, "Try a different identifier.");
        assert_eq!(g.suggested_intent, Some("analyze_training".to_string()));
    }

    #[test]
    fn test_error_guidance_empty_period() {
        let g = ErrorGuidance::empty_period("last_week");
        assert_eq!(g.cause, "Period 'last_week' contains no activities");
        assert_eq!(g.correction, "Check the date range.");
        assert_eq!(g.suggested_intent, Some("analyze_training".to_string()));
    }

    #[test]
    fn test_error_guidance_idempotency_conflict() {
        let g = ErrorGuidance::idempotency_conflict("token-abc-123");
        assert_eq!(g.cause, "Token already used: token-abc-123");
        assert_eq!(g.correction, "Returning cached result.");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_api_error() {
        let g = ErrorGuidance::api_error("get_athlete_profile", "Connection timeout");
        assert_eq!(g.cause, "API operation failed: get_athlete_profile");
        assert_eq!(
            g.correction,
            "Details: Connection timeout. Check connection."
        );
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_destructive_requires_dry_run() {
        let g = ErrorGuidance::destructive_requires_dry_run();
        assert_eq!(g.cause, "Delete requires dry_run confirmation");
        assert_eq!(g.correction, "Call with dry_run=true first.");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_nested_dict_not_allowed() {
        let g = ErrorGuidance::nested_dict_not_allowed("metadata.tags");
        assert_eq!(g.cause, "Nested structure not allowed: metadata.tags");
        assert_eq!(g.correction, "Use flat parameters only.");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_invalid_date() {
        let g = ErrorGuidance::invalid_date("2026/03/21");
        assert_eq!(g.cause, "Invalid date: 2026/03/21");
        assert_eq!(g.correction, "Use YYYY-MM-DD format.");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_invalid_date_range() {
        let g = ErrorGuidance::invalid_date_range("2026-03-21", "2026-03-20");
        assert_eq!(g.cause, "Invalid range: 2026-03-21 to 2026-03-20");
        assert_eq!(g.correction, "Ensure start < end.");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_multiple_found() {
        let g = ErrorGuidance::multiple_found("name=Test");
        assert_eq!(g.cause, "Multiple results for: name=Test");
        assert_eq!(g.correction, "Refine search criteria.");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_rate_limit() {
        let g = ErrorGuidance::rate_limit(60);
        assert_eq!(g.cause, "Rate limit exceeded");
        assert_eq!(g.correction, "Wait 60 seconds");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_rate_limit_different_duration() {
        let g = ErrorGuidance::rate_limit(300);
        assert_eq!(g.cause, "Rate limit exceeded");
        assert_eq!(g.correction, "Wait 300 seconds");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_serialize_without_suggested_intent() {
        let g = ErrorGuidance::new("Cause".into(), "Fix".into(), None);
        let serialized = serde_json::to_string(&g).unwrap();
        assert!(!serialized.contains("suggested_intent"));
    }

    #[test]
    fn test_error_guidance_serialize_with_suggested_intent() {
        let g = ErrorGuidance::new("Cause".into(), "Fix".into(), Some("intent".into()));
        let serialized = serde_json::to_string(&g).unwrap();
        assert!(serialized.contains("suggested_intent"));
        assert!(serialized.contains("intent"));
    }

    #[test]
    fn test_error_guidance_deserialize_without_suggested_intent() {
        let json = r#"{"cause":"Cause","correction":"Fix"}"#;
        let g: ErrorGuidance = serde_json::from_str(json).unwrap();
        assert_eq!(g.cause, "Cause");
        assert_eq!(g.correction, "Fix");
        assert!(g.suggested_intent.is_none());
    }

    #[test]
    fn test_error_guidance_deserialize_with_suggested_intent() {
        let json = r#"{"cause":"Cause","correction":"Fix","suggested_intent":"test"}"#;
        let g: ErrorGuidance = serde_json::from_str(json).unwrap();
        assert_eq!(g.cause, "Cause");
        assert_eq!(g.correction, "Fix");
        assert_eq!(g.suggested_intent, Some("test".to_string()));
    }

    #[test]
    fn test_error_guidance_debug_format() {
        let g = ErrorGuidance::new("Cause".into(), "Fix".into(), Some("intent".into()));
        let debug_str = format!("{:?}", g);
        assert!(debug_str.contains("ErrorGuidance"));
        assert!(debug_str.contains("Cause"));
        assert!(debug_str.contains("Fix"));
    }

    #[test]
    fn test_error_guidance_clone() {
        let g1 = ErrorGuidance::new("Cause".into(), "Fix".into(), Some("intent".into()));
        let g2 = g1.clone();
        assert_eq!(g1.cause, g2.cause);
        assert_eq!(g1.correction, g2.correction);
        assert_eq!(g1.suggested_intent, g2.suggested_intent);
    }
}
