//! `PlannedActivity` enum and its readiness helpers.
//!
//! The four variants correspond to the four activity classes the recovery
//! intent reasons about: easy, intensity, long, and race. Each class has its
//! own readiness thresholds (sleep, TSB, recovery index) stored in the
//! sibling [`super::constants`] submodule.
//!
//! The enum exposes three things:
//! - [`PlannedActivity::parse`] — parses a free-form string (defaults to
//!   [`PlannedActivity::Easy`] when nothing matches).
//! - [`PlannedActivity::as_str`] — the inverse mapping for logging/UI.
//! - [`PlannedActivity::readiness_copy`] — given the current wellness,
//!   fitness and red-flag state, returns a `(verdict, copy)` tuple that the
//!   caller composes into the JSON response.

use crate::domains::coach::{FitnessMetrics, WellnessMetrics};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PlannedActivity {
    Easy,
    Intensity,
    Long,
    Race,
}

impl PlannedActivity {
    /// Parse a free-form string into a `PlannedActivity`.
    ///
    /// Defaults to [`PlannedActivity::Easy`] for `None`, empty string, or any
    /// unrecognised value.
    pub(super) fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("easy") {
            "intensity" => Self::Intensity,
            "long" => Self::Long,
            "race" => Self::Race,
            _ => Self::Easy,
        }
    }

    /// Inverse mapping for logging and structured responses.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Intensity => "intensity",
            Self::Long => "long",
            Self::Race => "race",
        }
    }

    /// Build a `(verdict, copy)` tuple for the planned activity.
    ///
    /// The verdict reflects whether current markers support the planned
    /// intensity. The copy is the human-facing explanation that follows it.
    pub(super) fn readiness_copy(
        self,
        wellness: &WellnessMetrics,
        fitness: &FitnessMetrics,
        red_flags: &[String],
    ) -> (String, String) {
        let sleep = wellness.avg_sleep_hours.unwrap_or(0.0);
        let recovery_index = wellness.recovery_index.unwrap_or(0.0);
        let tsb = fitness.tsb.unwrap_or(0.0);
        let has_flags = !red_flags.is_empty();

        match self {
            Self::Easy => {
                let verdict = if has_flags && sleep < super::constants::READINESS_SLEEP_EASY {
                    "Caution"
                } else {
                    "Green light"
                };
                (
                    verdict.to_string(),
                    if verdict == "Green light" {
                        "Easy training is appropriate; keep the session conversational and use it as recovery support.".to_string()
                    } else {
                        "Keep the day gentle and shorten or skip the session if fatigue rises during warm-up.".to_string()
                    },
                )
            }
            Self::Intensity => {
                let ready = !has_flags
                    && sleep >= super::constants::READINESS_SLEEP_INTENSITY
                    && tsb > super::constants::READINESS_TSB_INTENSITY
                    && recovery_index >= super::constants::READINESS_RECOVERY_INDEX_INTENSITY;
                (
                    if ready {
                        "Ready for quality"
                    } else {
                        "Hold intensity"
                    }
                    .to_string(),
                    if ready {
                        "Metrics support a quality session; keep the hard work controlled and protect the cooldown.".to_string()
                    } else {
                        "Today's recovery signals are not strong enough for a quality session; if HRV is below your personal baseline, swap intensity for aerobic work or rest.".to_string()
                    },
                )
            }
            Self::Long => {
                let ready = !has_flags
                    && sleep >= super::constants::READINESS_SLEEP_LONG
                    && tsb > super::constants::READINESS_TSB_LONG
                    && recovery_index >= super::constants::READINESS_RECOVERY_INDEX_LONG;
                (
                    if ready {
                        "Long run acceptable"
                    } else {
                        "Trim the long day"
                    }
                    .to_string(),
                    if ready {
                        "You can handle a long aerobic session, but keep fueling disciplined and cap any late surges.".to_string()
                    } else {
                        "Recovery is borderline for a long session; if HRV is below your personal baseline, reduce duration or convert the day to steady aerobic mileage.".to_string()
                    },
                )
            }
            Self::Race => {
                let ready = !has_flags
                    && sleep >= super::constants::READINESS_SLEEP_RACE
                    && tsb > super::constants::READINESS_TSB_RACE
                    && recovery_index >= super::constants::READINESS_RECOVERY_INDEX_RACE;
                (
                    if ready {
                        "Race-ready"
                    } else {
                        "Not race-ready"
                    }
                    .to_string(),
                    if ready {
                        "Current markers support a race effort; preserve freshness, finalize logistics, and avoid adding extra load.".to_string()
                    } else {
                        "Current recovery markers do not support a race-level effort; if HRV is below your personal baseline, prioritize recovery and reassess before racing.".to_string()
                    },
                )
            }
        }
    }
}
