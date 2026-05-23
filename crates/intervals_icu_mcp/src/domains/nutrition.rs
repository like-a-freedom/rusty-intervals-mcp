use serde::{Deserialize, Serialize};

// =============================================================================
// Nutrition Constants
// Sources: ACSM, ISSN, Thomas et al. JISSN 2016, Kerksick et al. JISSN 2018
// =============================================================================

/// Duration threshold between short and moderate (hours).
const DURATION_MODERATE_HOURS: f64 = 2.0;

/// Duration threshold between moderate and long (hours).
const DURATION_LONG_HOURS: f64 = 3.0;

/// Carb demand for short efforts (1-2h): g/kg.
const CARB_SHORT_G_PER_KG: f64 = 6.0;

/// Carb demand for moderate efforts (2-3h): g/kg.
const CARB_MODERATE_G_PER_KG: f64 = 8.0;

/// Carb demand for long efforts (>3h): g/kg.
const CARB_LONG_G_PER_KG: f64 = 9.5;

/// Intensity factor threshold for upward carb adjustment.
const IF_HIGH_THRESHOLD: f64 = 0.85;

/// Intensity factor threshold for downward carb adjustment.
const IF_LOW_THRESHOLD: f64 = 0.65;

/// Carb adjustment when IF > high threshold (g/kg).
const CARB_UP_ADJUSTMENT: f64 = 0.5;

/// Carb adjustment when IF < low threshold (g/kg).
const CARB_DOWN_ADJUSTMENT: f64 = -0.5;

/// Minimum carb demand clamp (g/kg).
const CARB_MIN_CLAMP: f64 = 3.0;

/// Maximum carb demand clamp (g/kg).
const CARB_MAX_CLAMP: f64 = 12.0;

/// Protein demand under adaptation pressure (g/kg).
const PROTEIN_ADAPTATION_G_PER_KG: f64 = 2.0;

/// Protein demand baseline (g/kg).
const PROTEIN_BASELINE_G_PER_KG: f64 = 1.6;

/// Carb fraction threshold for severely underfuelled.
const CARB_SEVERE_FRACTION: f64 = 0.5;

/// Carb fraction threshold for underfuelled.
const CARB_UNDER_FRACTION: f64 = 0.8;

/// Carb fraction threshold for overfuelled.
const CARB_OVER_FRACTION: f64 = 1.2;

/// Protein fraction threshold for underfuelled.
const PROTEIN_UNDER_FRACTION: f64 = 0.7;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NutritionDemand {
    pub supported: bool,
    pub unsupported_reason: String,
    pub carb_demand_g_kg: Option<f64>,
    pub protein_demand_g_kg: Option<f64>,
    pub nutrition_state: String,
    pub carb_delta: Option<f64>,
    pub protein_delta: Option<f64>,
}

impl NutritionDemand {
    pub fn unsupported(reason: &str) -> Self {
        Self {
            supported: false,
            unsupported_reason: reason.to_string(),
            carb_demand_g_kg: None,
            protein_demand_g_kg: None,
            nutrition_state: "unknown".to_string(),
            carb_delta: None,
            protein_delta: None,
        }
    }
}

/// Compute carbohydrate demand based on duration and intensity.
/// Sources: Thomas et al. JISSN 2016, Kerksick et al. JISSN 2018.
/// 1-2h: 5-7 g/kg, 2-3h: 7-9 g/kg, >3h: 9-10 g/kg.
/// IF adjustment: >0.85 → +0.5, <0.65 → -0.5.
pub fn compute_carb_demand(duration_hours: f64, intensity_factor: Option<f64>) -> f64 {
    let base: f64 = if duration_hours <= DURATION_MODERATE_HOURS {
        CARB_SHORT_G_PER_KG
    } else if duration_hours <= DURATION_LONG_HOURS {
        CARB_MODERATE_G_PER_KG
    } else {
        CARB_LONG_G_PER_KG
    };
    let adjustment: f64 = intensity_factor
        .map(|if_val| {
            if if_val > IF_HIGH_THRESHOLD {
                CARB_UP_ADJUSTMENT
            } else if if_val < IF_LOW_THRESHOLD {
                CARB_DOWN_ADJUSTMENT
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);
    (base + adjustment).clamp(CARB_MIN_CLAMP, CARB_MAX_CLAMP)
}

/// Compute protein demand. Baseline 1.6 g/kg, adaptation pressure → 2.0 g/kg.
/// Source: Kerksick et al. JISSN 2018.
pub fn compute_protein_demand(adaptation_pressure: bool) -> f64 {
    if adaptation_pressure {
        PROTEIN_ADAPTATION_G_PER_KG
    } else {
        PROTEIN_BASELINE_G_PER_KG
    }
}

/// Compute nutrition balance from wellness data over a rolling window.
/// Status: severely_underfuelled, underfuelled, balanced, overfuelled.
pub fn compute_nutrition_balance(
    actual_carbs_g_kg: Option<f64>,
    carb_demand: f64,
    actual_protein_g_kg: Option<f64>,
    protein_demand: f64,
) -> String {
    match (actual_carbs_g_kg, actual_protein_g_kg) {
        (None, None) => "unknown".to_string(),
        (Some(carbs), _) if carbs < carb_demand * CARB_SEVERE_FRACTION => {
            "severely_underfuelled".to_string()
        }
        (Some(carbs), _) if carbs < carb_demand * CARB_UNDER_FRACTION => {
            "underfuelled".to_string()
        }
        (Some(carbs), _) if carbs > carb_demand * CARB_OVER_FRACTION => "overfuelled".to_string(),
        (_, Some(protein)) if protein < protein_demand * PROTEIN_UNDER_FRACTION => {
            "underfuelled".to_string()
        }
        _ => "balanced".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carb_demand_short_duration() {
        let demand = compute_carb_demand(1.5, None);
        assert!((demand - 6.0).abs() < 0.01);
    }

    #[test]
    fn carb_demand_long_duration() {
        let demand = compute_carb_demand(4.0, None);
        assert!((demand - 9.5).abs() < 0.01);
    }

    #[test]
    fn carb_demand_intensity_adjustment_up() {
        let demand = compute_carb_demand(2.5, Some(0.90));
        assert!((demand - 8.5).abs() < 0.01);
    }

    #[test]
    fn carb_demand_intensity_adjustment_down() {
        let demand = compute_carb_demand(2.5, Some(0.60));
        assert!((demand - 7.5).abs() < 0.01);
    }

    #[test]
    fn protein_demand_baseline() {
        let demand = compute_protein_demand(false);
        assert!((demand - 1.6).abs() < 0.01);
    }

    #[test]
    fn protein_demand_adaptation() {
        let demand = compute_protein_demand(true);
        assert!((demand - 2.0).abs() < 0.01);
    }

    #[test]
    fn nutrition_balance_underfuelled() {
        let state = compute_nutrition_balance(Some(5.0), 8.0, Some(1.0), 1.6);
        assert_eq!(state, "underfuelled");
    }

    #[test]
    fn nutrition_balance_balanced() {
        let state = compute_nutrition_balance(Some(7.0), 8.0, Some(1.5), 1.6);
        assert_eq!(state, "balanced");
    }

    #[test]
    fn nutrition_balance_unsupported() {
        let nd = NutritionDemand::unsupported("no nutrition tracking data");
        assert!(!nd.supported);
        assert_eq!(nd.unsupported_reason, "no nutrition tracking data");
    }
}
