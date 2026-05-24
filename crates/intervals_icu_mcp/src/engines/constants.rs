//! Single source of truth for cross-engine constants.
//!
//! All engine files should import shared constants from here
//! instead of redefining their own private copies, to eliminate
//! drift (e.g. TSB thresholds defined 3 ways across 5 files).

// =============================================================================
// TSB (Training Stress Balance) thresholds
// =============================================================================

/// TSB: overreached / maladaptation risk (< -30).
/// Source: Banister EWMA impulse-response model — standard TSB classification for overreaching.
/// Used in: forecast, ade.
pub const TSB_OVERREACHED: f64 = -30.0;

/// TSB: deep fatigue / functional overreach (< -20).
/// Source: Banister EWMA — TSB between -20 and -30 indicates functional overreach (Friel, TrainingPeaks).
/// Used in: coach_guidance, ade.
pub const TSB_FUNCTIONAL_OVERREACH: f64 = -20.0;

/// TSB: load pressure / fatigued (< -10).
/// Source: Banister EWMA — TSB below -10 signals load pressure requiring attention.
/// Used in: forecast, ade, coach_metrics, coach_guidance.
pub const TSB_LOAD_PRESSURE: f64 = -10.0;

/// TSB: balanced / fresh (≥ 10).
/// Source: Banister EWMA — TSB above 10 indicates recovered / fresh state.
/// Used in: forecast, coach_metrics, coach_guidance.
pub const TSB_FRESH: f64 = 10.0;

/// Alias for TSB_LOAD_PRESSURE used in coach_guidance and assess_recovery.
pub const TSB_FATIGUED: f64 = TSB_LOAD_PRESSURE;

/// Alias for TSB_FUNCTIONAL_OVERREACH used in coach_guidance.
pub const TSB_DEEP_FATIGUE: f64 = TSB_FUNCTIONAL_OVERREACH;

/// TSB: fresh upper bound (> 25).
/// Source: Banister EWMA — TSB above 25 indicates peak freshness (rare outside taper).
/// Used in: forecast.
pub const TSB_FRESH_UPPER: f64 = 25.0;

// =============================================================================
// ACWR (Acute:Chronic Workload Ratio) bounds
// =============================================================================

/// ACWR: safe zone lower bound (≥ 0.8). Below = underload risk.
/// Source: Gabbett TJ, BJSM 2016 — ACWR < 0.8 associated with increased injury risk.
/// Used in: coach_metrics, ade.
pub const ACWR_SAFE_LOWER: f64 = 0.8;

/// ACWR: safe zone upper bound (≤ 1.3). Above = alert/watch zone.
/// Source: Gabbett TJ, BJSM 2016 — ACWR 0.8–1.3 is the 'sweet spot' for training adaptation.
/// Used in: coach_metrics, ade, coach_guidance.
pub const ACWR_SAFE_UPPER: f64 = 1.3;

/// ACWR: overreaching threshold (> 1.5). Above = critical risk.
/// Source: Gabbett TJ, BJSM 2016 — ACWR > 1.5 indicates very high injury risk.
/// Used in: coach_metrics, coach_guidance.
pub const ACWR_OVERREACH_RATIO: f64 = 1.5;

// =============================================================================
// WDRM depletion thresholds
// =============================================================================

/// WDRM: high depletion session threshold (60 % of W′).
/// Source: Montis.icu Coach V5 WDRM validation.
/// Used in: coach_metrics, coach_guidance.
pub const WDRM_HIGH_DEPLETION_PCT: f64 = 0.60;

/// WDRM: maximum depletion percentage clip (150 % of W′).
/// Source: Montis.icu Coach V5 WDRM validation.
/// Used in: coach_metrics.
pub const WDRM_MAX_DEPLETION_PCT: f64 = 1.5;

// =============================================================================
// HRV baselines and thresholds
// =============================================================================

/// HRV: lookback window for baseline computation (28 days).
/// Source: Plews et al., 2013, Eur J Appl Physiol — 28-day rolling baseline for lnRMSSD.
/// Used in: coach_metrics.
pub const HRV_BASELINE_WINDOW: usize = 28;

/// HRV: adaptive lookback for analysis fetch (35 days).
/// Source: Montis.icu analysis — 35-day window extends baseline for seasonal HRV trends.
/// Used in: analysis_fetch.
pub const ADAPTIVE_HRV_LOOKBACK_DAYS: i32 = 35;

