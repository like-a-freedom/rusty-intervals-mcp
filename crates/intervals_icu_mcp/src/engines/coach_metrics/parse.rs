use crate::domains::baseline::{BaselineTransform, compute_personal_baseline};
use crate::domains::coach::{FitnessMetrics, WellnessMetrics};
use crate::domains::progress::LnRmssdRollup;
use crate::engines::coach_metrics_constants::*;
use crate::engines::shared::parse_activity_date;
use chrono::NaiveDate;
use serde_json::Value;

use super::helpers::{average, collect_numbers, get_number, percent_delta};
use super::recovery::{
    classify_hrv_state, compute_hrv_ratio, compute_hrv_trend_slope, compute_readiness_score,
    compute_recovery_index, compute_recovery_quality_index,
};
use super::trend::interpret_fitness_metrics;

pub fn parse_fitness_metrics(payload: Option<&Value>) -> Option<FitnessMetrics> {
    let value = payload?;
    let object = if let Some(items) = value.as_array() {
        // The wellness endpoint returns one object per day (carrying
        // ctl/atl/rampRate per the vendored OpenAPI `Wellness` schema), so a
        // multi-day array must resolve the latest day by date — the
        // historical first-object pick silently pinned stale fitness whenever
        // the API order put an older day first. Fully undated payloads keep
        // the first object.
        latest_entry_by_date(items)
            .and_then(Value::as_object)
            .or_else(|| items.iter().filter_map(Value::as_object).next())?
    } else {
        value.as_object()?
    };

    // CTL/ATL are EWMAs of non-negative daily loads, so negatives are
    // corruption, never measurements. TSB and ramp rate are legitimately
    // negative (fatigue / declining fitness) and pass through unfiltered.
    let ctl = get_number(object, FITNESS_CTL_KEYS).filter(|value| *value >= 0.0);
    let atl = get_number(object, FITNESS_ATL_KEYS).filter(|value| *value >= 0.0);
    let tsb = get_number(object, FITNESS_TSB_KEYS);
    let ramp_rate = get_number(object, FITNESS_RAMP_RATE_KEYS);

    Some(interpret_fitness_metrics(ctl, atl, tsb, ramp_rate))
}

