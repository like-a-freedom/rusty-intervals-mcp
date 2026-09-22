//! Recovery-metric rows for the `assess_recovery` table.
//!
//! The intent renders a human-readable three-column table (metric, value,
//! status) summarising the recovery state. `build_recovery_metric_rows`
//! produces that table from the wellness and fitness payloads, using
//! thresholds from [`crate::engines::coach_guidance`] for the status icons.

use crate::domains::coach::{FitnessMetrics, WellnessMetrics};

/// Render a maybe-missing metric: `None` becomes `n/a` instead of a fabricated
/// zero (e.g. `0.0 hrs` sleep reads as measured, not missing).
fn render_optional(
    value: Option<f64>,
    render: impl FnOnce(f64) -> (String, String),
) -> (String, String) {
    value
        .map(render)
        .unwrap_or_else(|| ("n/a".into(), "n/a".into()))
}

/// Physiologically positive-only metrics: zero (or negative) is missing data,
/// not a measurement. TSB is excluded — 0.0 is a legitimate balanced value.
fn positive(value: Option<f64>) -> Option<f64> {
    value.filter(|v| *v > 0.0)
}

/// Build the recovery-metric rows for the assess_recovery table.
///
/// The first four rows (Avg Sleep, Resting HR, HRV, TSB) are always emitted.
/// Remaining rows (CTL, ATL, Ramp Rate, HRV Trend Slope, Recovery Quality,
/// HRV Suppression, Recovery Index, Readiness Score, Mood/Stress/Fatigue)
/// are emitted conditionally based on which fields are present.
pub(super) fn build_recovery_metric_rows(
    wellness: &WellnessMetrics,
    fitness: &FitnessMetrics,
) -> Vec<Vec<String>> {
    let (sleep_value, sleep_status) =
        render_optional(positive(wellness.avg_sleep_hours), |avg_sleep| {
            let status = if avg_sleep >= crate::engines::coach_guidance::SLEEP_GOOD_HOURS {
                "✅ Good"
            } else if avg_sleep >= crate::engines::coach_guidance::SLEEP_FAIR_MIN_HOURS {
                "⚠️ Fair"
            } else {
                "❌ Poor"
            };
            (format!("{avg_sleep:.1} hrs"), status.into())
        });

    let (rhr_value, rhr_status) =
        render_optional(positive(wellness.avg_resting_hr), |resting_hr| {
            let status = if resting_hr <= crate::engines::coach_guidance::RHR_NORMAL_BPM {
                "✅ Normal"
            } else if resting_hr <= crate::engines::coach_guidance::RHR_ELEVATED_MAX_BPM {
                "⚠️ Elevated"
            } else {
                "❌ High"
            };
            (format!("{} bpm", resting_hr as u32), status.into())
        });

    let (hrv_value, hrv_status) = render_optional(positive(wellness.avg_hrv), |hrv| {
        let status = match wellness.hrv_trend_state.as_deref() {
            Some("suppressed") => "❌ Suppressed vs personal baseline",
            Some("below_range") => "⚠️ Below personal baseline",
            Some("within_range") => "✅ Within personal range",
            // Non-positive HRV is normalized to missing by `positive()` above.
            _ => "⚪ Build personal baseline",
        };
        (format!("{hrv:.0} ms"), status.into())
    });

    let (tsb_value, tsb_status) = render_optional(fitness.tsb, |tsb| {
        let status = if tsb > crate::engines::coach_guidance::TSB_FRESH {
            "✅ Fresh"
        } else if tsb > crate::engines::coach_guidance::TSB_FATIGUED {
            "⚪ Balanced"
        } else {
            "❌ Fatigued"
        };
        (format!("{tsb:.0}"), status.into())
    });

    let mut rows = vec![
        vec!["Avg Sleep".into(), sleep_value, sleep_status],
        vec!["Resting HR".into(), rhr_value, rhr_status],
        vec!["HRV".into(), hrv_value, hrv_status],
        vec!["TSB".into(), tsb_value, tsb_status],
    ];

    if let Some(ctl) = fitness.ctl {
        rows.push(vec!["CTL".into(), format!("{:.0}", ctl), "".into()]);
    }
    if let Some(atl) = fitness.atl {
        rows.push(vec!["ATL".into(), format!("{:.0}", atl), "".into()]);
    }
    if let Some(rr) = fitness.ramp_rate {
        rows.push(vec![
            "Ramp Rate".into(),
            format!("{:+.1}/wk", rr),
            "".into(),
        ]);
    }

    if let Some(trend_slope) = wellness.hrv_trend_slope {
        let status = if trend_slope > 0.0 {
            "↗ Improving"
        } else {
            "↘ Declining"
        };
        rows.push(vec![
            "HRV Trend Slope".into(),
            format!("{:.2}", trend_slope),
            status.into(),
        ]);
    }

    if let Some(rqi) = wellness.recovery_quality_index {
        let rqi_status = if rqi >= 0.8 {
            "✅ Good"
        } else if rqi >= 0.5 {
            "⚠️ Fair"
        } else {
            "❌ Low"
        };
        rows.push(vec![
            "Recovery Quality".into(),
            format!("{:.2}", rqi),
            rqi_status.into(),
        ]);
    }

    if wellness.hrv_suppression_flag {
        rows.push(vec![
            "HRV Suppression".into(),
            "detected".into(),
            "❌ Suppressed".into(),
        ]);
    }

    if let Some(recovery_index) = wellness.recovery_index {
        let recovery_status = if recovery_index >= 1.2 {
            "✅ Supportive"
        } else if recovery_index >= 0.9 {
            "⚠️ Watch"
        } else {
            "❌ Low"
        };
        rows.push(vec![
            "Recovery Index".into(),
            format!("{:.2}", recovery_index),
            recovery_status.into(),
        ]);
    }

    if let Some(readiness_score) = wellness.readiness_score {
        let readiness_status = if readiness_score >= 7.0 {
            "✅ Supportive"
        } else if readiness_score >= 5.0 {
            "⚠️ Watch"
        } else {
            "❌ Low"
        };
        rows.push(vec![
            "Readiness Score".into(),
            format!("{:.1}", readiness_score),
            readiness_status.into(),
        ]);
    }

    if let (Some(mood), Some(stress), Some(fatigue)) =
        (wellness.avg_mood, wellness.avg_stress, wellness.avg_fatigue)
    {
        rows.push(vec!["Mood".into(), format!("{:.0}/10", mood), "".into()]);
        rows.push(vec![
            "Stress".into(),
            format!("{:.0}/10", stress),
            "".into(),
        ]);
        rows.push(vec![
            "Fatigue".into(),
            format!("{:.0}/10", fatigue),
            "".into(),
        ]);
    }

    rows
}
