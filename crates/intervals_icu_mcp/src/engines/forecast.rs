//! Banister impulse-response TSB projection and taper efficiency.
//! Source: Banister impulse-response model (standard CTL/ATL time constants).

// =============================================================================
// Forecast Constants
// =============================================================================

use crate::engines::constants::{TSB_FRESH, TSB_FRESH_UPPER, TSB_LOAD_PRESSURE, TSB_OVERREACHED};

/// CTL time constant (42 days = 6 weeks half-life).
/// Source: Banister model — standard endurance sports value.
const CTL_TIME_CONSTANT: f64 = 42.0;

/// ATL time constant (7 days = 1 week half-life).
const ATL_TIME_CONSTANT: f64 = 7.0;

/// CTL-based load multipliers for personalized TSS estimation.
/// Source: Montis.icu Coach V5 — easy ~0.5×CTL, tempo ~1.0×CTL, hard ~1.5×CTL, race ~2.5×CTL.
const LOAD_MULTIPLIER_EASY: f64 = 0.5;
const LOAD_MULTIPLIER_TEMPO: f64 = 1.0;
const LOAD_MULTIPLIER_HARD: f64 = 1.5;
const LOAD_MULTIPLIER_RACE: f64 = 2.5;
const LOAD_MULTIPLIER_FALLBACK: f64 = 0.75;

/// Taper efficiency clamp bounds.
/// Source: Banister taper model — minimum efficiency floor (no supercompensation).
const TAPER_EFFICIENCY_MIN: f64 = 0.0;
/// Source: Banister taper model — max efficiency ceiling (empirical 2× baseline).
const TAPER_EFFICIENCY_MAX: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TsbProjection {
    pub day: i32,
    pub ctl: f64,
    pub atl: f64,
    pub tsb: f64,
    pub fatigue_class: &'static str,
}

/// Project TSB forward N days using Banister impulse-response model.
/// `current_ctl` / `current_atl`: current CTL and ATL values.
/// `daily_loads`: planned TSS loads for each future day.
pub fn project_tsb(current_ctl: f64, current_atl: f64, daily_loads: &[f64]) -> Vec<TsbProjection> {
    let mut ctl = current_ctl;
    let mut atl = current_atl;
    let mut results = Vec::with_capacity(daily_loads.len());

    for (day, &load) in daily_loads.iter().enumerate() {
        ctl = ctl + (load - ctl) / CTL_TIME_CONSTANT;
        atl = atl + (load - atl) / ATL_TIME_CONSTANT;
        let tsb = ctl - atl;

        let fatigue_class = if tsb < TSB_OVERREACHED {
            "overreached"
        } else if tsb < TSB_LOAD_PRESSURE {
            "load_pressure"
        } else if tsb < TSB_FRESH {
            "balanced"
        } else if tsb < TSB_FRESH_UPPER {
            "fresh"
        } else {
            "transition"
        };

        results.push(TsbProjection {
            day: day as i32 + 1,
            ctl,
            atl,
            tsb,
            fatigue_class,
        });
    }

    results
}

/// Parameterized load values by intensity, scaled from athlete's CTL.
/// Multipliers: easy = 0.5×CTL, tempo = 1.0×CTL, hard = 1.5×CTL, race = 2.5×CTL.
/// CTL floor at 1.0 to avoid degenerate zero loads.
pub fn parameterized_load(intensity: &str, ctl: f64) -> f64 {
    let ctl = ctl.max(1.0);
    match intensity {
        "easy" => ctl * LOAD_MULTIPLIER_EASY,
        "tempo" => ctl * LOAD_MULTIPLIER_TEMPO,
        "hard" => ctl * LOAD_MULTIPLIER_HARD,
        "race" => ctl * LOAD_MULTIPLIER_RACE,
        _ => ctl * LOAD_MULTIPLIER_FALLBACK,
    }
}

/// Compute taper efficiency.
/// `actual_volume_reduction_pct`: actual % volume reduction achieved.
/// `target_volume_reduction_pct`: target % volume reduction planned.
/// `tsb_gain`: TSB gain during taper period.
pub fn compute_taper_efficiency(
    actual_volume_reduction_pct: f64,
    target_volume_reduction_pct: f64,
    tsb_gain: f64,
) -> (f64, f64) {
    let efficiency = if target_volume_reduction_pct > 0.0 {
        (actual_volume_reduction_pct / target_volume_reduction_pct)
            .clamp(TAPER_EFFICIENCY_MIN, TAPER_EFFICIENCY_MAX)
    } else {
        1.0
    };
    let tsb_response = if actual_volume_reduction_pct > 0.0 {
        tsb_gain / actual_volume_reduction_pct
    } else {
        0.0
    };
    (efficiency, tsb_response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsb_projection_stable_load() {
        let results = project_tsb(50.0, 50.0, &[50.0; 7]);
        // With equal load, CTL and ATL should converge to 50.0
        assert!((results.last().unwrap().tsb).abs() < 1.0);
    }

    #[test]
    fn tsb_projection_hard_load_dips_tsb() {
        let results = project_tsb(50.0, 50.0, &[150.0; 7]);
        // High load should cause ATL to rise faster than CTL → negative TSB
        assert!(results.last().unwrap().tsb < -5.0);
    }

    #[test]
    fn tsb_projection_easy_load_raises_tsb() {
        let results = project_tsb(50.0, 60.0, &[30.0; 7]);
        // Low load with elevated ATL should let ATL drop faster → TSB rises
        assert!(results.last().unwrap().tsb > 0.0);
    }

    #[test]
    fn tsb_fatigue_classification() {
        let results = project_tsb(50.0, 100.0, &[150.0; 14]);
        let last = results.last().unwrap();
        assert_eq!(last.fatigue_class, "overreached");
    }

    #[test]
    fn parameterized_load_values_at_ctl_100() {
        assert!((parameterized_load("easy", 100.0) - 50.0).abs() < 0.01);
        assert!((parameterized_load("tempo", 100.0) - 100.0).abs() < 0.01);
        assert!((parameterized_load("hard", 100.0) - 150.0).abs() < 0.01);
        assert!((parameterized_load("race", 100.0) - 250.0).abs() < 0.01);
    }

    #[test]
    fn parameterized_load_scales_with_ctl() {
        assert!((parameterized_load("easy", 50.0) - 25.0).abs() < 0.01);
        assert!((parameterized_load("hard", 120.0) - 180.0).abs() < 0.01);
    }

    #[test]
    fn parameterized_load_fallback_for_unknown() {
        let val = parameterized_load("unknown", 100.0);
        assert!((val - 75.0).abs() < 0.01);
    }

    #[test]
    fn parameterized_load_zero_ctl_uses_floor() {
        assert!((parameterized_load("easy", 0.0) - 0.5).abs() < 0.01);
    }

    #[test]
    fn taper_efficiency_perfect() {
        let (efficiency, response) = compute_taper_efficiency(40.0, 40.0, 15.0);
        assert!((efficiency - 1.0).abs() < 0.01);
        assert!((response - 0.375).abs() < 0.01);
    }

    #[test]
    fn taper_efficiency_partial() {
        let (efficiency, _) = compute_taper_efficiency(20.0, 40.0, 8.0);
        assert!((efficiency - 0.5).abs() < 0.01);
    }
}
