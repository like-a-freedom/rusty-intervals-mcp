//! Neural-density (NDLI), heat-stress, WDRM, and ISDM decoupling alert builders.
//!
//! Each block inspects a different `CoachMetrics` payload but they all share
//! the same `alerts.push(...)` side effect, so they live in one helper to keep
//! the orchestrator short.

use crate::domains::coach::{CoachAlert, CoachAlertSeverity, CoachMetrics};
use crate::engines::coach_metrics_constants::WDRM_HIGH_DEPLETION_PCT;

/// Append neural / heat / WDRM / decoupling alerts into `alerts`.
pub(super) fn push_neural_alerts(metrics: &CoachMetrics, alerts: &mut Vec<CoachAlert>) {
    // NDLI — neural overload alert.
    if let Some(ndli) = &metrics.ndli
        && ndli.supported
        && ndli.ndli_overload_flag
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Priority,
            code: "ndli_overload".to_string(),
            title: "Neural density overload".to_string(),
            evidence: vec![format!(
                "{} high-intensity days in the last 7 — NDLI state: red",
                ndli.high_intensity_days_7d
            )],
            section: "ndli".to_string(),
        });
    }

    // NDLI — elevated alert (amber).
    if let Some(ndli) = &metrics.ndli
        && ndli.supported
        && ndli.ndli_state == "amber"
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "ndli_elevated".to_string(),
            title: "Elevated neural density".to_string(),
            evidence: vec![format!(
                "{} high-intensity days in the last 7 — NDLI state: amber",
                ndli.high_intensity_days_7d
            )],
            section: "ndli".to_string(),
        });
    }

    // Heat — stress elevated alert.
    if let Some(heat) = &metrics.heat
        && heat.supported
        && heat.heat_state == "high"
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "heat_stress_elevated".to_string(),
            title: "Heat stress elevated".to_string(),
            evidence: vec![format!(
                "Heat index {:.2} — high heat exposure over the last 7 days",
                heat.heat_index_7d.unwrap_or(0.0)
            )],
            section: "heat".to_string(),
        });
    }

    // Heat — moderate alert.
    if let Some(heat) = &metrics.heat
        && heat.supported
        && heat.heat_state == "moderate"
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Info,
            code: "heat_stress_moderate".to_string(),
            title: "Moderate heat exposure".to_string(),
            evidence: vec![format!(
                "Heat index {:.2} — moderate heat over the last 7 days",
                heat.heat_index_7d.unwrap_or(0.0)
            )],
            section: "heat".to_string(),
        });
    }

    // WDRM — high W′ depletion alert.
    if let Some(wdrm) = &metrics.wdrm
        && wdrm.supported
        && let Some(depletion_pct) = wdrm.depletion_pct
        && depletion_pct >= WDRM_HIGH_DEPLETION_PCT
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "high_wbal_depletion".to_string(),
            title: "High W′ depletion".to_string(),
            evidence: vec![format!(
                "W′ depletion at {:.0}% — anaerobic reserves significantly drained",
                depletion_pct * 100.0
            )],
            section: "wdrm".to_string(),
        });
    }

    // ISDM — durability drifting alert.
    if let Some(workout) = &metrics.workout
        && let Some(decoupling) = &workout.aerobic_decoupling
        && decoupling.durability_state == "drifting"
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "durability_drifting".to_string(),
            title: "Durability drift detected".to_string(),
            evidence: vec![format!(
                "Signed decoupling {:.1}% — power/HR ratio shifting across the session",
                decoupling.signed_decoupling_pct
            )],
            section: "decoupling".to_string(),
        });
    }

    // ISDM — durability improving info.
    if let Some(workout) = &metrics.workout
        && let Some(decoupling) = &workout.aerobic_decoupling
        && decoupling.durability_state == "improving"
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Info,
            code: "durability_improving".to_string(),
            title: "Durability improvement".to_string(),
            evidence: vec![format!(
                "Signed decoupling {:.1}% — negative drift indicates improving aerobic durability",
                decoupling.signed_decoupling_pct
            )],
            section: "decoupling".to_string(),
        });
    }
}
