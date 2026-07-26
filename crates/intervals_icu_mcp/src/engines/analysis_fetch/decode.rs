use serde_json::Value;

/// Returns true when a payload has no usable content (empty array/object or null).
pub(crate) fn value_is_empty(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Null => true,
        _ => false,
    }
}

pub(crate) fn normalize_upcoming_events_payload(payload: Value) -> Value {
    let Some(items) = payload.as_array() else {
        return payload;
    };

    Value::Array(
        items
            .iter()
            .map(|event| {
                let Some(object) = event.as_object() else {
                    return event.clone();
                };

                let has_name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty());
                if has_name {
                    return event.clone();
                }

                let fallback_name = object
                    .get("description")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| {
                        object
                            .get("category")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                    })
                    .unwrap_or("Untitled event");

                let mut normalized = object.clone();
                normalized.insert("name".to_string(), Value::String(fallback_name.to_string()));
                Value::Object(normalized)
            })
            .collect(),
    )
}

pub(crate) fn upcoming_rate_limit_warning() -> String {
    "planned workouts unavailable due to Intervals.icu rate limiting; continuing with completed-activity history only".to_string()
}

pub(crate) fn normalize_intervals_payload(payload: Value) -> Value {
    if payload.is_array() {
        return payload;
    }

    let Some(object) = payload.as_object() else {
        return payload;
    };

    if let Some(intervals) = object.get("icu_intervals").and_then(Value::as_array) {
        return Value::Array(intervals.clone());
    }

    if let Some(groups) = object.get("icu_groups").and_then(Value::as_array) {
        return Value::Array(groups.clone());
    }

    payload
}

pub(crate) fn normalize_stream_descriptor_array(items: &[Value]) -> Option<Value> {
    let mut normalized = serde_json::Map::new();

    for item in items.iter().filter_map(Value::as_object) {
        let key = item
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                item.get("type")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
            })?;
        let data = item.get("data").filter(|value| value.is_array())?;
        normalized.insert(key.to_string(), data.clone());
    }

    if normalized.is_empty() {
        None
    } else {
        Some(Value::Object(normalized))
    }
}

pub(crate) fn normalize_streams_payload(payload: Value) -> Value {
    if let Some(items) = payload.as_array() {
        return normalize_stream_descriptor_array(items).unwrap_or(payload);
    }

    let Some(object) = payload.as_object() else {
        return payload;
    };

    if let Some(streams) = object.get("streams") {
        if let Some(stream_map) = streams.as_object() {
            return Value::Object(stream_map.clone());
        }

        if let Some(stream_items) = streams.as_array() {
            return normalize_stream_descriptor_array(stream_items).unwrap_or(payload);
        }
    }

    payload
}
