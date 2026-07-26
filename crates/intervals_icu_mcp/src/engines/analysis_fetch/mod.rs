use std::collections::HashMap;

use intervals_icu_client::{ActivityMessage, ActivitySummary, Event};
use serde_json::Value;

use crate::domains::coach::AnalysisWindow;

mod decode;
mod fetch;
mod load;

#[cfg(test)]
mod tests;

// ── Endurance evidence fetch contract ────────────────────────────────
//
// Constants and the `collect_endurance_evidence` function have been
// extracted to `endurance_evidence_fetch.rs`. Re-export for backward
// compatibility.
pub use super::endurance_evidence_fetch::{
    ENDURANCE_EVIDENCE_LOOKBACK_DAYS, ENDURANCE_EVIDENCE_MAX_RECENT_CANDIDATES,
    ENDURANCE_EVIDENCE_MAX_REFERENCE_CANDIDATES, ENDURANCE_EVIDENCE_MIN_MOVING_TIME_S,
    collect_endurance_evidence,
};

// ── Load module re-exports ───────────────────────────────────────────
pub use load::{
    PERSONAL_BASELINE_WINDOW_DAYS, activity_load, activity_lookback_days, build_daily_load_series,
    build_previous_window, extract_activity_load, required_activity_window,
};

// ── Decode module re-exports ─────────────────────────────────────────
pub(crate) use decode::{normalize_streams_payload, value_is_empty};

#[cfg(test)]
pub(crate) use decode::{normalize_intervals_payload, normalize_upcoming_events_payload};

// ── Fetch module re-exports ──────────────────────────────────────────
pub use fetch::{
    fetch_calendar_events_between, fetch_period_data, fetch_race_data, fetch_recovery_data,
    fetch_single_workout_data,
};

#[cfg(test)]
pub(crate) use fetch::parse_planned_workout;

// ── Request/response types ───────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct PeriodFetchRequest {
    pub window: AnalysisWindow,
    pub include_activity_details: bool,
    pub include_comparison_window: bool,
    /// Internal-only flag: when true, `fetch_period_data` performs the
    /// bounded historical profile retrieval required for the endurance
    /// evidence report. Always false for summary-mode period analysis.
    pub include_endurance_evidence: bool,
}

#[derive(Debug, Clone)]
pub struct SingleWorkoutFetchRequest {
    pub activity_id: String,
    pub include_intervals: bool,
    pub include_streams: bool,
    pub include_best_efforts: bool,
    pub include_hr_histogram: bool,
    pub include_power_histogram: bool,
    pub include_pace_histogram: bool,
}

#[derive(Debug, Clone)]
pub struct RecoveryFetchRequest {
    pub period_days: i32,
    pub include_wellness: bool,
}

#[derive(Debug, Clone)]
pub struct RaceFetchRequest {
    pub activity_id: String,
    pub include_intervals: bool,
    pub include_streams: bool,
}

/// State of a single upstream data source after a fetch attempt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SourceFetchState {
    #[default]
    NotRequested,
    Available,
    Empty,
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct FetchedAnalysisData {
    pub activities: Vec<ActivitySummary>,
    pub comparison_activities: Vec<ActivitySummary>,
    pub calendar_events: Vec<Event>,
    pub fetch_warnings: Vec<String>,
    pub activity_messages: Vec<ActivityMessage>,
    pub activity_details: HashMap<String, Value>,
    pub workout_detail: Option<Value>,
    pub fitness: Option<Value>,
    pub wellness: Option<Value>,
    pub intervals: Option<Value>,
    pub streams: Option<Value>,
    pub best_efforts: Option<Value>,
    pub hr_histogram: Option<Value>,
    pub power_histogram: Option<Value>,
    pub pace_histogram: Option<Value>,
    pub intervals_state: SourceFetchState,
    pub streams_state: SourceFetchState,
    /// Historical ride activities scanned for endurance evidence.
    /// Sorted descending by parsed date. May be partial when fetch
    /// degraded.
    pub endurance_profile_activities: Vec<ActivitySummary>,
    /// Per-activity-id stream payloads for endurance evidence, post
    /// `normalize_streams_payload`. Stream count is bounded by the
    /// candidate caps in `PeriodFetchRequest`.
    pub endurance_profile_streams: HashMap<String, Value>,
}
