use crate::engines::coach_metrics_constants::*;

/// HRV ratio = current RMSSD / rolling 7-day baseline RMSSD.
pub fn compute_hrv_ratio(current_rmssd: f64, baseline_rmssd: f64) -> Option<f64> {
    if baseline_rmssd <= 0.0 {
        None
    } else {
        Some(current_rmssd / baseline_rmssd)
    }
}

/// Classify HRV state from RMSSD ratio.
/// suppressed: ratio < 0.88, recovering: ratio > 1.15, normal: else.
/// Source: Front. Physiol. 2025 — RMSSD clinical reliability thresholds.
pub fn classify_hrv_state(ratio: f64) -> (bool, bool) {
    let suppressed = ratio < HRV_SUPPRESSION_RATIO;
    let recovering = ratio > HRV_RECOVERY_RATIO;
    (suppressed, recovering)
}

/// Compute HRV trend slope using simple linear regression over last 7 days.
/// Returns slope (change in RMSSD per day).
pub fn compute_hrv_trend_slope(hrv_values: &[f64]) -> Option<f64> {
    if hrv_values.len() < HRV_TREND_MIN_VALUES {
        return None;
    }
    let n = hrv_values.len() as f64;
    let sum_x: f64 = (0..hrv_values.len()).map(|i| i as f64).sum();
    let sum_y: f64 = hrv_values.iter().sum();
    let sum_xy: f64 = (0..hrv_values.len())
        .map(|i| i as f64 * hrv_values[i])
        .sum();
    let sum_x2: f64 = (0..hrv_values.len()).map(|i| (i as f64) * (i as f64)).sum();

    let denominator = n * sum_x2 - sum_x * sum_x;
    if denominator.abs() < f64::EPSILON {
        return None;
    }

    Some((n * sum_xy - sum_x * sum_y) / denominator)
}

/// Composite recovery quality index: HRV ratio × 0.4 + RHR_baseline/current × 0.3 + sleep_quality × 0.3.
/// Source: Front. Physiol. 2025 — multi-domain recovery assessment.
/// Every component is clamped into the shared [`RQI_COMPONENT_MIN`]/[`RQI_COMPONENT_MAX`]
/// band (see `coach_metrics_constants`): an unclamped HRV ratio lets one
/// glitchy-baseline day explode the score (ratio 60 from a ~1 ms artifact
/// baseline would display as "24.00"), and the shared band keeps the score
/// on the same ~[0.5, 1.5] scale as its siblings.
pub fn compute_recovery_quality_index(
    hrv_ratio: f64,
    rhr_baseline: f64,
    rhr_current: f64,
    sleep_hours: f64,
) -> Option<f64> {
    if rhr_current <= 0.0 {
        return None;
    }
    // rhr_current > 0.0 is guaranteed by the guard above.
    let hrv_component = hrv_ratio.clamp(RQI_COMPONENT_MIN, RQI_COMPONENT_MAX);
    let rhr_component = (rhr_baseline / rhr_current).clamp(RQI_COMPONENT_MIN, RQI_COMPONENT_MAX);
    let sleep_component =
        (sleep_hours / IDEAL_SLEEP_HOURS).clamp(RQI_COMPONENT_MIN, RQI_COMPONENT_MAX);
    Some(
        hrv_component * RECOVERY_QUALITY_HRV_WEIGHT
            + rhr_component * RECOVERY_QUALITY_RHR_WEIGHT
            + sleep_component * RECOVERY_QUALITY_SLEEP_WEIGHT,
    )
}

pub fn compute_recovery_index(
    hrv: f64,
    resting_hr: f64,
    hrv_baseline: Option<f64>,
    resting_hr_baseline: Option<f64>,
) -> Option<f64> {
    if resting_hr <= 0.0 {
        None
    } else if let Some((hrv_baseline, resting_hr_baseline)) = hrv_baseline.zip(resting_hr_baseline)
    {
        if hrv_baseline <= 0.0 || resting_hr_baseline <= 0.0 {
            None
        } else {
            let hrv_ratio = hrv / hrv_baseline;
            let resting_hr_ratio = resting_hr / resting_hr_baseline;
            Some(hrv_ratio / resting_hr_ratio)
        }
    } else {
        Some(hrv / resting_hr)
    }
}

pub fn compute_fatigue_index(load_7d: f64, recovery_index: f64) -> Option<f64> {
    if recovery_index.abs() < f64::EPSILON {
        None
    } else {
        Some(load_7d / recovery_index)
    }
}

pub fn compute_stress_tolerance(strain: f64, monotony: f64) -> Option<f64> {
    if monotony.abs() < f64::EPSILON {
        None
    } else {
        Some((strain / monotony) / STRESS_TOLERANCE_DIVISOR)
    }
}

pub fn compute_durability_index(
    current_power_at_duration: f64,
    baseline_power_at_duration: f64,
) -> Option<f64> {
    if baseline_power_at_duration <= 0.0 {
        None
    } else {
        Some(current_power_at_duration / baseline_power_at_duration)
    }
}

/// Contract: inputs arrive on a 0–10 higher-is-better scale (mood, stress,
/// fatigue as scored in the source payload; sleep in hours clamped to
/// [0, 10]); the weighted sum therefore reads on the same 0–10 scale the
/// render bands (7.0 / 5.0) assume. Polarity is pinned by
/// `readiness_score_computes_weighted_average`: higher stress/fatigue inputs
/// raise the score, i.e. the source fields are treated as higher = better
/// state. If the upstream API ever flips a field's polarity, this function —
/// not the bands — is the place to invert it.
pub fn compute_readiness_score(
    mood: Option<f64>,
    sleep_hours: Option<f64>,
    stress: Option<f64>,
    fatigue: Option<f64>,
) -> Option<f64> {
    let mood = mood?;
    let sleep_hours = sleep_hours?;
    let stress = stress?;
    let fatigue = fatigue?;
    let normalized_sleep = sleep_hours.clamp(SLEEP_CLAMP_MIN, READINESS_SLEEP_CLAMP_MAX);
    let weighted_sum = mood * READINESS_MOOD_WEIGHT
        + normalized_sleep * READINESS_SLEEP_WEIGHT
        + stress * READINESS_STRESS_WEIGHT
        + fatigue * READINESS_FATIGUE_WEIGHT;
    Some(weighted_sum)
}
