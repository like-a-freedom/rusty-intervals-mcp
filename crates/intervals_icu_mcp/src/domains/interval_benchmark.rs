//! Interval-detection corpus validation and accuracy scoring.
//!
//! This module is the *second* benchmark layer from the design spec: given an
//! annotated gold corpus and a detector's predictions, it reports dataset
//! readiness and per-segment/per-class accuracy. It is deliberately gated: a
//! corpus that lacks labels, source hashes, split assignment, or negative
//! classes is reported as not scoreable instead of producing a misleading score.
//!
//! It does **not** contain the detector itself (see `interval_detection`).

use serde::Deserialize;
use std::collections::HashMap;

/// Session intent class. Mirrors the `intent_class` values in the corpus
/// contract (`docs/superpowers/specs/...interval-detection-benchmark-design.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionClass {
    StructuredInterval,
    Fartlek,
    SteadyOrTempo,
    Progression,
    Mixed,
    UnusableOrUnknown,
}

impl SessionClass {
    #[allow(dead_code)]
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "structured_interval" => Some(Self::StructuredInterval),
            "fartlek" => Some(Self::Fartlek),
            "steady_or_tempo" => Some(Self::SteadyOrTempo),
            "progression" => Some(Self::Progression),
            "mixed" => Some(Self::Mixed),
            "unusable_or_unknown" => Some(Self::UnusableOrUnknown),
            _ => None,
        }
    }
}

/// Phase of a single annotated/derived segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentPhase {
    Warmup,
    WorkRep,
    Recovery,
    FartlekSurge,
    FartlekEasy,
    Steady,
    Cooldown,
    Pause,
    Unknown,
}

impl SegmentPhase {
    #[allow(dead_code)]
    fn from_str(value: &str) -> Self {
        match value {
            "warmup" => Self::Warmup,
            "work_rep" => Self::WorkRep,
            "recovery" => Self::Recovery,
            "fartlek_surge" => Self::FartlekSurge,
            "fartlek_easy" => Self::FartlekEasy,
            "steady" => Self::Steady,
            "cooldown" => Self::Cooldown,
            "pause" => Self::Pause,
            _ => Self::Unknown,
        }
    }
}

/// Half-open temporal range `[start, end)` measured in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
}

impl TimeRange {
    fn intersection(&self, other: &TimeRange) -> f64 {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (end - start).max(0.0)
    }

    fn union(&self, other: &TimeRange) -> f64 {
        let start = self.start.min(other.start);
        let end = self.end.max(other.end);
        (end - start).max(0.0)
    }

    /// Temporal Intersection-over-Union. Returns `0.0` when there is no overlap
    /// or either range is degenerate.
    pub fn iou(&self, other: &TimeRange) -> f64 {
        let intersection = self.intersection(other);
        if intersection <= 0.0 {
            return 0.0;
        }
        let union = self.union(other);
        if union <= 0.0 {
            return 0.0;
        }
        intersection / union
    }
}

/// A human-reviewed (gold) segment.
#[derive(Debug, Clone)]
pub struct GoldSegment {
    pub phase: SegmentPhase,
    pub range: TimeRange,
    pub excluded_from_scoring: bool,
}

/// A gold session: derived stream annotation plus the metadata the contract
/// requires before a corpus may be scored.
#[derive(Debug, Clone)]
pub struct GoldSession {
    pub id: String,
    pub session_class: SessionClass,
    pub source_sha256: Option<String>,
    pub label_confidence: Option<String>,
    pub split: Option<String>,
    pub segments: Vec<GoldSegment>,
}

/// A detector-produced segment for a single session.
#[derive(Debug, Clone)]
pub struct PredictedSegment {
    pub phase: SegmentPhase,
    pub range: TimeRange,
}

/// A detector prediction aligned by session id.
#[derive(Debug, Clone)]
pub struct DetectorPrediction {
    pub session_id: String,
    pub segments: Vec<PredictedSegment>,
}

/// Result of checking whether a corpus may be scored.
#[derive(Debug, Clone)]
pub struct CorpusReadiness {
    pub accuracy_ready: bool,
    pub blockers: Vec<String>,
}

