use crate::domains::coach::{AcwrMetrics, LoadManagementMetrics};
use crate::engines::coach_metrics_constants::*;
use serde_json::Value;

use super::helpers::get_number;
use super::recovery::{compute_fatigue_index, compute_stress_tolerance};

#[must_use]
pub fn compute_acwr(loads: &[f64]) -> Option<AcwrMetrics> {
    if loads.len() < ACWR_MIN_LOOKBACK_DAYS {
        return None;
    }

    let acute_lambda = ACWR_ACUTE_LAMBDA;
    let chronic_lambda = ACWR_CHRONIC_LAMBDA;

    let mut acute_load = *loads.first()?;
    let mut chronic_load = acute_load;

    for load in loads.iter().skip(1) {
        acute_load = (acute_lambda * load) + ((1.0 - acute_lambda) * acute_load);
        chronic_load = (chronic_lambda * load) + ((1.0 - chronic_lambda) * chronic_load);
    }

    build_acwr_metrics(acute_load, chronic_load)
}

#[must_use]
pub fn parse_api_load_snapshot(payload: Option<&Value>) -> Option<AcwrMetrics> {
    let object = payload?.as_object()?;
    let acute_load = get_number(object, API_LOAD_ACUTE_KEYS)?;
    let chronic_load = get_number(object, API_LOAD_CHRONIC_KEYS)?;

    build_acwr_metrics(acute_load, chronic_load)
}

fn build_acwr_metrics(acute_load: f64, chronic_load: f64) -> Option<AcwrMetrics> {
    if chronic_load.abs() < f64::EPSILON {
        return None;
    }

    let ratio = acute_load / chronic_load;
    let state = classify_acwr_ratio(ratio);

    Some(AcwrMetrics {
        acute_load,
        chronic_load,
        ratio,
        state: state.to_string(),
    })
}

fn classify_acwr_ratio(ratio: f64) -> &'static str {
    if ratio < ACWR_SAFE_LOWER {
        "underloaded"
    } else if ratio <= ACWR_SAFE_UPPER {
        "productive"
    } else if ratio <= ACWR_WATCH_RATIO {
        "watch"
    } else {
        "overreaching"
    }
}

#[must_use]
pub fn compute_monotony(loads_7d: &[f64]) -> Option<f64> {
    if loads_7d.len() < MONOTONY_MIN_LOOKBACK_DAYS {
        return None;
    }

    let mean = loads_7d.iter().sum::<f64>() / loads_7d.len() as f64;
    if mean <= 0.0 {
        return None;
    }

    let variance = loads_7d
        .iter()
        .map(|load| {
            let delta = load - mean;
            delta * delta
        })
        .sum::<f64>()
        / loads_7d.len() as f64;

    // Use population stddev; floor stddev to 10% of mean to avoid infinity when all loads are identical.
    // Real-world identical loads → monotony is high but finite (cap at MONOTONY_CAP).
    let stddev = variance.sqrt().max(mean * MONOTONY_STDDEV_FLOOR_RATIO);
    Some((mean / stddev).min(MONOTONY_CAP))
}

#[must_use]
pub fn compute_strain(loads_7d: &[f64], monotony: f64) -> f64 {
    loads_7d.iter().sum::<f64>() * monotony
}
pub fn compute_load_management_metrics(
    loads: &[f64],
    recovery_index: Option<f64>,
) -> Option<LoadManagementMetrics> {
    if loads.is_empty() {
        return None;
    }

    let acwr = compute_acwr(loads);
    let last_seven = if loads.len() >= 7 {
        &loads[loads.len() - LOAD_MGMT_WINDOW_DAYS..]
    } else {
        return Some(LoadManagementMetrics {
            acwr,
            monotony: None,
            strain: None,
            fatigue_index: None,
            stress_tolerance: None,
            durability_index: None,
        });
    };

    let monotony = compute_monotony(last_seven);
    let strain = monotony.map(|value| compute_strain(last_seven, value));
    let stress_tolerance = monotony
        .zip(strain)
        .and_then(|(m, s)| compute_stress_tolerance(s, m));
    let load_7d = last_seven.iter().sum::<f64>();
    let fatigue_index = recovery_index.and_then(|ri| compute_fatigue_index(load_7d, ri));

    Some(LoadManagementMetrics {
        acwr,
        monotony,
        strain,
        fatigue_index,
        stress_tolerance,
        durability_index: None,
    })
}
