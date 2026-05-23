/// Banister impulse-response TSB projection and taper efficiency.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TsbProjection {
    pub day: i32,
    pub ctl: f64,
    pub atl: f64,
    pub tsb: f64,
    pub fatigue_class: &'static str,
}

const CTL_TIME_CONSTANT: f64 = 42.0;
const ATL_TIME_CONSTANT: f64 = 7.0;

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

        let fatigue_class = if tsb < -30.0 {
            "overreached"
        } else if tsb < -10.0 {
            "load_pressure"
        } else if tsb < 10.0 {
            "balanced"
        } else if tsb < 25.0 {
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

/// Parameterized load values by intensity.
pub fn parameterized_load(intensity: &str) -> f64 {
    match intensity {
        "easy" => 50.0,
        "tempo" => 100.0,
        "hard" => 150.0,
        "race" => 250.0,
        _ => 75.0,
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
        (actual_volume_reduction_pct / target_volume_reduction_pct).clamp(0.0, 2.0)
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
    fn parameterized_load_values() {
        assert!((parameterized_load("easy") - 50.0).abs() < 0.01);
        assert!((parameterized_load("tempo") - 100.0).abs() < 0.01);
        assert!((parameterized_load("hard") - 150.0).abs() < 0.01);
        assert!((parameterized_load("race") - 250.0).abs() < 0.01);
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
