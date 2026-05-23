#![allow(clippy::collapsible_if)]

/// Adaptive Decision Engine (ADE v1).
/// Synthesizes multiple signals into an operational state directive.

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
        if t <= -30.0 {
            maladaptation_risk = true;
        } else if t <= -20.0 {
            functional_overreach = true;
        } else if t <= -10.0 {
            load_pressure = true;
        }
    }

    // HRV + load_pressure → maladaptation_risk (when no TSB)
    if tsb.is_none() {
        if let Some(hrv_r) = hrv_ratio {
            if hrv_r < 0.90 && load_pressure {
                maladaptation_risk = true;
            }
        }
    }

    // rampRate > 8 → load_pressure
    if let Some(rr) = ramp_rate {
        if rr > 8.0 {
            load_pressure = true;
        }
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

    // ACWR validation gate: [0.8, 1.3] + durable → reduce severity
    if let Some(acwr) = acwr_ratio {
        if (0.8..=1.3).contains(&acwr) && !durability_drifting {
            if maladaptation_risk {
                maladaptation_risk = false;
                functional_overreach = true;
            } else if functional_overreach {
                functional_overreach = false;
                load_pressure = true;
            }
        }
    }

    // NDLI red + TSB > 0 → loaded_taper warning
    if ndli_high >= 4 {
        if let Some(tsb_val) = tsb_value {
            if tsb_val > 0.0 {
                loaded_taper = true;
            }
        }
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
}
