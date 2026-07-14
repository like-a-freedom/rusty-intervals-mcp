use crate::engines::coach_metrics_constants::*;

// =============================================================================
// P2.4 — TID Classifier
// =============================================================================

/// Classify TID model from zone percentages.
/// pyramidal: z1 > z2 > z3, threshold: z2 dominant, polarized: z1 + z3 dominant.
pub fn classify_tid_model(z1_pct: f64, z2_pct: f64, z3_pct: f64) -> (String, Option<f64>) {
    let polarization_index = if z2_pct > 0.0 {
        Some(z3_pct / z2_pct)
    } else {
        None
    };

    let tid_model = if z2_pct > z1_pct && z2_pct > z3_pct {
        "threshold".to_string()
    } else if z1_pct > z2_pct && z1_pct > z3_pct && z2_pct > z3_pct {
        "pyramidal".to_string()
    } else {
        "polarized".to_string()
    };

    (tid_model, polarization_index)
}

// =============================================================================
// P0.3a — Z2 HR Stability
// =============================================================================

/// Compute HR variance within Z2 bounds from stream data.
/// Returns None if fewer than 10 Z2 points are available.
pub fn compute_z2_hr_variance(hr_stream: &[f64], z2_lower: f64, z2_upper: f64) -> Option<f64> {
    let z2_points: Vec<f64> = hr_stream
        .iter()
        .copied()
        .filter(|hr| *hr >= z2_lower && *hr <= z2_upper)
        .collect();

    if z2_points.len() < Z2_MIN_POINTS {
        return None;
    }

    let mean = z2_points.iter().sum::<f64>() / z2_points.len() as f64;
    let variance = z2_points
        .iter()
        .map(|hr| {
            let diff = hr - mean;
            diff * diff
        })
        .sum::<f64>()
        / z2_points.len() as f64;

    Some(variance)
}
