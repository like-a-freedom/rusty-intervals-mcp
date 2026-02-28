use serde_json::{Map, Value};

// Re-export date normalization utilities from transforms
pub use crate::transforms::{normalize_date_str, normalize_event_start};

/// Default fields for compact event listings
const DEFAULT_COMPACT_FIELDS: &[&str] = &["id", "start_date_local", "name", "category", "type"];

/// Default fields for full event details
const DEFAULT_EVENT_FIELDS: &[&str] = &[
    "id",
    "name",
    "start_date_local",
    "category",
    "type",
    "description",
];

pub fn compact_events(
    events: Vec<intervals_icu_client::Event>,
    compact: bool,
    fields: Option<&[String]>,
) -> Vec<Value> {
    if !compact {
        return events
            .into_iter()
            .map(|e| {
                let serialized = serde_json::to_value(&e).unwrap_or_default();
                match (fields, serialized.as_object()) {
                    (Some(filter), Some(obj)) => {
                        let mut result = Map::new();
                        for field in filter {
                            if let Some(val) = obj.get(field) {
                                result.insert(field.clone(), val.clone());
                            }
                        }
                        Value::Object(result)
                    }
                    _ => serialized,
                }
            })
            .collect();
    }

    events
        .into_iter()
        .map(|event| crate::compact::compact_item(&event, DEFAULT_COMPACT_FIELDS, fields))
        .collect()
}

pub fn compact_single_event(
    event: &intervals_icu_client::Event,
    fields: Option<&[String]>,
) -> Value {
    crate::compact::compact_item(event, DEFAULT_EVENT_FIELDS, fields)
}

pub fn filter_event_fields(event: &intervals_icu_client::Event, fields: &[String]) -> Value {
    crate::compact::compact_item(event, &[], Some(fields))
}

pub fn compact_events_from_value(
    value: &Value,
    compact: bool,
    limit: usize,
    fields: Option<&[String]>,
) -> Value {
    if compact {
        let arr = value.as_array().cloned().unwrap_or_default();
        crate::compact::compact_array(
            &Value::Array(arr),
            DEFAULT_COMPACT_FIELDS,
            fields,
            Some(limit),
        )
    } else {
        let limited = match value {
            Value::Array(arr) => Value::Array(arr.iter().take(limit).cloned().collect()),
            _ => value.clone(),
        };

        if let Some(field_list) = fields {
            crate::compact::filter_array_fields(&limited, field_list)
        } else {
            limited
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EventValidationError {
    EmptyName,
    InvalidStartDate(String),
    UnknownCategory,
}

impl std::fmt::Display for EventValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(f, "invalid event: name is empty"),
            Self::InvalidStartDate(s) => write!(f, "invalid start_date_local: {}", s),
            Self::UnknownCategory => write!(f, "invalid category: unknown"),
        }
    }
}

impl std::error::Error for EventValidationError {}

/// Validate, normalize and apply sensible defaults to an `Event` before sending to the API.
///
/// - Ensures `name` is non-empty
/// - Normalizes `start_date_local` (accepts YYYY-MM-DD or ISO datetimes)
/// - Rejects `EventCategory::Unknown`
/// - If category == Workout and `type` is missing, default to `Run` (logged)
pub fn validate_and_prepare_event(
    mut ev: intervals_icu_client::Event,
) -> Result<intervals_icu_client::Event, EventValidationError> {
    if ev.name.trim().is_empty() {
        return Err(EventValidationError::EmptyName);
    }

    match normalize_event_start(&ev.start_date_local) {
        Some(s) => ev.start_date_local = s,
        None => return Err(EventValidationError::InvalidStartDate(ev.start_date_local)),
    }

    if ev.category == intervals_icu_client::EventCategory::Unknown {
        return Err(EventValidationError::UnknownCategory);
    }

    if ev.category == intervals_icu_client::EventCategory::Workout
        && ev
            .r#type
            .as_ref()
            .map(|s| s.trim())
            .unwrap_or("")
            .is_empty()
    {
        tracing::debug!("validate_and_prepare_event: missing type for WORKOUT - defaulting to Run");
        ev.r#type = Some("Run".into());
    }

    Ok(ev)
}

/// Convert a validation error to a user-friendly error message.
///
/// This helper follows the **Information Expert** principle by keeping
/// error message formatting logic in the same module as the validation.
pub fn validation_error_to_string(e: EventValidationError) -> String {
    e.to_string()
}