pub fn parse_wellness_metrics(payload: Option<&Value>) -> Option<WellnessMetrics> {
    let entries = payload?.as_array()?;
    if entries.is_empty() {
        return None;
    }

    // Normalize entry order: oldest-first so recent window = last N entries.
    // Real API may return newest-first; without this we average the wrong window.
    let ordered = order_entries_oldest_first(entries);
    let entries: &[Value] = &ordered;

    let (recent_entries, baseline_entries) = split_recent_and_baseline(entries);

    let sleep_values = recent_entries
        .iter()
        .filter_map(Value::as_object)
        .filter_map(sleep_hours_from_entry)
        .collect::<Vec<_>>();
    let rhr_values = plausible_numbers(recent_entries, RESTING_HR_KEYS, is_plausible_resting_hr);
    let hrv_values = plausible_numbers(recent_entries, HRV_KEYS, is_plausible_hrv);
    let baseline_hrv_values = plausible_numbers(baseline_entries, HRV_KEYS, is_plausible_hrv);
    let baseline_rhr_values =
        plausible_numbers(baseline_entries, RESTING_HR_KEYS, is_plausible_resting_hr);

    let avg_hrv = average(&hrv_values);
    // A baseline shorter than the window it benchmarks is noise, not a
    // baseline: single-day denominators fabricate huge deviations/ratios.
    let hrv_baseline = gated_average(&baseline_hrv_values);
    let hrv_deviation_pct = hrv_baseline.zip(avg_hrv).and_then(|(baseline, current)| {
        percent_delta(baseline, current)
            .map(|delta| (delta * ROUNDING_DECIMAL_FACTOR).round() / ROUNDING_DECIMAL_FACTOR)
    });

    let avg_sleep_hours = average(&sleep_values);
    let avg_resting_hr = average(&rhr_values);
    let resting_hr_baseline = gated_average(&baseline_rhr_values);
    let hrv_trend_state = classify_hrv_trend_state(hrv_deviation_pct);
    let recovery_index = avg_hrv.zip(avg_resting_hr).and_then(|(hrv, resting_hr)| {
        compute_recovery_index(hrv, resting_hr, hrv_baseline, resting_hr_baseline)
    });

    let readiness_values = non_negative_numbers(recent_entries, READINESS_KEYS);
    let api_readiness = average(&readiness_values);
    let mood_values = non_negative_numbers(recent_entries, MOOD_KEYS);
    let stress_values = non_negative_numbers(recent_entries, STRESS_KEYS);
    let fatigue_values = non_negative_numbers(recent_entries, FATIGUE_KEYS);
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
    // No silent sleep default: when every sample is filtered as implausible,
    // sleep is unknown and RQI must stay None rather than invent neutral 7h.
    let recovery_quality_index =
        hrv_ratio
            .zip(avg_resting_hr)
            .zip(avg_sleep_hours)
            .and_then(|((ratio, rhr), sleep)| {
                compute_recovery_quality_index(
                    ratio,
                    resting_hr_baseline.unwrap_or(rhr),
                    rhr,
                    sleep,
                )
            });

    let hrv_observations = extract_wellness_observations(entries, HRV_KEYS, is_plausible_hrv);
    let rhr_observations =
        extract_wellness_observations(entries, RESTING_HR_KEYS, is_plausible_resting_hr);

    Some(WellnessMetrics {
        avg_sleep_hours,
        avg_resting_hr,
        avg_hrv,
        hrv_baseline,
        resting_hr_baseline,
        hrv_deviation_pct,
        hrv_trend_state,
        recovery_index,
        // Days with data, not raw rows: empty `{}` entries must not inflate
        // the count (presence, not plausibility — a 12.8 h artifact day is
        // still a day, its sleep just doesn't enter the average).
        wellness_days_count: recent_entries
            .iter()
            .filter(|entry| entry.as_object().is_some_and(entry_has_wellness_data))
            .count(),
        avg_mood,
        avg_stress,
        avg_fatigue,
        readiness_score,
        hrv_ratio,
        hrv_suppression_flag,
        hrv_recovery_flag,
        hrv_trend_slope,
        recovery_quality_index,
        hrv_personal_baseline: compute_personal_baseline(
            &hrv_observations,
            BaselineTransform::LogLnRmssd,
        ),
        resting_hr_personal_baseline: compute_personal_baseline(
            &rhr_observations,
            BaselineTransform::RawBpm,
        ),
    })
}

/// Collect numbers for `keys`, keeping only physiologically plausible samples.
/// Sensor dropouts (0) and glitch spikes otherwise corrupt averages,
/// baselines, and ratios — the same artifact class as implausible sleep.
fn plausible_numbers(entries: &[Value], keys: &[&str], plausible: fn(f64) -> bool) -> Vec<f64> {
    collect_numbers(entries, keys)
        .into_iter()
        .filter(|value| plausible(*value))
        .collect()
}

/// Collect rating-scale numbers (mood, stress, fatigue, readiness), rejecting
/// impossible negatives: every scale the API uses is zero-floored, so a
/// negative is corruption, not "very bad". Zero is kept — it may be a
/// legitimate worst rating.
fn non_negative_numbers(entries: &[Value], keys: &[&str]) -> Vec<f64> {
    plausible_numbers(entries, keys, |value| value >= 0.0)
}

/// Average over a baseline window, gated on sample count: a baseline shorter
/// than [`WELLNESS_BASELINE_MIN_SAMPLES`] is noise, not a baseline —
/// single-day denominators fabricate huge deviations and ratios.
fn gated_average(values: &[f64]) -> Option<f64> {
    if values.len() >= WELLNESS_BASELINE_MIN_SAMPLES {
        average(values)
    } else {
        None
    }
}

/// A recent entry counts as a wellness day when it carries at least one
/// recognized metric value.
fn entry_has_wellness_data(obj: &serde_json::Map<String, Value>) -> bool {
    const ALL_WELLNESS_KEYS: &[&[&str]] = &[
        SLEEP_KEYS,
        RESTING_HR_KEYS,
        HRV_KEYS,
        READINESS_KEYS,
        MOOD_KEYS,
        STRESS_KEYS,
        FATIGUE_KEYS,
    ];
    ALL_WELLNESS_KEYS
        .iter()
        .any(|keys| get_number(obj, keys).is_some())
}

