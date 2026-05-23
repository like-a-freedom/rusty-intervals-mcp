// =============================================================================
// Adaptation State & Curve Profile Constants
// =============================================================================

/// Adaptation: Plateau threshold — all deltas below this % indicate no meaningful change.
const ADAPTATION_PLATEAU_THRESHOLD_PCT: f64 = 1.0;

/// Adaptation: Fatigue state — threshold % decline for threshold power.
const ADAPTATION_FATIGUE_THR_THRESHOLD: f64 = -3.0;

/// Adaptation: Fatigue state — threshold % decline for VO2max power.
const ADAPTATION_FATIGUE_VO2_THRESHOLD: f64 = -3.0;

/// Adaptation: VO2 expansion — threshold % gain.
const ADAPTATION_VO2_EXPANSION_THRESHOLD: f64 = 3.0;

/// Adaptation: Aerobic consolidation — threshold % gain for threshold power.
const ADAPTATION_AEROBIC_THR_THRESHOLD: f64 = 1.0;

/// Adaptation: Aerobic consolidation — threshold % gain for endurance power.
const ADAPTATION_AEROBIC_DUR_THRESHOLD: f64 = 2.0;

/// Adaptation: Anaerobic build — threshold % gain for neural/sprint power.
const ADAPTATION_ANAEROBIC_NEURAL_THRESHOLD: f64 = 5.0;

/// Adaptation: Anaerobic build — threshold % gain for 1-minute power.
const ADAPTATION_ANAEROBIC_1M_THRESHOLD: f64 = 2.0;

/// Adaptation: Mixed adaptation — neural gain threshold.
const ADAPTATION_MIXED_NEURAL_THRESHOLD: f64 = 5.0;

/// Adaptation: Mixed adaptation — endurance decline threshold.
const ADAPTATION_MIXED_DUR_DECLINE: f64 = -2.0;

/// Adaptation: Mixed adaptation — VO2 gain threshold.
const ADAPTATION_MIXED_VO2_THRESHOLD: f64 = 3.0;

/// Adaptation: Mixed adaptation — threshold decline.
const ADAPTATION_MIXED_THR_DECLINE: f64 = -1.0;

/// Curve Profile: duration span for endurance slope (20m to 60m = 40 min).
const CURVE_ENDURANCE_SLOPE_SPAN_MIN: f64 = 40.0;

/// Curve Profile: duration span for power slope (1m to 5m = 4 min).
const CURVE_POWER_SLOPE_SPAN_MIN: f64 = 4.0;

/// Curve Profile: Running — endurance slope threshold for flat profile.
const RUNNING_ENDURANCE_FLAT_THRESHOLD: f64 = 1.0;

/// Curve Profile: Running — power slope threshold for speed profile.
const RUNNING_POWER_SPEED_THRESHOLD: f64 = 2.0;

/// Curve Profile: Running — punchy runner endurance decline threshold.
const RUNNING_PUNCHY_ENDURANCE_DECLINE: f64 = -2.0;

/// Curve Profile: Running — punchy runner power threshold.
const RUNNING_PUNCHY_POWER_THRESHOLD: f64 = 3.0;

/// Curve Profile: Cycling — sprint ratio threshold (P5s/P1m).
const CYCLING_SPRINT_RATIO_THRESHOLD: f64 = 1.5;

/// Curve Profile: Cycling — anaerobic ratio threshold (P1m/P5m).
const CYCLING_ANAEROBIC_RATIO_THRESHOLD: f64 = 1.3;

/// Curve Profile: Cycling — moderate sprint ratio.
const CYCLING_MODERATE_SPRINT_RATIO: f64 = 1.3;

/// Curve Profile: Cycling — moderate anaerobic ratio.
const CYCLING_MODERATE_ANAEROBIC_RATIO: f64 = 1.2;

/// Curve Profile: Cycling — endurance specialist endurance ratio (P20m/P60m).
const CYCLING_ENDURANCE_RATIO_THRESHOLD: f64 = 1.15;

/// Curve Profile: Cycling — endurance specialist aerobic ratio (P5m/P20m).
const CYCLING_ENDURANCE_AEROBIC_RATIO: f64 = 1.1;

/// Curve Profile: Cycling — punchy climber sprint ratio.
const CYCLING_PUNCHY_CLIMBER_SPRINT: f64 = 1.2;

/// Curve Profile: Cycling — punchy climber anaerobic ratio.
const CYCLING_PUNCHY_CLIMBER_ANAEROBIC: f64 = 1.1;

/// Curve Profile: Cycling — punchy climber endurance ratio.
const CYCLING_PUNCHY_CLIMBER_ENDURANCE: f64 = 1.05;

