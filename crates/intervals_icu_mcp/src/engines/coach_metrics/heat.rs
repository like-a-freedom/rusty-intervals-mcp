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
