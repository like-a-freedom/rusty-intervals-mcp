//! Load-management alert builders.
//!
//! All four alerts share the `metrics.load_management` payload:
//! - `acwr_overreaching` (Priority) and `acwr_watch` (Caution) for ACWR.
//! - `high_monotony` for repetitive load pattern.
//! - `high_fatigue_index` for accumulated fatigue outpacing recovery.
//! - `low_durability_index` for power-curve degradation after accumulated work.
//!
//! Volume-side alert:
//! - `high_training_load` when weekly average hours exceeds [`super::WEEKLY_AVG_HIGH_HOURS`].

use crate::domains::coach::{CoachAlert, CoachAlertSeverity, CoachMetrics};

/// Append ACWR / monotony / fatigue-index / durability-index / volume alerts.
pub(super) fn push_load_alerts(metrics: &CoachMetrics, alerts: &mut Vec<CoachAlert>) {
    if let Some(load_management) = &metrics.load_management {
        // ACWR alerts: `overreaching` (priority) above the watch threshold, otherwise `watch` (caution).
        if let Some(acwr) = &load_management.acwr {
            if acwr.ratio > super::ACWR_OVERREACH_RATIO {
                alerts.push(CoachAlert {
                    severity: CoachAlertSeverity::Priority,
                    code: "acwr_overreaching".to_string(),
                    title: "Acute load spike".to_string(),
                    evidence: vec![format!(
                        "ACWR {:.2} exceeds {:.1}",
                        acwr.ratio,
                        super::ACWR_OVERREACH_RATIO
                    )],
                    section: "load_management".to_string(),
                });
            } else if acwr.ratio > super::ACWR_ALERT_RATIO {
                alerts.push(CoachAlert {
                    severity: CoachAlertSeverity::Caution,
                    code: "acwr_watch".to_string(),
                    title: "Load ramp watch".to_string(),
                    evidence: vec![format!(
                        "ACWR {:.2} exceeds {:.1}",
                        acwr.ratio,
                        super::ACWR_ALERT_RATIO
                    )],
                    section: "load_management".to_string(),
                });
            }
        }

        // Monotony alert.
        if let Some(monotony) = load_management.monotony
            && monotony > super::MONOTONY_ALERT
        {
            alerts.push(CoachAlert {
                severity: CoachAlertSeverity::Caution,
                code: "high_monotony".to_string(),
                title: "Repetitive load pattern".to_string(),
                evidence: vec![format!(
                    "Monotony {:.2} exceeds {:.1}",
                    monotony,
                    super::MONOTONY_ALERT
                )],
                section: "load_management".to_string(),
            });
        }

        // Fatigue Index alert.
        if let Some(fatigue_index) = load_management.fatigue_index
            && fatigue_index > super::FATIGUE_INDEX_ALERT
        {
            alerts.push(CoachAlert {
                severity: CoachAlertSeverity::Caution,
                code: "high_fatigue_index".to_string(),
                title: "High fatigue index".to_string(),
                evidence: vec![format!(
                    "Fatigue Index {:.2} exceeds {:.1}",
                    fatigue_index,
                    super::FATIGUE_INDEX_ALERT
                )],
                section: "load_management".to_string(),
            });
        }

        // Durability Index alert.
        if let Some(durability_index) = load_management.durability_index
            && durability_index < super::DURABILITY_INDEX_ALERT
        {
            alerts.push(CoachAlert {
                severity: CoachAlertSeverity::Caution,
                code: "low_durability_index".to_string(),
                title: "Low durability index".to_string(),
                evidence: vec![format!(
                    "Durability Index {:.3} is below {:.2} — power curve degraded after accumulated work",
                    durability_index, super::DURABILITY_INDEX_ALERT
                )],
                section: "load_management".to_string(),
            });
        }
    }

    // Volume alert: high weekly average hours.
    if let Some(volume) = &metrics.volume
        && volume.weekly_avg_hours > super::WEEKLY_AVG_HIGH_HOURS
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "high_training_load".to_string(),
            title: "High training load".to_string(),
            evidence: vec![format!(
                "Weekly average {:.1}h exceeds {:.0}h threshold",
                volume.weekly_avg_hours,
                super::WEEKLY_AVG_HIGH_HOURS
            )],
            section: "volume".to_string(),
        });
    }
}
