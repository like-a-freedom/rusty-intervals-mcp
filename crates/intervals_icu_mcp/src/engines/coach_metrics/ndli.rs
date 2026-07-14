use crate::domains::coach::NdliMetrics;
use crate::engines::coach_metrics_constants::*;
use serde_json::Value;

use super::helpers::get_number;

/// Compute NDLI from a 7-day window of activities.
/// Classification: Green ≤2, Amber =3, Red ≥4 high-intensity days.
pub fn compute_ndli_7d(
    activity_details: &std::collections::HashMap<String, Value>,
    activity_ids: &[String],
) -> NdliMetrics {
    let mut high_intensity_days: usize = 0;
    let mut if_values: Vec<f64> = Vec::new();
    let mut ef_values: Vec<f64> = Vec::new();
    let mut vi_values: Vec<f64> = Vec::new();
    let mut days_with_data: usize = 0;

    for id in activity_ids {
        let Some(detail) = activity_details.get(id).and_then(Value::as_object) else {
            continue;
        };
        days_with_data += 1;

        // Primary: icu_joules_above_ftp > 20000 → high-intensity day
        let joules = get_number(detail, &["icu_joules_above_ftp", "joules_above_ftp"]);
        let is_high = if let Some(j) = joules {
            j > NDLI_HIGH_INTENSITY_JOULES_THRESHOLD
        } else {
            // Fallback for running: icu_training_load > 80 TSS/day
            // Scaled relative to typical CTL: 80 TSS ≈ ~1.2× CTL for moderate athletes.
            get_number(detail, &["icu_training_load", "training_load", "tss"])
                .map(|load| load > NDLI_RUNNING_TSS_PROXY_THRESHOLD)
                .unwrap_or(false)
        };

        if is_high {
            high_intensity_days += 1;
        }

        // Collect mean IF, EF, VI
        if let Some(if_val) = get_number(detail, &["icu_intensity_factor", "intensity_factor"]) {
            let normalized = if if_val > NDLI_IF_NORMALIZATION_THRESHOLD {
                if_val / PCT_SCALING_FACTOR
            } else {
                if_val
            };
            if_values.push(normalized);
        }
        if let Some(ef) = get_number(detail, &["icu_efficiency_factor", "efficiency_factor"]) {
            ef_values.push(ef);
        }
        if let Some(vi) = get_number(detail, &["icu_variability_index", "variability_index"]) {
            vi_values.push(vi);
        }
    }

    if days_with_data == 0 {
        return NdliMetrics {
            supported: false,
            ..Default::default()
        };
    }

    let mean_if = if if_values.is_empty() {
        None
    } else {
        Some(if_values.iter().sum::<f64>() / if_values.len() as f64)
    };
    let mean_ef = if ef_values.is_empty() {
        None
    } else {
        Some(ef_values.iter().sum::<f64>() / ef_values.len() as f64)
    };
    let mean_vi = if vi_values.is_empty() {
        None
    } else {
        Some(vi_values.iter().sum::<f64>() / vi_values.len() as f64)
    };

    let ndli_state = if high_intensity_days >= NDLI_RED_DAYS {
        "red".to_string()
    } else if high_intensity_days == NDLI_AMBER_DAYS {
        "amber".to_string()
    } else {
        "green".to_string()
    };

    NdliMetrics {
        supported: true,
        high_intensity_days_7d: high_intensity_days,
        mean_intensity_factor_7d: mean_if,
        mean_efficiency_factor_7d: mean_ef,
        mean_variability_index_7d: mean_vi,
        ndli_state,
        ndli_overload_flag: high_intensity_days >= NDLI_RED_DAYS,
    }
}
