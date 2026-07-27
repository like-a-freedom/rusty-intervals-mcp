//! Adaptation-state alert builder.

use crate::domains::coach::{CoachAlert, CoachAlertSeverity, CoachMetrics};
use crate::engines::adaptation::AdaptationState;

/// Append adaptation-state alerts into `alerts`.
///
/// Only `Plateau` (Caution) and `FatigueState` (Priority) emit alerts. All
/// other states are silent because the underlying ESPE engine handles them
/// through its own reporting.
pub(super) fn push_adaptation_alerts(metrics: &CoachMetrics, alerts: &mut Vec<CoachAlert>) {
    let Some(espe) = &metrics.espe_derived else {
        return;
    };
    let Some(ref state_str) = espe.adaptation_state else {
        return;
    };
    let Some(state) = super::parse_adaptation_state(state_str) else {
        return;
    };

    if state == AdaptationState::Plateau {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "adaptation_stalled".to_string(),
            title: "Adaptation plateau detected".to_string(),
            evidence: vec![
                "Power-curve deltas below threshold — no meaningful adaptation across any system."
                    .to_string(),
            ],
            section: "adaptation".to_string(),
        });
    }
    if state == AdaptationState::FatigueState {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Priority,
            code: "adaptation_fatigue".to_string(),
            title: "Fatigue-dominant adaptation pattern".to_string(),
            evidence: vec![
                "Threshold and VO2max power declining — consider reducing load or adding recovery."
                    .to_string(),
            ],
            section: "adaptation".to_string(),
        });
    }
}
