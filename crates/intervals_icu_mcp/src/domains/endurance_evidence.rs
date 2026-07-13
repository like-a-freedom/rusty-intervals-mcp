//! Serializable report contracts for the evidence-gated endurance submaximal
//! and prolonged-ride metrics.
//!
//! These types are deliberately descriptive: they emit raw observations and
//! provenance, never a readiness, fatigue, fitness, or durability score.

use serde::{Deserialize, Serialize};

/// Why an endurance-evidence observation is unavailable.
///
/// `Available` is the only status that carries numeric values. Every other
/// status is paired with a deterministic, user-visible reason in the
/// renderer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnduranceEvidenceStatus {
    Available,
    UnsupportedSport,
    MissingEftp,
    InsufficientCandidateSessions,
    NoComparableControlWindows,
    IncompleteSignalCoverage,
    NoEligibleProlongedRide,
    NoMatchedEarlyLateWindows,
}

impl EnduranceEvidenceStatus {
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Submaximal HR--power longitudinal response.
///
/// Reports raw, traceable observations with explicit provenance (source
/// activity ids and counts). The renderer must never label the sign of a
/// delta as good, bad, ready, fatigued, or durable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubmaximalHrPowerMetrics {
    pub status: EnduranceEvidenceStatus,
    pub activities_considered: usize,
    pub activities_accepted: usize,
    pub source_activity_ids: Vec<String>,
    pub anchor_power_w: Option<f64>,
    pub recent_median_hr_bpm: Option<f64>,
    pub reference_median_hr_bpm: Option<f64>,
    pub hr_delta_bpm: Option<f64>,
    pub recent_efficiency_w_per_bpm: Option<f64>,
    pub reference_efficiency_w_per_bpm: Option<f64>,
    pub efficiency_delta_pct: Option<f64>,
}

impl Default for SubmaximalHrPowerMetrics {
    fn default() -> Self {
        Self {
            status: EnduranceEvidenceStatus::InsufficientCandidateSessions,
            activities_considered: 0,
            activities_accepted: 0,
            source_activity_ids: Vec::new(),
            anchor_power_w: None,
            recent_median_hr_bpm: None,
            reference_median_hr_bpm: None,
            hr_delta_bpm: None,
            recent_efficiency_w_per_bpm: None,
            reference_efficiency_w_per_bpm: None,
            efficiency_delta_pct: None,
        }
    }
}

/// Matched early-to-late HR--power shift within a single prolonged ride.
///
/// Reports the matched early and late control windows and their raw
/// HR/efficiency deltas. The renderer must never call this a "durability
/// score", a "fatigue index", or a "readiness" verdict.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProlongedRideResponseMetrics {
    pub status: EnduranceEvidenceStatus,
    pub source_activity_id: Option<String>,
    pub early_window_end_min: Option<f64>,
    pub late_window_end_min: Option<f64>,
    pub matched_power_w: Option<f64>,
    pub early_hr_bpm: Option<f64>,
    pub late_hr_bpm: Option<f64>,
    pub hr_delta_bpm: Option<f64>,
    pub early_efficiency_w_per_bpm: Option<f64>,
    pub late_efficiency_w_per_bpm: Option<f64>,
    pub efficiency_delta_pct: Option<f64>,
}

impl Default for ProlongedRideResponseMetrics {
    fn default() -> Self {
        Self {
            status: EnduranceEvidenceStatus::NoEligibleProlongedRide,
            source_activity_id: None,
            early_window_end_min: None,
            late_window_end_min: None,
            matched_power_w: None,
            early_hr_bpm: None,
            late_hr_bpm: None,
            hr_delta_bpm: None,
            early_efficiency_w_per_bpm: None,
            late_efficiency_w_per_bpm: None,
            efficiency_delta_pct: None,
        }
    }
}

impl ProlongedRideResponseMetrics {
    pub fn unavailable(status: EnduranceEvidenceStatus) -> Self {
        debug_assert!(!status.is_available());
        Self {
            status,
            ..Self::default()
        }
    }
}

