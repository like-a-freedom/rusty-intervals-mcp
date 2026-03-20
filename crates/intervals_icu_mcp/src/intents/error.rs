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
    fn test_error_guidance() {
        let g = ErrorGuidance::new("Cause".into(), "Fix".into(), Some("intent".into()));
        assert_eq!(g.cause, "Cause");
        assert_eq!(g.correction, "Fix");
    }
}
