use crate::domains::coach::{
    AcwrMetrics, DecouplingMetrics, EspeDerivedMetrics, EspePowerAnchors, FitnessMetrics,
    HeatMetrics, LoadManagementMetrics, NdliMetrics, TrendMetrics, VolumeMetrics, WellnessMetrics,
    WorkoutMetricsContext,
};
use intervals_icu_client::ActivitySummary;
use serde_json::Value;
use std::collections::HashMap;

const API_LOAD_ACUTE_KEYS: &[&str] = &["atlLoad", "icu_atl"];
const API_LOAD_CHRONIC_KEYS: &[&str] = &["ctlLoad", "icu_ctl"];
const FITNESS_CTL_KEYS: &[&str] = &["fitness", "ctl"];
const FITNESS_ATL_KEYS: &[&str] = &["fatigue", "atl"];
const FITNESS_TSB_KEYS: &[&str] = &["form", "tsb"];
const SLEEP_KEYS: &[&str] = &["sleep_hours", "sleepSecs", "sleep_secs"];
const RESTING_HR_KEYS: &[&str] = &["resting_hr", "restingHR", "resting_hr_bpm", "avgSleepingHR"];
const HRV_KEYS: &[&str] = &["hrv"];
const MOOD_KEYS: &[&str] = &["mood"];
const STRESS_KEYS: &[&str] = &["stress"];
const FATIGUE_KEYS: &[&str] = &["fatigue"];
const READINESS_KEYS: &[&str] = &["readiness"];
const RECENT_WELLNESS_WINDOW: usize = 7;
const HRV_BASELINE_WINDOW: usize = 28;
const HRV_WATCH_DROP_PCT: f64 = -6.0;
const HRV_SUPPRESSED_DROP_PCT: f64 = -12.0;
const EFFICIENCY_FACTOR_KEYS: &[&str] = &["icu_efficiency_factor", "efficiency_factor"];
const AEROBIC_DECOUPLING_KEYS: &[&str] = &["decoupling", "aerobic_decoupling"];
const HR_STREAM_KEYS: &[&str] = &["heartrate", "heart_rate", "hr"];
const OUTPUT_STREAM_KEYS: &[&str] = &["watts", "velocity_smooth", "pace"];
const MONOTONY_STDDEV_FLOOR_RATIO: f64 = 0.1;
const MONOTONY_CAP: f64 = 10.0;

// =============================================================================
// P0 — Performance Intelligence Constants
// =============================================================================

/// NDLI: Joules above FTP threshold for high-intensity day classification (cycling).
/// Source: Montis.icu Coach V5 validation (20 kJ ≈ 2×3min VO2max effort at 400W).
const NDLI_HIGH_INTENSITY_JOULES_THRESHOLD: f64 = 20000.0;

/// NDLI: Running fallback — TSS/day proxy for high-intensity when joules_above_ftp is null.
/// Scaled relative to typical CTL: 80 TSS ≈ ~1.2× CTL for moderate athletes.
const NDLI_RUNNING_TSS_PROXY_THRESHOLD: f64 = 80.0;

/// NDLI: IF normalization threshold — values > 2.0 are assumed to be % not decimal.
const NDLI_IF_NORMALIZATION_THRESHOLD: f64 = 2.0;

/// NDLI: Days threshold for "red" (overload) state.
const NDLI_RED_DAYS: usize = 4;

/// NDLI: Days threshold for "amber" (watch) state.
const NDLI_AMBER_DAYS: usize = 3;

/// WDRM: High depletion session threshold (60% of W′).
const WDRM_HIGH_DEPLETION_PCT: f64 = 0.60;

/// WDRM: Maximum depletion percentage clip (150% of W′).
const WDRM_MAX_DEPLETION_PCT: f64 = 1.5;

/// WDRM: Minimum Z2 points for HR variance computation.
const Z2_MIN_POINTS: usize = 10;

// =============================================================================
// P1 — Coaching Intelligence Constants
// =============================================================================

/// HRV: Suppression threshold (ratio < 0.88 × baseline).
/// Source: Front. Physiol. 2025, Nature Sci Reports 2025 — RMSSD clinical reliability.
const HRV_SUPPRESSION_RATIO: f64 = 0.88;

/// HRV: Recovery threshold (ratio > 1.15 × baseline).
const HRV_RECOVERY_RATIO: f64 = 1.15;

/// HRV: Minimum trend values for slope computation.
const HRV_TREND_MIN_VALUES: usize = 3;

/// HRV: Ideal sleep hours for recovery quality normalization.
const IDEAL_SLEEP_HOURS: f64 = 8.0;

/// HRV: Recovery quality component weights (HRV × 0.4 + RHR × 0.3 + sleep × 0.3).
const RECOVERY_QUALITY_HRV_WEIGHT: f64 = 0.4;
const RECOVERY_QUALITY_RHR_WEIGHT: f64 = 0.3;
const RECOVERY_QUALITY_SLEEP_WEIGHT: f64 = 0.3;

/// HRV: RHR component clamp bounds.
const RHR_COMPONENT_MIN: f64 = 0.5;
const RHR_COMPONENT_MAX: f64 = 1.5;

/// HRV: Sleep component clamp bounds.
const SLEEP_COMPONENT_MIN: f64 = 0.5;
const SLEEP_COMPONENT_MAX: f64 = 1.5;

/// Volume: seconds per hour for duration conversion.
const SECONDS_PER_HOUR: f64 = 3600.0;

/// Volume: days per week for workload normalization.
const DAYS_PER_WEEK: f64 = 7.0;

/// ACWR: Acute EWMA lambda = 2 / (N + 1) with N = 7.
const ACWR_ACUTE_LAMBDA: f64 = 2.0 / 8.0;

/// ACWR: Chronic EWMA lambda = 2 / (N + 1) with N = 28.
const ACWR_CHRONIC_LAMBDA: f64 = 2.0 / 29.0;

/// ACWR: ACWR "watch" threshold (ratio ≤ 1.5).
/// Above ACWR safe upper (1.3) up to 1.5 = watch zone.
const ACWR_WATCH_RATIO: f64 = 1.5;

/// ACWR: safe zone lower bound (ratio ≥ 0.8 = underload risk).
const ACWR_SAFE_LOWER: f64 = 0.8;

/// ACWR: safe zone upper bound (ratio ≤ 1.3 = overreach risk).
const ACWR_SAFE_UPPER: f64 = 1.3;

/// Decoupling: minimum data points for valid split-half analysis.
const DECOUPLING_MIN_POINTS: usize = 4;

/// Decoupling: durability drift — absolute % threshold.
const DECOUPLING_DRIFT_ABS_THRESHOLD: f64 = 8.0;

/// Decoupling: durability drift — signed % threshold.
const DECOUPLING_DRIFT_SIGNED_THRESHOLD: f64 = 5.0;

/// Decoupling: durability improving — signed % below this = improving.
const DECOUPLING_IMPROVING_THRESHOLD: f64 = -2.0;

/// Decoupling: durability stable — signed % within this = stable.
const DECOUPLING_STABLE_THRESHOLD: f64 = 3.0;

/// Decoupling: acceptable decoupling threshold (%).
const DECOUPLING_ACCEPTABLE_PCT: f64 = 5.0;

/// Decoupling: watch decoupling threshold (%).
const DECOUPLING_WATCH_PCT: f64 = 10.0;

/// Consistency: excellent Pearson R threshold.
const CONSISTENCY_EXCELLENT_THRESHOLD: f64 = 0.9;

/// Consistency: good Pearson R threshold.
const CONSISTENCY_GOOD_THRESHOLD: f64 = 0.7;

/// Consistency: moderate Pearson R threshold.
const CONSISTENCY_MODERATE_THRESHOLD: f64 = 0.5;

/// Polarisation: threshold biased index threshold.
const POLARISATION_BIASED_THRESHOLD: f64 = 0.75;

/// Wellness: sleep seconds heuristic threshold (if value > 24h, assume seconds not hours).
const WELLNESS_SLEEP_HEURISTIC_THRESHOLD: f64 = 24.0;

/// Wellness: default sleep hours when no sleep data available.
const WELLNESS_DEFAULT_SLEEP_HOURS: f64 = 7.0;

/// Rounding: decimal precision factor (10 → 1 decimal).
const ROUNDING_DECIMAL_FACTOR: f64 = 10.0;

/// Readiness: mood weight in composite score.
const READINESS_MOOD_WEIGHT: f64 = 0.3;

/// Readiness: sleep weight in composite score.
const READINESS_SLEEP_WEIGHT: f64 = 0.3;

/// Readiness: stress weight in composite score.
const READINESS_STRESS_WEIGHT: f64 = 0.2;

/// Readiness: fatigue weight in composite score.
const READINESS_FATIGUE_WEIGHT: f64 = 0.2;

/// Readiness: sleep hours clamp upper bound.
const READINESS_SLEEP_CLAMP_MAX: f64 = 10.0;

