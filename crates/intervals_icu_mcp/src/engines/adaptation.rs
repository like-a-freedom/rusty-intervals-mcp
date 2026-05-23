#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AdaptationState {
    Baseline,
    FatigueState,
    Vo2Expansion,
    AerobicConsolidation,
    AnaerobicBuild,
    MixedAdaptation,
    Plateau,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CurveProfile {
    TimeTrialist,
    EnduranceSpecialist,
    AllRounder,
    PunchyClimber,
    Punchy,
    AnaerobicSpecialist,
    Sprinter,
    EnduranceRunner,
    BalancedRunner,
    PunchyRunner,
    SpeedRunner,
}

/// Classify adaptation state from 2-window power-curve deltas.
/// `thr_delta` = % change in threshold power (≈20-60min power)
/// `vo2_delta` = % change in VO2max power (≈5min power)
/// `dur_delta` = % change in endurance power (≈60min+ power)
/// `neural_delta` = % change in short sprint power (≈5s-1min)
/// `ana_1m_delta` = % change in 1-minute anaerobic power
pub fn classify_adaptation(
    thr_delta: Option<f64>,
    vo2_delta: Option<f64>,
    dur_delta: Option<f64>,
    neural_delta: Option<f64>,
    ana_1m_delta: Option<f64>,
) -> AdaptationState {
    let all_some =
        thr_delta.is_some() && vo2_delta.is_some() && dur_delta.is_some() && neural_delta.is_some();

    if !all_some {
        return AdaptationState::Baseline;
    }

    let thr = thr_delta.unwrap();
    let vo2 = vo2_delta.unwrap();
    let dur = dur_delta.unwrap();
    let neural = neural_delta.unwrap();
    let ana_1m = ana_1m_delta.unwrap_or(0.0);

    // Plateau: all deltas < 1%
    if thr.abs() < 1.0 && vo2.abs() < 1.0 && dur.abs() < 1.0 && neural.abs() < 1.0 {
        return AdaptationState::Plateau;
    }

    // FatigueState: thr < -3% && vo2 < -3%
    if thr < -3.0 && vo2 < -3.0 {
        return AdaptationState::FatigueState;
    }

    // Vo2Expansion: vo2_delta > 3%
    if vo2 > 3.0 {
        return AdaptationState::Vo2Expansion;
    }

    // AerobicConsolidation: thr_delta > 1% && dur_delta > 2%
    if thr > 1.0 && dur > 2.0 {
        return AdaptationState::AerobicConsolidation;
    }

    // AnaerobicBuild: neural_delta > 5% && ana_1m_delta > 2%
    if neural > 5.0 && ana_1m > 2.0 {
        return AdaptationState::AnaerobicBuild;
    }

    // MixedAdaptation: conflicting signals
    if (neural > 5.0 && dur < -2.0) || (vo2 > 3.0 && thr < -1.0) {
        return AdaptationState::MixedAdaptation;
    }

    AdaptationState::Baseline
}