/// Per-class confusion and quality metrics.
#[derive(Debug, Clone, Default)]
pub struct ClassMetrics {
    pub session_count: usize,
    pub true_positive: usize,
    pub false_positive: usize,
    pub false_negative: usize,
}

/// Absolute start/end boundary error summary in seconds.
#[derive(Debug, Clone, Default)]
pub struct BoundaryErrorSummary {
    pub matched_count: usize,
    pub sum_start_error: f64,
    pub sum_end_error: f64,
}

#[derive(Debug, Default)]
pub struct SegmentMetrics {
    pub true_positive: usize,
    pub false_positive: usize,
    pub false_negative: usize,
}

/// Full accuracy report produced by [`score_sessions`].
#[derive(Debug, Default)]
pub struct AccuracyReport {
    pub session_count: usize,
    pub segment_metrics: SegmentMetrics,
    pub class_metrics: HashMap<SessionClass, ClassMetrics>,
    pub boundary_errors: BoundaryErrorSummary,
    pub count_mae: f64,
}

/// Required presence checks for a corpus to be accuracy-ready. A positive
/// structured-interval session and at least one negative (fartlek) session must
/// both be present, otherwise accuracy metrics would be biased.
    fn required_classes() -> &'static [SessionClass] {
        &[SessionClass::StructuredInterval, SessionClass::Fartlek]
    }

    fn class_label(class: SessionClass) -> &'static str {
        match class {
            SessionClass::StructuredInterval => "structured_interval",
            SessionClass::Fartlek => "fartlek",
            SessionClass::SteadyOrTempo => "steady_or_tempo",
            SessionClass::Progression => "progression",
            SessionClass::Mixed => "mixed",
            SessionClass::UnusableOrUnknown => "unusable_or_unknown",
        }
    }

/// Validate a gold corpus for accuracy readiness.
///
/// Returns blockers (instead of a manufactured score) when labels, source
/// hashes, split assignment, or required session classes are absent.
pub fn validate_corpus(sessions: &[GoldSession]) -> CorpusReadiness {
    let mut blockers: Vec<String> = Vec::new();

    if sessions.is_empty() {
        blockers.push("corpus has no sessions".to_string());
    }

    for session in sessions {
        if session.source_sha256.as_ref().map(String::is_empty).unwrap_or(true) {
            blockers.push(format!(
                "session {} missing source sha256",
                session.id
            ));
        }
        if session
            .label_confidence
            .as_ref()
            .map(String::is_empty)
            .unwrap_or(true)
        {
            blockers.push(format!(
                "session {} missing label confidence",
                session.id
            ));
        }
        if session.split.as_ref().map(String::is_empty).unwrap_or(true) {
            blockers.push(format!("session {} missing split assignment", session.id));
        }
    }

    for required in required_classes() {
        if !sessions.iter().any(|s| s.session_class == *required) {
            blockers.push(format!(
                "corpus missing required {} session (negative classes required)",
                class_label(*required)
            ));
        }
    }

    let accuracy_ready = blockers.is_empty();
    CorpusReadiness {
        accuracy_ready,
        blockers,
    }
}