/// CTL/fitness value for one wellness day across both key families
/// (`FITNESS_CTL_KEYS`, then `API_LOAD_CHRONIC_KEYS`), rejecting impossible
/// negatives — CTL is an EWMA of non-negative daily loads. Shared by the
/// series extractor and the point counter so both agree on what counts
/// (same keys, same integer handling, same non-negative rule).
pub(crate) fn ctl_value_from_entry(obj: &serde_json::Map<String, Value>) -> Option<f64> {
    get_number(obj, FITNESS_CTL_KEYS)
        .or_else(|| get_number(obj, API_LOAD_CHRONIC_KEYS))
        .filter(|value| *value >= 0.0)
}

/// Resting-HR plausibility band (bpm). Shared by the parser, progress
/// tracking, and the training-plan snapshot so all paths agree.
pub(crate) fn is_plausible_resting_hr(value: f64) -> bool {
    (WELLNESS_RHR_MIN_PLAUSIBLE_BPM..=WELLNESS_RHR_MAX_PLAUSIBLE_BPM).contains(&value)
}

/// HRV (RMSSD, ms) plausibility: strictly positive (zero is a dropout that
/// also poisons ratios and log transforms) and below the glitch ceiling.
pub(crate) fn is_plausible_hrv(value: f64) -> bool {
    value > 0.0 && value <= WELLNESS_HRV_MAX_PLAUSIBLE_MS
}

/// Convert a raw sleep value to hours using the >24 heuristic
/// (values >24 are treated as seconds, ≤24 as already-hours).
fn normalize_sleep_to_hours(value: f64) -> f64 {
    if value > WELLNESS_SLEEP_HEURISTIC_THRESHOLD {
        value / SECONDS_PER_HOUR
    } else {
        value
    }
}

/// Extract sleep hours from a single wellness entry with the shared pipeline:
/// first matching `SLEEP_KEYS` (f64 or i64), the over-24-means-seconds
/// heuristic, then the plausibility filter. Returns `None` when absent or
/// an artifact — never a bogus average component.
/// Shared by the wellness parser and the training-plan snapshot so both
/// paths agree on the same payload.
pub(crate) fn sleep_hours_from_entry(obj: &serde_json::Map<String, Value>) -> Option<f64> {
    let hours = normalize_sleep_to_hours(get_number(obj, SLEEP_KEYS)?);
    is_plausible_sleep_hours(hours).then_some(hours)
}

/// Filter out sensor artifacts / time-in-bed values outside physiological range.
pub(crate) fn is_plausible_sleep_hours(value: f64) -> bool {
    (WELLNESS_SLEEP_MIN_PLAUSIBLE_HOURS..=WELLNESS_SLEEP_TYPICAL_MAX_HOURS).contains(&value)
}

/// Return a copy of entries sorted oldest-first (stable). Dates resolve from
/// `date` or `id`; entries without a parseable date sort first, so one
/// malformed row can no longer defeat normalization for the whole payload.
fn order_entries_oldest_first(entries: &[Value]) -> Vec<Value> {
    let mut indexed: Vec<(Option<NaiveDate>, usize, &Value)> = Vec::with_capacity(entries.len());
    for (idx, entry) in entries.iter().enumerate() {
        let date = entry.as_object().and_then(parse_entry_date);
        indexed.push((date, idx, entry));
    }
    indexed.sort_by_key(|(date, idx, _)| (*date, *idx));
    indexed
        .into_iter()
        .map(|(_, _, entry)| entry.clone())
        .collect()
}

/// Resolve the ISO date string from a wellness entry: prefer `date`, fall back to `id`
/// (Intervals.icu uses `id` as the day).
pub(crate) fn entry_date_str(obj: &serde_json::Map<String, Value>) -> Option<&str> {
    obj.get("date")
        .and_then(Value::as_str)
        .or_else(|| obj.get("id").and_then(Value::as_str))
}

/// Parse the entry date (`date` preferred, `id` fallback) as an ISO day.
/// Shared by the wellness parser, progress tracking, and training-plan snapshot
/// so all paths resolve real-API entries identically.
pub(crate) fn parse_entry_date(obj: &serde_json::Map<String, Value>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(entry_date_str(obj)?, "%Y-%m-%d").ok()
}

