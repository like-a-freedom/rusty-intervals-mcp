use crate::domains::coach::DecouplingMetrics;
use crate::engines::coach_metrics_constants::*;
use serde_json::Value;

use super::helpers::{average, extract_numeric_stream, get_number};

pub fn compute_efficiency_factor(hr: &[f64], output: &[f64]) -> Option<f64> {
    if hr.len() != output.len() || hr.is_empty() {
        return None;
    }

    let avg_hr = average(hr)?;
    if avg_hr <= 0.0 {
        return None;
    }

    Some(average(output)? / avg_hr)
}

pub fn compute_aerobic_decoupling(hr: &[f64], output: &[f64]) -> Option<DecouplingMetrics> {
    if hr.len() != output.len() || hr.len() < DECOUPLING_MIN_POINTS {
        return None;
    }

    let midpoint = hr.len() / 2;
    if midpoint == 0 || midpoint == hr.len() {
        return None;
    }

    let first_half = compute_efficiency_factor(&hr[..midpoint], &output[..midpoint])?;
    let second_half = compute_efficiency_factor(&hr[midpoint..], &output[midpoint..])?;
    if first_half <= 0.0 {
        return None;
    }

    let raw_pct = ((first_half - second_half) / first_half) * PCT_SCALING_FACTOR;
    let decoupling_pct = raw_pct.abs();
    let state = classify_decoupling_state(decoupling_pct);
    let durability_state = classify_durability_state(raw_pct, decoupling_pct);

    Some(DecouplingMetrics {
        efficiency_factor_first_half: Some(first_half),
        efficiency_factor_second_half: Some(second_half),
        decoupling_pct,
        state,
        signed_decoupling_pct: raw_pct,
        durability_state,
        z2_hr_variance: None,
    })
}

pub fn classify_durability_state(signed_pct: f64, abs_pct: f64) -> String {
    if abs_pct > DECOUPLING_DRIFT_ABS_THRESHOLD || signed_pct > DECOUPLING_DRIFT_SIGNED_THRESHOLD {
        "drifting".to_string()
    } else if signed_pct < DECOUPLING_IMPROVING_THRESHOLD {
        "improving".to_string()
    } else if signed_pct.abs() <= DECOUPLING_STABLE_THRESHOLD {
        "stable".to_string()
    } else {
        "watch".to_string()
    }
}

fn classify_decoupling_state(decoupling_pct: f64) -> String {
    if decoupling_pct <= DECOUPLING_ACCEPTABLE_PCT {
        "acceptable".to_string()
    } else if decoupling_pct <= DECOUPLING_WATCH_PCT {
        "watch".to_string()
    } else {
        "high".to_string()
    }
}

fn parse_efficiency_factor(detail: Option<&Value>) -> Option<f64> {
    let object = detail?.as_object()?;
    get_number(object, EFFICIENCY_FACTOR_KEYS)
}

fn parse_aerobic_decoupling(detail: Option<&Value>) -> Option<DecouplingMetrics> {
    let object = detail?.as_object()?;
    let decoupling_pct = get_number(object, AEROBIC_DECOUPLING_KEYS)?;

    Some(DecouplingMetrics {
        efficiency_factor_first_half: None,
        efficiency_factor_second_half: None,
        decoupling_pct,
        state: classify_decoupling_state(decoupling_pct),
        signed_decoupling_pct: decoupling_pct,
        durability_state: "unknown".into(),
        z2_hr_variance: None,
    })
}

pub fn derive_execution_metrics_from_streams(
    streams: Option<&Value>,
) -> (Option<f64>, Option<DecouplingMetrics>) {
    let Some(streams) = streams else {
        return (None, None);
    };

    let hr = extract_numeric_stream(streams, HR_STREAM_KEYS);
    let output = extract_numeric_stream(streams, OUTPUT_STREAM_KEYS);

    match (hr, output) {
        (Some(hr), Some(output)) => {
            let efficiency_factor = compute_efficiency_factor(&hr, &output);
            let decoupling = compute_aerobic_decoupling(&hr, &output);
            (efficiency_factor, decoupling)
        }
        _ => (None, None),
    }
}

pub fn derive_execution_metrics(
    detail: Option<&Value>,
    streams: Option<&Value>,
) -> (Option<f64>, Option<DecouplingMetrics>) {
    let (stream_efficiency_factor, stream_decoupling) =
        derive_execution_metrics_from_streams(streams);

    (
        parse_efficiency_factor(detail).or(stream_efficiency_factor),
        parse_aerobic_decoupling(detail).or(stream_decoupling),
    )
}
