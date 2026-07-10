//! Local, stream-driven interval detection.
//!
//! This is the *future detector* referenced by the design spec. Unlike the
//! legacy `count_work_intervals` heuristic (which re-labels upstream interval
//! objects by median speed/HR), this module consumes **normalized stream
//! samples** and finds work/recovery boundaries itself.
//!
//! Design invariants (from the spec):
//! - Recording gaps are *exclusions*, never recovery segments.
//! - Fartlek (irregular surges) is reported as `Fartlek` and emits **no**
//!   structured work-set result.
//! - Heart rate is a lagging corroborating signal; velocity/power are the
//!   primary change signals. No hardcoded universal pace/HR threshold defines
//!   an interval.

/// Classification produced by [`detect_intervals`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    StructuredIntervals,
    Fartlek,
    Other,
    InsufficientData,
}

impl SessionKind {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "structured_interval" => Some(Self::StructuredIntervals),
            "fartlek" => Some(Self::Fartlek),
            "other" => Some(Self::Other),
            "insufficient_data" => Some(Self::InsufficientData),
            _ => None,
        }
    }
}

/// A detected temporal segment.
#[derive(Debug, Clone)]
pub struct DetectedSegment {
    pub phase: SegmentPhase,
    pub range: TimeRange,
    pub mean_intensity: f64,
}

/// Result of running the detector on a single session.
#[derive(Debug, Clone)]
pub struct IntervalDetectionResult {
    pub session_kind: SessionKind,
    pub work_segments: Vec<DetectedSegment>,
    pub recovery_segments: Vec<DetectedSegment>,
    pub confidence: Option<f64>,
    pub reasons: Vec<String>,
}

/// Raw, aligned stream arrays as supplied by a source (intervals.icu or a
/// derived fixture). `power` and `elevation` are optional; `speed` and
/// `heartrate` must be present.
#[derive(Debug, Clone)]
pub struct RawStream {
    pub time_s: Vec<f64>,
    pub speed: Vec<f64>,
    pub heartrate: Vec<f64>,
    pub power: Option<Vec<f64>>,
    pub elevation: Option<Vec<f64>>,
}

/// A single resampled, validated sample used for detection.
#[derive(Debug, Clone, Copy)]
pub struct NormalizedSample {
    pub t: f64,
    pub speed: f64,
    pub hr: f64,
    pub power: Option<f64>,
}

/// Normalized, validation-checked stream. `exclusions` records recording gaps
/// that must not be mistaken for recovery.
#[derive(Debug, Clone)]
pub struct NormalizedStream {
    pub samples: Vec<NormalizedSample>,
    pub exclusions: Vec<TimeRange>,
}

/// Tuning for normalization and detection.
#[derive(Debug, Clone)]
pub struct NormalizationConfig {
    pub gap_tolerance_s: f64,
    pub min_work_s: f64,
    pub min_recovery_s: f64,
    pub work_cv_threshold: f64,
    pub recovery_cv_threshold: f64,
    pub intensity_separation: f64,
    pub min_samples: usize,
}

impl Default for NormalizationConfig {
    fn default() -> Self {
        Self {
            gap_tolerance_s: 10.0,
            min_work_s: 15.0,
            min_recovery_s: 10.0,
            work_cv_threshold: 0.35,
            recovery_cv_threshold: 0.5,
            intensity_separation: 1.3,
            min_samples: 30,
        }
    }
}

/// Half-open temporal range `[start, end)` in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
}

/// Phase of a detected segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentPhase {
    WorkRep,
    Recovery,
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// Coefficient of variation (std/mean). Returns `1.0` for degenerate input so
/// that an empty or single-element series is treated as maximally irregular.
fn coeff_of_variation(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 1.0;
    }
    let m = mean(values);
    if m <= 0.0 {
        return 1.0;
    }
    let variance = values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len() as f64;
    variance.sqrt() / m
}

