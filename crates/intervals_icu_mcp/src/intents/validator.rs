use super::error::ErrorGuidance;
use super::types::IntentError;
use super::utils::{parse_date, validate_date_range as utils_validate_range};
use serde_json::Value;

pub fn validate_flat_structure(input: &Value) -> Result<(), IntentError> {
    let obj = input
        .as_object()
        .ok_or_else(|| IntentError::validation("Input must be object"))?;
    for (key, value) in obj {
        match value {
            Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null => {}
            Value::Array(arr) => {
                for (i, item) in arr.iter().enumerate() {
                    if !matches!(
                        item,
                        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
                    ) {
                        return Err(IntentError::validation(
                            ErrorGuidance::nested_dict_not_allowed(&format!("{}[{}]", key, i))
                                .cause,
                        ));
                    }
                }
            }
            Value::Object(_) => {
                return Err(IntentError::validation(
                    ErrorGuidance::nested_dict_not_allowed(key).cause,
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_date_format(date: &str) -> Result<(), IntentError> {
    if is_relative(date) {
        return Ok(());
    }
    // Use shared utility for consistent date parsing
    parse_date(date, "date").map(|_| ())
}

fn is_relative(s: &str) -> bool {
    let patterns = [
        "today",
        "tomorrow",
        "yesterday",
        "next_",
        "last_",
        "weeks",
        "days",
    ];
    let lower = s.to_lowercase();
    patterns.iter().any(|p| lower.contains(p))
}

pub fn validate_date_range(start: &str, end: &str, max_days: u32) -> Result<(), IntentError> {
    if is_relative(start) || is_relative(end) {
        return Ok(());
    }
    // Use shared utility for consistent date parsing and validation
    let start_date = parse_date(start, "start_date")?;
    let end_date = parse_date(end, "end_date")?;
    utils_validate_range(&start_date, &end_date, max_days as i32)
}

pub fn validate_required_fields(input: &Value, fields: &[&str]) -> Result<(), IntentError> {
    let obj = input
        .as_object()
        .ok_or_else(|| IntentError::validation("Input must be object"))?;
    for f in fields {
        if !obj.contains_key(*f) || obj.get(*f).is_none_or(|v| v.is_null()) {
            return Err(IntentError::validation(
                ErrorGuidance::missing_field(f).cause,
            ));
        }
    }
    Ok(())
}

pub struct Validator {
    required: Vec<String>,
    date_fields: Vec<String>,
    date_ranges: Vec<(String, String, u32)>,
    require_idempotency: bool,
    require_dry_run_delete: bool,
}

impl Validator {
    pub fn new() -> Self {
        Self {
            required: Vec::new(),
            date_fields: Vec::new(),
            date_ranges: Vec::new(),
            require_idempotency: false,
            require_dry_run_delete: false,
        }
    }
    pub fn required_fields(mut self, fields: Vec<&str>) -> Self {
        self.required = fields.into_iter().map(String::from).collect();
        self
    }
    pub fn date_field(mut self, f: &str) -> Self {
        self.date_fields.push(f.to_string());
        self
    }
    pub fn date_range(mut self, s: &str, e: &str, max: u32) -> Self {
        self.date_ranges.push((s.into(), e.into(), max));
        self
    }
    pub fn require_idempotency(mut self) -> Self {
        self.require_idempotency = true;
        self
    }
    pub fn require_dry_run_delete(mut self) -> Self {
        self.require_dry_run_delete = true;
        self
    }
    pub fn validate(&self, input: &Value) -> Result<(), IntentError> {
        validate_flat_structure(input)?;
        if !self.required.is_empty() {
            let r: Vec<&str> = self.required.iter().map(|s| s.as_str()).collect();
            validate_required_fields(input, &r)?;
        }
        for f in &self.date_fields {
            if let Some(v) = input.get(f).and_then(|x| x.as_str()) {
                validate_date_format(v)?;
            }
        }
        for (s, e, m) in &self.date_ranges {
            if let (Some(sv), Some(ev)) = (
                input.get(s).and_then(|x| x.as_str()),
                input.get(e).and_then(|x| x.as_str()),
            ) {
                validate_date_range(sv, ev, *m)?;
            }
        }
        if self.require_idempotency
            && input
                .get("idempotency_token")
                .and_then(|x| x.as_str())
                .is_none_or(|s| s.is_empty())
        {
            return Err(IntentError::validation("Idempotency token required"));
        }
        if self.require_dry_run_delete
            && input.get("action").and_then(|x| x.as_str()) == Some("delete")
            && !input
                .get("dry_run")
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        {
            return Err(IntentError::validation(
                ErrorGuidance::destructive_requires_dry_run().cause,
            ));
        }
        Ok(())
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_flat_valid() {
        assert!(validate_flat_structure(&json!({"a": 1, "b": "x"})).is_ok());
    }

    #[test]
    fn test_flat_invalid_nested_dict() {
        assert!(validate_flat_structure(&json!({"a": {"b": 1}})).is_err());
    }

    #[test]
    fn test_flat_invalid_nested_in_array() {
        assert!(validate_flat_structure(&json!({"items": [{"nested": 1}]})).is_err());
    }

    #[test]
    fn test_date_valid() {
        assert!(validate_date_format("2026-03-01").is_ok());
    }

    #[test]
    fn test_date_invalid() {
        assert!(validate_date_format("bad").is_err());
    }

    #[test]
    fn test_date_relative_patterns_allowed() {
        // Relative dates should be allowed
        assert!(validate_date_format("today").is_ok());
        assert!(validate_date_format("tomorrow").is_ok());
        assert!(validate_date_format("yesterday").is_ok());
        assert!(validate_date_format("next_monday").is_ok());
        assert!(validate_date_format("last_week").is_ok());
    }

    #[test]
    fn test_date_range_valid() {
        assert!(validate_date_range("2026-03-01", "2026-03-31", 365).is_ok());
    }

    #[test]
    fn test_date_range_invalid_order() {
        let result = validate_date_range("2026-03-31", "2026-03-01", 365);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("before"));
    }

    #[test]
    fn test_date_range_too_large() {
        // 366 days is larger than max of 365
        let result = validate_date_range("2026-01-01", "2027-01-02", 365);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("large"));
    }

    #[test]
    fn test_required_fields_present() {
        let input = json!({"field1": "value1", "field2": "value2"});
        let result = validate_required_fields(&input, &["field1", "field2"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_required_fields_missing() {
        let input = json!({"field1": "value1"});
        let result = validate_required_fields(&input, &["field1", "field2"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("field2"));
    }

    #[test]
    fn test_required_fields_null() {
        let input = json!({"field1": null});
        let result = validate_required_fields(&input, &["field1"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validator_builder_pattern() {
        let validator = Validator::new()
            .required_fields(vec!["date", "type"])
            .date_field("date")
            .date_range("start", "end", 90)
            .require_idempotency()
            .require_dry_run_delete();

        let input = json!({
            "date": "2026-03-04",
            "type": "single",
            "start": "2026-03-01",
            "end": "2026-03-07",
            "idempotency_token": "test-token",
            "action": "delete",
            "dry_run": true
        });

        let result = validator.validate(&input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validator_missing_idempotency() {
        let validator = Validator::new()
            .required_fields(vec!["date"])
            .require_idempotency();

        let input = json!({"date": "2026-03-04"});
        let result = validator.validate(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Idempotency"));
    }

    #[test]
    fn test_validator_missing_dry_run_for_delete() {
        let validator = Validator::new()
            .required_fields(vec!["action"])
            .require_dry_run_delete();

        let input = json!({"action": "delete"});
        let result = validator.validate(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("dry_run"));
    }

    #[test]
    fn test_validator_dry_run_not_required_for_modify() {
        let validator = Validator::new()
            .required_fields(vec!["action"])
            .require_dry_run_delete();

        let input = json!({"action": "modify"});
        let result = validator.validate(&input);
        assert!(result.is_ok());
    }
}
