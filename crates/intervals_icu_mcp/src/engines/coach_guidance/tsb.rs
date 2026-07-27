//! TSB (Training Stress Balance) alert builders.
//!
//! Two related fatigue alerts:
//! - `deep_fatigue`: TSB < `TSB_DEEP_FATIGUE` (-20). Priority.
//! - `fatigue`: TSB in `[TSB_DEEP_FATIGUE, TSB_FATIGUED]`. Caution.
//!
//! Both branch on the same `fitness.tsb` value so the helper accepts an
//! `Option<f64>` directly to avoid re-borrowing `metrics.fitness` at the call
//! site.

use crate::domains::coach::{CoachAlert, CoachAlertSeverity, CoachMetrics};

/// Append TSB-based alerts into `alerts`.
///
/// Idempotent and side-effect-free apart from `alerts.push(...)`. Safe to call
/// even when no `fitness` payload is present — the function becomes a no-op.
pub(super) fn push_tsb_alerts(metrics: &CoachMetrics, alerts: &mut Vec<CoachAlert>) {
    let Some(fitness) = &metrics.fitness else {
        return;
    };
    let Some(tsb) = fitness.tsb else { return };

    if tsb < super::TSB_DEEP_FATIGUE {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Priority,
            code: "deep_fatigue".to_string(),
            title: "Deep fatigue signal".to_string(),
            evidence: vec![format!("TSB below -20 ({:.1})", tsb)],
            section: "fitness".to_string(),
        });
    }

    if (super::TSB_DEEP_FATIGUE..=super::TSB_FATIGUED).contains(&tsb) {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "fatigue".to_string(),
            title: "Fatigue accumulation".to_string(),
            evidence: vec![format!("TSB below -10 ({:.1})", tsb)],
            section: "fitness".to_string(),
        });
    }
}
