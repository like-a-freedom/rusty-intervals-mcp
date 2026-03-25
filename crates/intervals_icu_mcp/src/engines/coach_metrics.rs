use crate::domains::coach::{
    AcwrMetrics, DecouplingMetrics, FitnessMetrics, LoadManagementMetrics, TrendMetrics,
    VolumeMetrics, WellnessMetrics, WorkoutMetricsContext,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendSnapshot {
    pub activity_count: usize,
    pub total_time_secs: i64,
    pub total_distance_m: f64,
    pub total_elevation_m: f64,
}

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

pub fn derive_volume_metrics(
    window_days: i64,
    total_moving_time_secs: i64,
    total_distance_m: f64,
    total_elevation_gain_m: f64,
    activity_count: usize,
) -> VolumeMetrics {
    let weeks = (window_days as f64 / 7.0).max(1.0);
    let total_moving_time_hours = total_moving_time_secs as f64 / 3600.0;

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

pub fn interpret_fitness_metrics(
    ctl: Option<f64>,
    atl: Option<f64>,
    tsb: Option<f64>,
) -> FitnessMetrics {
    let load_state = tsb.map(|value| {
        if value > 10.0 {
            "fresh".to_string()
        } else if value < -10.0 {
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

pub fn compute_acwr(loads: &[f64]) -> Option<AcwrMetrics> {
    if loads.len() < 28 {
        return None;
    }

    let acute_lambda = 2.0 / 8.0;
    let chronic_lambda = 2.0 / 29.0;

    let mut acute_load = *loads.first()?;
    let mut chronic_load = acute_load;

    for load in loads.iter().skip(1) {
        acute_load = (acute_lambda * load) + ((1.0 - acute_lambda) * acute_load);
        chronic_load = (chronic_lambda * load) + ((1.0 - chronic_lambda) * chronic_load);
    }

    build_acwr_metrics(acute_load, chronic_load)
}

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
    if ratio < 0.8 {
        "underloaded"
    } else if ratio <= 1.3 {
        "productive"
    } else if ratio <= 1.5 {
        "watch"
    } else {
        "overreaching"
    }
}

pub fn compute_monotony(loads_7d: &[f64]) -> Option<f64> {
    if loads_7d.len() < 7 {
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

    // Use population stddev; cap floor to avoid infinity when all loads are identical.
    // Real-world identical loads → monotony is high but finite (cap at 10.0).
    let stddev = variance.sqrt().max(mean * 0.1);
    Some((mean / stddev).min(10.0))
}

pub fn compute_strain(loads_7d: &[f64], monotony: f64) -> f64 {
    loads_7d.iter().sum::<f64>() * monotony
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
        Some((strain / monotony) / 100.0)
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
    let normalized_sleep = sleep_hours.clamp(0.0, 10.0);
    let weighted_sum = mood * 0.3 + normalized_sleep * 0.3 + stress * 0.2 + fatigue * 0.2;
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
        &loads[loads.len() - 7..]
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
    if hr.len() != output.len() || hr.len() < 4 {
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

    let decoupling_pct = ((first_half - second_half) / first_half).abs() * 100.0;
    let state = classify_decoupling_state(decoupling_pct);

    Some(DecouplingMetrics {
        efficiency_factor_first_half: Some(first_half),
        efficiency_factor_second_half: Some(second_half),
        decoupling_pct,
        state,
    })
}

fn classify_decoupling_state(decoupling_pct: f64) -> String {
    if decoupling_pct <= 5.0 {
        "acceptable".to_string()
    } else if decoupling_pct <= 10.0 {
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
        .map(|value| if value > 24.0 { value / 3600.0 } else { value })
        .collect::<Vec<_>>();
    let rhr_values = collect_numbers(recent_entries, RESTING_HR_KEYS);
    let hrv_values = collect_numbers(recent_entries, HRV_KEYS);
    let baseline_hrv_values = collect_numbers(baseline_entries, HRV_KEYS);
    let baseline_rhr_values = collect_numbers(baseline_entries, RESTING_HR_KEYS);

    let avg_hrv = average(&hrv_values);
    let hrv_baseline = average(&baseline_hrv_values);
    let hrv_deviation_pct = hrv_baseline.zip(avg_hrv).and_then(|(baseline, current)| {
        percent_delta(baseline, current).map(|delta| (delta * 10.0).round() / 10.0)
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
        Some(((current - previous) / previous) * 100.0)
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
        Some((z1_pct + z3_pct) / (2.0 * z2_pct))
    };

    let state = ratio.map(|r| {
        if r < 0.75 {
            "threshold_biased".to_string()
        } else if r <= 1.0 {
            "polarised".to_string()
        } else {
            "high_intensity_dominant".to_string()
        }
    });

    Some(PolarisationMetrics {
        z1_pct,
        z2_pct,
        z3_pct,
        ratio,
        state,
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
        let state = if index < 0.75 {
            Some("threshold_biased".to_string())
        } else if index <= 1.0 {
            Some("polarised".to_string())
        } else {
            Some("high_intensity_dominant".to_string())
        };
        return Some(PolarisationMetrics {
            z1_pct: 0.0,
            z2_pct: 0.0,
            z3_pct: 0.0,
            ratio: Some(index),
            state,
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
        if r >= 0.9 {
            "excellent".to_string()
        } else if r >= 0.7 {
            "good".to_string()
        } else if r >= 0.5 {
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
        assert!((m.z1_pct - 0.70).abs() < f64::EPSILON);
        assert!((m.z2_pct - 0.20).abs() < f64::EPSILON);
        assert!((m.z3_pct - 0.10).abs() < f64::EPSILON);
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
        // Realistic 80/20 distribution across 5 zones:
        // Z1: 6000+2000=8000 (easy), Z2: 1000 (threshold), Z3: 800+200=1000 (high)
        // total = 10000, z1_pct=0.80, z2_pct=0.10, z3_pct=0.10
        // ratio = (0.80 + 0.10) / (2 * 0.10) = 0.90 / 0.20 = 4.5 -> not polarised
        // Need more threshold: z1=0.70, z2=0.20, z3=0.10 -> ratio = 0.80/0.40 = 2.0
        // Let's use: z1=0.75, z2=0.15, z3=0.10 -> ratio = 0.85/0.30 = 2.83
        // Actually let's aim for ratio ~0.9:
        // z1=0.70, z2=0.20, z3=0.10 -> ratio = 0.80/0.40 = 2.0 (still high)
        // z1=0.50, z2=0.35, z3=0.15 -> ratio = 0.65/0.70 = 0.928 (polarised!)
        let zone_times = json!([
            {"id": "z1", "secs": 3000},
            {"id": "z2", "secs": 2000},
            {"id": "z3", "secs": 3500},
            {"id": "z4", "secs": 1000},
            {"id": "z5", "secs": 500}
        ]);
        // z1_macro = 5000, z2_macro = 3500, z3_macro = 1500, total=10000
        // z1_pct=0.50, z2_pct=0.35, z3_pct=0.15
        // ratio = 0.65 / 0.70 = 0.928 -> polarised
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
}