// =============================================================================
// P2 — Ultra-Sport Constants
// =============================================================================

/// Heat: baseline temperature (°C) below which heat stress is zero.
/// Source: Cheuvront & Kenefick — 18°C threshold for uncompensable heat stress.
const HEAT_BASELINE_TEMP_C: f64 = 18.0;

/// Heat: temperature range for normalization (18°C to 23°C = 5°C span).
const HEAT_NORMALIZATION_RANGE_C: f64 = 5.0;

/// Heat: high stress threshold (heat_index > 1.0).
const HEAT_HIGH_THRESHOLD: f64 = 1.0;

/// Heat: moderate stress threshold (heat_index ≥ 0.5).
const HEAT_MODERATE_THRESHOLD: f64 = 0.5;

/// Heat: index clamp maximum (capped at 2.0 for safety).
const HEAT_INDEX_CLAMP_MAX: f64 = 2.0;

/// Running power: Stryd device correction factor (Stryd reports ~8% higher than reference).
const STRYD_POWER_CORRECTION: f64 = 0.92;

/// Running power: Garmin Running Power correction factor.
const GARMIN_RP_POWER_CORRECTION: f64 = 1.08;

/// GAP to running power: cubic speed coefficient (W = speed³ × k).
/// Approximation based on air resistance + metabolic cost model.
const GAP_POWER_COEFFICIENT: f64 = 0.25;

/// GAP to running power: uphill elevation multiplier per % gradient.
const GAP_UPHILL_GRADIENT_FACTOR: f64 = 0.08;

/// GAP to running power: downhill elevation multiplier per % gradient.
const GAP_DOWNHILL_GRADIENT_FACTOR: f64 = 0.02;

/// Ideal P1m/P20m ratio for well-rounded cyclists.
/// Source: Power profiling literature (Vilela et al., JSCR 2023).
const IDEAL_P1M_P20M_RATIO: f64 = 1.8;

/// Power curve comparison: mild gain threshold (%).
const POWER_CURVE_MILD_GAIN_PCT: f64 = 3.0;

/// Power curve comparison: moderate gain threshold (%).
const POWER_CURVE_MODERATE_GAIN_PCT: f64 = 5.0;

/// Forecast: TSB load pressure threshold (also used in interpret_fitness_metrics).
const TSB_LOAD_PRESSURE_THRESHOLD: f64 = -10.0;

/// Forecast: TSB balanced upper (also used in interpret_fitness_metrics).
const TSB_BALANCED_UPPER: f64 = 10.0;

// =============================================================================
// Bare-literal extractions (previously inline magic numbers)
// =============================================================================

/// Minimum weeks floor guard for volume calculations.
const WEEKS_FLOOR_MIN: f64 = 1.0;
/// Minimum lookback days for ACWR computation (28 = 4 weeks).
const ACWR_MIN_LOOKBACK_DAYS: usize = 28;
/// Minimum lookback days for monotony computation (7 = 1 week).
const MONOTONY_MIN_LOOKBACK_DAYS: usize = 7;
/// Rolling window days for load management metrics.
const LOAD_MGMT_WINDOW_DAYS: usize = 7;
/// Divisor for stress_tolerance scaling to meaningful units.
const STRESS_TOLERANCE_DIVISOR: f64 = 100.0;
/// Sleep hours lower clamp for readiness score.
const SLEEP_CLAMP_MIN: f64 = 0.0;
/// Percentage scaling factor for delta calculations.
const PCT_SCALING_FACTOR: f64 = 100.0;
/// Seiler polarisation model denominator factor (z1+z3)/(2*z2).
const POLARISATION_DENOMINATOR_FACTOR: f64 = 2.0;
/// Power curve decline threshold (< this % = decline).
const POWER_CURVE_DECLINE_THRESHOLD: f64 = -1.0;
/// Power curve stable upper threshold (< this % = stable).
const POWER_CURVE_STABLE_THRESHOLD: f64 = 1.0;
/// Power curve rotation index averaging factor.
const POWER_CURVE_ROTATION_AVERAGE: f64 = 2.0;
/// Running power gap model: speed exponent (cubic = 3).
const GAP_SPEED_EXPONENT: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendSnapshot {
    pub activity_count: usize,
    pub total_time_secs: i64,
    pub total_distance_m: f64,
    pub total_elevation_m: f64,
}

#[must_use]
pub fn build_trend_snapshot(
    activities: &[&ActivitySummary],
    details: &HashMap<String, Value>,
) -> TrendSnapshot {
    activities.iter().fold(
        TrendSnapshot {
            activity_count: activities.len(),
            total_time_secs: 0,
            total_distance_m: 0.0,
            total_elevation_m: 0.0,
        },
        |mut acc, activity| {
            if let Some(detail) = details.get(&activity.id).and_then(Value::as_object) {
                acc.total_time_secs += detail
                    .get("moving_time")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                acc.total_distance_m += detail
                    .get("distance")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                acc.total_elevation_m += detail
                    .get("total_elevation_gain")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
            }
            acc
        },
    )
}

#[must_use]
pub fn derive_volume_metrics(
    window_days: i64,
    total_moving_time_secs: i64,
    total_distance_m: f64,
    total_elevation_gain_m: f64,
    activity_count: usize,
) -> VolumeMetrics {
    let weeks = (window_days as f64 / DAYS_PER_WEEK).max(WEEKS_FLOOR_MIN);
    let total_moving_time_hours = total_moving_time_secs as f64 / SECONDS_PER_HOUR;

    VolumeMetrics {
        activity_count,
        total_moving_time_secs,
        total_distance_m,
        total_elevation_gain_m,
        weekly_avg_hours: total_moving_time_hours / weeks,
        avg_activity_duration_secs: if activity_count > 0 {
            total_moving_time_secs as f64 / activity_count as f64
        } else {
            0.0
        },
        activities_per_week: activity_count as f64 / weeks,
    }
}

