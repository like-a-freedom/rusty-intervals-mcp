use crate::domains::coach::{EspeDerivedMetrics, EspePowerAnchors};
use crate::engines::adaptation::{AdaptationState, classify_adaptation};
use crate::engines::coach_metrics_constants::*;
use serde_json::Value;

use super::helpers::get_number;

/// Extract eFTP / W′ / pMax from wellness `sportInfo[]` entries.
/// Multi-sport aware: iterates all entries, first-non-null-wins per field.
pub fn extract_sportinfo_anchors(wellness_payload: Option<&Value>) -> EspePowerAnchors {
    let Some(entries) = wellness_payload.and_then(|v| v.as_array()) else {
        return EspePowerAnchors::unsupported();
    };

    let mut eftp: Option<f64> = None;
    let mut w_prime: Option<f64> = None;
    let mut p_max: Option<f64> = None;
    let source = String::from("sportinfo");

    for entry in entries {
        if let Some(obj) = entry.as_object() {
            if eftp.is_none() {
                eftp = get_number(obj, &["eftp"]);
            }
            if w_prime.is_none() {
                w_prime = get_number(obj, &["wPrime", "w_prime"]);
            }
            if p_max.is_none() {
                p_max = get_number(obj, &["pMax", "p_max"]);
            }
        }
    }

    if eftp.is_none() && w_prime.is_none() && p_max.is_none() {
        return EspePowerAnchors::unsupported();
    }

    EspePowerAnchors {
        eftp,
        w_prime,
        p_max,
        source,
        supported: true,
    }
}

/// Fallback anchor extraction from activity detail fields.
/// Priority: pm_ > icu_rolling_ > ss_
pub fn enrich_anchors_from_activity(
    anchors: &mut EspePowerAnchors,
    activity_detail: Option<&Value>,
) {
    let Some(obj) = activity_detail.and_then(|v| v.as_object()) else {
        return;
    };

    if anchors.eftp.is_none() {
        anchors.eftp = get_number(obj, &["icu_pm_ftp", "icu_rolling_ftp", "ss_ftp"]);
    }
    if anchors.w_prime.is_none() {
        anchors.w_prime = get_number(
            obj,
            &["icu_pm_w_prime", "icu_rolling_w_prime", "ss_w_prime"],
        );
    }
    if anchors.p_max.is_none() {
        anchors.p_max = get_number(obj, &["icu_pm_p_max", "icu_rolling_p_max", "ss_p_max"]);
    }

    if anchors.eftp.is_some() || anchors.w_prime.is_some() || anchors.p_max.is_some() {
        anchors.supported = true;
        anchors.source = String::from("activity");
    }
}

/// Compute derived ESPE metrics from available power-curve anchors + MMP data.
pub fn derive_espe_metrics(
    anchors: &EspePowerAnchors,
    mmp_p1m: Option<f64>,
    mmp_p5m: Option<f64>,
    mmp_p20m: Option<f64>,
    mmp_p60m: Option<f64>,
) -> EspeDerivedMetrics {
    let eftp = anchors.eftp;
    let _w_prime = anchors.w_prime;
    let _p_max = anchors.p_max;

    // glycolytic_bias_ratio = P1m / P20m (plan spec: P0.1)
    let glycolytic_bias = mmp_p1m.zip(mmp_p20m).map(|(p1, p20)| p1 / p20);
    // aerobic_durability_ratio = P60m / P5m
    let aerobic_durability = mmp_p60m.zip(mmp_p5m).map(|(p60, p5)| p60 / p5);
    // durability_gradient = P60m / P20m
    let durability_gradient = mmp_p60m.zip(mmp_p20m).map(|(p60, p20)| p60 / p20);
    // balance_score: deviation from ideal P1m/P20m ratio
    // Source: power profiling literature (Vilela et al., JSCR 2023)
    let balance_score = mmp_p1m.zip(mmp_p20m).map(|(p1, p20)| {
        let ratio = p1 / p20;
        ratio - IDEAL_P1M_P20M_RATIO
    });
    // vo2_reserve_ratio = P5m / eFTP
    let vo2_reserve_ratio = mmp_p5m.zip(eftp).map(|(p5, f)| p5 / f);

    EspeDerivedMetrics {
        glycolytic_bias,
        aerobic_durability,
        durability_gradient,
        balance_score,
        vo2_reserve_ratio,
        p1m: mmp_p1m,
        p5m: mmp_p5m,
        p20m: mmp_p20m,
        p60m: mmp_p60m,
        supported: eftp.is_some() || anchors.p_max.is_some(),
        adaptation_state: None,
    }
}

/// Compare two power curve windows and compute deltas per anchor.
/// Returns (deltas, rotation_index, system_statuses, adaptation_state).
pub fn compare_power_curves(
    current: &EspeDerivedMetrics,
    previous: &EspeDerivedMetrics,
) -> (
    std::collections::HashMap<String, f64>,
    f64,
    std::collections::HashMap<String, String>,
    Option<String>,
) {
    let mut deltas = std::collections::HashMap::new();
    let mut statuses = std::collections::HashMap::new();

    let anchors = [
        ("1m", current.p1m, previous.p1m),
        ("5m", current.p5m, previous.p5m),
        ("20m", current.p20m, previous.p20m),
        ("60m", current.p60m, previous.p60m),
    ];

    for (name, cur, prev) in anchors {
        if let (Some(c), Some(p)) = (cur, prev)
            && p.abs() > 0.0
        {
            let delta = ((c - p) / p) * PCT_SCALING_FACTOR;
            deltas.insert(name.to_string(), delta);
            let status = if delta < POWER_CURVE_DECLINE_THRESHOLD {
                "decline"
            } else if delta < POWER_CURVE_STABLE_THRESHOLD {
                "stable"
            } else if delta < POWER_CURVE_MILD_GAIN_PCT {
                "mild_gain"
            } else if delta < POWER_CURVE_MODERATE_GAIN_PCT {
                "moderate_gain"
            } else {
                "strong_gain"
            };
            statuses.insert(name.to_string(), status.to_string());
        }
    }

    let rotation_index = current
        .p1m
        .zip(current.p5m)
        .zip(current.p20m.zip(current.p60m))
        .map(|((p1, p5), (p20, p60))| {
            ((p1 + p5) / POWER_CURVE_ROTATION_AVERAGE)
                - ((p20 + p60) / POWER_CURVE_ROTATION_AVERAGE)
        })
        .unwrap_or(0.0);

    let adaptation_state = classify_adaptation(
        deltas.get("20m").copied(),
        deltas.get("5m").copied(),
        deltas.get("60m").copied(),
        deltas.get("1m").copied(),
        deltas.get("1m").copied(),
    );
    let adaptation_state_str = if adaptation_state != AdaptationState::Baseline {
        Some(format!("{:?}", adaptation_state))
    } else {
        None
    };

    (deltas, rotation_index, statuses, adaptation_state_str)
}
