//! Distribution-side alert builders.
//!
//! - `threshold_biased_polarisation` (Caution): polarisation state == "threshold_biased".
//! - `polarized_confirmation` (Info): polarisation state == "polarised".
//! - `low_consistency` (Caution): plan adherence below target.

use crate::domains::coach::{CoachAlert, CoachAlertSeverity, CoachMetrics};

/// Append polarisation + consistency alerts into `alerts`.
pub(super) fn push_distribution_alerts(metrics: &CoachMetrics, alerts: &mut Vec<CoachAlert>) {
    // Threshold-biased polarisation alert.
    if let Some(polarisation) = &metrics.polarisation
        && let Some(state) = polarisation.state.as_deref()
        && state == "threshold_biased"
        && let Some(z2_pct) = polarisation.z2_pct
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "threshold_biased_polarisation".to_string(),
            title: "Threshold-biased training distribution".to_string(),
            evidence: vec![format!(
                "Polarisation ratio {:.2} indicates too much threshold-zone work ({:.0}% Z2)",
                polarisation.ratio.unwrap_or(0.0),
                z2_pct * 100.0
            )],
            section: "distribution".to_string(),
        });
    }

    // Polarized confirmation alert.
    if let Some(polarisation) = &metrics.polarisation
        && let Some(state) = polarisation.state.as_deref()
        && state == "polarised"
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Info,
            code: "polarized_confirmation".to_string(),
            title: "Polarized training distribution".to_string(),
            evidence: vec!["Training distribution follows the 80/20 polarised model — appropriate for most endurance phases.".to_string()],
            section: "distribution".to_string(),
        });
    }

    // Low consistency alert.
    if let Some(consistency) = &metrics.consistency
        && let Some(state) = consistency.state.as_deref()
        && state == "low"
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "low_consistency".to_string(),
            title: "Low training plan adherence".to_string(),
            evidence: vec![format!(
                "Only {} of {} planned sessions completed ({:.0}%)",
                consistency.sessions_completed,
                consistency.sessions_planned,
                consistency.ratio.unwrap_or(0.0) * 100.0
            )],
            section: "adherence".to_string(),
        });
    }
}