#[must_use]
pub fn interpret_fitness_metrics(
    ctl: Option<f64>,
    atl: Option<f64>,
    tsb: Option<f64>,
) -> FitnessMetrics {
    let load_state = tsb.map(|value| {
        if value > TSB_BALANCED_UPPER {
            "fresh".to_string()
        } else if value < TSB_LOAD_PRESSURE_THRESHOLD {
            "fatigued".to_string()
        } else {
            "neutral".to_string()
        }
    });

    FitnessMetrics {
        ctl,
        atl,
        tsb,
        load_state,
    }
}

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
pub fn compute_recovery_quality_index(
    hrv_ratio: f64,
    rhr_baseline: f64,
    rhr_current: f64,
    sleep_hours: f64,
) -> Option<f64> {
    if rhr_current <= 0.0 {
        return None;
    }
    let rhr_component = if rhr_current > 0.0 {
        (rhr_baseline / rhr_current).clamp(RHR_COMPONENT_MIN, RHR_COMPONENT_MAX)
    } else {
        1.0
    };
    let sleep_component =
        (sleep_hours / IDEAL_SLEEP_HOURS).clamp(SLEEP_COMPONENT_MIN, SLEEP_COMPONENT_MAX);
    Some(
        hrv_ratio * RECOVERY_QUALITY_HRV_WEIGHT
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

pub fn derive_trend_metrics(current: TrendSnapshot, previous: TrendSnapshot) -> TrendMetrics {
    TrendMetrics {
        activity_count_delta: Some(current.activity_count as i64 - previous.activity_count as i64),
        time_delta_pct: percent_delta(
            previous.total_time_secs as f64,
            current.total_time_secs as f64,
        ),
        distance_delta_pct: percent_delta(previous.total_distance_m, current.total_distance_m),
        elevation_delta_pct: percent_delta(previous.total_elevation_m, current.total_elevation_m),
    }
}

pub fn derive_workout_metrics_context(
    interval_count: Option<usize>,
    avg_hr: Option<f64>,
    avg_power: Option<f64>,
    execution_notes: Vec<String>,
) -> WorkoutMetricsContext {
    WorkoutMetricsContext {
        interval_count,
        avg_hr,
        avg_power,
        efficiency_factor: None,
        aerobic_decoupling: None,
        execution_notes,
    }
}

pub fn parse_fitness_metrics(payload: Option<&Value>) -> Option<FitnessMetrics> {
    let value = payload?;
    let object = if let Some(items) = value.as_array() {
        items.iter().find_map(Value::as_object)
    } else {
        value.as_object()
    }?;

    let ctl = get_number(object, FITNESS_CTL_KEYS);
    let atl = get_number(object, FITNESS_ATL_KEYS);
    let tsb = get_number(object, FITNESS_TSB_KEYS);

    Some(interpret_fitness_metrics(ctl, atl, tsb))
}

pub fn parse_wellness_metrics(payload: Option<&Value>) -> Option<WellnessMetrics> {
    let entries = payload?.as_array()?;
    if entries.is_empty() {
        return None;
    }

    let (recent_entries, baseline_entries) = split_recent_and_baseline(entries);

    let sleep_values = collect_numbers(recent_entries, SLEEP_KEYS)
        .into_iter()
        .map(|value| {
            if value > WELLNESS_SLEEP_HEURISTIC_THRESHOLD {
                value / SECONDS_PER_HOUR
            } else {
                value
            }
        })
        .collect::<Vec<_>>();
    let rhr_values = collect_numbers(recent_entries, RESTING_HR_KEYS);
    let hrv_values = collect_numbers(recent_entries, HRV_KEYS);
    let baseline_hrv_values = collect_numbers(baseline_entries, HRV_KEYS);
    let baseline_rhr_values = collect_numbers(baseline_entries, RESTING_HR_KEYS);

    let avg_hrv = average(&hrv_values);
    let hrv_baseline = average(&baseline_hrv_values);
    let hrv_deviation_pct = hrv_baseline.zip(avg_hrv).and_then(|(baseline, current)| {
        percent_delta(baseline, current)
            .map(|delta| (delta * ROUNDING_DECIMAL_FACTOR).round() / ROUNDING_DECIMAL_FACTOR)
    });

    let avg_sleep_hours = average(&sleep_values);
    let avg_resting_hr = average(&rhr_values);
    let resting_hr_baseline = average(&baseline_rhr_values);
    let hrv_trend_state = classify_hrv_trend_state(hrv_deviation_pct);
    let recovery_index = avg_hrv.zip(avg_resting_hr).and_then(|(hrv, resting_hr)| {
        compute_recovery_index(hrv, resting_hr, hrv_baseline, resting_hr_baseline)
    });

    let readiness_values = collect_numbers(recent_entries, READINESS_KEYS);
    let api_readiness = average(&readiness_values);
    let mood_values = collect_numbers(recent_entries, MOOD_KEYS);
    let stress_values = collect_numbers(recent_entries, STRESS_KEYS);
    let fatigue_values = collect_numbers(recent_entries, FATIGUE_KEYS);
    let avg_mood = average(&mood_values);
    let avg_stress = average(&stress_values);
    let avg_fatigue = average(&fatigue_values);
    let readiness_score = api_readiness
        .or_else(|| compute_readiness_score(avg_mood, avg_sleep_hours, avg_stress, avg_fatigue));

    let hrv_ratio = avg_hrv
        .zip(hrv_baseline)
        .and_then(|(current, baseline)| compute_hrv_ratio(current, baseline));
    let (hrv_suppression_flag, hrv_recovery_flag) =
        hrv_ratio.map(classify_hrv_state).unwrap_or((false, false));
    let hrv_trend_slope = if hrv_values.len() >= HRV_TREND_MIN_VALUES {
        compute_hrv_trend_slope(&hrv_values)
    } else {
        None
    };
    let recovery_quality_index = hrv_ratio.zip(avg_resting_hr).and_then(|(ratio, rhr)| {
        compute_recovery_quality_index(
            ratio,
            resting_hr_baseline.unwrap_or(rhr),
            rhr,
            avg_sleep_hours.unwrap_or(WELLNESS_DEFAULT_SLEEP_HOURS),
        )
    });

    Some(WellnessMetrics {
        avg_sleep_hours,
        avg_resting_hr,
        avg_hrv,
        hrv_baseline,
        resting_hr_baseline,
        hrv_deviation_pct,
        hrv_trend_state,
        recovery_index,
        wellness_days_count: recent_entries.len(),
        avg_mood,
        avg_stress,
        avg_fatigue,
        readiness_score,
        hrv_ratio,
        hrv_suppression_flag,
        hrv_recovery_flag,
        hrv_trend_slope,
        recovery_quality_index,
    })
}

fn split_recent_and_baseline(entries: &[Value]) -> (&[Value], &[Value]) {
    let recent_len = entries.len().min(RECENT_WELLNESS_WINDOW);
    let recent_start = entries.len().saturating_sub(recent_len);
    let recent = &entries[recent_start..];
    let historical = &entries[..recent_start];
    let baseline_len = historical.len().min(HRV_BASELINE_WINDOW);
    let baseline_start = historical.len().saturating_sub(baseline_len);
    let baseline = &historical[baseline_start..];

    (recent, baseline)
}

fn classify_hrv_trend_state(hrv_deviation_pct: Option<f64>) -> Option<String> {
    let deviation = hrv_deviation_pct?;

    Some(
        if deviation <= HRV_SUPPRESSED_DROP_PCT {
            "suppressed"
        } else if deviation <= HRV_WATCH_DROP_PCT {
            "below_range"
        } else {
            "within_range"
        }
        .to_string(),
    )
}

fn percent_delta(previous: f64, current: f64) -> Option<f64> {
    if previous.abs() < f64::EPSILON {
        None
    } else {
        Some(((current - previous) / previous) * PCT_SCALING_FACTOR)
    }
}

fn get_number(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
    })
}

fn collect_numbers(entries: &[Value], keys: &[&str]) -> Vec<f64> {
    entries
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|object| get_number(object, keys))
        .collect()
}

fn extract_numeric_stream(streams: &Value, keys: &[&str]) -> Option<Vec<f64>> {
    let object = streams.as_object()?;
    let values = keys
        .iter()
        .find_map(|key| object.get(*key)?.as_array().cloned())?;
    let numbers = values
        .into_iter()
        .filter_map(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
        .collect::<Vec<_>>();

    if numbers.is_empty() {
        None
    } else {
        Some(numbers)
    }
}

fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

pub fn compute_polarisation(
    z1_pct: f64,
    z2_pct: f64,
    z3_pct: f64,
) -> Option<crate::domains::coach::PolarisationMetrics> {
    use crate::domains::coach::PolarisationMetrics;

    let ratio = if z2_pct.abs() < f64::EPSILON {
        None
    } else {
        Some((z1_pct + z3_pct) / (POLARISATION_DENOMINATOR_FACTOR * z2_pct))
    };

    let state = ratio.map(|r| {
        if r < POLARISATION_BIASED_THRESHOLD {
            "threshold_biased".to_string()
        } else if r <= 1.0 {
            "polarised".to_string()
        } else {
            "high_intensity_dominant".to_string()
        }
    });

    Some(PolarisationMetrics {
        z1_pct: Some(z1_pct),
        z2_pct: Some(z2_pct),
        z3_pct: Some(z3_pct),
        ratio,
        state,
        polarization_index: None,
        tid_model: None,
    })
}

pub fn parse_polarisation_from_api(
    activity_detail: Option<&Value>,
    zone_times: Option<&Value>,
) -> Option<crate::domains::coach::PolarisationMetrics> {
    use crate::domains::coach::PolarisationMetrics;

    // Priority 1: Use pre-computed polarization_index from API
    if let Some(detail) = activity_detail.and_then(|v| v.as_object())
        && let Some(index) = get_number(detail, &["polarization_index"])
    {
        let state = if index < POLARISATION_BIASED_THRESHOLD {
            Some("threshold_biased".to_string())
        } else if index <= 1.0 {
            Some("polarised".to_string())
        } else {
            Some("high_intensity_dominant".to_string())
        };
        return Some(PolarisationMetrics {
            z1_pct: None,
            z2_pct: None,
            z3_pct: None,
            ratio: Some(index),
            state,
            polarization_index: Some(index),
            tid_model: None,
        });
    }

    // Priority 2: Aggregate from icu_zone_times
    // API returns zone times as [{id, secs}, ...] — typically 5 zones.
    // Seiler mapping: zones 1+2 → Z1, zone 3 → Z2, zones 4+5 → Z3
    if let Some(zt) = zone_times.and_then(|v| v.as_array()) {
        let zone_secs: Vec<f64> = zt
            .iter()
            .filter_map(|entry| entry.get("secs").and_then(|s| s.as_f64()))
            .collect();
        let total: f64 = zone_secs.iter().sum();

        if zone_secs.len() >= 5 && total > 0.0 {
            // 5-zone model: [0,1]=easy, [2]=threshold, [3,4]=high
            let z1_pct = (zone_secs[0] + zone_secs[1]) / total;
            let z2_pct = zone_secs[2] / total;
            let z3_pct = (zone_secs[3] + zone_secs[4]) / total;
            return compute_polarisation(z1_pct, z2_pct, z3_pct);
        } else if zone_secs.len() >= 3 && total > 0.0 {
            // 3-zone model or unknown: map directly
            let z1_pct = zone_secs[0] / total;
            let z2_pct = zone_secs[1] / total;
            let z3_pct: f64 = zone_secs[2..].iter().sum::<f64>() / total;
            return compute_polarisation(z1_pct, z2_pct, z3_pct);
        }
    }

    None
}

pub fn compute_consistency_index(
    sessions_completed: usize,
    sessions_planned: usize,
) -> crate::domains::coach::ConsistencyMetrics {
    use crate::domains::coach::ConsistencyMetrics;

    let ratio = if sessions_planned == 0 {
        None
    } else {
        Some(sessions_completed as f64 / sessions_planned as f64)
    };

    let state = ratio.map(|r| {
        if r >= CONSISTENCY_EXCELLENT_THRESHOLD {
            "excellent".to_string()
        } else if r >= CONSISTENCY_GOOD_THRESHOLD {
            "good".to_string()
        } else if r >= CONSISTENCY_MODERATE_THRESHOLD {
            "moderate".to_string()
        } else {
            "low".to_string()
        }
    });

    ConsistencyMetrics {
        sessions_planned,
        sessions_completed,
        ratio,
        state,
    }
}

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
    }
}

