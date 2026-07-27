//! Readiness thresholds for the `assess_recovery` intent.
//!
//! The numeric values are intentionally local — they are not part of the
//! engine contract and do not need to be shared with `coach_guidance` or
//! other engines. They live here so the parent handler module stays focused
//! on the request/response shape.
//!
//! Each constant pairs an activity type (`Easy`, `Intensity`, `Long`, `Race`)
//! with the threshold below which the readiness verdict becomes more
//! conservative.
//!
//! See `PlannedActivity::readiness_copy` in [`super::planned_activity`] for
//! how these thresholds are combined into the final `(verdict, copy)` pair.

// =============================================================================
// Sleep thresholds (hours) per planned activity type.
// =============================================================================

/// Sleep below this for an easy session flips the verdict to "Caution".
pub(super) const READINESS_SLEEP_EASY: f64 = 6.0;
/// Sleep below this for an intensity session holds intensity.
pub(super) const READINESS_SLEEP_INTENSITY: f64 = 7.0;
/// Sleep below this for a long session flips the verdict to "Hold".
pub(super) const READINESS_SLEEP_LONG: f64 = 6.5;
/// Sleep below this for a race session is a red flag.
pub(super) const READINESS_SLEEP_RACE: f64 = 7.5;

// =============================================================================
// TSB thresholds per planned activity type.
// =============================================================================

/// TSB must be above this for an intensity session to be marked ready.
pub(super) const READINESS_TSB_INTENSITY: f64 = -5.0;
/// TSB must be above this for a long session to be marked ready.
pub(super) const READINESS_TSB_LONG: f64 = -8.0;
/// TSB must be above this for a race session to be marked ready.
pub(super) const READINESS_TSB_RACE: f64 = 5.0;

// =============================================================================
// Recovery-index thresholds per planned activity type.
// =============================================================================

/// Recovery index must be above this for an intensity session.
pub(super) const READINESS_RECOVERY_INDEX_INTENSITY: f64 = 0.95;
/// Recovery index must be above this for a long session.
pub(super) const READINESS_RECOVERY_INDEX_LONG: f64 = 0.9;
/// Recovery index must be above this for a race session.
pub(super) const READINESS_RECOVERY_INDEX_RACE: f64 = 1.1;
