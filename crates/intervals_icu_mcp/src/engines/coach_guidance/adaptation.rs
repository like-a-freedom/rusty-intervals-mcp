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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::coach::{EspeDerivedMetrics, FitnessMetrics};

    fn metrics_with_espe(adaptation_state: Option<&str>) -> CoachMetrics {
        CoachMetrics {
            espe_derived: Some(EspeDerivedMetrics {
                adaptation_state: adaptation_state.map(str::to_owned),
                supported: true,
                ..Default::default()
            }),
            fitness: Some(FitnessMetrics {
                ctl: Some(50.0),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn no_espe_derived_returns_early() {
        let metrics = CoachMetrics::default();
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert!(alerts.is_empty());
    }

    #[test]
    fn espe_present_but_no_adaptation_state_returns_early() {
        let metrics = metrics_with_espe(None);
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert!(alerts.is_empty());
    }

    #[test]
    fn unknown_adaptation_state_returns_early() {
        let metrics = metrics_with_espe(Some("UnknownVariant"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert!(alerts.is_empty());
    }

    #[test]
    fn plateau_state_pushes_caution_alert() {
        let metrics = metrics_with_espe(Some("Plateau"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, CoachAlertSeverity::Caution);
        assert_eq!(alerts[0].code, "adaptation_stalled");
        assert_eq!(alerts[0].section, "adaptation");
    }

    #[test]
    fn fatigue_state_pushes_priority_alert() {
        let metrics = metrics_with_espe(Some("FatigueState"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, CoachAlertSeverity::Priority);
        assert_eq!(alerts[0].code, "adaptation_fatigue");
        assert_eq!(alerts[0].section, "adaptation");
    }

    #[test]
    fn baseline_state_pushes_no_alerts() {
        let metrics = metrics_with_espe(Some("Baseline"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert!(alerts.is_empty());
    }

    #[test]
    fn vo2_expansion_state_pushes_no_alerts() {
        let metrics = metrics_with_espe(Some("Vo2Expansion"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert!(alerts.is_empty());
    }

    #[test]
    fn aerobic_consolidation_state_pushes_no_alerts() {
        let metrics = metrics_with_espe(Some("AerobicConsolidation"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert!(alerts.is_empty());
    }

    #[test]
    fn anaerobic_build_state_pushes_no_alerts() {
        let metrics = metrics_with_espe(Some("AnaerobicBuild"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert!(alerts.is_empty());
    }

    #[test]
    fn mixed_adaptation_state_pushes_no_alerts() {
        let metrics = metrics_with_espe(Some("MixedAdaptation"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert!(alerts.is_empty());
    }

    #[test]
    fn plateau_alert_contains_evidence() {
        let metrics = metrics_with_espe(Some("Plateau"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert_eq!(alerts.len(), 1);
        assert!(
            alerts[0].evidence[0].contains("Power-curve deltas"),
            "evidence should mention Power-curve deltas"
        );
    }

    #[test]
    fn fatigue_alert_contains_evidence() {
        let metrics = metrics_with_espe(Some("FatigueState"));
        let mut alerts = Vec::new();
        push_adaptation_alerts(&metrics, &mut alerts);
        assert_eq!(alerts.len(), 1);
        assert!(
            alerts[0].evidence[0].contains("Threshold and VO2max"),
            "evidence should mention threshold and VO2max"
        );
    }
}