/// Compare two power curve windows and compute deltas per anchor.
/// Returns (deltas, rotation_index, system_statuses).
pub fn compare_power_curves(
    current: &EspeDerivedMetrics,
    previous: &EspeDerivedMetrics,
) -> (
    std::collections::HashMap<String, f64>,
    f64,
    std::collections::HashMap<String, String>,
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

    (deltas, rotation_index, statuses)
}

/// Primary: max(wbal_start - wbal_end) across intervals; fallback: icu_max_wbal_depletion.
#[allow(clippy::needless_pass_by_value)]
pub fn compute_wdr_metrics(
    intervals: Option<&Value>,
    activity_detail: Option<&Value>,
    w_prime: Option<f64>,
) -> crate::domains::coach::WdrMetrics {
    use crate::domains::coach::WdrMetrics;

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

/// Aggregate WDRM metrics across a 7-day window of activities.
pub fn aggregate_wdr_metrics_7d(
    activity_details: &std::collections::HashMap<String, Value>,
    intervals_map: &std::collections::HashMap<String, Value>,
    w_prime: Option<f64>,
    activity_ids: &[String],
) -> crate::domains::coach::WdrMetrics {
    use crate::domains::coach::WdrMetrics;

    let mut depletion_values: Vec<f64> = Vec::new();
    let mut high_count: usize = 0;
    let mut total_with_data: usize = 0;

    for id in activity_ids {
        let detail = activity_details.get(id);
        let intervals = intervals_map.get(id);
        let single = compute_wdr_metrics(intervals, detail, w_prime);
        if single.supported {
            total_with_data += 1;
            if let Some(dp) = single.depletion_pct {
                depletion_values.push(dp);
                if dp >= WDRM_HIGH_DEPLETION_PCT {
                    high_count += 1;
                }
            }
        }
    }

    if depletion_values.is_empty() {
        return WdrMetrics::unsupported();
    }

    let mean_depletion = Some(depletion_values.iter().sum::<f64>() / depletion_values.len() as f64);

    WdrMetrics {
        supported: true,
        mean_depletion_pct_7d: mean_depletion,
        high_depletion_sessions_7d: high_count,
        sessions_with_data_7d: total_with_data,
        ..Default::default()
    }
}

/// Compute NDLI from a 7-day window of activities.
/// Classification: Green ≤2, Amber =3, Red ≥4 high-intensity days.
pub fn compute_ndli_7d(
    activity_details: &std::collections::HashMap<String, Value>,
    activity_ids: &[String],
) -> NdliMetrics {
    let mut high_intensity_days: usize = 0;
    let mut if_values: Vec<f64> = Vec::new();
    let mut ef_values: Vec<f64> = Vec::new();
    let mut vi_values: Vec<f64> = Vec::new();
    let mut days_with_data: usize = 0;

    for id in activity_ids {
        let Some(detail) = activity_details.get(id).and_then(Value::as_object) else {
            continue;
        };
        days_with_data += 1;

        // Primary: icu_joules_above_ftp > 20000 → high-intensity day
        let joules = get_number(detail, &["icu_joules_above_ftp", "joules_above_ftp"]);
        let is_high = if let Some(j) = joules {
            j > NDLI_HIGH_INTENSITY_JOULES_THRESHOLD
        } else {
            // Fallback for running: icu_training_load > 80 TSS/day
            // Scaled relative to typical CTL: 80 TSS ≈ ~1.2× CTL for moderate athletes.
            get_number(detail, &["icu_training_load", "training_load", "tss"])
                .map(|load| load > NDLI_RUNNING_TSS_PROXY_THRESHOLD)
                .unwrap_or(false)
        };

        if is_high {
            high_intensity_days += 1;
        }

        // Collect mean IF, EF, VI
        if let Some(if_val) = get_number(detail, &["icu_intensity_factor", "intensity_factor"]) {
            let normalized = if if_val > NDLI_IF_NORMALIZATION_THRESHOLD {
                if_val / PCT_SCALING_FACTOR
            } else {
                if_val
            };
            if_values.push(normalized);
        }
        if let Some(ef) = get_number(detail, &["icu_efficiency_factor", "efficiency_factor"]) {
            ef_values.push(ef);
        }
        if let Some(vi) = get_number(detail, &["icu_variability_index", "variability_index"]) {
            vi_values.push(vi);
        }
    }

    if days_with_data == 0 {
        return NdliMetrics {
            supported: false,
            ..Default::default()
        };
    }

    let mean_if = if if_values.is_empty() {
        None
    } else {
        Some(if_values.iter().sum::<f64>() / if_values.len() as f64)
    };
    let mean_ef = if ef_values.is_empty() {
        None
    } else {
        Some(ef_values.iter().sum::<f64>() / ef_values.len() as f64)
    };
    let mean_vi = if vi_values.is_empty() {
        None
    } else {
        Some(vi_values.iter().sum::<f64>() / vi_values.len() as f64)
    };

    let ndli_state = if high_intensity_days >= NDLI_RED_DAYS {
        "red".to_string()
    } else if high_intensity_days == NDLI_AMBER_DAYS {
        "amber".to_string()
    } else {
        "green".to_string()
    };

    NdliMetrics {
        supported: true,
        high_intensity_days_7d: high_intensity_days,
        mean_intensity_factor_7d: mean_if,
        mean_efficiency_factor_7d: mean_ef,
        mean_variability_index_7d: mean_vi,
        ndli_state,
        ndli_overload_flag: high_intensity_days >= NDLI_RED_DAYS,
    }
}

/// Normalize running power from different device sources (Stryd, Garmin).
/// Stryd typically reports ~8% higher than reference; Garmin Running Power differs by algorithm.
/// Applies a correction factor for cross-device consistency.
pub fn normalize_running_power(power_watts: f64, source: &str) -> f64 {
    match source {
        "stryd" => power_watts * STRYD_POWER_CORRECTION,
        "garmin_rp" => power_watts * GARMIN_RP_POWER_CORRECTION,
        _ => power_watts,
    }
}

/// Convert Grade-Adjusted Pace (GAP) from speed m/s to equivalent running power.
/// Approximate conversion: Power (W) ≈ speed³ × 0.25 + elevation_factor.
/// GAP data comes from `get_gap_histogram()` endpoint.
pub fn gap_to_running_power(gap_speed_ms: f64, gradient_pct: f64) -> f64 {
    let base_power = gap_speed_ms.powi(GAP_SPEED_EXPONENT) * GAP_POWER_COEFFICIENT;
    let elevation_factor = if gradient_pct > 0.0 {
        1.0 + gradient_pct * GAP_UPHILL_GRADIENT_FACTOR
    } else {
        1.0 + gradient_pct * GAP_DOWNHILL_GRADIENT_FACTOR
    };
    base_power * elevation_factor
}

// =============================================================================
// P2.2 — Heat Stress Metrics
// =============================================================================

/// Compute heat metrics from 7-day activity window.
/// Fallback chain: average_temp → average_weather_temp → average_feels_like.
pub fn compute_heat_metrics_7d(
    activity_details: &std::collections::HashMap<String, Value>,
    activity_ids: &[String],
) -> HeatMetrics {
    let mut temps: Vec<f64> = Vec::new();

    for id in activity_ids {
        let Some(detail) = activity_details.get(id).and_then(Value::as_object) else {
            continue;
        };
        let temp = get_number(detail, &["average_temp"])
            .or_else(|| get_number(detail, &["average_weather_temp"]))
            .or_else(|| get_number(detail, &["average_feels_like"]));
        if let Some(t) = temp {
            temps.push(t);
        }
    }

    if temps.is_empty() {
        return HeatMetrics::default();
    }

    let mean_temp = temps.iter().sum::<f64>() / temps.len() as f64;
    let max_temp = temps.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    // Source: Cheuvront & Kenefick — 18°C baseline, 23°C = moderate heat stress.
    let heat_index = ((mean_temp - HEAT_BASELINE_TEMP_C) / HEAT_NORMALIZATION_RANGE_C)
        .clamp(0.0, HEAT_INDEX_CLAMP_MAX);
    let heat_state = if heat_index > HEAT_HIGH_THRESHOLD {
        "high".to_string()
    } else if heat_index >= HEAT_MODERATE_THRESHOLD {
        "moderate".to_string()
    } else {
        "low".to_string()
    };

    HeatMetrics {
        supported: true,
        heat_index_7d: Some(heat_index),
        heat_max_7d: Some(max_temp),
        heat_state,
    }
}

// =============================================================================
// P2.4 — TID Classifier
// =============================================================================

/// Classify TID model from zone percentages.
/// pyramidal: z1 > z2 > z3, threshold: z2 dominant, polarized: z1 + z3 dominant.
pub fn classify_tid_model(z1_pct: f64, z2_pct: f64, z3_pct: f64) -> (String, Option<f64>) {
    let polarization_index = if z2_pct > 0.0 {
        Some(z3_pct / z2_pct)
    } else {
        None
    };

    let tid_model = if z2_pct > z1_pct && z2_pct > z3_pct {
        "threshold".to_string()
    } else if z1_pct > z2_pct && z1_pct > z3_pct && z2_pct > z3_pct {
        "pyramidal".to_string()
    } else {
        "polarized".to_string()
    };

    (tid_model, polarization_index)
}

// =============================================================================
// P0.3a — Z2 HR Stability
// =============================================================================

/// Compute HR variance within Z2 bounds from stream data.
/// Returns None if fewer than 10 Z2 points are available.
pub fn compute_z2_hr_variance(hr_stream: &[f64], z2_lower: f64, z2_upper: f64) -> Option<f64> {
    let z2_points: Vec<f64> = hr_stream
        .iter()
        .copied()
        .filter(|hr| *hr >= z2_lower && *hr <= z2_upper)
        .collect();

    if z2_points.len() < Z2_MIN_POINTS {
        return None;
    }

    let mean = z2_points.iter().sum::<f64>() / z2_points.len() as f64;
    let variance = z2_points
        .iter()
        .map(|hr| {
            let diff = hr - mean;
            diff * diff
        })
        .sum::<f64>()
        / z2_points.len() as f64;

    Some(variance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::coach::{DecouplingMetrics, FitnessMetrics};
    use serde_json::json;

    fn wellness_entry(sleep_secs: f64, resting_hr: f64, hrv: f64) -> Value {
        json!({
            "sleepSecs": sleep_secs,
            "restingHR": resting_hr,
            "hrv": hrv
        })
    }

    #[test]
    fn weekly_avg_hours_is_computed_from_total_time_and_window() {
        let metrics = derive_volume_metrics(7, 28_800, 42_000.0, 1_200.0, 4);

        assert_eq!(metrics.weekly_avg_hours, 8.0);
    }

    #[test]
    fn tsb_below_minus_20_is_classified_as_fatigued() {
        let fitness = interpret_fitness_metrics(Some(50.0), Some(70.0), Some(-25.0));

        assert_eq!(fitness.load_state.as_deref(), Some("fatigued"));
    }

    #[test]
    fn empty_fitness_values_stay_optional() {
        let fitness = FitnessMetrics::default();

        assert!(fitness.ctl.is_none());
        assert!(fitness.atl.is_none());
        assert!(fitness.tsb.is_none());
    }

    #[test]
    fn trend_metrics_compute_percentage_deltas() {
        let trend = derive_trend_metrics(
            TrendSnapshot {
                activity_count: 4,
                total_time_secs: 28_800,
                total_distance_m: 42_000.0,
                total_elevation_m: 1500.0,
            },
            TrendSnapshot {
                activity_count: 3,
                total_time_secs: 21_600,
                total_distance_m: 35_000.0,
                total_elevation_m: 1200.0,
            },
        );

        assert_eq!(trend.activity_count_delta, Some(1));
        assert!(trend.time_delta_pct.unwrap() > 30.0);
        assert!(trend.distance_delta_pct.unwrap() > 15.0);
    }

    #[test]
    fn build_trend_snapshot_aggregates_activity_details() {
        let activities = [
            ActivitySummary {
                id: "a1".into(),
                name: Some("Run 1".into()),
                start_date_local: "2026-03-01".into(),
                ..Default::default()
            },
            ActivitySummary {
                id: "a2".into(),
                name: Some("Run 2".into()),
                start_date_local: "2026-03-02".into(),
                ..Default::default()
            },
        ];
        let refs = activities.iter().collect::<Vec<_>>();
        let details = HashMap::from([
            (
                "a1".to_string(),
                json!({"moving_time": 3600, "distance": 10000.0, "total_elevation_gain": 100.0}),
            ),
            (
                "a2".to_string(),
                json!({"moving_time": 5400, "distance": 15000.0, "total_elevation_gain": 250.0}),
            ),
        ]);

        let snapshot = build_trend_snapshot(&refs, &details);

        assert_eq!(snapshot.activity_count, 2);
        assert_eq!(snapshot.total_time_secs, 9000);
        assert_eq!(snapshot.total_distance_m, 25_000.0);
        assert_eq!(snapshot.total_elevation_m, 350.0);
    }

    #[test]
    fn parse_fitness_metrics_supports_summary_payload() {
        let payload = json!([{"fitness": 50.0, "fatigue": 70.0, "form": -20.0}]);

        let metrics = parse_fitness_metrics(Some(&payload)).unwrap();
        assert_eq!(metrics.ctl, Some(50.0));
        assert_eq!(metrics.atl, Some(70.0));
        assert_eq!(metrics.tsb, Some(-20.0));
    }

    #[test]
    fn parse_wellness_metrics_supports_seconds_and_snake_case() {
        let payload = json!([
            {"sleepSecs": 25200.0, "restingHR": 50.0, "hrv": 60.0},
            {"sleep_hours": 8.0, "resting_hr": 52.0, "hrv": 66.0}
        ]);

        let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
        assert!(metrics.avg_sleep_hours.unwrap() > 7.4);
        assert_eq!(metrics.wellness_days_count, 2);
        assert!(metrics.recovery_index.unwrap() > 1.0);
    }

    #[test]
    fn parse_wellness_metrics_derives_adaptive_hrv_baseline_and_recent_deviation() {
        let mut entries = Vec::new();
        entries.extend((0..28).map(|_| wellness_entry(28_800.0, 50.0, 80.0)));
        entries.extend((0..7).map(|_| wellness_entry(25_200.0, 55.0, 64.0)));

        let payload = Value::Array(entries);

        let metrics = parse_wellness_metrics(Some(&payload)).unwrap();

        assert_eq!(metrics.avg_sleep_hours, Some(7.0));
        assert_eq!(metrics.avg_resting_hr, Some(55.0));
        assert_eq!(metrics.avg_hrv, Some(64.0));
        assert_eq!(metrics.wellness_days_count, 7);
        assert_eq!(metrics.hrv_baseline, Some(80.0));
        assert_eq!(metrics.resting_hr_baseline, Some(50.0));
        assert_eq!(metrics.hrv_deviation_pct, Some(-20.0));
        assert_eq!(metrics.hrv_trend_state.as_deref(), Some("suppressed"));
    }

    #[test]
    fn parse_api_load_snapshot_supports_wellness_load_fields() {
        let payload = json!({"atlLoad": 432.0, "ctlLoad": 360.0});

        let metrics = parse_api_load_snapshot(Some(&payload)).unwrap();

        assert_eq!(metrics.acute_load, 432.0);
        assert_eq!(metrics.chronic_load, 360.0);
        assert!((metrics.ratio - 1.2).abs() < 0.001);
    }

    #[test]
    fn parse_api_load_snapshot_supports_activity_load_fields() {
        let payload = json!({"icu_atl": 510.0, "icu_ctl": 400.0});

        let metrics = parse_api_load_snapshot(Some(&payload)).unwrap();

        assert_eq!(metrics.acute_load, 510.0);
        assert_eq!(metrics.chronic_load, 400.0);
        assert_eq!(metrics.state, "productive");
    }

    #[test]
    fn parse_api_load_snapshot_supports_integer_load_fields() {
        let payload = json!({"atlLoad": 432, "ctlLoad": 360});

        let metrics = parse_api_load_snapshot(Some(&payload)).unwrap();

        assert_eq!(metrics.acute_load, 432.0);
        assert_eq!(metrics.chronic_load, 360.0);
    }

    #[test]
    fn derive_execution_metrics_prefers_api_values_over_stream_fallbacks() {
        let detail = json!({
            "icu_efficiency_factor": 1.23,
            "decoupling": 4.0
        });
        let streams = json!({
            "heartrate": [100.0, 100.0, 100.0, 100.0],
            "watts": [100.0, 200.0, 300.0, 400.0]
        });

        let (efficiency_factor, decoupling) =
            derive_execution_metrics(Some(&detail), Some(&streams));

        assert_eq!(efficiency_factor, Some(1.23));
        assert_eq!(
            decoupling.as_ref().map(|metric| metric.decoupling_pct),
            Some(4.0)
        );
        assert_eq!(
            decoupling.as_ref().map(|metric| metric.state.as_str()),
            Some("acceptable")
        );
    }

    #[test]
    fn acwr_ewma_marks_ratio_above_1_5_as_overreaching() {
        let loads = vec![
            10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0,
            10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 150.0,
        ];

        let acwr = compute_acwr(&loads).unwrap();
        assert_eq!(acwr.state, "overreaching");
        assert!(acwr.ratio > 1.5);
    }

    #[test]
    fn acwr_maps_productive_and_underloaded_states() {
        let productive = compute_acwr(&vec![40.0; 28]).unwrap();
        assert_eq!(productive.state, "productive");

        let mut underloaded_loads = vec![50.0; 21];
        underloaded_loads.extend(vec![0.0; 7]);
        let underloaded = compute_acwr(&underloaded_loads).unwrap();
        assert_eq!(underloaded.state, "underloaded");
    }

    #[test]
    fn monotony_is_mean_divided_by_standard_deviation() {
        let loads = [10.0, 10.0, 20.0, 20.0, 30.0, 30.0, 40.0];

        let monotony = compute_monotony(&loads).unwrap();

        assert!((monotony - 2.22).abs() < 0.05);
    }

    #[test]
    fn monotony_capped_at_10_for_identical_loads() {
        let loads = [25.0; 7];
        let monotony = compute_monotony(&loads).unwrap();
        assert_eq!(monotony, 10.0);
    }

    #[test]
    fn strain_is_weekly_total_multiplied_by_monotony() {
        let loads = [10.0, 10.0, 20.0, 20.0, 30.0, 30.0, 40.0];
        let monotony = compute_monotony(&loads).unwrap();

        let strain = compute_strain(&loads, monotony);

        assert!((strain - (160.0 * monotony)).abs() < 0.01);
    }

    #[test]
    fn recovery_index_is_hrv_divided_by_resting_hr() {
        let recovery_index = compute_recovery_index(72.0, 48.0, None, None).unwrap();

        assert!((recovery_index - 1.5).abs() < 0.001);
    }

    #[test]
    fn recovery_index_compares_recent_hrv_and_rhr_to_personal_baseline() {
        let recovery_index = compute_recovery_index(64.0, 55.0, Some(80.0), Some(50.0)).unwrap();

        assert!((recovery_index - 0.727).abs() < 0.01);
    }

    #[test]
    fn load_management_metrics_require_sufficient_lookback_for_acwr() {
        let loads = vec![25.0; 14];

        let metrics = compute_load_management_metrics(&loads, None).unwrap();

        // Identical loads → monotony capped at 10.0 (not infinity)
        assert_eq!(metrics.acwr, None);
        assert_eq!(metrics.monotony, Some(10.0));
        assert_eq!(metrics.strain, Some(175.0 * 10.0));
        assert_eq!(metrics.fatigue_index, None);
        assert!((metrics.stress_tolerance.unwrap() - 1.75).abs() < 0.01);
    }

    #[test]
    fn acwr_returns_none_for_short_lookback() {
        let loads = vec![40.0; 14];

        assert!(compute_acwr(&loads).is_none());
    }

    #[test]
    fn efficiency_factor_is_mean_output_divided_by_mean_heart_rate() {
        let hr = [140.0, 142.0, 144.0, 146.0];
        let output = [220.0, 224.0, 228.0, 232.0];

        let efficiency_factor = compute_efficiency_factor(&hr, &output).unwrap();

        assert!((efficiency_factor - 1.58).abs() < 0.01);
    }

    #[test]
    fn aerobic_decoupling_above_five_percent_creates_watch_signal() {
        let hr = [140.0, 141.0, 142.0, 150.0, 151.0, 152.0];
        let output = [220.0, 220.0, 220.0, 220.0, 220.0, 220.0];

        let metrics = compute_aerobic_decoupling(&hr, &output).unwrap();

        assert!(metrics.decoupling_pct > 5.0);
        assert_eq!(metrics.state, "watch");
    }

    #[test]
    fn aerobic_decoupling_returns_none_for_mismatched_stream_lengths() {
        let hr = [140.0, 141.0, 142.0, 150.0];
        let output = [220.0, 220.0, 220.0];

        assert_eq!(
            compute_aerobic_decoupling(&hr, &output),
            None::<DecouplingMetrics>
        );
    }

    // Polarisation tests
    // Seiler 80/20 collapses 5 zones into 3 macro-zones:
    //   Z1 (Easy)    = zones 1+2 (below LT1)
    //   Z2 (Threshold) = zone 3 (LT1-LT2)
    //   Z3 (High)    = zones 4+5 (above LT2)
    // Formula: ratio = (Z1 + Z3) / (2 * Z2)

    #[test]
    fn polarisation_ratio_classifies_threshold_biased() {
        // z1=0.50, z2=0.45, z3=0.05 -> ratio = 0.55 / 0.90 = 0.611 -> threshold_biased
        let m = compute_polarisation(0.50, 0.45, 0.05).unwrap();
        assert!(m.ratio.unwrap() < 0.75);
        assert_eq!(m.state.as_deref(), Some("threshold_biased"));
    }

    #[test]
    fn polarisation_ratio_classifies_polarised() {
        // z1=0.50, z2=0.35, z3=0.15 -> ratio = 0.65 / 0.70 = 0.928 -> polarised
        let m = compute_polarisation(0.50, 0.35, 0.15).unwrap();
        assert!(m.ratio.unwrap() > 0.75);
        assert!(m.ratio.unwrap() <= 1.0);
        assert_eq!(m.state.as_deref(), Some("polarised"));
    }

    #[test]
    fn polarisation_ratio_classifies_high_intensity_dominant() {
        let m = compute_polarisation(0.50, 0.10, 0.40).unwrap();
        assert!(m.ratio.unwrap() > 1.0);
        assert_eq!(m.state.as_deref(), Some("high_intensity_dominant"));
    }

    #[test]
    fn polarisation_returns_none_ratio_when_z2_is_zero() {
        let m = compute_polarisation(0.50, 0.0, 0.50).unwrap();
        assert_eq!(m.ratio, None);
        assert_eq!(m.state, None);
    }

    #[test]
    fn polarisation_preserves_input_percentages() {
        let m = compute_polarisation(0.70, 0.20, 0.10).unwrap();
        assert!((m.z1_pct.unwrap() - 0.70).abs() < f64::EPSILON);
        assert!((m.z2_pct.unwrap() - 0.20).abs() < f64::EPSILON);
        assert!((m.z3_pct.unwrap() - 0.10).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_polarisation_from_api_uses_polarization_index() {
        let detail = json!({"polarization_index": 0.85});
        let m = parse_polarisation_from_api(Some(&detail), None).unwrap();
        assert_eq!(m.ratio, Some(0.85));
        assert_eq!(m.state.as_deref(), Some("polarised"));
    }

    #[test]
    fn parse_polarisation_from_api_aggregates_5_zone_times() {
        // 5-zone model: zones 1+2 → Z1 (3600+3000=6600), zone 3 → Z2 (600), zones 4+5 → Z3 (500+300=800)
        // total = 8000, z1_pct=0.825, z2_pct=0.075, z3_pct=0.10
        // ratio = (0.825 + 0.10) / (2 * 0.075) = 0.925 / 0.15 = 6.17 -> high_intensity_dominant
        let zone_times = json!([
            {"id": "z1", "secs": 3600},
            {"id": "z2", "secs": 3000},
            {"id": "z3", "secs": 600},
            {"id": "z4", "secs": 500},
            {"id": "z5", "secs": 300}
        ]);
        let m = parse_polarisation_from_api(None, Some(&zone_times)).unwrap();
        assert!(m.ratio.unwrap() > 1.0);
        assert_eq!(m.state.as_deref(), Some("high_intensity_dominant"));
    }

    #[test]
    fn parse_polarisation_from_api_aggregates_5_zone_polarised() {
        // Seiler mapping: zones 1+2 → Z1 (easy), zone 3 → Z2 (threshold), zones 4+5 → Z3 (high)
        // Zone times: z1=3000, z2=2000, z3=3500, z4=1000, z5=500 → total=10000
        // Macro zones: Z1=5000, Z2=3500, Z3=1500
        // z1_pct=0.50, z2_pct=0.35, z3_pct=0.15
        // ratio = (0.50 + 0.15) / (2 * 0.35) = 0.65 / 0.70 ≈ 0.93 → polarised
        let zone_times = json!([
            {"id": "z1", "secs": 3000},
            {"id": "z2", "secs": 2000},
            {"id": "z3", "secs": 3500},
            {"id": "z4", "secs": 1000},
            {"id": "z5", "secs": 500}
        ]);
        let m = parse_polarisation_from_api(None, Some(&zone_times)).unwrap();
        assert!(m.ratio.unwrap() > 0.75);
        assert!(m.ratio.unwrap() <= 1.0);
        assert_eq!(m.state.as_deref(), Some("polarised"));
    }

    #[test]
    fn parse_polarisation_from_api_returns_none_when_no_data() {
        assert!(parse_polarisation_from_api(None, None).is_none());
    }

    // Consistency tests

    #[test]
    fn consistency_index_full_adherence() {
        let m = compute_consistency_index(10, 10);
        assert_eq!(m.ratio, Some(1.0));
        assert_eq!(m.state.as_deref(), Some("excellent"));
    }

    #[test]
    fn consistency_index_good_adherence() {
        let m = compute_consistency_index(8, 10);
        assert_eq!(m.ratio, Some(0.8));
        assert_eq!(m.state.as_deref(), Some("good"));
    }

    #[test]
    fn consistency_index_moderate_adherence() {
        let m = compute_consistency_index(5, 10);
        assert_eq!(m.ratio, Some(0.5));
        assert_eq!(m.state.as_deref(), Some("moderate"));
    }

    #[test]
    fn consistency_index_low_adherence() {
        let m = compute_consistency_index(3, 10);
        assert_eq!(m.ratio, Some(0.3));
        assert_eq!(m.state.as_deref(), Some("low"));
    }

    #[test]
    fn consistency_index_no_plans_returns_none_ratio() {
        let m = compute_consistency_index(0, 0);
        assert_eq!(m.ratio, None);
        assert_eq!(m.state, None);
    }

    #[test]
    fn fatigue_index_is_load_7d_divided_by_recovery_index() {
        let fi = compute_fatigue_index(175.0, 1.4).unwrap();
        assert!((fi - 125.0).abs() < 0.01);
    }

    #[test]
    fn fatigue_index_returns_none_when_recovery_index_is_zero() {
        assert!(compute_fatigue_index(175.0, 0.0).is_none());
    }

    #[test]
    fn stress_tolerance_is_strain_over_monotony_divided_by_100() {
        let st = compute_stress_tolerance(450.0, 2.0).unwrap();
        assert!((st - 2.25).abs() < 0.01);
    }

    #[test]
    fn stress_tolerance_returns_none_when_monotony_is_zero() {
        assert!(compute_stress_tolerance(450.0, 0.0).is_none());
    }

    #[test]
    fn readiness_score_computes_weighted_average() {
        let rs = compute_readiness_score(Some(8.0), Some(7.5), Some(5.0), Some(4.0)).unwrap();
        assert!((rs - 6.45).abs() < 0.01);
    }

    #[test]
    fn readiness_score_returns_none_when_all_inputs_are_none() {
        assert!(compute_readiness_score(None, None, None, None).is_none());
    }

    #[test]
    fn readiness_score_returns_none_for_partial_inputs() {
        assert!(compute_readiness_score(Some(8.0), None, Some(5.0), None).is_none());
        assert!(compute_readiness_score(Some(8.0), Some(7.5), None, None).is_none());
        assert!(compute_readiness_score(None, Some(7.5), Some(5.0), Some(4.0)).is_none());
    }

    #[test]
    fn readiness_score_normalizes_sleep_hours_over_10() {
        let rs = compute_readiness_score(Some(8.0), Some(8.0), Some(5.0), Some(4.0)).unwrap();
        assert!((rs - 6.6).abs() < 0.01);
    }

    #[test]
    fn readiness_score_clamps_sleep_above_10_hours() {
        let rs = compute_readiness_score(Some(8.0), Some(12.0), Some(5.0), Some(4.0)).unwrap();
        assert!((rs - 7.2).abs() < 0.01);
    }

    #[test]
    fn readiness_score_handles_partial_inputs() {
        assert!(compute_readiness_score(Some(8.0), None, Some(5.0), None).is_none());
        assert!(compute_readiness_score(Some(8.0), Some(7.5), None, None).is_none());
        assert!(compute_readiness_score(None, Some(7.5), Some(5.0), Some(4.0)).is_none());
    }

    #[test]
    fn load_management_metrics_computes_fatigue_index_and_stress_tolerance() {
        let loads = vec![25.0; 28];
        let metrics = compute_load_management_metrics(&loads, Some(1.5)).unwrap();
        assert!(metrics.fatigue_index.is_some());
        assert!(metrics.stress_tolerance.is_some());
    }

    #[test]
    fn load_management_metrics_without_recovery_index_has_no_fatigue_index() {
        let loads = vec![25.0; 28];
        let metrics = compute_load_management_metrics(&loads, None).unwrap();
        assert!(metrics.fatigue_index.is_none());
        assert!(metrics.stress_tolerance.is_some());
    }

    #[test]
    fn parse_wellness_metrics_extracts_mood_stress_fatigue() {
        let payload = json!([
            {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 65.0, "mood": 8.0, "stress": 5.0, "fatigue": 4.0},
            {"sleep_hours": 7.5, "resting_hr": 51.0, "hrv": 63.0, "mood": 7.0, "stress": 6.0, "fatigue": 5.0}
        ]);
        let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
        assert_eq!(metrics.avg_mood, Some(7.5));
        assert_eq!(metrics.avg_stress, Some(5.5));
        assert_eq!(metrics.avg_fatigue, Some(4.5));
        assert!(metrics.readiness_score.is_some());
    }

    #[test]
    fn parse_wellness_metrics_uses_api_readiness_as_primary() {
        let payload = json!([
            {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 65.0, "readiness": 7.5}
        ]);
        let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
        assert_eq!(metrics.readiness_score, Some(7.5));
    }

    #[test]
    fn parse_wellness_metrics_readiness_falls_back_to_formula_when_api_missing() {
        let payload = json!([
            {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 65.0, "mood": 8.0, "stress": 3.0, "fatigue": 2.0}
        ]);
        let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
        assert!(metrics.readiness_score.is_some());
        let rs = metrics.readiness_score.unwrap();
        let expected = 8.0 * 0.3 + 8.0 * 0.3 + 3.0 * 0.2 + 2.0 * 0.2;
        assert!((rs - expected).abs() < 0.01);
    }

    #[test]
    fn parse_wellness_metrics_readiness_requires_api_or_all_components() {
        let payload = json!([
            {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 65.0}
        ]);
        let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
        assert!(metrics.avg_mood.is_none());
        assert!(metrics.avg_stress.is_none());
        assert!(metrics.avg_fatigue.is_none());
        assert!(metrics.readiness_score.is_none());
    }

    #[test]
    fn durability_index_is_current_divided_by_baseline() {
        let di = compute_durability_index(295.0, 310.0).unwrap();
        assert!((di - 0.952).abs() < 0.001);
    }

    #[test]
    fn durability_index_returns_none_when_baseline_is_zero() {
        assert!(compute_durability_index(295.0, 0.0).is_none());
    }

    #[test]
    fn durability_index_returns_none_when_baseline_is_negative() {
        assert!(compute_durability_index(295.0, -310.0).is_none());
    }

    #[test]
    fn durability_index_handles_zero_current() {
        let di = compute_durability_index(0.0, 310.0).unwrap();
        assert!((di - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn extract_sportinfo_anchors_from_wellness_array() {
        let payload = json!([
            {"type": "Ride", "eftp": 250.0, "wPrime": 20000.0, "pMax": 800.0},
            {"type": "Run", "eftp": null, "wPrime": null, "pMax": null}
        ]);
        let anchors = extract_sportinfo_anchors(Some(&payload));
        assert!(anchors.supported);
        assert_eq!(anchors.eftp, Some(250.0));
        assert_eq!(anchors.w_prime, Some(20000.0));
        assert_eq!(anchors.p_max, Some(800.0));
        assert_eq!(anchors.source, "sportinfo");
    }

    #[test]
    fn extract_sportinfo_anchors_unsupported_when_empty() {
        let payload = json!([]);
        let anchors = extract_sportinfo_anchors(Some(&payload));
        assert!(!anchors.supported);
        assert_eq!(anchors.source, "none");
    }

    #[test]
    fn extract_sportinfo_anchors_unsupported_when_null() {
        let anchors = extract_sportinfo_anchors(None);
        assert!(!anchors.supported);
    }

    #[test]
    fn extract_sportinfo_anchors_multi_sport_first_non_null_wins() {
        let payload = json!([
            {"type": "Ride", "eftp": null, "wPrime": 18000.0, "pMax": null},
            {"type": "Run", "eftp": 280.0, "wPrime": null, "pMax": 900.0}
        ]);
        let anchors = extract_sportinfo_anchors(Some(&payload));
        assert!(anchors.supported);
        assert_eq!(anchors.eftp, Some(280.0));
        assert_eq!(anchors.w_prime, Some(18000.0));
        assert_eq!(anchors.p_max, Some(900.0));
    }

    #[test]
    fn enrich_anchors_from_activity_pm_fallback() {
        let mut anchors = EspePowerAnchors::unsupported();
        let detail = json!({
            "icu_pm_ftp": 265.0,
            "icu_pm_w_prime": 22000.0,
            "icu_pm_p_max": 850.0
        });
        enrich_anchors_from_activity(&mut anchors, Some(&detail));
        assert!(anchors.supported);
        assert_eq!(anchors.eftp, Some(265.0));
        assert_eq!(anchors.w_prime, Some(22000.0));
        assert_eq!(anchors.p_max, Some(850.0));
        assert_eq!(anchors.source, "activity");
    }

    #[test]
    fn enrich_anchors_does_not_overwrite_existing_values() {
        let mut anchors = EspePowerAnchors {
            eftp: Some(250.0),
            w_prime: None,
            p_max: None,
            source: "sportinfo".into(),
            supported: true,
        };
        let detail = json!({"icu_pm_ftp": 999.0, "icu_pm_w_prime": 99999.0});
        enrich_anchors_from_activity(&mut anchors, Some(&detail));
        assert_eq!(anchors.eftp, Some(250.0));
        assert_eq!(anchors.w_prime, Some(99999.0));
    }

    #[test]
    fn derive_espe_metrics_computes_glycolytic_bias() {
        let anchors = EspePowerAnchors {
            eftp: Some(250.0),
            p_max: Some(800.0),
            ..Default::default()
        };
        let derived = derive_espe_metrics(&anchors, Some(600.0), None, Some(300.0), None);
        assert!(derived.supported);
        assert!((derived.glycolytic_bias.unwrap() - 2.0).abs() < 0.01);
    }

    #[test]
    fn derive_espe_metrics_unsupported_when_no_anchors() {
        let anchors = EspePowerAnchors::unsupported();
        let derived = derive_espe_metrics(&anchors, None, None, None, None);
        assert!(!derived.supported);
        assert!(derived.glycolytic_bias.is_none());
    }

    #[test]
    fn compute_wdr_metrics_from_wbal_intervals() {
        let intervals = json!([
            {"wbal_start": 20000.0, "wbal_end": 15000.0},
            {"wbal_start": 15000.0, "wbal_end": 8000.0}
        ]);
        let wdrm = compute_wdr_metrics(Some(&intervals), None, Some(20000.0));
        assert!(wdrm.supported);
        assert_eq!(wdrm.max_wbal_depletion, Some(7000.0));
        assert!((wdrm.depletion_pct.unwrap() - 0.35).abs() < 0.01);
    }

    #[test]
    fn compute_wdr_metrics_unsupported_for_null_wbal() {
        let intervals = json!([
            {"moving_time": 300, "average_heartrate": 140.0}
        ]);
        let wdrm = compute_wdr_metrics(Some(&intervals), None, None);
        assert!(!wdrm.supported);
    }

    #[test]
    fn compute_wdr_metrics_fallback_to_icu_max_wbal_depletion() {
        let detail = json!({
            "icu_max_wbal_depletion": 12000.0,
            "icu_joules_above_ftp": 45000.0
        });
        let wdrm = compute_wdr_metrics(None, Some(&detail), Some(20000.0));
        assert!(wdrm.supported);
        assert_eq!(wdrm.max_wbal_depletion, Some(12000.0));
        assert!((wdrm.depletion_pct.unwrap() - 0.60).abs() < 0.01);
        assert_eq!(wdrm.joules_above_ftp, Some(45000.0));
    }

    #[test]
    fn compute_wdr_metrics_clips_depletion_at_150_pct() {
        let detail = json!({"icu_max_wbal_depletion": 30000.0});
        let wdrm = compute_wdr_metrics(None, Some(&detail), Some(10000.0));
        assert!(wdrm.supported);
        assert!((wdrm.depletion_pct.unwrap() - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_wdr_metrics_7d_counts_high_depletion_sessions() {
        let mut details = std::collections::HashMap::new();
        let mut intervals = std::collections::HashMap::new();
        for i in 1..=5 {
            let detail = json!({"icu_max_wbal_depletion": (i as f64) * 5000.0});
            details.insert(format!("act-{i}"), detail);
            intervals.insert(format!("act-{i}"), json!([]));
        }
        let ids = vec![
            "act-1".into(),
            "act-2".into(),
            "act-3".into(),
            "act-4".into(),
            "act-5".into(),
        ];
        let wdrm = aggregate_wdr_metrics_7d(&details, &intervals, Some(10000.0), &ids);
        assert!(wdrm.supported);
        // activity depletion pct: 0.5, 1.0, 1.5, 1.5, 1.5 → mean ≈ 1.2
        assert!(wdrm.mean_depletion_pct_7d.unwrap() > 0.5);
        // activities with depletion >= 60%: 3-5 (pct: 1.0, 1.5, 1.5, 1.5) = 4
        assert_eq!(wdrm.high_depletion_sessions_7d, 4);
        assert_eq!(wdrm.sessions_with_data_7d, 5);
    }

    // ========================================================================
    // P0.3 — ISDM: Signed Decoupling + Durability State
    // ========================================================================

    #[test]
    fn classify_durability_state_stable() {
        assert_eq!(classify_durability_state(1.0, 1.0), "stable");
        assert_eq!(classify_durability_state(-1.0, 1.0), "stable");
        assert_eq!(classify_durability_state(3.0, 3.0), "stable");
    }

    #[test]
    fn classify_durability_state_improving() {
        assert_eq!(classify_durability_state(-5.0, 5.0), "improving");
        assert_eq!(classify_durability_state(-3.5, 3.5), "improving");
    }

    #[test]
    fn classify_durability_state_drifting() {
        assert_eq!(classify_durability_state(6.0, 6.0), "drifting");
        assert_eq!(classify_durability_state(4.0, 9.0), "drifting");
        assert_eq!(classify_durability_state(2.0, 10.0), "drifting");
    }

    #[test]
    fn classify_durability_state_watch() {
        assert_eq!(classify_durability_state(4.0, 4.0), "watch");
        assert_eq!(classify_durability_state(3.5, 3.5), "watch");
    }

    #[test]
    fn signed_decoupling_positive_is_drifting() {
        let hr = [140.0, 141.0, 142.0, 150.0, 151.0, 152.0];
        let output = [220.0, 220.0, 220.0, 220.0, 220.0, 220.0];
        let metrics = compute_aerobic_decoupling(&hr, &output).unwrap();
        assert!(metrics.signed_decoupling_pct > 0.0);
        assert_eq!(metrics.durability_state, "drifting");
    }

    #[test]
    fn signed_decoupling_negative_is_improving() {
        let hr = [150.0, 151.0, 152.0, 140.0, 141.0, 142.0];
        let output = [220.0, 220.0, 220.0, 220.0, 220.0, 220.0];
        let metrics = compute_aerobic_decoupling(&hr, &output).unwrap();
        assert!(metrics.signed_decoupling_pct < 0.0);
        assert_eq!(metrics.durability_state, "improving");
    }

    // ========================================================================
    // P0.3a — Z2 HR Stability
    // ========================================================================

    #[test]
    fn compute_z2_hr_variance_returns_some() {
        let hr = vec![
            120.0, 122.0, 124.0, 126.0, 128.0, 130.0, 132.0, 134.0, 136.0, 138.0,
        ];
        let variance = compute_z2_hr_variance(&hr, 120.0, 140.0);
        assert!(variance.is_some());
        assert!(variance.unwrap() > 0.0);
    }

    #[test]
    fn compute_z2_hr_variance_returns_none_for_too_few_points() {
        let hr = vec![120.0, 122.0, 124.0];
        let variance = compute_z2_hr_variance(&hr, 120.0, 140.0);
        assert!(variance.is_none());
    }

    #[test]
    fn compute_z2_hr_variance_returns_none_when_no_points_in_z2() {
        let hr = vec![150.0; 20];
        let variance = compute_z2_hr_variance(&hr, 120.0, 140.0);
        assert!(variance.is_none());
    }

    #[test]
    fn compute_consistency_index_perfect() {
        let m = compute_consistency_index(5, 5);
        assert_eq!(m.ratio, Some(1.0));
        assert_eq!(m.state.as_deref(), Some("excellent"));
    }

    #[test]
    fn compute_consistency_index_zero_planned() {
        let m = compute_consistency_index(0, 0);
        assert_eq!(m.ratio, None);
        assert_eq!(m.state, None);
    }

    #[test]
    fn compute_consistency_index_above_100() {
        let m = compute_consistency_index(6, 5);
        assert_eq!(m.ratio, Some(1.2));
        assert_eq!(m.state.as_deref(), Some("excellent"));
    }

    #[test]
    fn compute_load_management_empty() {
        let metrics = compute_load_management_metrics(&[], None);
        assert!(metrics.is_none());
    }

    #[test]
    fn compute_ndli_7d_empty_returns_not_supported() {
        let metrics = compute_ndli_7d(&HashMap::new(), &[]);
        assert!(!metrics.supported);
    }

    #[test]
    fn compute_heat_metrics_7d_empty_returns_default() {
        let metrics = compute_heat_metrics_7d(&HashMap::new(), &[]);
        assert!(!metrics.supported);
    }

    #[test]
    fn compute_acwr_empty_returns_none() {
        assert!(compute_acwr(&[]).is_none());
    }

    #[test]
    fn compute_monotony_empty_returns_none() {
        assert!(compute_monotony(&[]).is_none());
    }

    #[test]
    fn compute_monotony_constant_load() {
        assert_eq!(compute_monotony(&[100.0; 7]), Some(10.0));
    }
}
