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

    #[test]
    fn audit_detects_wellness_data_available() {
        let fetched = FetchedAnalysisData {
            wellness: Some(serde_json::json!([{"date": "2026-03-01", "sleep": 8}])),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(audit.wellness_available);
    }

    #[test]
    fn audit_detects_fitness_data_available_with_all_fields() {
        let fetched = FetchedAnalysisData {
            fitness: Some(serde_json::json!({
                "fitness": 50,
                "fatigue": 30,
                "form": 20,
                "ctl": 50,
                "atl": 30,
                "tsb": 20
            })),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(audit.fitness_available);
    }

    #[test]
    fn audit_detects_fitness_data_available_with_partial_fields() {
        let fetched = FetchedAnalysisData {
            fitness: Some(serde_json::json!({
                "ctl": 50,
                "atl": 30
            })),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(audit.fitness_available);
    }

    #[test]
    fn audit_fitness_null_values_not_considered_signal() {
        let fetched = FetchedAnalysisData {
            fitness: Some(serde_json::json!({
                "fitness": null,
                "fatigue": null,
                "form": null
            })),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(!audit.fitness_available);
    }

    #[test]
    fn audit_fitness_array_with_signal() {
        let fetched = FetchedAnalysisData {
            fitness: Some(serde_json::json!([
                {"ctl": null},
                {"ctl": 50, "atl": 30}
            ])),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(audit.fitness_available);
    }

    #[test]
    fn audit_fitness_array_all_null() {
        let fetched = FetchedAnalysisData {
            fitness: Some(serde_json::json!([
                {"ctl": null},
                {"atl": null}
            ])),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(!audit.fitness_available);
    }

    #[test]
    fn audit_fitness_non_object_non_array_is_false() {
        let fetched = FetchedAnalysisData {
            fitness: Some(serde_json::json!("string_value")),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(!audit.fitness_available);
    }

    #[test]
    fn audit_detects_intervals_available() {
        let fetched = FetchedAnalysisData {
            intervals: Some(serde_json::json!([{"name": "interval1"}])),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(audit.intervals_available);
    }

    #[test]
    fn audit_detects_intervals_unavailable_empty_array() {
        let fetched = FetchedAnalysisData {
            intervals: Some(serde_json::json!([])),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(!audit.intervals_available);
    }

    #[test]
    fn audit_detects_intervals_unavailable_null() {
        let fetched = FetchedAnalysisData {
            intervals: None,
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(!audit.intervals_available);
    }

    #[test]
    fn audit_detects_streams_available() {
        let fetched = FetchedAnalysisData {
            streams: Some(serde_json::json!({"heartrate": [100, 110, 120]})),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(audit.streams_available);
    }

    #[test]
    fn audit_detects_activities_available_from_activities_list() {
        use intervals_icu_client::ActivitySummary;

        let fetched = FetchedAnalysisData {
            activities: vec![ActivitySummary {
                id: "123".to_string(),
                name: Some("Test".to_string()),
                start_date_local: "2026-03-01".to_string(),
            }],
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(audit.activities_available);
    }

    #[test]
    fn audit_detects_activities_available_from_workout_detail() {
        let fetched = FetchedAnalysisData {
            workout_detail: Some(serde_json::json!({"id": "w1"})),
            activities: vec![],
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(audit.activities_available);
    }

    #[test]
    fn audit_records_degraded_mode_for_no_activities() {
        let fetched = FetchedAnalysisData {
            activities: vec![],
            workout_detail: None,
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(
            audit
                .degraded_mode_reasons
                .iter()
                .any(|reason| reason.contains("activities unavailable"))
        );
    }

    #[test]
    fn audit_records_degraded_mode_for_empty_wellness() {
        use intervals_icu_client::ActivitySummary;

        let fetched = FetchedAnalysisData {
            wellness: Some(serde_json::json!([])),
            activities: vec![ActivitySummary {
                id: "123".to_string(),
                name: Some("Test".to_string()),
                start_date_local: "2026-03-01".to_string(),
            }],
            fitness: Some(serde_json::json!({"ctl": 50})),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(
            audit
                .degraded_mode_reasons
                .iter()
                .any(|reason| reason.contains("wellness data unavailable"))
        );
    }

    #[test]
    fn audit_records_degraded_mode_for_missing_intervals() {
        use intervals_icu_client::ActivitySummary;

        let fetched = FetchedAnalysisData {
            intervals: Some(serde_json::json!([])),
            activities: vec![ActivitySummary {
                id: "123".to_string(),
                name: Some("Test".to_string()),
                start_date_local: "2026-03-01".to_string(),
            }],
            fitness: Some(serde_json::json!({"ctl": 50})),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(
            audit
                .degraded_mode_reasons
                .iter()
                .any(|reason| reason.contains("interval data unavailable"))
        );
    }

    #[test]
    fn audit_records_degraded_mode_for_missing_streams() {
        use intervals_icu_client::ActivitySummary;

        let fetched = FetchedAnalysisData {
            streams: Some(serde_json::json!({})),
            activities: vec![ActivitySummary {
                id: "123".to_string(),
                name: Some("Test".to_string()),
                start_date_local: "2026-03-01".to_string(),
            }],
            fitness: Some(serde_json::json!({"ctl": 50})),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(
            audit
                .degraded_mode_reasons
                .iter()
                .any(|reason| reason.contains("stream data unavailable"))
        );
    }

    #[test]
    fn audit_full_data_no_degraded_mode() {
        use intervals_icu_client::ActivitySummary;

        let fetched = FetchedAnalysisData {
            activities: vec![ActivitySummary {
                id: "123".to_string(),
                name: Some("Test".to_string()),
                start_date_local: "2026-03-01".to_string(),
            }],
            wellness: Some(serde_json::json!([{"date": "2026-03-01"}])),
            fitness: Some(serde_json::json!({"ctl": 50, "atl": 30})),
            intervals: Some(serde_json::json!([{"name": "interval1"}])),
            streams: Some(serde_json::json!({"heartrate": [100, 110]})),
            ..Default::default()
        };

        let audit = build_data_audit(&fetched);
        assert!(audit.activities_available);
        assert!(audit.wellness_available);
        assert!(audit.fitness_available);
        assert!(audit.intervals_available);
        assert!(audit.streams_available);
        assert!(audit.degraded_mode_reasons.is_empty());
    }

    #[test]
    fn payload_has_entries_null_returns_false() {
        assert!(!payload_has_entries(Some(&serde_json::Value::Null)));
    }

    #[test]
    fn payload_has_entries_empty_object_returns_false() {
        assert!(!payload_has_entries(Some(&serde_json::json!({}))));
    }

    #[test]
    fn payload_has_entries_empty_array_returns_false() {
        assert!(!payload_has_entries(Some(&serde_json::json!([]))));
    }

    #[test]
    fn payload_has_entries_non_empty_object_returns_true() {
        assert!(payload_has_entries(Some(
            &serde_json::json!({"key": "value"})
        )));
    }

    #[test]
    fn payload_has_entries_non_empty_array_returns_true() {
        assert!(payload_has_entries(Some(&serde_json::json!([1, 2, 3]))));
    }

    #[test]
    fn payload_has_entries_none_returns_false() {
        assert!(!payload_has_entries(None));
    }

    #[test]
    fn payload_has_entries_primitive_returns_true() {
        assert!(payload_has_entries(Some(&serde_json::json!(42))));
    }
}
