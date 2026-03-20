use crate::domains::coach::DataAudit;

use super::analysis_fetch::FetchedAnalysisData;

pub fn build_data_audit(fetched: &FetchedAnalysisData) -> DataAudit {
    let wellness_available = fetched
        .wellness
        .as_ref()
        .and_then(|value| value.as_array())
        .map(|entries| !entries.is_empty())
        .unwrap_or(false);

    let fitness_available = fetched
        .fitness
        .as_ref()
        .map(fitness_payload_has_signal)
        .unwrap_or(false);

    let intervals_available = payload_has_entries(fetched.intervals.as_ref());
    let streams_available = payload_has_entries(fetched.streams.as_ref());

    let mut degraded_mode_reasons = Vec::new();
    if fetched.activities.is_empty() && fetched.workout_detail.is_none() {
        degraded_mode_reasons.push("activities unavailable for requested window".to_string());
    }
    if fetched.wellness.is_some() && !wellness_available {
        degraded_mode_reasons.push("wellness data unavailable or empty".to_string());
    }
    if !fitness_available {
        degraded_mode_reasons.push("fitness summary unavailable".to_string());
    }
    if fetched.intervals.is_some() && !intervals_available {
        degraded_mode_reasons.push("interval data unavailable".to_string());
    }
    if fetched.streams.is_some() && !streams_available {
        degraded_mode_reasons.push("stream data unavailable".to_string());
    }

    DataAudit {
        activities_available: !fetched.activities.is_empty() || fetched.workout_detail.is_some(),
        wellness_available,
        fitness_available,
        intervals_available,
        streams_available,
        degraded_mode_reasons,
    }
}

fn fitness_payload_has_signal(value: &serde_json::Value) -> bool {
    if let Some(obj) = value.as_object() {
        return ["fitness", "fatigue", "form", "ctl", "atl", "tsb"]
            .iter()
            .any(|key| obj.get(*key).is_some_and(|entry| !entry.is_null()));
    }

    if let Some(arr) = value.as_array() {
        return arr.iter().any(fitness_payload_has_signal);
    }

    false
}

fn payload_has_entries(value: Option<&serde_json::Value>) -> bool {
    value.is_some_and(|payload| match payload {
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(map) => !map.is_empty(),
        serde_json::Value::Null => false,
        _ => true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_marks_wellness_unavailable_for_empty_wellness_payload() {
        let fetched = FetchedAnalysisData {
            wellness: Some(serde_json::json!([])),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(!audit.wellness_available);
    }

    #[test]
    fn audit_records_missing_fitness_summary_reason() {
        let fetched = FetchedAnalysisData::default();
        let audit = build_data_audit(&fetched);

        assert!(
            audit
                .degraded_mode_reasons
                .iter()
                .any(|reason| reason.contains("fitness"))
        );
    }

    #[test]
    fn audit_marks_streams_unavailable_when_payload_is_missing() {
        let fetched = FetchedAnalysisData {
            streams: Some(serde_json::json!({})),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(!audit.streams_available);
    }
}
