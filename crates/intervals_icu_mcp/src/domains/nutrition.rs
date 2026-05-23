use serde::{Deserialize, Serialize};

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
/// 1-2h: 5-7 g/kg, 2-3h: 7-9 g/kg, >3h: 9-10 g/kg.
/// IF adjustment: >0.85 → +0.5, <0.65 → -0.5.
pub fn compute_carb_demand(duration_hours: f64, intensity_factor: Option<f64>) -> f64 {
    let base: f64 = if duration_hours <= 2.0 {
        6.0
    } else if duration_hours <= 3.0 {
        8.0
    } else {
        9.5
    };
    let adjustment: f64 = intensity_factor
        .map(|if_val| {
            if if_val > 0.85 {
                0.5_f64
            } else if if_val < 0.65 {
                -0.5_f64
            } else {
                0.0_f64
            }
        })
        .unwrap_or(0.0);
    (base + adjustment).clamp(3.0_f64, 12.0_f64)
}

/// Compute protein demand. Baseline 1.6 g/kg, adaptation pressure → 2.0 g/kg.
pub fn compute_protein_demand(adaptation_pressure: bool) -> f64 {
    if adaptation_pressure { 2.0 } else { 1.6 }
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
        (Some(carbs), _) if carbs < carb_demand * 0.5 => "severely_underfuelled".to_string(),
        (Some(carbs), _) if carbs < carb_demand * 0.8 => "underfuelled".to_string(),
        (Some(carbs), _) if carbs > carb_demand * 1.2 => "overfuelled".to_string(),
        (_, Some(protein)) if protein < protein_demand * 0.7 => "underfuelled".to_string(),
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