/// Normalize and validate a raw stream.
///
/// Returns an error when the required aligned signals are missing or of unequal
/// length. Recording gaps longer than `gap_tolerance_s` are recorded as
/// exclusions rather than interpolated.
pub fn normalize_streams(
    raw: &RawStream,
    config: &NormalizationConfig,
) -> Result<NormalizedStream, String> {
    if raw.time_s.is_empty() {
        return Err("empty stream".to_string());
    }
    if raw.time_s.len() != raw.speed.len() || raw.time_s.len() != raw.heartrate.len() {
        return Err("misaligned stream arrays".to_string());
    }
    if let Some(power) = &raw.power
        && power.len() != raw.time_s.len()
    {
        return Err("misaligned power array".to_string());
    }

    let mut samples = Vec::with_capacity(raw.time_s.len());
    for i in 0..raw.time_s.len() {
        samples.push(NormalizedSample {
            t: raw.time_s[i],
            speed: raw.speed[i],
            hr: raw.heartrate[i],
            power: raw.power.as_ref().map(|p| p[i]),
        });
    }

    let mut exclusions = Vec::new();
    for i in 1..samples.len() {
        let delta = samples[i].t - samples[i - 1].t;
        if delta > config.gap_tolerance_s {
            exclusions.push(TimeRange {
                start: samples[i - 1].t,
                end: samples[i].t,
            });
        }
    }

    Ok(NormalizedStream {
        samples,
        exclusions,
    })
}

