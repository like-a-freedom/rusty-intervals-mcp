// =============================================================================
// Curve Profile Constants
// =============================================================================
//
// Adaptation-state classification (`AdaptationState` + `classify_adaptation`)
// was removed during the comprehensive audit refactor — it was never wired
// into any handler or engine. `CurveProfile` / `classify_curve_profile`
// remain in active use by `analyze_training`.

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

    #[test]
    fn curve_profile_anaerobic_specialist() {
        assert_eq!(
            classify_curve_profile(
                Some(700.0),
                Some(500.0),
                Some(400.0),
                Some(330.0),
                Some(300.0),
                false
            ),
            CurveProfile::AnaerobicSpecialist
        );
    }

    #[test]
    fn curve_profile_punchy_climber() {
        assert_eq!(
            classify_curve_profile(
                Some(640.0),
                Some(500.0),
                Some(430.0),
                Some(300.0),
                Some(290.0),
                false
            ),
            CurveProfile::PunchyClimber
        );
    }

    #[test]
    fn curve_profile_punchy() {
        assert_eq!(
            classify_curve_profile(
                Some(590.0),
                Some(500.0),
                Some(424.0),
                Some(320.0),
                Some(296.0),
                false
            ),
            CurveProfile::Punchy
        );
    }

    #[test]
    fn curve_profile_time_trialist() {
        assert_eq!(
            classify_curve_profile(
                Some(625.0),
                Some(500.0),
                Some(410.0),
                Some(340.0),
                Some(300.0),
                false
            ),
            CurveProfile::TimeTrialist
        );
    }

    #[test]
    fn curve_profile_cycling_all_rounder() {
        assert_eq!(
            classify_curve_profile(
                Some(500.0),
                Some(450.0),
                Some(400.0),
                Some(350.0),
                Some(320.0),
                false
            ),
            CurveProfile::AllRounder
        );
    }

    #[test]
    fn curve_profile_speed_runner() {
        assert_eq!(
            classify_curve_profile(
                Some(500.0),
                Some(280.0),
                Some(290.0),
                Some(296.0),
                Some(300.0),
                true
            ),
            CurveProfile::SpeedRunner
        );
    }

    #[test]
    fn curve_profile_endurance_runner() {
        assert_eq!(
            classify_curve_profile(
                Some(500.0),
                Some(280.0),
                Some(284.0),
                Some(296.0),
                Some(300.0),
                true
            ),
            CurveProfile::EnduranceRunner
        );
    }

    #[test]
    fn curve_profile_punchy_runner() {
        assert_eq!(
            classify_curve_profile(
                Some(500.0),
                Some(280.0),
                Some(296.0),
                Some(400.0),
                Some(300.0),
                true
            ),
            CurveProfile::PunchyRunner
        );
    }

    #[test]
    fn curve_profile_running_balanced_runner() {
        assert_eq!(
            classify_curve_profile(
                Some(500.0),
                Some(280.0),
                Some(284.0),
                Some(320.0),
                Some(280.0),
                true
            ),
            CurveProfile::BalancedRunner
        );
    }
}
