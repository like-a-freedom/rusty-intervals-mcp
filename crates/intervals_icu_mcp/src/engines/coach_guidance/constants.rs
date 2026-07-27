//! Coach guidance thresholds.
//!
//! All numeric constants used by `build_alerts` and `build_guidance`.
//!
//! They are split into three sections that mirror the metric domains the coach
//! guidance engine reasons about:
//!
//! - **Wellness**: sleep hours, resting heart rate, HRV, recovery index.
//! - **Fitness / load**: TSB bands.
//! - **Volume**: weekly hours and load-management ratios (ACWR, monotony,
//!   fatigue index, durability index, race-readiness score).
//!
//! The constants are re-exported from the parent `coach_guidance` module so
//! that external call sites can keep using
//! `crate::engines::coach_guidance::SLEEP_GOOD_HOURS` and friends.
//!
//! Visibility rule:
//! - `pub(super)` here means visible inside the `coach_guidance` module tree
//!   only. The original public surface is preserved by `pub use` in the parent.

// `WDRM_HIGH_DEPLETION_PCT` lives in `coach_metrics_constants` and is
// imported at the top of the parent `coach_guidance.rs`. This file does not
// need to re-export it because `build_alerts` resolves it through the parent
// module.

// =============================================================================
// Wellness Thresholds
// =============================================================================

/// Sleep: good threshold (≥ this value).
pub const SLEEP_GOOD_HOURS: f64 = 7.0;
/// Sleep: fair minimum threshold (6.0–7.0).
pub const SLEEP_FAIR_MIN_HOURS: f64 = 6.0;
/// Sleep: alert threshold (< this value).
pub const SLEEP_ALERT_HOURS: f64 = 6.5;

/// RHR: normal threshold (≤ this value).
pub const RHR_NORMAL_BPM: f64 = 55.0;
/// RHR: elevated threshold (56–60).
pub const RHR_ELEVATED_MAX_BPM: f64 = 60.0;
/// RHR: alert threshold (> this value).
pub const RHR_ALERT_BPM: f64 = 60.0;

/// HRV: stable threshold (≥ this value).
pub const HRV_STABLE_MS: f64 = 60.0;
/// HRV: low threshold (40–60).
pub const HRV_LOW_MIN_MS: f64 = 40.0;
/// Recovery index: alert threshold (< this value).
pub const RECOVERY_INDEX_ALERT: f64 = 0.6;

// =============================================================================
// Fitness / Load Thresholds
// =============================================================================

/// TSB: fresh threshold (> this value).
pub const TSB_FRESH: f64 = 10.0;
/// TSB: fatigued threshold (< this value).
pub const TSB_FATIGUED: f64 = -10.0;
/// TSB: deep fatigue alert threshold (< this value).
pub const TSB_DEEP_FATIGUE: f64 = -20.0;

// =============================================================================
// Volume Thresholds
// =============================================================================

/// Weekly average hours: low volume threshold (< this value).
pub const WEEKLY_AVG_LOW_HOURS: f64 = 5.0;
/// Weekly average hours: high volume threshold (> this value).
pub const WEEKLY_AVG_HIGH_HOURS: f64 = 15.0;

/// ACWR: safe zone upper bound — values above this trigger an alert.
/// Maps to `coach_metrics::ACWR_SAFE_UPPER` (1.3).
/// NOT the same as `coach_metrics::ACWR_WATCH_RATIO` (1.5 — overreaching threshold).
pub const ACWR_ALERT_RATIO: f64 = 1.3;
/// ACWR: overreaching threshold — values above this are critical.
/// Maps to `coach_metrics::ACWR_WATCH_RATIO`.
pub const ACWR_OVERREACH_RATIO: f64 = 1.5;
/// Monotony: repetitive-stress threshold (Foster 1998 recommends ≤ 2.0; Seiler uses 2.5).
/// Values above this indicate insufficient training variety → elevated injury/overtraining risk.
pub const MONOTONY_ALERT: f64 = 2.5;
/// Fatigue Index: high fatigue alert threshold (> this value).
pub const FATIGUE_INDEX_ALERT: f64 = 2.5;
/// Durability Index: low durability alert threshold (< this value).
pub const DURABILITY_INDEX_ALERT: f64 = 0.85;

/// Race readiness: alert threshold for suboptimal preparation (< this score).
pub const RACE_READINESS_ALERT: i32 = 60;
/// Race readiness: below this score the alert is Priority, otherwise Caution.
pub const RACE_READINESS_CRITICAL: i32 = 40;

// =============================================================================
// `WDRM_HIGH_DEPLETION_PCT` lives in `coach_metrics_constants` and is
// imported at the top of the parent `coach_guidance.rs`. This file does not
// need to re-export it because `build_alerts` resolves it through the parent
// module.
