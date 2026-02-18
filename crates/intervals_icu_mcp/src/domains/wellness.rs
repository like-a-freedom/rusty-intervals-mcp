use serde_json::{Map, Value};

pub fn transform_wellness(value: &Value, summary_only: bool, fields: Option<&[String]>) -> Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    if summary_only {
        let mut sleep_total: f64 = 0.0;
        let mut stress_total: f64 = 0.0;
        let mut hr_total: f64 = 0.0;
        let mut hrv_total: f64 = 0.0;
        let mut sleep_count: usize = 0;
        let mut stress_count: usize = 0;
        let mut hr_count: usize = 0;
        let mut hrv_count: usize = 0;

        for item in arr {
            if let Some(obj) = item.as_object() {
                if let Some(v) = obj.get("sleepSecs").and_then(|v| v.as_f64()) {
                    sleep_total += v / 3600.0;
                    sleep_count += 1;
                }
                if let Some(v) = obj.get("stress").and_then(|v| v.as_f64()) {
                    stress_total += v;
                    stress_count += 1;
                }
                if let Some(v) = obj.get("restingHR").and_then(|v| v.as_f64()) {
                    hr_total += v;
                    hr_count += 1;
                }
                if let Some(v) = obj.get("hrv").and_then(|v| v.as_f64()) {
                    hrv_total += v;
                    hrv_count += 1;
                }
            }
        }

        return serde_json::json!({
            "days": arr.len(),
            "avg_sleep_hours": if sleep_count > 0 { (sleep_total / sleep_count as f64 * 10.0).round() / 10.0 } else { 0.0 },
            "avg_stress": if stress_count > 0 { (stress_total / stress_count as f64 * 10.0).round() / 10.0 } else { 0.0 },
            "avg_resting_hr": if hr_count > 0 { (hr_total / hr_count as f64).round() } else { 0.0 },
            "avg_hrv": if hrv_count > 0 { (hrv_total / hrv_count as f64).round() } else { 0.0 }
        });
    }

    if let Some(field_list) = fields {
        let default_fields = ["id", "sleepSecs", "stress", "restingHR", "hrv", "weight"];
        let fields_to_use: Vec<&str> = field_list.iter().map(|s| s.as_str()).collect();
        let fields_to_use = if fields_to_use.is_empty() {
            default_fields.to_vec()
        } else {
            fields_to_use
        };

        let filtered: Vec<Value> = arr
            .iter()
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

        return Value::Array(filtered);
    }

    value.clone()
}

pub fn compact_wellness_entry(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = [
        "id",
        "sleepSecs",
        "stress",
        "restingHR",
        "hrv",
        "weight",
        "fatigue",
        "motivation",
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
    fn transform_wellness_summary_returns_aggregates() {
        let input = serde_json::json!([
            {"sleepSecs": 28800, "stress": 20, "restingHR": 50, "hrv": 45},
            {"sleepSecs": 25200, "stress": 30, "restingHR": 55, "hrv": 40}
        ]);

        let out = transform_wellness(&input, true, None);
        assert_eq!(out["days"], 2);
        assert_eq!(out["avg_sleep_hours"], 7.5);
    }

    #[test]
    fn compact_wellness_entry_filters_fields() {
        let input = serde_json::json!({"id":"w1","sleepSecs":28800,"extra":"x"});
        let out = compact_wellness_entry(&input, Some(&["id".into()]));
        assert_eq!(out["id"], "w1");
        assert!(out.get("sleepSecs").is_none());
    }
}
