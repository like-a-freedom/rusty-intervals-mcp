use crate::domains::coach::HeatMetrics;
use crate::engines::coach_metrics_constants::*;
use serde_json::Value;

use super::helpers::get_number;

// =============================================================================
// P2.2 — Heat Stress Metrics
// =============================================================================

/// Compute heat metrics from 7-day activity window.
/// Fallback chain: average_temp → average_weather_temp → average_feels_like.
pub fn compute_heat_metrics_7d(
    activity_details: &std::collections::HashMap<String, Value>,
    activity_ids: &[String],
) -> HeatMetrics {
    let mut temps: Vec<f64> = Vec::new();

    for id in activity_ids {
        let Some(detail) = activity_details.get(id).and_then(Value::as_object) else {
            continue;
        };
        let temp = get_number(detail, &["average_temp"])
            .or_else(|| get_number(detail, &["average_weather_temp"]))
            .or_else(|| get_number(detail, &["average_feels_like"]));
        if let Some(t) = temp {
            temps.push(t);
        }
    }

    if temps.is_empty() {
        return HeatMetrics::default();
    }

    let mean_temp = temps.iter().sum::<f64>() / temps.len() as f64;
    let max_temp = temps.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    // Source: Cheuvront & Kenefick — 18°C baseline, 23°C = moderate heat stress.
    let heat_index = ((mean_temp - HEAT_BASELINE_TEMP_C) / HEAT_NORMALIZATION_RANGE_C)
        .clamp(0.0, HEAT_INDEX_CLAMP_MAX);
    let heat_state = if heat_index > HEAT_HIGH_THRESHOLD {
        "high".to_string()
    } else if heat_index >= HEAT_MODERATE_THRESHOLD {
        "moderate".to_string()
    } else {
        "low".to_string()
    };

    HeatMetrics {
        supported: true,
        heat_index_7d: Some(heat_index),
        heat_max_7d: Some(max_temp),
        heat_state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn empty_ids_returns_default() {
        let metrics = compute_heat_metrics_7d(&HashMap::new(), &[]);
        assert!(!metrics.supported);
        assert!(metrics.heat_index_7d.is_none());
        assert!(metrics.heat_max_7d.is_none());
        assert!(metrics.heat_state.is_empty());
    }

    #[test]
    fn activity_not_in_details_skipped() {
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&HashMap::new(), &ids);
        assert!(!metrics.supported);
    }

    #[test]
    fn activity_not_object_skipped() {
        let mut details = HashMap::new();
        details.insert("a1".to_string(), json!("not_an_object"));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert!(!metrics.supported);
    }

    #[test]
    fn activity_with_no_temp_fields_skipped() {
        let mut details = HashMap::new();
        details.insert("a1".to_string(), json!({"distance": 5000.0}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert!(!metrics.supported);
    }

    #[test]
    fn single_activity_average_temp() {
        let mut details = HashMap::new();
        details.insert("a1".to_string(), json!({"average_temp": 25.0}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert!(metrics.supported);
        assert_eq!(metrics.heat_max_7d, Some(25.0));
    }

    #[test]
    fn fallback_to_average_weather_temp() {
        let mut details = HashMap::new();
        details.insert("a1".to_string(), json!({"average_weather_temp": 22.0}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert!(metrics.supported);
        assert_eq!(metrics.heat_max_7d, Some(22.0));
    }

    #[test]
    fn fallback_to_average_feels_like() {
        let mut details = HashMap::new();
        details.insert("a1".to_string(), json!({"average_feels_like": 28.0}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert!(metrics.supported);
        assert_eq!(metrics.heat_max_7d, Some(28.0));
    }

    #[test]
    fn average_temp_preferred_over_fallbacks() {
        let mut details = HashMap::new();
        details.insert(
            "a1".to_string(),
            json!({
                "average_temp": 20.0,
                "average_weather_temp": 22.0,
                "average_feels_like": 24.0
            }),
        );
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert!(metrics.supported);
        // mean = 20.0 → heat_index = (20-18)/5 = 0.4 → low
        assert_eq!(metrics.heat_state, "low");
    }

    #[test]
    fn low_heat_state_when_below_moderate_threshold() {
        let mut details = HashMap::new();
        // 20°C → (20-18)/5 = 0.4 < 0.5 → "low"
        details.insert("a1".to_string(), json!({"average_temp": 20.0}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert_eq!(metrics.heat_state, "low");
    }

    #[test]
    fn moderate_heat_state() {
        let mut details = HashMap::new();
        // 21°C → (21-18)/5 = 0.6 ≥ 0.5 → "moderate"
        details.insert("a1".to_string(), json!({"average_temp": 21.0}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert_eq!(metrics.heat_state, "moderate");
    }

    #[test]
    fn high_heat_state() {
        let mut details = HashMap::new();
        // 24°C → (24-18)/5 = 1.2 > 1.0 → "high"
        details.insert("a1".to_string(), json!({"average_temp": 24.0}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert_eq!(metrics.heat_state, "high");
    }

    #[test]
    fn heat_index_clamped_at_max() {
        let mut details = HashMap::new();
        // 30°C → (30-18)/5 = 2.4, clamped to 2.0
        details.insert("a1".to_string(), json!({"average_temp": 30.0}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert_eq!(metrics.heat_index_7d, Some(HEAT_INDEX_CLAMP_MAX));
        assert_eq!(metrics.heat_state, "high");
    }

    #[test]
    fn heat_index_clamped_at_zero() {
        let mut details = HashMap::new();
        // 15°C → (15-18)/5 = -0.6, clamped to 0.0
        details.insert("a1".to_string(), json!({"average_temp": 15.0}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert_eq!(metrics.heat_index_7d, Some(0.0));
        assert_eq!(metrics.heat_state, "low");
    }

    #[test]
    fn multiple_activities_computes_mean_and_max() {
        let mut details = HashMap::new();
        details.insert("a1".to_string(), json!({"average_temp": 20.0}));
        details.insert("a2".to_string(), json!({"average_temp": 26.0}));
        details.insert("a3".to_string(), json!({"average_temp": 22.0}));
        let ids = vec!["a1".to_string(), "a2".to_string(), "a3".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert!(metrics.supported);
        assert_eq!(metrics.heat_max_7d, Some(26.0));
        // mean = (20+26+22)/3 = 22.6667, heat_index = (22.6667-18)/5 ≈ 0.933
        let mean_index = metrics.heat_index_7d.unwrap();
        assert!((mean_index - 0.933).abs() < 0.01);
        assert_eq!(metrics.heat_state, "moderate");
    }

    #[test]
    fn some_activities_missing_temp_are_skipped() {
        let mut details = HashMap::new();
        details.insert("a1".to_string(), json!({"average_temp": 25.0}));
        details.insert("a2".to_string(), json!({"distance": 5000.0}));
        let ids = vec!["a1".to_string(), "a2".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert!(metrics.supported);
        // Only a1 has temp, mean = 25, max = 25
        assert_eq!(metrics.heat_max_7d, Some(25.0));
    }

    #[test]
    fn integer_temp_value_is_parsed() {
        let mut details = HashMap::new();
        details.insert("a1".to_string(), json!({"average_temp": 23}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert!(metrics.supported);
        assert_eq!(metrics.heat_max_7d, Some(23.0));
    }

    #[test]
    fn boundary_moderate_threshold() {
        let mut details = HashMap::new();
        // Exactly at moderate: heat_index = 0.5 → ≥ 0.5 → "moderate"
        // mean_temp = 0.5 * 5 + 18 = 20.5
        details.insert("a1".to_string(), json!({"average_temp": 20.5}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert_eq!(metrics.heat_state, "moderate");
    }

    #[test]
    fn boundary_high_threshold() {
        let mut details = HashMap::new();
        // Just above high: heat_index = 1.01 → "high"
        // mean_temp = 1.01 * 5 + 18 = 23.05
        details.insert("a1".to_string(), json!({"average_temp": 23.05}));
        let ids = vec!["a1".to_string()];
        let metrics = compute_heat_metrics_7d(&details, &ids);
        assert_eq!(metrics.heat_state, "high");
    }
}
