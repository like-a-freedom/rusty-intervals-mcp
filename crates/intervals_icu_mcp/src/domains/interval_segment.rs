//! Types for coverage-aware interval segment metrics.
//!
//! These types are internal to analysis and rendering — they describe
//! enriched time ranges with independently measured metric streams and
//! are not a new wire format.

use crate::domains::interval_detection::TimeRange;

/// Where a segment's time boundaries came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentProvenance {
    LocalStructured,
    LocalFartlek,
    UpstreamIntervalsIcu,
}

/// What the segment represents in the session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentRole {
    Work,
    Surge,
    Recovery,
}

/// How speed is presented for the activity's sport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SportPresentation {
    Pace,
    Speed,
    Unknown,
}

/// A time window for metric enrichment, carrying a role label.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentWindow {
    pub role: SegmentRole,
    pub range: TimeRange,
}

/// Raw, aligned metric stream arrays for one activity.
///
/// Speed, HR, and power are optional; at least one must be present.
/// Missing or non-finite signal samples are `f64::NAN` so per-signal
/// coverage can be measured independently.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetricStreams {
    pub time_s: Vec<f64>,
    pub speed_mps: Option<Vec<f64>>,
    pub heartrate_bpm: Option<Vec<f64>>,
    pub power_w: Option<Vec<f64>>,
}

/// How much of a segment a given signal actually covers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SignalCoverage {
    pub sample_count: usize,
    pub covered_duration_s: f64,
    pub ratio: f64,
}

/// All metrics computed for one segment window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IntervalSegmentMetrics {
    pub duration_s: f64,
    pub distance_m: Option<f64>,
    pub avg_speed_mps: Option<f64>,
    pub low_speed_p05_mps: Option<f64>,
    pub high_speed_p95_mps: Option<f64>,
    pub avg_hr_bpm: Option<f64>,
    pub peak_hr_p95_bpm: Option<f64>,
    pub avg_power_w: Option<f64>,
    pub best_5s_power_w: Option<f64>,
    pub power_cv_pct: Option<f64>,
    pub speed_coverage: SignalCoverage,
    pub hr_coverage: SignalCoverage,
    pub power_coverage: SignalCoverage,
}

/// A segment with its computed metrics attached.
#[derive(Clone, Debug, PartialEq)]
pub struct EnrichedSegment {
    pub window: SegmentWindow,
    pub metrics: IntervalSegmentMetrics,
}

/// Repeat-consistency statistics for a homogeneous structured set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeriesConsistency {
    pub pace_cv_pct: Option<f64>,
    pub speed_cv_pct: Option<f64>,
    pub power_cv_pct: Option<f64>,
    pub first_to_last_pace_change_pct: Option<f64>,
    pub first_to_last_speed_change_pct: Option<f64>,
    pub first_to_last_power_change_pct: Option<f64>,
}

/// A complete report for one source (local structured, local fartlek, or
/// upstream).
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentSeriesReport {
    pub provenance: SegmentProvenance,
    pub efforts: Vec<EnrichedSegment>,
    pub recoveries: Vec<EnrichedSegment>,
    pub consistency: Option<SeriesConsistency>,
}