/// Aggregate evidence-gated endurance metrics, intended for the
/// `CoachMetrics::endurance_evidence` field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EnduranceEvidenceMetrics {
    pub submaximal: SubmaximalHrPowerMetrics,
    pub prolonged_response: ProlongedRideResponseMetrics,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endurance_evidence_round_trips_without_losing_provenance() {
        let evidence = EnduranceEvidenceMetrics {
            submaximal: SubmaximalHrPowerMetrics {
                status: EnduranceEvidenceStatus::Available,
                activities_considered: 12,
                activities_accepted: 4,
                source_activity_ids: vec!["r1".into(), "r2".into(), "b1".into(), "b2".into()],
                anchor_power_w: Some(200.0),
                recent_median_hr_bpm: Some(145.0),
                reference_median_hr_bpm: Some(150.0),
                hr_delta_bpm: Some(-5.0),
                recent_efficiency_w_per_bpm: Some(200.0 / 145.0),
                reference_efficiency_w_per_bpm: Some(200.0 / 150.0),
                efficiency_delta_pct: Some(3.448_275_862),
            },
            prolonged_response: ProlongedRideResponseMetrics::unavailable(
                EnduranceEvidenceStatus::NoEligibleProlongedRide,
            ),
        };

        let encoded = serde_json::to_value(&evidence).unwrap();
        let decoded: EnduranceEvidenceMetrics = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, evidence);
        assert!(decoded.submaximal.status.is_available());
        assert!(!decoded.prolonged_response.status.is_available());
    }

    #[test]
    fn default_status_never_claims_availability_for_partial_inputs() {
        let sub = SubmaximalHrPowerMetrics::default();
        assert_eq!(sub.status, EnduranceEvidenceStatus::InsufficientCandidateSessions);
        assert!(sub.anchor_power_w.is_none());
        assert!(sub.recent_median_hr_bpm.is_none());
        assert!(sub.efficiency_delta_pct.is_none());

        let prolonged = ProlongedRideResponseMetrics::default();
        assert_eq!(prolonged.status, EnduranceEvidenceStatus::NoEligibleProlongedRide);
        assert!(prolonged.hr_delta_bpm.is_none());
        assert!(prolonged.matched_power_w.is_none());
    }

    #[test]
    fn unavailable_prolonged_response_keeps_status_and_clears_numeric_fields() {
        let prolonged = ProlongedRideResponseMetrics::unavailable(
            EnduranceEvidenceStatus::NoMatchedEarlyLateWindows,
        );
        assert_eq!(prolonged.status, EnduranceEvidenceStatus::NoMatchedEarlyLateWindows);
        assert!(prolonged.early_hr_bpm.is_none());
        assert!(prolonged.late_hr_bpm.is_none());
        assert!(prolonged.early_window_end_min.is_none());
        assert!(prolonged.late_window_end_min.is_none());
    }

    #[test]
    fn is_available_only_matches_status_available() {
        assert!(EnduranceEvidenceStatus::Available.is_available());
        for unavailable in [
            EnduranceEvidenceStatus::UnsupportedSport,
            EnduranceEvidenceStatus::MissingEftp,
            EnduranceEvidenceStatus::InsufficientCandidateSessions,
            EnduranceEvidenceStatus::NoComparableControlWindows,
            EnduranceEvidenceStatus::IncompleteSignalCoverage,
            EnduranceEvidenceStatus::NoEligibleProlongedRide,
            EnduranceEvidenceStatus::NoMatchedEarlyLateWindows,
        ] {
            assert!(!unavailable.is_available(), "{unavailable:?} must not be available");
        }
    }

    #[test]
    fn endurance_evidence_metric_defaults_to_unavailable() {
        let evidence = EnduranceEvidenceMetrics::default();
        assert_eq!(evidence.submaximal.activities_accepted, 0);
        assert_eq!(evidence.submaximal.source_activity_ids.len(), 0);
        assert_eq!(
            evidence.submaximal.status,
            EnduranceEvidenceStatus::InsufficientCandidateSessions
        );
        assert_eq!(
            evidence.prolonged_response.status,
            EnduranceEvidenceStatus::NoEligibleProlongedRide
        );
    }
}
