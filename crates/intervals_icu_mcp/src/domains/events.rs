use serde_json::{Map, Value};

pub fn compact_events(
    events: Vec<intervals_icu_client::Event>,
    compact: bool,
    fields: Option<&[String]>,
) -> Vec<Value> {
    let default_fields = ["id", "start_date_local", "name", "category", "type"];
    let fields_to_use: Vec<&str> = if compact {
        fields
            .map(|f| f.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| default_fields.to_vec())
    } else {
        return events
            .into_iter()
            .map(|e| {
                let mut obj = Map::new();
                if let Some(_val) = serde_json::to_value(&e)
                    .ok()
                    .and_then(|v| v.as_object().cloned())
                {
                    if let Some(filter) = fields {
                        for field in filter {
                            if let Some(v) = obj.get(field) {
                                obj.insert(field.clone(), v.clone());
                            }
                        }
                        Value::Object(obj)
                    } else {
                        Value::Object(obj)
                    }
                } else {
                    serde_json::to_value(e).unwrap_or_default()
                }
            })
            .collect();
    };

    events
        .into_iter()
        .map(|event| {
            let mut result = Map::new();
            let event_json = serde_json::to_value(&event).unwrap_or_default();

            if let Some(obj) = event_json.as_object() {
                for field in &fields_to_use {
                    if let Some(val) = obj.get(*field) {
                        result.insert(field.to_string(), val.clone());
                    }
                }
            }

            Value::Object(result)
        })
        .collect()
}

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

pub fn compact_single_event(
    event: &intervals_icu_client::Event,
    fields: Option<&[String]>,
) -> Value {
    let default_fields = [
        "id",
        "name",
        "start_date_local",
        "category",
        "type",
        "description",
    ];
    let fields_to_use: Vec<&str> = fields
        .map(|f| f.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| default_fields.to_vec());

    let mut result = Map::new();
    let event_json = serde_json::to_value(event).unwrap_or_default();

    if let Some(obj) = event_json.as_object() {
        for field in &fields_to_use {
            if let Some(val) = obj.get(*field) {
                result.insert(field.to_string(), val.clone());
            }
        }
    }

    Value::Object(result)
}

pub fn filter_event_fields(event: &intervals_icu_client::Event, fields: &[String]) -> Value {
    let mut result = Map::new();
    let event_json = serde_json::to_value(event).unwrap_or_default();

    if let Some(obj) = event_json.as_object() {
        for field in fields {
            if let Some(val) = obj.get(field) {
                result.insert(field.clone(), val.clone());
            }
        }
    }

    Value::Object(result)
}

pub fn compact_events_from_value(
    value: &Value,
    compact: bool,
    limit: usize,
    fields: Option<&[String]>,
) -> Value {
    if compact {
        let default_fields = ["id", "name", "start_date_local", "category", "type"];
        let fields_to_use: Vec<&str> = fields
            .map(|f| f.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| default_fields.to_vec());

        let arr = value.as_array().cloned().unwrap_or_default();
        let compacted: Vec<Value> = arr
            .iter()
            .take(limit)
            .map(|item| {
                let Some(obj) = item.as_object() else {
                    return item.clone();
                };
                let mut result = Map::new();
                for field in &fields_to_use {
                    if let Some(val) = obj.get(*field) {
                        result.insert(field.to_string(), val.clone());
                    }
                }
                Value::Object(result)
            })
            .collect();

        Value::Array(compacted)
    } else if let Some(field_list) = fields {
        crate::compact::filter_array_fields(value, field_list)
    } else {
        value.clone()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EventValidationError {
    EmptyName,
    InvalidStartDate(String),
    UnknownCategory,
}

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

pub fn compact_json_event(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = [
        "id",
        "name",
        "start_date_local",
        "category",
        "type",
        "description",
    ];
    let fields_to_use: Vec<&str> = fields
        .map(|f| f.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| default_fields.to_vec());

    let Some(obj) = value.as_object() else {
        return value.clone();
    };

    let mut result = Map::new();
    for field in &fields_to_use {
        if let Some(val) = obj.get(*field) {
            result.insert(field.to_string(), val.clone());
        }
    }

    Value::Object(result)
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
    fn normalize_event_start_expands_date() {
        let normalized = normalize_event_start("2026-01-19").unwrap();
        assert_eq!(normalized, "2026-01-19T00:00:00");
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
    fn validate_and_prepare_event_rejects_invalid_start_date() {
        let ev = intervals_icu_client::Event {
            id: None,
            start_date_local: "not-a-date".into(),
            name: "Test".into(),
            category: intervals_icu_client::EventCategory::Note,
            description: None,
            r#type: None,
        };

        match validate_and_prepare_event(ev) {
            Err(EventValidationError::InvalidStartDate(s)) => assert_eq!(s, "not-a-date"),
            other => panic!("unexpected result: {:?}", other),
        }
    }
}