/// Classify curve profile from MMP anchor points.
/// `p5s`..`p60m` = best power at each duration.
/// `is_running` = use running-specific profiles.
pub fn classify_curve_profile(
    p5s: Option<f64>,
    p1m: Option<f64>,
    p5m: Option<f64>,
    p20m: Option<f64>,
    p60m: Option<f64>,
    is_running: bool,
) -> CurveProfile {
    let (Some(p5s), Some(p1m), Some(p5m), Some(p20m), Some(p60m)) = (p5s, p1m, p5m, p20m, p60m)
    else {
        return if is_running {
            CurveProfile::BalancedRunner
        } else {
            CurveProfile::AllRounder
        };
    };

    if is_running {
        // Running profiles based on pace-curve slope
        let endurance_slope = (p60m - p20m) / 40.0;
        let power_slope = (p5m - p1m) / 4.0;

        if endurance_slope.abs() < 1.0 && power_slope > 2.0 {
            CurveProfile::SpeedRunner
        } else if endurance_slope.abs() < 1.0 && power_slope <= 2.0 {
            CurveProfile::EnduranceRunner
        } else if endurance_slope < -2.0 && power_slope > 3.0 {
            CurveProfile::PunchyRunner
        } else {
            CurveProfile::BalancedRunner
        }
    } else {
        // Cycling profiles based on power-duration curve shape
        let sprint_ratio = p5s / p1m;
        let anaerobic_ratio = p1m / p5m;
        let aerobic_ratio = p5m / p20m;
        let endurance_ratio = p20m / p60m;

        if sprint_ratio > 1.5 && anaerobic_ratio > 1.3 {
            CurveProfile::Sprinter
        } else if sprint_ratio > 1.3 && anaerobic_ratio > 1.2 {
            CurveProfile::AnaerobicSpecialist
        } else if endurance_ratio > 1.15 && aerobic_ratio > 1.1 {
            CurveProfile::EnduranceSpecialist
        } else if sprint_ratio > 1.2 && anaerobic_ratio > 1.1 && endurance_ratio < 1.05 {
            CurveProfile::PunchyClimber
        } else if anaerobic_ratio > 1.15 && endurance_ratio < 1.1 {
            CurveProfile::Punchy
        } else if aerobic_ratio > 1.2 && endurance_ratio > 1.1 {
            CurveProfile::TimeTrialist
        } else {
            CurveProfile::AllRounder
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptation_baseline_when_no_data() {
        assert_eq!(
            classify_adaptation(None, None, None, None, None),
            AdaptationState::Baseline
        );
    }

    #[test]
    fn adaptation_fatigue_state() {
        assert_eq!(
            classify_adaptation(Some(-5.0), Some(-4.0), Some(-2.0), Some(-1.0), None),
            AdaptationState::FatigueState
        );
    }

    #[test]
    fn adaptation_vo2_expansion() {
        assert_eq!(
            classify_adaptation(Some(1.0), Some(5.0), Some(0.5), Some(2.0), None),
            AdaptationState::Vo2Expansion
        );
    }

    #[test]
    fn adaptation_aerobic_consolidation() {
        assert_eq!(
            classify_adaptation(Some(2.0), Some(1.0), Some(3.0), Some(1.0), None),
            AdaptationState::AerobicConsolidation
        );
    }

    #[test]
    fn adaptation_anaerobic_build() {
        assert_eq!(
            classify_adaptation(Some(0.5), Some(1.0), Some(0.5), Some(7.0), Some(3.0)),
            AdaptationState::AnaerobicBuild
        );
    }

    #[test]
    fn adaptation_plateau() {
        assert_eq!(
            classify_adaptation(Some(0.5), Some(0.3), Some(0.4), Some(0.2), None),
            AdaptationState::Plateau
        );
    }

    #[test]
    fn adaptation_mixed_when_conflicting() {
        // neural > 5% + dur < -2% should trigger MixedAdaptation
        assert_eq!(
            classify_adaptation(Some(0.5), Some(1.0), Some(-3.0), Some(6.0), None),
            AdaptationState::MixedAdaptation
        );
    }

    #[test]
    fn curve_profile_all_rounder_default() {
        assert_eq!(
            classify_curve_profile(None, None, None, None, None, false),
            CurveProfile::AllRounder
        );
    }

    #[test]
    fn curve_profile_sprinter() {
        assert_eq!(
            classify_curve_profile(
                Some(1200.0),
                Some(700.0),
                Some(400.0),
                Some(300.0),
                Some(250.0),
                false
            ),
            CurveProfile::Sprinter
        );
    }

    #[test]
    fn curve_profile_endurance_specialist() {
        assert_eq!(
            classify_curve_profile(
                Some(600.0),
                Some(450.0),
                Some(380.0),
                Some(320.0),
                Some(250.0),
                false
            ),
            CurveProfile::EnduranceSpecialist
        );
    }

    #[test]
    fn curve_profile_balanced_runner_default() {
        assert_eq!(
            classify_curve_profile(None, None, None, None, None, true),
            CurveProfile::BalancedRunner
        );
    }
}