/// Return the entry carrying the strictly latest parseable date.
///
/// Undated entries never win over dated ones; on a dated tie the first
/// matching entry is kept (first-wins). Returns `None` for an empty slice
/// or when no entry carries a parseable date — callers decide their own
/// undated fallback (first object vs last entry).
pub(crate) fn latest_entry_by_date(entries: &[Value]) -> Option<&Value> {
    let mut latest: Option<(NaiveDate, &Value)> = None;
    for entry in entries {
        let Some(date) = entry.as_object().and_then(parse_entry_date) else {
            continue;
        };
        match latest {
            Some((current, _)) if date <= current => {}
            _ => latest = Some((date, entry)),
        }
    }
    latest.map(|(_, entry)| entry)
}

/// Extract dated numeric observations for the personal-baseline engine,
/// keeping only plausible samples so artifacts cannot poison long-window
/// means. Shared by the wellness parser and progress tracking.
pub(crate) fn extract_wellness_observations(
    entries: &[Value],
    keys: &[&str],
    plausible: fn(f64) -> bool,
) -> Vec<(NaiveDate, f64)> {
    entries
        .iter()
        .filter_map(|entry| {
            let obj = entry.as_object()?;
            let date = parse_entry_date(obj)?;
            let value = get_number(obj, keys).filter(|value| plausible(*value))?;
            Some((date, value))
        })
        .collect()
}

pub fn extract_ctl_series(payload: Option<&Value>) -> Option<(Vec<String>, Vec<f64>)> {
    let entries = payload?.as_array()?;
    let mut ordered = entries
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|object| {
            let date = entry_date_str(object)?.to_string();
            let ctl = ctl_value_from_entry(object)?;
            Some((date, ctl))
        })
        .collect::<Vec<_>>();

    ordered.sort_by_key(|(date, _)| parse_activity_date(date).unwrap_or(NaiveDate::MIN));
    if ordered.is_empty() {
        return None;
    }

    let dates = ordered.iter().map(|(date, _)| date.clone()).collect();
    let values = ordered.iter().map(|(_, ctl)| *ctl).collect();
    Some((dates, values))
}

pub fn extract_hrv_series(payload: Option<&Value>) -> Option<Vec<f64>> {
    let entries = payload?.as_array()?;
    let mut ordered = entries
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|object| {
            let date = entry_date_str(object)?.to_string();
            let hrv = get_number(object, HRV_KEYS).filter(|value| is_plausible_hrv(*value))?;
            Some((date, hrv))
        })
        .collect::<Vec<_>>();

    ordered.sort_by_key(|(date, _)| parse_activity_date(date).unwrap_or(NaiveDate::MIN));
    if ordered.is_empty() {
        return None;
    }

    Some(ordered.into_iter().map(|(_, value)| value).collect())
}

pub fn compute_lnrmssd_rollup(daily_hrv: &[f64]) -> LnRmssdRollup {
    // Delegate to the shared personal baseline engine for consistency
    // This function retains backward compatibility but uses the same formula
    // as compute_personal_baseline
    let ln_values = daily_hrv
        .iter()
        .copied()
        .filter(|value| *value > 0.0)
        .map(f64::ln)
        .collect::<Vec<_>>();

    if ln_values.len() < 7 {
        return LnRmssdRollup {
            supported: false,
            sample_count: ln_values.len(),
            ..Default::default()
        };
    }

    let recent = &ln_values[ln_values.len() - 7..];
    let mean = recent.iter().sum::<f64>() / recent.len() as f64;
    let variance = recent
        .iter()
        .map(|value| {
            let delta = value - mean;
            delta * delta
        })
        .sum::<f64>()
        / recent.len() as f64;

    LnRmssdRollup {
        supported: true,
        recent_mean_7d: Some(mean),
        recent_cv_7d: if mean.abs() < f64::EPSILON {
            None
        } else {
            Some(variance.sqrt() / mean.abs())
        },
        trend_slope: compute_hrv_trend_slope(recent),
        sample_count: ln_values.len(),
    }
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
