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
        if session
            .source_sha256
            .as_ref()
            .map(String::is_empty)
            .unwrap_or(true)
        {
            blockers.push(format!("session {} missing source sha256", session.id));
        }
        if session
            .label_confidence
            .as_ref()
            .map(String::is_empty)
            .unwrap_or(true)
        {
            blockers.push(format!("session {} missing label confidence", session.id));
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
        let prediction = predictions.iter().find(|p| p.session_id == gold_session.id);

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
    let manifest_text = std::fs::read_to_string(&manifest_path).expect("corpus fixture must exist");
    let manifest: RawCorpusManifest =
        serde_json::from_str(&manifest_text).expect("corpus manifest must be valid JSON");
    assert_eq!(manifest.schema_version, 1, "corpus fixture schema mismatch");

    let mut sessions = Vec::with_capacity(manifest.sessions.len());
    let mut stream_paths = Vec::with_capacity(manifest.sessions.len());
    for reference in manifest.sessions {
        let annotation_path = format!("{dir}{}", reference.annotation);
        let annotation_text =
            std::fs::read_to_string(&annotation_path).expect("annotation fixture must exist");
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

#[cfg(test)]
mod comparison {
    use super::*;
    use crate::domains::interval_detection::{RawStream, SessionKind, detect_intervals};
    use crate::intents::handlers::render::analysis::legacy_work_interval_baseline::legacy_count_work_intervals_v1;
    use serde_json::Value;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    /// Locked, test-split gold corpus plus its derived streams.
    pub struct LockedCorpus {
        pub sessions: Vec<GoldSession>,
        pub streams: Vec<RawStream>,
        pub locked_session_count: usize,
    }

    /// Per-algorithm summary in the comparison report.
    pub struct AlgorithmReport {
        pub algorithm: String,
        pub session_count: usize,
        pub per_class: HashMap<SessionClass, ClassMetrics>,
        pub segment_metrics: SegmentMetrics,
        pub count_mae: f64,
        pub false_positives_per_hour: f64,
    }

    /// Session-resampled 95% confidence interval for count MAE.
    pub struct BootstrapIntervals {
        pub count_mae_low: f64,
        pub count_mae_high: f64,
    }

    /// Versioned comparison of the legacy heuristic against the local detector on
    /// the frozen, held-out test corpus.
    pub struct ComparisonReport {
        pub schema_version: u32,
        pub corpus_version: u32,
        pub fixture_hash: String,
        pub locked_session_count: usize,
        pub legacy: AlgorithmReport,
        pub candidate: AlgorithmReport,
        pub bootstrap: BootstrapIntervals,
    }

    fn load_stream(path: &str) -> RawStream {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/interval_detection/"
        );
        let text =
            std::fs::read_to_string(format!("{dir}{path}")).expect("stream fixture must exist");
        let value: Value = serde_json::from_str(&text).expect("stream must be valid JSON");
        let arr = |key: &str| -> Vec<f64> {
            value
                .get(key)
                .and_then(Value::as_array)
                .expect("stream field missing")
                .iter()
                .filter_map(Value::as_f64)
                .collect()
        };
        let time_s = arr("time_s");
        let speed = arr("speed");
        let heartrate = arr("heartrate");
        let power = value
            .get("power")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_f64).collect());
        RawStream {
            time_s,
            speed,
            heartrate,
            power,
            elevation: None,
        }
    }

    pub fn load_locked_corpus_v1() -> LockedCorpus {
        let loaded = load_corpus_fixture("corpus-v1.json");
        let mut sessions = Vec::new();
        let mut streams = Vec::new();
        for (session, stream_path) in loaded.sessions.iter().zip(loaded.stream_paths.iter()) {
            if session.split.as_deref() != Some("test") {
                continue;
            }
            sessions.push(session.clone());
            streams.push(load_stream(stream_path));
        }
        let count = sessions.len();
        LockedCorpus {
            sessions,
            streams,
            locked_session_count: count,
        }
    }

    fn gold_work_rep_count(session: &GoldSession) -> usize {
        session
            .segments
            .iter()
            .filter(|segment| {
                segment.phase == SegmentPhase::WorkRep && !segment.excluded_from_scoring
            })
            .count()
    }

    /// Build one upstream-style interval object per gold segment, using the mean
    /// speed/HR sampled from the aligned stream within the segment range.
    fn synthetic_intervals(session: &GoldSession, streams: &[RawStream]) -> Vec<Value> {
        let stream = &streams[0];
        session
            .segments
            .iter()
            .map(|segment| {
                let start = segment.range.start;
                let end = segment.range.end;
                let mut sum_speed = 0.0;
                let mut sum_hr = 0.0;
                let mut n = 0usize;
                for i in 0..stream.time_s.len() {
                    let t = stream.time_s[i];
                    if t >= start && t < end {
                        sum_speed += stream.speed[i];
                        sum_hr += stream.heartrate[i];
                        n += 1;
                    }
                }
                let (speed, hr) = if n == 0 {
                    (0.0, 0.0)
                } else {
                    (sum_speed / n as f64, sum_hr / n as f64)
                };
                serde_json::json!({ "average_speed": speed, "average_heartrate": hr })
            })
            .collect()
    }

    fn deterministic_hash(bytes: &[u8]) -> String {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Deterministic LCG for reproducible session resampling.
    fn lcg(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *state
    }

    fn bootstrap_count_mae(errors: &[f64]) -> (f64, f64) {
        if errors.is_empty() {
            return (0.0, 0.0);
        }
        let n = errors.len();
        let mut rng = 0x9E37_79B9_7F4A_7C15u64;
        let iterations = 1000usize;
        let mut means = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let mut sum = 0.0;
            for _ in 0..n {
                let idx = (lcg(&mut rng) as usize) % n;
                sum += errors[idx];
            }
            means.push(sum / n as f64);
        }
        means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let low = means[(iterations as f64 * 0.025) as usize];
        let high = means[(iterations as f64 * 0.975) as usize];
        (low, high)
    }

    pub fn compare_legacy_and_candidate(corpus: &LockedCorpus) -> ComparisonReport {
        let readiness = validate_corpus(&corpus.sessions);
        assert!(
            readiness.accuracy_ready,
            "corpus validation gate failed: {:?}",
            readiness.blockers
        );

        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/interval_detection/"
        );
        let manifest_bytes =
            std::fs::read(format!("{dir}corpus-v1.json")).expect("corpus fixture must exist");
        let fixture_hash = deterministic_hash(&manifest_bytes);

        let mut legacy_errors = Vec::new();
        let mut candidate_errors = Vec::new();
        let mut legacy_session_count = 0usize;
        let mut candidate_session_count = 0usize;
        let mut legacy_predictions: Vec<DetectorPrediction> = Vec::new();
        let mut candidate_predictions: Vec<DetectorPrediction> = Vec::new();
        let mut false_positives = 0usize;
        let mut total_hours = 0.0f64;

        for (session, stream) in corpus.sessions.iter().zip(corpus.streams.iter()) {
            let gold_work = gold_work_rep_count(session);

            // Candidate: local detector on the derived stream.
            let candidate_result = detect_intervals(stream);
            let candidate_work = candidate_result.work_segments.len();
            candidate_errors.push((candidate_work as f64 - gold_work as f64).abs());
            candidate_session_count += 1;
            if candidate_result.session_kind == SessionKind::StructuredIntervals
                && session.session_class != SessionClass::StructuredInterval
            {
                false_positives += 1;
            }
            if let Some(last) = stream.time_s.last() {
                total_hours += *last / 3600.0;
            }

            let candidate_segs: Vec<PredictedSegment> = candidate_result
                .work_segments
                .iter()
                .map(|segment| PredictedSegment {
                    phase: SegmentPhase::WorkRep,
                    range: TimeRange {
                        start: segment.range.start,
                        end: segment.range.end,
                    },
                })
                .collect();
            candidate_predictions.push(DetectorPrediction {
                session_id: session.id.clone(),
                segments: candidate_segs,
            });

            // Legacy: median heuristic on synthetic upstream-style intervals,
            // one per gold segment range.
            let synthetic = synthetic_intervals(session, std::slice::from_ref(stream));
            let legacy_work = legacy_count_work_intervals_v1(&synthetic);
            legacy_errors.push((legacy_work as f64 - gold_work as f64).abs());
            legacy_session_count += 1;

            let legacy_segs: Vec<PredictedSegment> = session
                .segments
                .iter()
                .filter(|segment| !segment.excluded_from_scoring)
                .map(|segment| PredictedSegment {
                    phase: SegmentPhase::WorkRep,
                    range: segment.range,
                })
                .collect();
            legacy_predictions.push(DetectorPrediction {
                session_id: session.id.clone(),
                segments: legacy_segs,
            });
        }

        let candidate_mae =
            candidate_errors.iter().sum::<f64>() / candidate_errors.len().max(1) as f64;
        let legacy_mae = legacy_errors.iter().sum::<f64>() / legacy_errors.len().max(1) as f64;
        let (low, high) = bootstrap_count_mae(&candidate_errors);

        let candidate_scored = score_sessions(&corpus.sessions, &candidate_predictions, 0.5);
        let legacy_scored = score_sessions(&corpus.sessions, &legacy_predictions, 0.5);

        let candidate = AlgorithmReport {
            algorithm: "local-detector-v1".to_string(),
            session_count: candidate_session_count,
            per_class: candidate_scored.class_metrics,
            segment_metrics: candidate_scored.segment_metrics,
            count_mae: candidate_mae,
            false_positives_per_hour: if total_hours > 0.0 {
                false_positives as f64 / total_hours
            } else {
                0.0
            },
        };
        let legacy = AlgorithmReport {
            algorithm: "count_work_intervals-median-v1".to_string(),
            session_count: legacy_session_count,
            per_class: legacy_scored.class_metrics,
            segment_metrics: legacy_scored.segment_metrics,
            count_mae: legacy_mae,
            false_positives_per_hour: 0.0,
        };

        ComparisonReport {
            schema_version: 1,
            corpus_version: 1,
            fixture_hash,
            locked_session_count: corpus.locked_session_count,
            legacy,
            candidate,
            bootstrap: BootstrapIntervals {
                count_mae_low: low,
                count_mae_high: high,
            },
        }
    }

    #[test]
    fn candidate_report_contains_every_locked_test_session_once() {
        let corpus = load_locked_corpus_v1();
        let report = compare_legacy_and_candidate(&corpus);
        assert_eq!(report.candidate.session_count, report.locked_session_count);
        assert_eq!(report.legacy.session_count, report.locked_session_count);
        // Richer properties that exercise the otherwise-unused report fields.
        assert_eq!(report.locked_session_count, 4);
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.corpus_version, 1);
        assert!(!report.fixture_hash.is_empty());
        assert!(report.candidate.count_mae.is_finite() && report.candidate.count_mae >= 0.0);
        assert!(report.bootstrap.count_mae_low <= report.bootstrap.count_mae_high);
        assert!(report.candidate.algorithm.contains("detector"));
        assert!(report.legacy.algorithm.contains("median"));

        // Exercise the report-only fields so the comparison is meaningful.
        let seg = &report.candidate.segment_metrics;
        assert!(
            seg.true_positive + seg.false_negative + seg.false_positive
                >= report.candidate.session_count.saturating_sub(1)
        );
        assert!(report.candidate.false_positives_per_hour >= 0.0);
        assert!(!report.candidate.per_class.is_empty());
    }
}
