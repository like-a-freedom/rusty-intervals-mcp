//! Sleep / RHR / HRV / recovery-index alert builders.
//!
//! Wellness-side alerts:
//! - `low_sleep`: avg sleep hours below [`super::SLEEP_ALERT_HOURS`].
//! - `elevated_rhr`: RHR above [`super::RHR_ALERT_BPM`].
//! - `low_hrv`: HRV trend state is `suppressed` or `below_range`.
//! - `low_recovery_index`: recovery index below [`super::RECOVERY_INDEX_ALERT`].
//!
//! Idempotent and side-effect-free apart from `alerts.push(...)`.

use crate::domains::coach::{CoachAlert, CoachAlertSeverity, CoachMetrics};

/// Append wellness alerts (sleep, RHR, HRV, recovery index) into `alerts`.
pub(super) fn push_wellness_alerts(metrics: &CoachMetrics, alerts: &mut Vec<CoachAlert>) {
    let Some(wellness) = &metrics.wellness else {
        return;
    };

    // Low sleep alert.
    if let Some(sleep) = wellness.avg_sleep_hours
        && sleep < super::SLEEP_ALERT_HOURS
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "low_sleep".to_string(),
            title: "Low sleep support".to_string(),
            evidence: vec![format!(
                "Average sleep below {:.1}h ({:.1}h)",
                super::SLEEP_ALERT_HOURS,
                sleep
            )],
            section: "wellness".to_string(),
        });
    }

    // Elevated RHR alert.
    if let Some(rhr) = wellness.avg_resting_hr
        && rhr > super::RHR_ALERT_BPM
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "elevated_rhr".to_string(),
            title: "Elevated RHR signal".to_string(),
            evidence: vec![format!(
                "RHR above {:.0} bpm ({:.0} bpm)",
                super::RHR_ALERT_BPM,
                rhr
            )],
            section: "wellness".to_string(),
        });
    }

    // Personal-baseline HRV alert.
    if let Some(hrv_state) = wellness.hrv_trend_state.as_deref()
        && matches!(hrv_state, "suppressed" | "below_range")
    {
        let evidence = match (
            wellness.hrv_deviation_pct,
            wellness.hrv_baseline,
            wellness.avg_hrv,
        ) {
            (Some(deviation_pct), Some(baseline), Some(current)) => vec![format!(
                "HRV {:.1}% below personal baseline ({:.0} ms vs {:.0} ms)",
                deviation_pct.abs(),
                current,
                baseline
            )],
            _ => vec!["HRV is below the athlete's recent personal range".to_string()],
        };

        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "low_hrv".to_string(),
            title: "HRV below personal baseline".to_string(),
            evidence,
            section: "wellness".to_string(),
        });
    }

    // Low recovery index alert.
    if let Some(recovery_index) = wellness.recovery_index
        && recovery_index < super::RECOVERY_INDEX_ALERT
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Priority,
            code: "low_recovery_index".to_string(),
            title: "Recovery-first signal".to_string(),
            evidence: vec![format!(
                "Recovery index below {:.2} ({:.2})",
                super::RECOVERY_INDEX_ALERT,
                recovery_index
            )],
            section: "wellness".to_string(),
        });
    }
}