/// Detect the session type and work/recovery structure from a raw stream.
///
/// Internally normalizes, selects a primary change signal (velocity, falling
/// back to power), threshold splits work vs recovery, merges sub-threshold
/// candidates, scores repetition regularity, and classifies.
pub fn detect_intervals(raw: &RawStream) -> IntervalDetectionResult {
    let config = NormalizationConfig::default();
    let normalized = match normalize_streams(raw, &config) {
        Ok(n) => n,
        Err(reason) => {
            return IntervalDetectionResult {
                session_kind: SessionKind::InsufficientData,
                work_segments: Vec::new(),
                recovery_segments: Vec::new(),
                confidence: None,
                reasons: vec![reason],
            };
        }
    };

    if normalized.samples.len() < config.min_samples {
        return IntervalDetectionResult {
            session_kind: SessionKind::InsufficientData,
            work_segments: Vec::new(),
            recovery_segments: Vec::new(),
            confidence: None,
            reasons: vec!["stream too short for detection".to_string()],
        };
    }

    // Select primary signal: speed if it varies, else power.
    let primary: Vec<f64> = normalized
        .samples
        .iter()
        .map(|s| {
            if s.speed > 0.0 {
                s.speed
            } else {
                s.power.unwrap_or(0.0)
            }
        })
        .collect();

    let max = primary.iter().cloned().fold(f64::MIN, f64::max);
    let low = primary.iter().cloned().fold(f64::MAX, f64::min);
    // Baseline is the low/easy intensity (recovery). Using the minimum rather
    // than the median avoids being skewed when work samples outnumber
    // recovery samples. Threshold sits halfway between easy and peak effort.
    let spread = (max - low).max(0.0);
    if spread <= f64::EPSILON {
        return IntervalDetectionResult {
            session_kind: SessionKind::Other,
            work_segments: Vec::new(),
            recovery_segments: Vec::new(),
            confidence: None,
            reasons: vec!["no intensity variation to detect intervals".to_string()],
        };
    }
    let threshold = low + 0.5 * spread;

    // Label each sample and break runs at recording gaps.
    let gap = config.gap_tolerance_s;
    let mut runs: Vec<(bool, f64, f64, f64)> = Vec::new(); // (is_work, start, end, mean_intensity)
    let mut i = 0;
    while i < normalized.samples.len() {
        let is_work = primary[i] >= threshold;
        let start = normalized.samples[i].t;
        let mut j = i;
        let mut sum = 0.0;
        let mut count = 0usize;
        while j < normalized.samples.len() {
            let is_work_j = primary[j] >= threshold;
            if is_work_j != is_work {
                break;
            }
            if j + 1 < normalized.samples.len()
                && normalized.samples[j + 1].t - normalized.samples[j].t > gap
            {
                break;
            }
            sum += primary[j];
            count += 1;
            j += 1;
        }
        let end = normalized.samples[j.saturating_sub(1)].t + 1.0;
        runs.push((
            is_work,
            start,
            end,
            if count == 0 { 0.0 } else { sum / count as f64 },
        ));
        i = j;
    }

    let mut work_blocks: Vec<(f64, f64, f64)> = Vec::new();
    let mut recovery_blocks: Vec<(f64, f64, f64)> = Vec::new();
    for (is_work, start, end, intensity) in &runs {
        let duration = *end - *start;
        if *is_work {
            if duration >= config.min_work_s {
                work_blocks.push((*start, *end, *intensity));
            }
        } else if duration >= config.min_recovery_s {
            recovery_blocks.push((*start, *end, *intensity));
        }
    }

    if work_blocks.is_empty() {
        return IntervalDetectionResult {
            session_kind: SessionKind::Other,
            work_segments: Vec::new(),
            recovery_segments: Vec::new(),
            confidence: None,
            reasons: vec!["no sustained work segments found".to_string()],
        };
    }

    let work_durations: Vec<f64> = work_blocks.iter().map(|b| b.1 - b.0).collect();
    let recovery_durations: Vec<f64> = recovery_blocks.iter().map(|b| b.1 - b.0).collect();
    let work_cv = coeff_of_variation(&work_durations);
    let recovery_cv = coeff_of_variation(&recovery_durations);

    let work_mean = mean(&work_blocks.iter().map(|b| b.2).collect::<Vec<_>>());
    let recovery_mean = mean(&recovery_blocks.iter().map(|b| b.2).collect::<Vec<_>>());
    let separation = if recovery_mean > 0.0 {
        work_mean / recovery_mean
    } else {
        f64::MAX
    };

    let is_structured = work_blocks.len() >= 2
        && recovery_blocks.len() >= work_blocks.len().saturating_sub(1)
        && work_cv <= config.work_cv_threshold
        && recovery_cv <= config.recovery_cv_threshold
        && separation >= config.intensity_separation;

    if is_structured {
        let work_segments = work_blocks
            .iter()
            .map(|b| DetectedSegment {
                phase: SegmentPhase::WorkRep,
                range: TimeRange {
                    start: b.0,
                    end: b.1,
                },
                mean_intensity: b.2,
            })
            .collect();
        let recovery_segments = recovery_blocks
            .iter()
            .map(|b| DetectedSegment {
                phase: SegmentPhase::Recovery,
                range: TimeRange {
                    start: b.0,
                    end: b.1,
                },
                mean_intensity: b.2,
            })
            .collect();

        let confidence = Some((1.0 - work_cv).clamp(0.0, 1.0));
        IntervalDetectionResult {
            session_kind: SessionKind::StructuredIntervals,
            work_segments,
            recovery_segments,
            confidence,
            reasons: vec![
                format!(
                    "{} regular work/recovery cycles detected",
                    work_blocks.len()
                ),
                format!("work-duration CV={work_cv:.2}, recovery-duration CV={recovery_cv:.2}"),
            ],
        }
    } else {
        // Irregular surges, ambiguous, or weak separation -> reported as
        // Fartlek/Other, never as a structured work-set result.
        let kind =
            if work_cv > config.work_cv_threshold || recovery_cv > config.recovery_cv_threshold {
                SessionKind::Fartlek
            } else {
                SessionKind::Other
            };
        let reason = if kind == SessionKind::Fartlek {
            "irregular surges detected; not a structured interval set"
        } else {
            "work present but regularity too low for structured intervals"
        };
        IntervalDetectionResult {
            session_kind: kind,
            work_segments: Vec::new(),
            recovery_segments: Vec::new(),
            confidence: Some(0.4),
            reasons: vec![reason.to_string()],
        }
    }
}

