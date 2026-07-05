//! Banister impulse-response TSB projection.
//! Source: Banister impulse-response model (standard CTL/ATL time constants).

// =============================================================================
// Forecast Constants
// =============================================================================

/// CTL time constant (42 days = 6 weeks half-life).
/// Source: Banister model — standard endurance sports value.
const CTL_TIME_CONSTANT: f64 = 42.0;

/// ATL time constant (7 days = 1 week half-life).
const ATL_TIME_CONSTANT: f64 = 7.0;

/// TSB fatigue classification thresholds.
const TSB_OVERREACHED: f64 = -30.0;
const TSB_LOAD_PRESSURE: f64 = -10.0;
const TSB_BALANCED_UPPER: f64 = 10.0;
const TSB_FRESH_UPPER: f64 = 25.0;

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
        } else if tsb < TSB_BALANCED_UPPER {
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
    fn tsb_projection_with_empty_loads_returns_empty_vec() {
        let results = project_tsb(50.0, 50.0, &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn tsb_projection_with_single_day_load_returns_single_projection() {
        let results = project_tsb(50.0, 50.0, &[100.0]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].day, 1);
        // First day: ctl=50 + (100-50)/42, atl=50 + (100-50)/7
        // ctl ≈ 51.19, atl ≈ 57.14, tsb ≈ -5.95
        assert!(results[0].tsb < 0.0);
    }

    #[test]
    fn tsb_projection_day_index_starts_at_one_and_increments() {
        let results = project_tsb(50.0, 50.0, &[100.0, 100.0, 100.0]);
        assert_eq!(results[0].day, 1);
        assert_eq!(results[1].day, 2);
        assert_eq!(results[2].day, 3);
    }

    #[test]
    fn tsb_projection_balanced_class_with_zero_loads() {
        let results = project_tsb(0.0, 0.0, &[0.0; 5]);
        for projection in &results {
            assert!((projection.tsb).abs() < 1e-9);
            assert_eq!(projection.ctl, 0.0);
            assert_eq!(projection.atl, 0.0);
        }
    }

    #[test]
    fn tsb_production_classification_load_pressure_band() {
        // TSB in -10 to 10 range is "balanced", below -10 is "load_pressure"
        let results = project_tsb(100.0, 100.0, &[50.0; 1]);
        // day 1: ctl=100+(50-100)/42≈98.81, atl=100+(50-100)/7≈92.86, tsb≈5.95
        assert_eq!(results[0].fatigue_class, "balanced");
    }

    #[test]
    fn tsb_production_classification_fresh_band() {
        // TSB in 10 to 25 is "fresh"
        let results = project_tsb(60.0, 40.0, &[30.0; 14]);
        let last = results.last().unwrap();
        // ATL drops faster than CTL, so TSB rises into the [10, 25) "fresh" band
        assert_eq!(last.fatigue_class, "fresh");
        assert!(last.tsb >= 10.0 && last.tsb < 25.0);
    }

    #[test]
    fn tsb_production_classification_transition_band() {
        // TSB > 25 is "transition"
        let results = project_tsb(100.0, 50.0, &[0.0; 21]);
        let last = results.last().unwrap();
        assert_eq!(last.fatigue_class, "transition");
    }
}
