use crate::domains::coach::WdrMetrics;
use crate::engines::coach_metrics_constants::*;
use serde_json::Value;
use std::collections::HashMap;

use super::helpers::get_number;

/// Primary: max(wbal_start - wbal_end) across intervals; fallback: icu_max_wbal_depletion.
pub fn compute_wdr_metrics(
    intervals: Option<&Value>,
    activity_detail: Option<&Value>,
    w_prime: Option<f64>,
) -> crate::domains::coach::WdrMetrics {
    let intervals_arr = match intervals.and_then(Value::as_array) {
        Some(arr) if !arr.is_empty() => arr,
        _ => {
            // Fallback: use icu_max_wbal_depletion from activity detail
            let detail_obj = match activity_detail.and_then(Value::as_object) {
                Some(obj) => obj,
                None => return WdrMetrics::unsupported(),
            };
            let max_depletion = match get_number(
                detail_obj,
                &["icu_max_wbal_depletion", "max_wbal_depletion"],
            ) {
                Some(v) => v,
                None => return WdrMetrics::unsupported(),
            };
            let depletion_pct = w_prime
                .filter(|w| *w > 0.0)
                .map(|w| (max_depletion / w).clamp(0.0, WDRM_MAX_DEPLETION_PCT));
            let joules_above_ftp =
                get_number(detail_obj, &["icu_joules_above_ftp", "joules_above_ftp"]);
            return WdrMetrics {
                supported: true,
                max_wbal_depletion: Some(max_depletion),
                joules_above_ftp,
                depletion_pct,
                ..Default::default()
            };
        }
    };

    // Primary: wbal_start - wbal_end across intervals
    let mut max_depletion: f64 = 0.0;
    let mut joules_above_ftp: f64 = 0.0;
    let mut found_any = false;

    for interval in intervals_arr.iter().filter_map(Value::as_object) {
        if let Some(start) = get_number(interval, &["wbal_start"])
            && let Some(end) = get_number(interval, &["wbal_end"])
        {
            let depletion = start - end;
            if depletion > 0.0 {
                max_depletion = max_depletion.max(depletion);
                found_any = true;
            }
        }
        if let Some(joules) = get_number(interval, &["joules_above_ftp"]) {
            joules_above_ftp = joules_above_ftp.max(joules);
            found_any = true;
        }
    }

    // Also check top-level activity detail for joules_above_ftp
    if let Some(detail_obj) = activity_detail.and_then(Value::as_object)
        && let Some(joules) = get_number(detail_obj, &["icu_joules_above_ftp", "joules_above_ftp"])
    {
        joules_above_ftp = joules_above_ftp.max(joules);
        found_any = true;
    }

    if !found_any {
        return WdrMetrics::unsupported();
    }

    let depletion_pct = w_prime
        .filter(|w| *w > 0.0)
        .map(|w| (max_depletion / w).clamp(0.0, WDRM_MAX_DEPLETION_PCT));

    let joules = if joules_above_ftp > 0.0 {
        Some(joules_above_ftp)
    } else {
        None
    };

    WdrMetrics {
        supported: true,
        max_wbal_depletion: Some(max_depletion),
        joules_above_ftp: joules,
        depletion_pct,
        ..Default::default()
    }
}

/// Compute WDR 7-day rollup from period activities.
///
/// Iterates activity details and aggregates `icu_max_wbal_depletion` values
/// into a period-level WDR summary with mean depletion percentage, count of
/// high-depletion sessions, and total sessions with data.
pub fn compute_wdr_7d_rollup(
    activity_details: &HashMap<String, Value>,
    activity_ids: &[String],
    w_prime: Option<f64>,
) -> crate::domains::coach::WdrMetrics {
    use crate::engines::coach_metrics_constants::{
        WDRM_HIGH_DEPLETION_PCT, WDRM_MAX_DEPLETION_PCT,
    };

    let mut depletion_pcts: Vec<f64> = Vec::new();
    let mut high_depletion_count: usize = 0;
    let mut sessions_with_data: usize = 0;

    for id in activity_ids {
        let Some(detail) = activity_details.get(id).and_then(Value::as_object) else {
            continue;
        };

        let max_depletion = get_number(detail, &["icu_max_wbal_depletion", "max_wbal_depletion"]);

        if let Some(depletion) = max_depletion {
            sessions_with_data += 1;

            if let Some(wp) = w_prime.filter(|w| *w > 0.0) {
                let pct = (depletion / wp).clamp(0.0, WDRM_MAX_DEPLETION_PCT);
                depletion_pcts.push(pct);
                if pct >= WDRM_HIGH_DEPLETION_PCT {
                    high_depletion_count += 1;
                }
            }
        }
    }

    let mean_depletion_pct_7d = if !depletion_pcts.is_empty() {
        Some(depletion_pcts.iter().sum::<f64>() / depletion_pcts.len() as f64)
    } else {
        None
    };

    WdrMetrics {
        supported: sessions_with_data > 0,
        mean_depletion_pct_7d,
        high_depletion_sessions_7d: high_depletion_count,
        sessions_with_data_7d: sessions_with_data,
        ..Default::default()
    }
}
