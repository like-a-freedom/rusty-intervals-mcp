use serde_json::Value;

pub fn percent_delta(previous: f64, current: f64) -> Option<f64> {
    if previous.abs() < f64::EPSILON {
        None
    } else {
        Some(((current - previous) / previous) * 100.0)
    }
}

pub fn get_number(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
    })
}

pub fn collect_numbers(entries: &[Value], keys: &[&str]) -> Vec<f64> {
    entries
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|object| get_number(object, keys))
        .collect()
}

pub fn extract_numeric_stream(streams: &Value, keys: &[&str]) -> Option<Vec<f64>> {
    let object = streams.as_object()?;
    let values = keys
        .iter()
        .find_map(|key| object.get(*key)?.as_array().cloned())?;
    let numbers = values
        .into_iter()
        .filter_map(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
        .collect::<Vec<_>>();

    if numbers.is_empty() {
        None
    } else {
        Some(numbers)
    }
}

pub fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}