/// Score detector predictions against gold sessions using one-to-one
/// maximum-IoU matching of `work_rep` segments.
///
/// Excluded gold segments do not participate in matching. Predictions whose best
/// IoU against any gold work-rep is below `iou_threshold` count as false
/// positives.
pub fn score_sessions(
    gold: &[GoldSession],
    predictions: &[DetectorPrediction],
    iou_threshold: f64,
) -> AccuracyReport {
    let mut report = AccuracyReport {
        session_count: gold.len(),
        ..Default::default()
    };

    let mut count_abs_error_total = 0usize;

    for gold_session in gold {
        // Gold work reps eligible for matching.
        let gold_reps: Vec<&GoldSegment> = gold_session
            .segments
            .iter()
            .filter(|seg| seg.phase == SegmentPhase::WorkRep && !seg.excluded_from_scoring)
            .collect();
        let prediction = predictions
            .iter()
            .find(|p| p.session_id == gold_session.id);

        let pred_reps: Vec<&PredictedSegment> = match prediction {
            Some(p) => p
                .segments
                .iter()
                .filter(|seg| seg.phase == SegmentPhase::WorkRep)
                .collect(),
            None => Vec::new(),
        };

        // Greedy maximum-IoU one-to-one assignment.
        let mut used_gold = vec![false; gold_reps.len()];
        let mut used_pred = vec![false; pred_reps.len()];

        let mut pairs: Vec<(usize, usize, f64)> = Vec::new();
        for (gi, gold_seg) in gold_reps.iter().enumerate() {
            for (pi, pred_seg) in pred_reps.iter().enumerate() {
                let iou = gold_seg.range.iou(&pred_seg.range);
                if iou >= iou_threshold {
                    pairs.push((gi, pi, iou));
                }
            }
        }
        pairs.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        for (gi, pi, _iou) in pairs {
            if used_gold[gi] || used_pred[pi] {
                continue;
            }
            used_gold[gi] = true;
            used_pred[pi] = true;
            report.segment_metrics.true_positive += 1;
            report.boundary_errors.matched_count += 1;
            report.boundary_errors.sum_start_error +=
                (gold_reps[gi].range.start - pred_reps[pi].range.start).abs();
            report.boundary_errors.sum_end_error +=
                (gold_reps[gi].range.end - pred_reps[pi].range.end).abs();
        }

        let matched_gold = used_gold.iter().filter(|x| **x).count();
        let matched_pred = used_pred.iter().filter(|x| **x).count();
        report.segment_metrics.false_negative += gold_reps.len() - matched_gold;
        report.segment_metrics.false_positive += pred_reps.len() - matched_pred;

        count_abs_error_total += gold_reps.len().abs_diff(pred_reps.len());
    }

    let denom = gold.len().max(1) as f64;
    report.count_mae = count_abs_error_total as f64 / denom;

    let class_entry = report
        .class_metrics
        .entry(SessionClass::StructuredInterval)
        .or_default();
    class_entry.session_count = gold.len();
    class_entry.true_positive = report.segment_metrics.true_positive;
    class_entry.false_positive = report.segment_metrics.false_positive;
    class_entry.false_negative = report.segment_metrics.false_negative;

    report
}

/// Parsed manifest metadata for a loaded corpus.
#[derive(Debug, Clone)]
pub struct CorpusManifestInfo {
    pub corpus_version: u32,
}

/// A loaded, annotated gold corpus.
#[derive(Debug, Clone)]
pub struct LoadedCorpus {
    pub sessions: Vec<GoldSession>,
    pub stream_paths: Vec<String>,
    pub manifest: CorpusManifestInfo,
}

#[derive(Deserialize)]
struct RawCorpusManifest {
    schema_version: u32,
    corpus_version: u32,
    sessions: Vec<RawCorpusSessionRef>,
}

#[derive(Deserialize)]
struct RawCorpusSessionRef {
    id: String,
    split: String,
    annotation: String,
    stream: String,
}

#[derive(Deserialize)]
struct RawAnnotation {
    schema_version: u32,
    intent_class: String,
    label_confidence: String,
    source_sha256: String,
    segments: Vec<RawSegment>,
}

#[derive(Deserialize)]
struct RawSegment {
    phase: String,
    start_s: f64,
    end_s: f64,
    excluded_from_scoring: bool,
}

