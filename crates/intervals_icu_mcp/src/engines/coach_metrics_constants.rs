//! Constants for coaching metrics computation.
//! Extracted from coach_metrics.rs for separation of concerns.

pub const API_LOAD_ACUTE_KEYS: &[&str] = &["atlLoad", "icu_atl"];
pub const API_LOAD_CHRONIC_KEYS: &[&str] = &["ctlLoad", "icu_ctl"];
pub const FITNESS_CTL_KEYS: &[&str] = &["fitness", "ctl"];
pub const FITNESS_ATL_KEYS: &[&str] = &["fatigue", "atl"];
pub const FITNESS_TSB_KEYS: &[&str] = &["form", "tsb"];
/// Keys for the ramp rate (CTL/wk) — Intervals.icu uses `rampRate`, some
/// normalized payloads use `ramp_rate`.
pub const FITNESS_RAMP_RATE_KEYS: &[&str] = &["rampRate", "ramp_rate"];
pub const SLEEP_KEYS: &[&str] = &["sleep_hours", "sleepSecs", "sleep_secs"];
pub const RESTING_HR_KEYS: &[&str] =
    &["resting_hr", "restingHR", "resting_hr_bpm", "avgSleepingHR"];
pub const HRV_KEYS: &[&str] = &["hrv"];
pub const MOOD_KEYS: &[&str] = &["mood"];
pub const STRESS_KEYS: &[&str] = &["stress"];
pub const FATIGUE_KEYS: &[&str] = &["fatigue"];
pub const READINESS_KEYS: &[&str] = &["readiness"];
pub const RECENT_WELLNESS_WINDOW: usize = 7;
pub const HRV_BASELINE_WINDOW: usize = 28;
pub const HRV_WATCH_DROP_PCT: f64 = -6.0;
pub const HRV_SUPPRESSED_DROP_PCT: f64 = -12.0;
pub const EFFICIENCY_FACTOR_KEYS: &[&str] = &["icu_efficiency_factor", "efficiency_factor"];
pub const AEROBIC_DECOUPLING_KEYS: &[&str] = &["decoupling", "aerobic_decoupling"];
pub const HR_STREAM_KEYS: &[&str] = &["heartrate", "heart_rate", "hr"];
pub const OUTPUT_STREAM_KEYS: &[&str] = &["watts", "velocity_smooth", "pace"];
pub const MONOTONY_STDDEV_FLOOR_RATIO: f64 = 0.1;
pub const MONOTONY_CAP: f64 = 10.0;

// =============================================================================
// P0 — Performance Intelligence Constants
// =============================================================================

pub const NDLI_HIGH_INTENSITY_JOULES_THRESHOLD: f64 = 20000.0;
pub const NDLI_RUNNING_TSS_PROXY_THRESHOLD: f64 = 80.0;
pub const NDLI_IF_NORMALIZATION_THRESHOLD: f64 = 2.0;
pub const NDLI_RED_DAYS: usize = 4;
pub const NDLI_AMBER_DAYS: usize = 3;
pub const WDRM_HIGH_DEPLETION_PCT: f64 = 0.60;
pub const WDRM_MAX_DEPLETION_PCT: f64 = 1.5;
pub const Z2_MIN_POINTS: usize = 10;

// =============================================================================
// P1 — Coaching Intelligence Constants
// =============================================================================