/// Map a [`SessionKind`] to its corpus annotation string.
pub fn session_kind_label(kind: SessionKind) -> &'static str {
    match kind {
        SessionKind::StructuredIntervals => "structured_interval",
        SessionKind::Fartlek => "fartlek",
        SessionKind::Other => "other",
        SessionKind::InsufficientData => "insufficient_data",
    }
}

/// Parse a [`SessionKind`] from its annotation string.
pub fn parse_session_kind(value: &str) -> Option<SessionKind> {
    SessionKind::from_str(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_raw(time_s: Vec<f64>, speed: Vec<f64>, hr: Vec<f64>, power: Vec<f64>) -> RawStream {
        RawStream {
            time_s,
            speed,
            heartrate: hr,
            power: Some(power),
            elevation: None,
        }
    }

    fn constant_n(n: usize, value: f64) -> Vec<f64> {
        (0..n).map(|_| value).collect()
    }

    #[test]
    fn gap_longer_than_tolerance_becomes_an_exclusion_not_a_recovery() {
        let config = NormalizationConfig::default();
        // 60s of data, then a 30s gap, then 60s more.
        let mut time_s: Vec<f64> = (0..60).map(|t| t as f64).collect();
        time_s.extend((90..150).map(|t| t as f64));
        let n = time_s.len();
        let speed = constant_n(n, 3.0);
        let hr = constant_n(n, 140.0);
        let power = constant_n(n, 150.0);
        let raw = build_raw(time_s, speed, hr, power);

        let normalized = normalize_streams(&raw, &config).unwrap();
        assert_eq!(normalized.exclusions.len(), 1);
    }

    #[test]
    fn regular_work_recovery_cycles_are_structured_intervals() {
        // 4 reps x 180s work @ speed 6.0, 120s recovery @ speed 2.5.
        let mut time_s = Vec::new();
        let mut speed = Vec::new();
        let mut hr = Vec::new();
        let mut power = Vec::new();
        let mut t = 0.0f64;
        for rep in 0..4 {
            for _ in 0..180 {
                time_s.push(t);
                speed.push(6.0);
                hr.push(175.0);
                power.push(300.0);
                t += 1.0;
            }
            if rep < 3 {
                for _ in 0..120 {
                    time_s.push(t);
                    speed.push(2.5);
                    hr.push(140.0);
                    power.push(120.0);
                    t += 1.0;
                }
            }
        }
        let raw = build_raw(time_s, speed, hr, power);
        let result = detect_intervals(&raw);
        assert_eq!(result.session_kind, SessionKind::StructuredIntervals);
        assert_eq!(result.work_segments.len(), 4);
    }

    #[test]
    fn irregular_surges_are_fartlek_not_structured_intervals() {
        // Irregular surge/easy alternation with inconsistent durations.
        let mut time_s = Vec::new();
        let mut speed = Vec::new();
        let mut hr = Vec::new();
        let mut power = Vec::new();
        let mut t = 0.0f64;
        let blocks: Vec<(f64, f64, f64, f64)> = vec![
            (20.0, 6.0, 178.0, 320.0),
            (250.0, 3.0, 145.0, 150.0),
            (90.0, 6.5, 182.0, 340.0),
            (50.0, 2.8, 140.0, 130.0),
            (40.0, 5.5, 172.0, 300.0),
        ];
        for (dur, spd, h, pw) in blocks {
            for _ in 0..dur as usize {
                time_s.push(t);
                speed.push(spd);
                hr.push(h);
                power.push(pw);
                t += 1.0;
            }
        }
        let raw = build_raw(time_s, speed, hr, power);
        let result = detect_intervals(&raw);
        assert_eq!(result.session_kind, SessionKind::Fartlek);
        assert!(result.work_segments.is_empty());
    }
}