pub fn compact_json_event(value: &Value, fields: Option<&[String]>) -> Value {
    crate::compact::compact_object(value, DEFAULT_EVENT_FIELDS, fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_date_str_accepts_iso_datetime() {
        let normalized = normalize_date_str("2026-01-19T06:30:00").unwrap();
        assert_eq!(normalized, "2026-01-19");
    }

    #[test]
    fn normalize_date_str_accepts_rfc3339() {
        assert_eq!(
            normalize_date_str("2026-01-19T06:30:00Z").unwrap(),
            "2026-01-19"
        );
    }

    #[test]
    fn normalize_date_str_accepts_date_only() {
        assert_eq!(normalize_date_str("2026-01-19").unwrap(), "2026-01-19");
    }

    #[test]
    fn normalize_date_str_rejects_invalid() {
        assert!(normalize_date_str("not-a-date").is_none());
    }

    #[test]
    fn normalize_event_start_expands_date() {
        let normalized = normalize_event_start("2026-01-19").unwrap();
        assert_eq!(normalized, "2026-01-19T00:00:00");
    }

    #[test]
    fn normalize_event_start_preserves_time_rfc3339() {
        assert_eq!(
            normalize_event_start("2026-01-19T06:30:00Z").unwrap(),
            "2026-01-19T06:30:00"
        );
    }

    #[test]
    fn normalize_event_start_preserves_naive_datetime() {
        assert_eq!(
            normalize_event_start("2026-01-19T06:30:00").unwrap(),
            "2026-01-19T06:30:00"
        );
    }

    #[test]
    fn normalize_event_start_rejects_invalid() {
        assert!(normalize_event_start("not-a-date").is_none());
    }

    #[test]
    fn validate_and_prepare_event_rejects_empty_name() {
        let ev = intervals_icu_client::Event {
            id: None,
            start_date_local: "2026-01-19".into(),
            name: "".into(),
            category: intervals_icu_client::EventCategory::Note,
            description: None,
            r#type: None,
        };
        matches!(
            validate_and_prepare_event(ev),
            Err(EventValidationError::EmptyName)
        );
    }

    #[test]
    fn validate_and_prepare_event_normalizes_start_date_and_defaults_workout_type() {
        let ev = intervals_icu_client::Event {
            id: None,
            start_date_local: "2026-01-19".into(),
            name: "Test".into(),
            category: intervals_icu_client::EventCategory::Workout,
            description: None,
            r#type: None,
        };

        let prepared = validate_and_prepare_event(ev).expect("should prepare event");
        assert_eq!(prepared.start_date_local, "2026-01-19T00:00:00");
        assert_eq!(prepared.r#type.as_deref(), Some("Run"));
    }

    #[test]
    fn validate_and_prepare_event_rejects_unknown_category() {
        let ev = intervals_icu_client::Event {
            id: None,
            start_date_local: "2026-01-19".into(),
            name: "Test".into(),
            category: intervals_icu_client::EventCategory::Unknown,
            description: None,
            r#type: None,
        };
        assert_eq!(
            validate_and_prepare_event(ev),
            Err(EventValidationError::UnknownCategory)
        );
    }

    // ── compact_events ──────────────────────────────────────────────────────

    fn make_event(id: &str, name: &str) -> intervals_icu_client::Event {
        intervals_icu_client::Event {
            id: Some(id.to_string()),
            start_date_local: "2026-03-01".into(),
            name: name.into(),
            category: intervals_icu_client::EventCategory::Note,
            description: Some("desc".into()),
            r#type: None,
        }
    }

    #[test]
    fn compact_events_compact_mode_uses_default_fields() {
        let events = vec![make_event("e1", "My Event")];
        let result = compact_events(events, true, None);
        assert_eq!(result.len(), 1);
        let obj = result[0].as_object().unwrap();
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("e1"));
        assert_eq!(
            obj.get("start_date_local").and_then(|v| v.as_str()),
            Some("2026-03-01")
        );
        assert_eq!(obj.get("name").and_then(|v| v.as_str()), Some("My Event"));
        // description is not in default compact fields
        assert!(obj.get("description").is_none());
    }

    #[test]
    fn compact_events_non_compact_returns_all_fields() {
        let events = vec![make_event("e2", "Full Event")];
        let result = compact_events(events, false, None);
        assert_eq!(result.len(), 1);
        let obj = result[0].as_object().unwrap();
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("e2"));
        // description should be present in non-compact mode
        assert_eq!(
            obj.get("description").and_then(|v| v.as_str()),
            Some("desc")
        );
    }

    #[test]
    fn compact_events_non_compact_with_fields_filters() {
        let events = vec![make_event("e3", "Filtered")];
        let fields = vec!["id".to_string(), "name".to_string()];
        let result = compact_events(events, false, Some(&fields));
        assert_eq!(result.len(), 1);
        let obj = result[0].as_object().unwrap();
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("e3"));
        assert_eq!(obj.get("name").and_then(|v| v.as_str()), Some("Filtered"));
        // description was serializable but not in filter
        assert!(obj.get("description").is_none());
    }

    // ── compact_single_event ────────────────────────────────────────────────

    #[test]
    fn compact_single_event_default_fields() {
        let ev = make_event("e4", "Single");
        let result = compact_single_event(&ev, None);
        let obj = result.as_object().unwrap();
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("e4"));
        assert_eq!(obj.get("name").and_then(|v| v.as_str()), Some("Single"));
        // description is in the single-event default fields
        assert_eq!(
            obj.get("description").and_then(|v| v.as_str()),
            Some("desc")
        );
    }

    #[test]
    fn compact_single_event_custom_fields() {
        let ev = make_event("e5", "Custom");
        let fields = vec!["id".to_string()];
        let result = compact_single_event(&ev, Some(&fields));
        let obj = result.as_object().unwrap();
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("e5"));
        assert!(obj.get("name").is_none());
    }

    // ── compact_json_event ──────────────────────────────────────────────────

    #[test]
    fn compact_json_event_default_fields() {
        use serde_json::json;
        let input = json!({
            "id": "e6",
            "name": "JSON Event",
            "start_date_local": "2026-03-01",
            "category": "NOTE",
            "type": null,
            "description": "some desc",
            "extra_field": 42
        });
        let result = compact_json_event(&input, None);
        let obj = result.as_object().unwrap();
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("e6"));
        assert_eq!(obj.get("name").and_then(|v| v.as_str()), Some("JSON Event"));
        assert!(obj.get("extra_field").is_none());
    }

    #[test]
    fn compact_json_event_non_object_returned_as_is() {
        use serde_json::json;
        let input = json!("not an object");
        let result = compact_json_event(&input, None);
        assert_eq!(result, input);
    }

    // ── compact_events_from_value ───────────────────────────────────────────

    #[test]
    fn compact_events_from_value_compact_limits_and_filters() {
        use serde_json::json;
        let input = json!([
            {"id": "x1", "name": "A", "start_date_local": "2026-03-01",
             "category": "NOTE", "extra": 1},
            {"id": "x2", "name": "B", "start_date_local": "2026-03-02",
             "category": "NOTE", "extra": 2},
            {"id": "x3", "name": "C", "start_date_local": "2026-03-03",
             "category": "NOTE", "extra": 3}
        ]);
        let result = compact_events_from_value(&input, true, 2, None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2, "limit=2 crops to 2 items");
        assert!(
            arr[0].get("extra").is_none(),
            "extra stripped in compact mode"
        );
        assert_eq!(arr[0].get("id").and_then(|v| v.as_str()), Some("x1"));
    }

    #[test]
    fn compact_events_from_value_non_compact_returns_all() {
        use serde_json::json;
        let input = json!([
            {"id": "y1", "name": "Z", "start_date_local": "2026-04-01",
             "category": "NOTE", "extra": 99}
        ]);
        let result = compact_events_from_value(&input, false, 10, None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(
            arr[0].get("extra").and_then(|v| v.as_i64()),
            Some(99),
            "extra preserved in non-compact mode"
        );
    }

    #[test]
    fn compact_events_from_value_non_compact_applies_limit() {
        use serde_json::json;
        let input = json!([
            {"id": "z1", "name": "A"},
            {"id": "z2", "name": "B"},
            {"id": "z3", "name": "C"}
        ]);

        let result = compact_events_from_value(&input, false, 2, None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].get("id").and_then(|v| v.as_str()), Some("z1"));
        assert_eq!(arr[1].get("id").and_then(|v| v.as_str()), Some("z2"));
    }
}