pub const HRV_SUPPRESSION_RATIO: f64 = 0.88;
pub const HRV_RECOVERY_RATIO: f64 = 1.15;
pub const HRV_TREND_MIN_VALUES: usize = 3;
pub const IDEAL_SLEEP_HOURS: f64 = 8.0;
pub const RECOVERY_QUALITY_HRV_WEIGHT: f64 = 0.4;
pub const RECOVERY_QUALITY_RHR_WEIGHT: f64 = 0.3;
pub const RECOVERY_QUALITY_SLEEP_WEIGHT: f64 = 0.3;
pub const RHR_COMPONENT_MIN: f64 = 0.5;
pub const RHR_COMPONENT_MAX: f64 = 1.5;
pub const SLEEP_COMPONENT_MIN: f64 = 0.5;
pub const SLEEP_COMPONENT_MAX: f64 = 1.5;
pub const SECONDS_PER_HOUR: f64 = 3600.0;
pub const DAYS_PER_WEEK: f64 = 7.0;
pub const ACWR_ACUTE_LAMBDA: f64 = 2.0 / 8.0;
pub const ACWR_CHRONIC_LAMBDA: f64 = 2.0 / 29.0;
pub const ACWR_WATCH_RATIO: f64 = 1.5;
pub const ACWR_SAFE_LOWER: f64 = 0.8;
pub const ACWR_SAFE_UPPER: f64 = 1.3;
pub const DECOUPLING_MIN_POINTS: usize = 4;
pub const DECOUPLING_DRIFT_ABS_THRESHOLD: f64 = 8.0;
pub const DECOUPLING_DRIFT_SIGNED_THRESHOLD: f64 = 5.0;
pub const DECOUPLING_IMPROVING_THRESHOLD: f64 = -2.0;
pub const DECOUPLING_STABLE_THRESHOLD: f64 = 3.0;
pub const DECOUPLING_ACCEPTABLE_PCT: f64 = 5.0;
pub const DECOUPLING_WATCH_PCT: f64 = 10.0;
pub const CONSISTENCY_EXCELLENT_THRESHOLD: f64 = 0.9;
pub const CONSISTENCY_GOOD_THRESHOLD: f64 = 0.7;
pub const CONSISTENCY_MODERATE_THRESHOLD: f64 = 0.5;
pub const POLARISATION_BIASED_THRESHOLD: f64 = 0.75;
pub const WELLNESS_SLEEP_HEURISTIC_THRESHOLD: f64 = 24.0;
pub const WELLNESS_DEFAULT_SLEEP_HOURS: f64 = 7.0;
pub const ROUNDING_DECIMAL_FACTOR: f64 = 10.0;
pub const READINESS_MOOD_WEIGHT: f64 = 0.3;
pub const READINESS_SLEEP_WEIGHT: f64 = 0.3;
pub const READINESS_STRESS_WEIGHT: f64 = 0.2;
pub const READINESS_FATIGUE_WEIGHT: f64 = 0.2;
pub const READINESS_SLEEP_CLAMP_MAX: f64 = 10.0;

// =============================================================================
// P2 — Ultra-Sport Constants
// =============================================================================

pub const HEAT_BASELINE_TEMP_C: f64 = 18.0;
pub const HEAT_NORMALIZATION_RANGE_C: f64 = 5.0;
pub const HEAT_HIGH_THRESHOLD: f64 = 1.0;
pub const HEAT_MODERATE_THRESHOLD: f64 = 0.5;
pub const HEAT_INDEX_CLAMP_MAX: f64 = 2.0;
pub const IDEAL_P1M_P20M_RATIO: f64 = 1.8;
pub const POWER_CURVE_MILD_GAIN_PCT: f64 = 3.0;
pub const POWER_CURVE_MODERATE_GAIN_PCT: f64 = 5.0;
pub const TSB_LOAD_PRESSURE_THRESHOLD: f64 = -10.0;
pub const TSB_BALANCED_UPPER: f64 = 10.0;

// =============================================================================
// Bare-literal extractions (previously inline magic numbers)
// =============================================================================

pub const WEEKS_FLOOR_MIN: f64 = 1.0;
pub const ACWR_MIN_LOOKBACK_DAYS: usize = 28;
pub const MONOTONY_MIN_LOOKBACK_DAYS: usize = 7;
pub const LOAD_MGMT_WINDOW_DAYS: usize = 7;
pub const STRESS_TOLERANCE_DIVISOR: f64 = 100.0;
pub const SLEEP_CLAMP_MIN: f64 = 0.0;
pub const PCT_SCALING_FACTOR: f64 = 100.0;
pub const POLARISATION_DENOMINATOR_FACTOR: f64 = 2.0;
pub const POWER_CURVE_DECLINE_THRESHOLD: f64 = -1.0;
pub const POWER_CURVE_STABLE_THRESHOLD: f64 = 1.0;
pub const POWER_CURVE_ROTATION_AVERAGE: f64 = 2.0;
