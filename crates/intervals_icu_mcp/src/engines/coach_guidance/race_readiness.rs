//! Race-readiness alert builder.

use crate::domains::coach::{CoachAlert, CoachAlertSeverity, CoachMetrics};

/// Append the race-readiness alert if the readiness score is below the alert threshold.
///
/// Score < [`super::RACE_READINESS_CRITICAL`] is `Priority`, otherwise `Caution`.
pub(super) fn push_race_readiness_alerts(metrics: &CoachMetrics, alerts: &mut Vec<CoachAlert>) {
    if let Some(race) = &metrics.race_readiness
        && race.supported
        && let Some(score) = race.readiness_score
        && score < super::RACE_READINESS_ALERT
    {
        alerts.push(CoachAlert {
            severity: if score < super::RACE_READINESS_CRITICAL {
                CoachAlertSeverity::Priority
            } else {
                CoachAlertSeverity::Caution
            },
            code: "low_race_readiness".to_string(),
            title: "Suboptimal race readiness".to_string(),
            evidence: vec![format!(
                "Readiness score {:.0}/100 (threshold: {:.0})",
                score,
                super::RACE_READINESS_ALERT
            )],
            section: "readiness".to_string(),
        });
    }
}
