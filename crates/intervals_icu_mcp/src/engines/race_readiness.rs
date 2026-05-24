//! 5-factor Race Readiness Score Engine.
//! Baseline 90, penalties applied for each risk factor.
//! Source: Montis.icu Coach V5 — Race Readiness Dashboard design.

// =============================================================================
// Race Readiness Constants
// =============================================================================

/// Baseline score before any modifiers.
const RACE_READINESS_BASELINE: i32 = 90;

/// TSB bonus when fresh (TSB > threshold).
const TSB_BONUS: i32 = 5;

/// TSB threshold for "fresh" classification.
const TSB_FRESH_THRESHOLD: f64 = 12.0;

/// Durability drifting penalty.
const DURABILITY_DRIFTING_PENALTY: i32 = -15;

/// Neural overload penalty (NDLI red).
const NEURAL_OVERLOAD_PENALTY: i32 = -15;

/// System alignment mismatch penalty.
const SYSTEM_MISMATCH_PENALTY: i32 = -10;

/// Maximum taper detraining penalty (CTL drop magnitude).
const TAPER_MAX_PENALTY: i32 = -60;

/// Score tier boundaries.
const TIER_READY: i32 = 80;
const TIER_MONITOR: i32 = 60;
const TIER_CAUTION: i32 = 40;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaceReadinessScore {
    pub score: i32,
    pub tier: &'static str,
    pub tsb_modifier: i32,
    pub durability_modifier: i32,
    pub neural_modifier: i32,
    pub system_modifier: i32,
    pub taper_modifier: i32,
}

/// Compute race readiness from 5 factors.
/// - tsb: current TSB value
/// - durability_drifting: true if ISDM durability state is "drifting"
/// - ndli_overload: true if NDLI state is "red" (≥4 high-intensity days)
/// - system_mismatch: true if curve profile doesn't match race type
/// - ctl_drop: CTL drop magnitude (for detraining penalty)
pub fn compute_race_readiness(
    tsb: Option<f64>,
    durability_drifting: bool,
    ndli_overload: bool,
    system_mismatch: bool,
    ctl_drop: Option<f64>,
) -> RaceReadinessScore {
    let mut score: i32 = RACE_READINESS_BASELINE;
    let mut tsb_modifier: i32 = 0;
    let mut durability_modifier: i32 = 0;
    let mut neural_modifier: i32 = 0;
    let mut system_modifier: i32 = 0;
    let mut taper_modifier: i32 = 0;

    // TSB: Fresh > threshold → bonus
    if let Some(t) = tsb
        && t > TSB_FRESH_THRESHOLD
    {
        tsb_modifier = TSB_BONUS;
    }

    // Durability: drifting → penalty
    if durability_drifting {
        durability_modifier = DURABILITY_DRIFTING_PENALTY;
    }

    // Neural: NDLI overload → penalty
    if ndli_overload {
        neural_modifier = NEURAL_OVERLOAD_PENALTY;
    }

    // System mismatch → penalty
    if system_mismatch {
        system_modifier = SYSTEM_MISMATCH_PENALTY;
    }

    // Taper: detraining penalty → up to max based on CTL drop
    if let Some(drop) = ctl_drop
        && drop > 0.0
    {
        taper_modifier = -(drop as i32).min(-TAPER_MAX_PENALTY);
    }

    score +=
        tsb_modifier + durability_modifier + neural_modifier + system_modifier + taper_modifier;
    score = score.clamp(0, 100);

    let tier = if score >= TIER_READY {
        "ready"
    } else if score >= TIER_MONITOR {
        "monitor"
    } else if score >= TIER_CAUTION {
        "caution"
    } else {
        "not_ready"
    };

    RaceReadinessScore {
        score,
        tier,
        tsb_modifier,
        durability_modifier,
        neural_modifier,
        system_modifier,
        taper_modifier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_baseline_is_ready() {
        let result = compute_race_readiness(None, false, false, false, None);
        assert_eq!(result.score, 90);
        assert_eq!(result.tier, "ready");
    }

    #[test]
    fn readiness_tsb_bonus() {
        let result = compute_race_readiness(Some(15.0), false, false, false, None);
        assert_eq!(result.score, 95);
        assert_eq!(result.tsb_modifier, 5);
    }

    #[test]
    fn readiness_durability_penalty() {
        let result = compute_race_readiness(None, true, false, false, None);
        assert_eq!(result.score, 75);
        assert_eq!(result.durability_modifier, -15);
    }

    #[test]
    fn readiness_neural_penalty() {
        let result = compute_race_readiness(None, false, true, false, None);
        assert_eq!(result.score, 75);
        assert_eq!(result.neural_modifier, -15);
    }

    #[test]
    fn readiness_system_mismatch_penalty() {
        let result = compute_race_readiness(None, false, false, true, None);
        assert_eq!(result.score, 80);
        assert_eq!(result.system_modifier, -10);
    }

    #[test]
    fn readiness_taper_penalty() {
        let result = compute_race_readiness(None, false, false, false, Some(30.0));
        assert_eq!(result.score, 60);
        assert_eq!(result.taper_modifier, -30);
    }

    #[test]
    fn readiness_taper_penalty_capped() {
        let result = compute_race_readiness(None, false, false, false, Some(100.0));
        assert_eq!(result.score, 30);
        assert_eq!(result.taper_modifier, -60);
    }

    #[test]
    fn readiness_score_cannot_go_below_zero() {
        let result = compute_race_readiness(Some(-30.0), true, true, true, Some(60.0));
        // 90 + 0 (bad tsb) - 15 - 15 - 10 - 60 = -10 → clamped to 0
        assert_eq!(result.score, 0);
    }

    #[test]
    fn readiness_monotonicity_each_penalty_reduces_score() {
        let baseline = compute_race_readiness(None, false, false, false, None);
        let with_durability = compute_race_readiness(None, true, false, false, None);
        let with_neural = compute_race_readiness(None, false, true, false, None);
        let with_system = compute_race_readiness(None, false, false, true, None);

        assert!(with_durability.score < baseline.score);
        assert!(with_neural.score < baseline.score);
        assert!(with_system.score < baseline.score);
    }
}