/// Load a frozen corpus fixture from `tests/fixtures/interval_detection/`.
///
/// Each manifest entry references an annotation document (parsed into a
/// [`GoldSession`]) and a derived stream fixture (consumed later by the
/// detector). The corpus is treated as immutable; fixture hashes are frozen
/// before any parameter tuning.
pub fn load_corpus_fixture(name: &str) -> LoadedCorpus {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/interval_detection/"
    );
    let manifest_path = format!("{dir}{name}");
    let manifest_text =
        std::fs::read_to_string(&manifest_path).expect("corpus fixture must exist");
    let manifest: RawCorpusManifest =
        serde_json::from_str(&manifest_text).expect("corpus manifest must be valid JSON");
    assert_eq!(manifest.schema_version, 1, "corpus fixture schema mismatch");

    let mut sessions = Vec::with_capacity(manifest.sessions.len());
    let mut stream_paths = Vec::with_capacity(manifest.sessions.len());
    for reference in manifest.sessions {
        let annotation_path = format!("{dir}{}", reference.annotation);
        let annotation_text = std::fs::read_to_string(&annotation_path)
            .expect("annotation fixture must exist");
        let annotation: RawAnnotation =
            serde_json::from_str(&annotation_text).expect("annotation must be valid JSON");
        assert_eq!(annotation.schema_version, 1, "annotation schema mismatch");

        let session_class = SessionClass::from_str(&annotation.intent_class)
            .expect("annotation must declare a valid intent_class");

        let segments = annotation
            .segments
            .iter()
            .map(|segment| GoldSegment {
                phase: SegmentPhase::from_str(&segment.phase),
                range: TimeRange {
                    start: segment.start_s,
                    end: segment.end_s,
                },
                excluded_from_scoring: segment.excluded_from_scoring,
            })
            .collect();

        sessions.push(GoldSession {
            id: reference.id.clone(),
            session_class,
            source_sha256: Some(annotation.source_sha256),
            label_confidence: Some(annotation.label_confidence),
            split: Some(reference.split.clone()),
            segments,
        });
        stream_paths.push(reference.stream.clone());
    }

    LoadedCorpus {
        sessions,
        stream_paths,
        manifest: CorpusManifestInfo {
            corpus_version: manifest.corpus_version,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structured_interval_session(id: &str) -> GoldSession {
        GoldSession {
            id: id.to_string(),
            session_class: SessionClass::StructuredInterval,
            source_sha256: Some(format!("sha-{id}")),
            label_confidence: Some("confirmed".to_string()),
            split: Some("test".to_string()),
            segments: vec![GoldSegment {
                phase: SegmentPhase::WorkRep,
                range: TimeRange {
                    start: 10.0,
                    end: 70.0,
                },
                excluded_from_scoring: false,
            }],
        }
    }

    fn gold_session_with_work_rep(start: f64, end: f64) -> GoldSession {
        GoldSession {
            id: "gold-1".to_string(),
            session_class: SessionClass::StructuredInterval,
            source_sha256: Some("sha-gold".to_string()),
            label_confidence: Some("confirmed".to_string()),
            split: Some("test".to_string()),
            segments: vec![GoldSegment {
                phase: SegmentPhase::WorkRep,
                range: TimeRange { start, end },
                excluded_from_scoring: false,
            }],
        }
    }

    fn prediction_with_work_rep(start: f64, end: f64) -> DetectorPrediction {
        DetectorPrediction {
            session_id: "gold-1".to_string(),
            segments: vec![PredictedSegment {
                phase: SegmentPhase::WorkRep,
                range: TimeRange { start, end },
            }],
        }
    }

    #[test]
    fn corpus_with_only_positive_sessions_is_not_accuracy_ready() {
        let readiness = validate_corpus(&[structured_interval_session("a")]);
        assert!(!readiness.accuracy_ready);
        assert!(
            readiness.blockers.iter().any(|b| b.contains("fartlek")),
            "expected a fartlek/negative-class blocker, got: {:?}",
            readiness.blockers
        );
    }

    #[test]
    fn scorer_matches_one_predicted_rep_to_one_gold_rep() {
        let report = score_sessions(
            &[gold_session_with_work_rep(10.0, 70.0)],
            &[prediction_with_work_rep(15.0, 65.0)],
            0.5,
        );
        assert_eq!(report.segment_metrics.true_positive, 1);
        assert_eq!(report.segment_metrics.false_positive, 0);
        assert_eq!(report.segment_metrics.false_negative, 0);
    }

    #[test]
    fn locked_corpus_v1_is_accuracy_ready() {
        let corpus = load_corpus_fixture("corpus-v1.json");
        let readiness = validate_corpus(&corpus.sessions);
        assert!(
            readiness.accuracy_ready,
            "corpus not accuracy-ready: {:?}",
            readiness.blockers
        );
    }
}