/// Curve Profile: Cycling — punchy anaerobic ratio.
const CYCLING_PUNCHY_ANAEROBIC: f64 = 1.15;

/// Curve Profile: Cycling — punchy endurance ratio.
const CYCLING_PUNCHY_ENDURANCE: f64 = 1.1;

/// Curve Profile: Cycling — time trialist aerobic ratio.
const CYCLING_TIMETRIALIST_AEROBIC: f64 = 1.2;

/// Curve Profile: Cycling — time trialist endurance ratio.
const CYCLING_TIMETRIALIST_ENDURANCE: f64 = 1.1;

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
    if thr.abs() < ADAPTATION_PLATEAU_THRESHOLD_PCT
        && vo2.abs() < ADAPTATION_PLATEAU_THRESHOLD_PCT
        && dur.abs() < ADAPTATION_PLATEAU_THRESHOLD_PCT
        && neural.abs() < ADAPTATION_PLATEAU_THRESHOLD_PCT
    {
        return AdaptationState::Plateau;
    }

    // FatigueState: thr < -3% && vo2 < -3%
    if thr < ADAPTATION_FATIGUE_THR_THRESHOLD && vo2 < ADAPTATION_FATIGUE_VO2_THRESHOLD {
        return AdaptationState::FatigueState;
    }

    // Vo2Expansion: vo2_delta > 3%
    if vo2 > ADAPTATION_VO2_EXPANSION_THRESHOLD {
        return AdaptationState::Vo2Expansion;
    }

    // AerobicConsolidation: thr_delta > 1% && dur_delta > 2%
    if thr > ADAPTATION_AEROBIC_THR_THRESHOLD && dur > ADAPTATION_AEROBIC_DUR_THRESHOLD {
        return AdaptationState::AerobicConsolidation;
    }

    // AnaerobicBuild: neural_delta > 5% && ana_1m_delta > 2%
    if neural > ADAPTATION_ANAEROBIC_NEURAL_THRESHOLD && ana_1m > ADAPTATION_ANAEROBIC_1M_THRESHOLD {
        return AdaptationState::AnaerobicBuild;
    }

    // MixedAdaptation: conflicting signals
    if (neural > ADAPTATION_MIXED_NEURAL_THRESHOLD && dur < ADAPTATION_MIXED_DUR_DECLINE)
        || (vo2 > ADAPTATION_MIXED_VO2_THRESHOLD && thr < ADAPTATION_MIXED_THR_DECLINE)
    {
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
        let endurance_slope = (p60m - p20m) / CURVE_ENDURANCE_SLOPE_SPAN_MIN;
        let power_slope = (p5m - p1m) / CURVE_POWER_SLOPE_SPAN_MIN;

        if endurance_slope.abs() < RUNNING_ENDURANCE_FLAT_THRESHOLD
            && power_slope > RUNNING_POWER_SPEED_THRESHOLD
        {
            CurveProfile::SpeedRunner
        } else if endurance_slope.abs() < RUNNING_ENDURANCE_FLAT_THRESHOLD
            && power_slope <= RUNNING_POWER_SPEED_THRESHOLD
        {
            CurveProfile::EnduranceRunner
        } else if endurance_slope < RUNNING_PUNCHY_ENDURANCE_DECLINE
            && power_slope > RUNNING_PUNCHY_POWER_THRESHOLD
        {
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

        if sprint_ratio > CYCLING_SPRINT_RATIO_THRESHOLD
            && anaerobic_ratio > CYCLING_ANAEROBIC_RATIO_THRESHOLD
        {
            CurveProfile::Sprinter
        } else if sprint_ratio > CYCLING_MODERATE_SPRINT_RATIO
            && anaerobic_ratio > CYCLING_MODERATE_ANAEROBIC_RATIO
        {
            CurveProfile::AnaerobicSpecialist
        } else if endurance_ratio > CYCLING_ENDURANCE_RATIO_THRESHOLD
            && aerobic_ratio > CYCLING_ENDURANCE_AEROBIC_RATIO
        {
            CurveProfile::EnduranceSpecialist
        } else if sprint_ratio > CYCLING_PUNCHY_CLIMBER_SPRINT
            && anaerobic_ratio > CYCLING_PUNCHY_CLIMBER_ANAEROBIC
            && endurance_ratio < CYCLING_PUNCHY_CLIMBER_ENDURANCE
        {
            CurveProfile::PunchyClimber
        } else if anaerobic_ratio > CYCLING_PUNCHY_ANAEROBIC
            && endurance_ratio < CYCLING_PUNCHY_ENDURANCE
        {
            CurveProfile::Punchy
        } else if aerobic_ratio > CYCLING_TIMETRIALIST_AEROBIC
            && endurance_ratio > CYCLING_TIMETRIALIST_ENDURANCE
        {
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
