use crate::domains::coach::PolarisationMetrics;
use crate::engines::coach_metrics_constants::*;
use crate::engines::shared::compute_zone_distribution;
use serde_json::Value;

use super::helpers::get_number;

pub fn compute_tid_entropy(z1_pct: f64, z2_pct: f64, z3_pct: f64) -> Option<f64> {
    let total = z1_pct + z2_pct + z3_pct;
    if total <= f64::EPSILON {
        return None;
    }

    let normalized = [z1_pct / total, z2_pct / total, z3_pct / total];
    let entropy = normalized
        .iter()
        .copied()
        .filter(|value| *value > f64::EPSILON)
        .map(|value| -value * value.log2())
        .sum();
    Some(entropy)
}
pub fn compute_polarisation(
    z1_pct: f64,
    z2_pct: f64,
    z3_pct: f64,
) -> Option<crate::domains::coach::PolarisationMetrics> {
    let ratio = if z2_pct.abs() < f64::EPSILON {
        None
    } else {
        Some((z1_pct + z3_pct) / (POLARISATION_DENOMINATOR_FACTOR * z2_pct))
    };

    let state = ratio.map(|r| {
        if r < POLARISATION_BIASED_THRESHOLD {
            "threshold_biased".to_string()
        } else if r <= 1.0 {
            "polarised".to_string()
        } else {
            "high_intensity_dominant".to_string()
        }
    });

    Some(PolarisationMetrics {
        z1_pct: Some(z1_pct),
        z2_pct: Some(z2_pct),
        z3_pct: Some(z3_pct),
        ratio,
        state,
        polarization_index: None,
        tid_model: None,
    })
}

pub fn parse_polarisation_from_api(
    activity_detail: Option<&Value>,
    zone_times: Option<&Value>,
) -> Option<crate::domains::coach::PolarisationMetrics> {
    // Priority 1: Use pre-computed polarization_index from API
    if let Some(detail) = activity_detail.and_then(|v| v.as_object())
        && let Some(index) = get_number(detail, &["polarization_index"])
    {
        let state = if index < POLARISATION_BIASED_THRESHOLD {
            Some("threshold_biased".to_string())
        } else if index <= 1.0 {
            Some("polarised".to_string())
        } else {
            Some("high_intensity_dominant".to_string())
        };
        return Some(PolarisationMetrics {
            z1_pct: None,
            z2_pct: None,
            z3_pct: None,
            ratio: Some(index),
            state,
            polarization_index: Some(index),
            tid_model: None,
        });
    }

    // Priority 2: Aggregate from icu_zone_times
    // Uses canonical name-based Seiler mapping (Z1+Z2→easy, Z3→threshold, Z4+→high)
    if let Some(zt) = zone_times
        && let Some((z1_pct, z2_pct, z3_pct)) = compute_zone_distribution(zt)
    {
        return compute_polarisation(z1_pct, z2_pct, z3_pct);
    }

    None
}

pub fn compute_consistency_index(
    sessions_completed: usize,
    sessions_planned: usize,
) -> crate::domains::coach::ConsistencyMetrics {
    use crate::domains::coach::ConsistencyMetrics;

    let ratio = if sessions_planned == 0 {
        None
    } else {
        Some(sessions_completed as f64 / sessions_planned as f64)
    };

    let state = ratio.map(|r| {
        if r >= CONSISTENCY_EXCELLENT_THRESHOLD {
            "excellent".to_string()
        } else if r >= CONSISTENCY_GOOD_THRESHOLD {
            "good".to_string()
        } else if r >= CONSISTENCY_MODERATE_THRESHOLD {
            "moderate".to_string()
        } else {
            "low".to_string()
        }
    });

    ConsistencyMetrics {
        sessions_planned,
        sessions_completed,
        ratio,
        state,
    }
}
