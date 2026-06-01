//! Adaptive Decision Engine (ADE v1).
//! Synthesizes multiple signals into an operational state directive.
//! Source: Montis ADE v1 — 2-state model.

// =============================================================================
// ADE Constants
// =============================================================================

/// TSB threshold for maladaptation risk.
const TSB_MALADAPTATION: f64 = -30.0;

/// TSB threshold for functional overreach.
const TSB_FUNCTIONAL_OVERREACH: f64 = -20.0;

/// TSB threshold for load pressure.
const TSB_LOAD_PRESSURE: f64 = -10.0;

/// HRV ratio threshold for maladaptation when TSB unavailable.
/// Source: Front. Physiol. 2025 — RMSSD suppression at 10% below baseline.
const HRV_MALADAPTATION_RATIO: f64 = 0.90;

/// CTL ramp rate threshold for load pressure (>8 CTL/week = rapid ramp).
const RAMP_RATE_THRESHOLD: f64 = 8.0;

/// ACWR safe zone lower bound.
const ACWR_SAFE_LOWER: f64 = 0.8;

/// ACWR safe zone upper bound.
const ACWR_SAFE_UPPER: f64 = 1.3;

/// NDLI high-intensity days threshold for loaded_taper check.
const NDLI_LOADED_TAPER_DAYS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OperationalState {
    LoadAccepting,
    RecoveryPriority,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RiskLevel {
    Low,
    Moderate,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdeOutput {
    pub operational_state: OperationalState,
    pub risk_level: RiskLevel,
    pub maladaptation_risk: bool,
    pub functional_overreach: bool,
    pub load_pressure: bool,
    pub loaded_taper: bool,
}

/// Compute ADE operational state from multi-signal synthesis.
/// - tsb: current TSB
/// - hrv_ratio: current HRV / baseline ratio
/// - durability_drifting: ISDM durability state is "drifting"
/// - ndli_overload: NDLI red (≥4 high-intensity days)
/// - heat_high: heat stress high
/// - ramp_rate: CTL ramp rate over 7 days
/// - acwr_ratio: ACWR ratio (0.8–1.3 = safe zone)
/// - ndli_high: NDLI high intensity days count
/// - tsb_value: TSB value for loaded_taper check
#[allow(clippy::too_many_arguments)]
pub fn compute_ade(
    tsb: Option<f64>,
    hrv_ratio: Option<f64>,
    durability_drifting: bool,
    _ndli_overload: bool,
    heat_high: bool,
    ramp_rate: Option<f64>,
    acwr_ratio: Option<f64>,
    ndli_high: usize,
    tsb_value: Option<f64>,
) -> AdeOutput {
    let mut maladaptation_risk = false;
    let mut functional_overreach = false;
    let mut load_pressure = false;
    let mut loaded_taper = false;

    // TSB escalation
    if let Some(t) = tsb {
        if t <= TSB_MALADAPTATION {
            maladaptation_risk = true;
        } else if t <= TSB_FUNCTIONAL_OVERREACH {
            functional_overreach = true;
        } else if t <= TSB_LOAD_PRESSURE {
            load_pressure = true;
        }
    }

    // rampRate > threshold → load_pressure
    if let Some(rr) = ramp_rate
        && rr > RAMP_RATE_THRESHOLD
    {
        load_pressure = true;
    }

    // Heat escalation: productive_load → adaptation_pressure
    // When heat_high is true, elevate urgency
    if heat_high {
        if load_pressure {
            maladaptation_risk = true;
        } else {
            load_pressure = true;
        }
    }

    // HRV + load_pressure → maladaptation_risk (when no TSB)
    // Must run after ramp rate and heat blocks so load_pressure is populated.
    if tsb.is_none()
        && let Some(hrv_r) = hrv_ratio
        && hrv_r < HRV_MALADAPTATION_RATIO
    {
        if load_pressure {
            maladaptation_risk = true;
        } else {
            load_pressure = true;
        }
    }

    // ACWR validation gate: safe zone + durable → reduce severity
    if let Some(acwr) = acwr_ratio
        && (ACWR_SAFE_LOWER..=ACWR_SAFE_UPPER).contains(&acwr)
        && !durability_drifting
    {
        if maladaptation_risk {
            maladaptation_risk = false;
            functional_overreach = true;
        } else if functional_overreach {
            functional_overreach = false;
            load_pressure = true;
        }
    }

    // NDLI red + TSB > 0 → loaded_taper warning
    if ndli_high >= NDLI_LOADED_TAPER_DAYS
        && let Some(tsb_val) = tsb_value
        && tsb_val > 0.0
    {
        loaded_taper = true;
    }

    let operational_state = if maladaptation_risk {
        OperationalState::RecoveryPriority
    } else {
        OperationalState::LoadAccepting
    };

    let risk_level = if maladaptation_risk {
        RiskLevel::Critical
    } else if functional_overreach {
        RiskLevel::High
    } else if load_pressure || loaded_taper {
        RiskLevel::Moderate
    } else {
        RiskLevel::Low
    };

    AdeOutput {
        operational_state,
        risk_level,
        maladaptation_risk,
        functional_overreach,
        load_pressure,
        loaded_taper,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ade_baseline_load_accepting() {
        let result = compute_ade(
            Some(5.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.0),
            1,
            Some(5.0),
        );
        assert_eq!(result.operational_state, OperationalState::LoadAccepting);
        assert_eq!(result.risk_level, RiskLevel::Low);
    }

    #[test]
    fn ade_tsb_maladaptation_risk() {
        // ACWR outside safe zone (1.5) so validation gate doesn't reduce severity
        let result = compute_ade(
            Some(-35.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.5),
            1,
            Some(-35.0),
        );
        assert!(result.maladaptation_risk);
        assert_eq!(result.operational_state, OperationalState::RecoveryPriority);
        assert_eq!(result.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn ade_functional_overreach() {
        // ACWR outside safe zone (1.5) so validation gate doesn't reduce severity
        let result = compute_ade(
            Some(-25.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.5),
            1,
            Some(-25.0),
        );
        assert!(result.functional_overreach);
        assert!(!result.maladaptation_risk);
    }

    #[test]
    fn ade_load_pressure_from_ramp_rate() {
        let result = compute_ade(
            Some(5.0),
            Some(1.0),
            false,
            false,
            false,
            Some(10.0),
            Some(1.0),
            1,
            Some(5.0),
        );
        assert!(result.load_pressure);
    }

    #[test]
    fn ade_loaded_taper() {
        let result = compute_ade(
            Some(5.0),
            Some(1.0),
            false,
            true,
            false,
            Some(5.0),
            Some(1.0),
            5,
            Some(5.0),
        );
        assert!(result.loaded_taper);
        // ndli_overload=true, ndli_high=5 >= 4, tsb_value=5.0 > 0 → loaded_taper
    }

    #[test]
    fn ade_acwr_validation_gate_reduces_severity() {
        let result = compute_ade(
            Some(-25.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.1),
            1,
            Some(-25.0),
        );
        // Without gate: functional_overreach. With ACWR 1.1 (safe) + durable → reduces to load_pressure
        assert!(result.load_pressure);
        assert!(!result.functional_overreach);
    }

    #[test]
    fn ade_heat_escalation() {
        let result = compute_ade(
            Some(5.0),
            Some(1.0),
            false,
            false,
            true,
            Some(5.0),
            Some(1.0),
            1,
            Some(5.0),
        );
        assert!(result.load_pressure);
    }

    #[test]
    fn ade_all_none_inputs_is_load_accepting_low_risk() {
        let result = compute_ade(None, None, false, false, false, None, None, 0, None);
        assert_eq!(result.operational_state, OperationalState::LoadAccepting);
        assert_eq!(result.risk_level, RiskLevel::Low);
        assert!(!result.maladaptation_risk);
        assert!(!result.functional_overreach);
        assert!(!result.load_pressure);
        assert!(!result.loaded_taper);
    }

    #[test]
    fn ade_hrv_low_without_tsb_triggers_load_pressure() {
        // TSB=None, HRV=0.85 (below 0.90 threshold) → load_pressure
        let result = compute_ade(
            None,
            Some(0.85),
            false,
            false,
            false,
            None,
            Some(1.0),
            1,
            None,
        );
        assert!(result.load_pressure);
        assert_eq!(result.operational_state, OperationalState::LoadAccepting);
        assert_eq!(result.risk_level, RiskLevel::Moderate);
    }

    #[test]
    fn ade_hrv_low_with_load_pressure_escalates_to_maladaptation() {
        // TSB=None, HRV=0.85 + ramp_rate=10 → load_pressure first, then HRV escalates to maladaptation
        let result = compute_ade(
            None,
            Some(0.85),
            false,
            false,
            false,
            Some(10.0),
            Some(1.5),
            1,
            None,
        );
        assert!(result.maladaptation_risk);
        assert_eq!(result.operational_state, OperationalState::RecoveryPriority);
        assert_eq!(result.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn ade_hrv_normal_without_tsb_is_safe() {
        // TSB=None, HRV=0.95 (above 0.90 threshold) → no escalation
        let result = compute_ade(
            None,
            Some(0.95),
            false,
            false,
            false,
            None,
            Some(1.0),
            1,
            None,
        );
        assert_eq!(result.operational_state, OperationalState::LoadAccepting);
        assert_eq!(result.risk_level, RiskLevel::Low);
    }

    #[test]
    fn ade_loaded_taper_with_positive_tsb_and_high_ndli() {
        // ndli_high=4 (>= threshold) + tsb_value=10.0 (>0) → loaded_taper
        let result = compute_ade(
            Some(10.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.0),
            4,
            Some(10.0),
        );
        assert!(result.loaded_taper);
    }

    #[test]
    fn ade_loaded_taper_requires_positive_tsb() {
        // ndli_high=4 but tsb_value=-5.0 (negative) → no loaded_taper
        let result = compute_ade(
            Some(-5.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.0),
            4,
            Some(-5.0),
        );
        assert!(!result.loaded_taper);
    }

    #[test]
    fn ade_loaded_taper_requires_ndli_at_or_above_threshold() {
        // ndli_high=3 (< 4) + positive TSB → no loaded_taper
        let result = compute_ade(
            Some(10.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.0),
            3,
            Some(10.0),
        );
        assert!(!result.loaded_taper);
    }

    #[test]
    fn ade_tsb_boundary_load_pressure_at_minus_10() {
        // TSB exactly at TSB_LOAD_PRESSURE threshold (-10.0) → load_pressure
        let result = compute_ade(
            Some(-10.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.5),
            1,
            Some(-10.0),
        );
        assert!(result.load_pressure);
    }

    #[test]
    fn ade_tsb_boundary_functional_overreach_at_minus_20() {
        // TSB exactly at TSB_FUNCTIONAL_OVERREACH threshold (-20.0) → functional_overreach
        let result = compute_ade(
            Some(-20.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.5),
            1,
            Some(-20.0),
        );
        assert!(result.functional_overreach);
        assert!(!result.maladaptation_risk);
    }

    #[test]
    fn ade_tsb_boundary_maladaptation_at_minus_30() {
        // TSB exactly at TSB_MALADAPTATION threshold (-30.0) → maladaptation_risk
        let result = compute_ade(
            Some(-30.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.5),
            1,
            Some(-30.0),
        );
        assert!(result.maladaptation_risk);
    }

    #[test]
    fn ade_ramp_rate_at_threshold_does_not_trigger() {
        // ramp_rate exactly = 8.0 (not > threshold) → no load_pressure from ramp
        let result = compute_ade(
            Some(5.0),
            Some(1.0),
            false,
            false,
            false,
            Some(8.0),
            Some(1.0),
            1,
            Some(5.0),
        );
        assert!(!result.load_pressure);
    }

    #[test]
    fn ade_acwr_safe_zone_reduces_maladaptation_to_functional() {
        // TSB=-35 (maladaptation) + ACWR=1.0 (safe) + durable → reduces to functional_overreach
        let result = compute_ade(
            Some(-35.0),
            Some(1.0),
            false,
            false,
            false,
            Some(5.0),
            Some(1.0),
            1,
            Some(-35.0),
        );
        assert!(result.functional_overreach);
        assert!(!result.maladaptation_risk);
        assert_eq!(result.risk_level, RiskLevel::High);
    }
}