/// HRV: suppression ratio threshold (< 0.88 × baseline).
/// Source: Front. Physiol. 2025, Nature Sci Reports 2025.
pub const HRV_SUPPRESSION_RATIO: f64 = 0.88;

/// HRV: maladaptation ratio for when TSB unavailable (< 0.90).
/// Source: Front. Physiol. 2025 — RMSSD suppression at 10 % below baseline.
pub const HRV_MALADAPTATION_RATIO: f64 = 0.90;

/// HRV: recovery threshold (> 1.15 × baseline).
/// Source: Front. Physiol. 2025 — RMSSD > 15 % above baseline signals recovery.
pub const HRV_RECOVERY_RATIO: f64 = 1.15;

/// HRV: minimum values for trend slope computation (3).
/// Source: Minimum 3 points for reliable linear regression on short HRV windows.
pub const HRV_TREND_MIN_VALUES: usize = 3;

/// HRV: stable threshold (≥ 60 ms).
/// Source: Buchheit, 2014, Front Physiol — RMSSD ≥ 60 ms indicates well-recovered vagal tone.
/// Used in: coach_guidance.
pub const HRV_STABLE_MS: f64 = 60.0;

/// HRV: low threshold (< 40 ms).
/// Source: Buchheit, 2014, Front Physiol — RMSSD < 40 ms indicates suppressed parasympathetic activity.
/// Used in: coach_guidance.
pub const HRV_LOW_MIN_MS: f64 = 40.0;

// =============================================================================
// Sleep thresholds
// =============================================================================

/// Sleep: ideal hours for recovery quality normalization (8 h).
/// Source: CDC/NHS standard sleep recommendation for adults (Hirshkowitz et al., 2015, Sleep Health).
pub const IDEAL_SLEEP_HOURS: f64 = 8.0;

/// Sleep: good threshold (≥ 7 h).
/// Source: Hirshkowitz et al., 2015, Sleep Health — ≥ 7 h sufficient for adults.
/// Used in: coach_guidance, assess_recovery handler.
pub const SLEEP_GOOD_HOURS: f64 = 7.0;

/// Sleep: fair minimum threshold (6 h).
/// Source: Hirshkowitz et al., 2015, Sleep Health — 6–7 h is insufficient but not severely restricted.
/// Used in: coach_guidance, assess_recovery handler.
pub const SLEEP_FAIR_MIN_HOURS: f64 = 6.0;

/// Sleep: alert threshold (< 6.5 h).
/// Source: Hirshkowitz et al., 2015 — < 6.5 h triggers alert for chronic sleep restriction.
pub const SLEEP_ALERT_HOURS: f64 = 6.5;

/// Sleep: clamp upper bound for readiness score (10 h).
/// Source: Empirical cap — sleep beyond 10 h provides diminishing recovery returns.
pub const SLEEP_CLAMP_MAX: f64 = 10.0;

// =============================================================================
// RHR (Resting Heart Rate) thresholds
// =============================================================================

/// RHR: normal threshold (≤ 55 bpm).
/// Source: AHA/ESC guidelines — 55 bpm normal for endurance athletes (Nanchen et al., 2013, Eur Heart J).
/// Used in: coach_guidance, assess_recovery handler.
pub const RHR_NORMAL_BPM: f64 = 55.0;

/// RHR: elevated threshold — 56–60 bpm.
/// Source: Nanchen et al., 2013, Eur Heart J — RHR > 60 bpm associated with elevated risk.
/// Used in: coach_guidance, assess_recovery handler.
pub const RHR_ELEVATED_MAX_BPM: f64 = 60.0;

// =============================================================================
// NDLI (Neural De-Load Index) thresholds
// =============================================================================

/// NDLI: high-intensity days threshold for "red" (overload) state (≥ 4).
/// Source: Montis.icu Coach V5 NDLI calibration — ≥ 4 high-intensity days = overload red zone.
/// Used in: coach_metrics, ade.
pub const NDLI_RED_DAYS: usize = 4;

/// NDLI: high-intensity days threshold for "amber" (watch) state (≥ 3).
/// Source: Montis.icu Coach V5 NDLI calibration — ≥ 3 high-intensity days = watch amber zone.
/// Used in: coach_metrics.
pub const NDLI_AMBER_DAYS: usize = 3;
