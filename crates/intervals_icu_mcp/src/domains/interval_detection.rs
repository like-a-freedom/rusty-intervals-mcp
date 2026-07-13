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

/// A detected temporal segment.
#[derive(Debug, Clone)]
pub struct DetectedSegment {
    pub range: TimeRange,
    pub mean_intensity: f64,
}

/// A series of detected effort/recovery segments (e.g. from fartlek).
#[derive(Debug, Clone)]
pub struct DetectedSegmentSeries {
    pub effort_segments: Vec<DetectedSegment>,
    pub recovery_segments: Vec<DetectedSegment>,
}

/// Result of running the detector on a single session.
#[derive(Debug, Clone)]
pub struct IntervalDetectionResult {
    pub session_kind: SessionKind,
    pub work_segments: Vec<DetectedSegment>,
    pub recovery_segments: Vec<DetectedSegment>,
    pub fartlek_series: Option<DetectedSegmentSeries>,
    pub confidence: Option<f64>,
    pub reasons: Vec<String>,
}

/// Raw, aligned stream arrays as supplied by a source (Intervals.icu or a
/// derived fixture). Speed and heart rate are required; power is optional.
#[derive(Debug, Clone)]
pub struct RawStream {
    pub time_s: Vec<f64>,
    pub speed: Vec<f64>,
    pub heartrate: Vec<f64>,
    pub power: Option<Vec<f64>>,
}

/// A single resampled, validated sample used for detection.
#[derive(Debug, Clone, Copy)]
pub struct NormalizedSample {
    pub t: f64,
    pub speed: f64,
}

/// Normalized, validation-checked stream. Recording gaps remain explicit time
/// discontinuities and are never interpolated into recovery.
#[derive(Debug, Clone)]
pub struct NormalizedStream {
    pub samples: Vec<NormalizedSample>,
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

fn detected_segments(blocks: &[(f64, f64, f64)]) -> Vec<DetectedSegment> {
    blocks
        .iter()
        .map(|(start, end, mean_intensity)| DetectedSegment {
            range: TimeRange {
                start: *start,
                end: *end,
            },
            mean_intensity: *mean_intensity,
        })
        .collect()
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
/// Returns an error when required aligned signals are missing, malformed, or
/// have non-monotonic timestamps. The detector preserves recording gaps as
/// discontinuities rather than interpolating them into recovery.
pub fn normalize_streams(raw: &RawStream) -> Result<NormalizedStream, String> {
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
        if !raw.time_s[i].is_finite()
            || !raw.speed[i].is_finite()
            || !raw.heartrate[i].is_finite()
            || raw
                .power
                .as_ref()
                .is_some_and(|power| !power[i].is_finite())
        {
            return Err("stream contains non-finite samples".to_string());
        }
        if i > 0 && raw.time_s[i] <= raw.time_s[i - 1] {
            return Err("stream timestamps must be strictly increasing".to_string());
        }
        samples.push(NormalizedSample {
            t: raw.time_s[i],
            speed: raw.speed[i],
        });
    }

    Ok(NormalizedStream { samples })
}

fn signal_spread(values: &[f64]) -> f64 {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (max - min).max(0.0)
}

fn nominal_sample_interval(samples: &[NormalizedSample], gap_tolerance_s: f64) -> f64 {
    let mut intervals = samples
        .windows(2)
        .map(|pair| pair[1].t - pair[0].t)
        .filter(|delta| *delta > 0.0 && *delta <= gap_tolerance_s)
        .collect::<Vec<_>>();
    if intervals.is_empty() {
        return 1.0;
    }
    intervals.sort_by(|a, b| a.total_cmp(b));
    intervals[intervals.len() / 2]
}

/// Detect the session type and work/recovery structure from a raw stream.
///
/// Internally normalizes, selects a primary change signal (velocity, falling
/// back to power), threshold splits work vs recovery, merges sub-threshold
/// candidates, scores repetition regularity, and classifies.
pub fn detect_intervals(raw: &RawStream) -> IntervalDetectionResult {
    let config = NormalizationConfig::default();
    let normalized = match normalize_streams(raw) {
        Ok(n) => n,
        Err(reason) => {
            return IntervalDetectionResult {
                session_kind: SessionKind::InsufficientData,
                work_segments: Vec::new(),
                recovery_segments: Vec::new(),
                fartlek_series: None,
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
            fartlek_series: None,
            confidence: None,
            reasons: vec!["stream too short for detection".to_string()],
        };
    }

    // Choose one primary signal for the whole session. Mixing speed and power
    // sample by sample would combine incomparable units around stops.
    let speed: Vec<f64> = normalized
        .samples
        .iter()
        .map(|sample| sample.speed)
        .collect();
    let primary = if signal_spread(&speed) > f64::EPSILON {
        speed
    } else if let Some(power) = raw
        .power
        .as_ref()
        .filter(|power| signal_spread(power) > 0.0)
    {
        power.clone()
    } else {
        return IntervalDetectionResult {
            session_kind: SessionKind::Other,
            work_segments: Vec::new(),
            recovery_segments: Vec::new(),
            fartlek_series: None,
            confidence: None,
            reasons: vec!["no intensity variation to detect intervals".to_string()],
        };
    };

    let max = primary.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low = primary.iter().copied().fold(f64::INFINITY, f64::min);
    // Baseline is the low/easy intensity (recovery). Using the minimum rather
    // than the median avoids being skewed when work samples outnumber
    // recovery samples. Threshold sits halfway between easy and peak effort.
    let spread = (max - low).max(0.0);
    if spread <= f64::EPSILON {
        return IntervalDetectionResult {
            session_kind: SessionKind::Other,
            work_segments: Vec::new(),
            recovery_segments: Vec::new(),
            fartlek_series: None,
            confidence: None,
            reasons: vec!["no intensity variation to detect intervals".to_string()],
        };
    }
    let threshold = low + 0.5 * spread;

    // Label each sample and break runs at recording gaps.
    let gap = config.gap_tolerance_s;
    let sample_interval = nominal_sample_interval(&normalized.samples, gap);
    let mut runs: Vec<(bool, f64, f64, f64)> = Vec::new(); // (is_work, start, end, mean_intensity)
    let mut i = 0;
    while i < normalized.samples.len() {
        let is_work = primary[i] >= threshold;
        let start = normalized.samples[i].t;
        let mut j = i;
        let mut sum = 0.0;
        let mut count = 0usize;
        while j < normalized.samples.len() {
            if j > i && normalized.samples[j].t - normalized.samples[j - 1].t > gap {
                break;
            }
            let is_work_j = primary[j] >= threshold;
            if is_work_j != is_work {
                break;
            }
            sum += primary[j];
            count += 1;
            j += 1;
        }
        let end = if j < normalized.samples.len()
            && normalized.samples[j].t - normalized.samples[j - 1].t <= gap
        {
            normalized.samples[j].t
        } else {
            normalized.samples[j - 1].t + sample_interval
        };
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
            fartlek_series: None,
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
        let work_segments = detected_segments(&work_blocks);
        let recovery_segments = detected_segments(&recovery_blocks);

        let confidence = Some((1.0 - work_cv).clamp(0.0, 1.0));
        IntervalDetectionResult {
            session_kind: SessionKind::StructuredIntervals,
            work_segments,
            recovery_segments,
            fartlek_series: None,
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
        let fartlek_series = (kind == SessionKind::Fartlek).then(|| DetectedSegmentSeries {
            effort_segments: detected_segments(&work_blocks),
            recovery_segments: detected_segments(&recovery_blocks),
        });
        IntervalDetectionResult {
            session_kind: kind,
            work_segments: Vec::new(),
            recovery_segments: Vec::new(),
            fartlek_series,
            confidence: Some(0.4),
            reasons: vec![reason.to_string()],
        }
    }
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
        }
    }

    fn constant_n(n: usize, value: f64) -> Vec<f64> {
        (0..n).map(|_| value).collect()
    }

    #[test]
    fn normalization_preserves_recording_gap_without_interpolation() {
        // 60s of data, then a 30s gap, then 60s more.
        let mut time_s: Vec<f64> = (0..60).map(|t| t as f64).collect();
        time_s.extend((90..150).map(|t| t as f64));
        let n = time_s.len();
        let speed = constant_n(n, 3.0);
        let hr = constant_n(n, 140.0);
        let power = constant_n(n, 150.0);
        let raw = build_raw(time_s, speed, hr, power);

        let normalized = normalize_streams(&raw).unwrap();
        assert_eq!(normalized.samples.len(), 120);
        assert_eq!(normalized.samples[59].t, 59.0);
        assert_eq!(normalized.samples[60].t, 90.0);
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

        let work_ranges = result
            .work_segments
            .iter()
            .map(|segment| (segment.range.start, segment.range.end))
            .collect::<Vec<_>>();
        let recovery_ranges = result
            .recovery_segments
            .iter()
            .map(|segment| (segment.range.start, segment.range.end))
            .collect::<Vec<_>>();

        assert_eq!(
            work_ranges,
            vec![
                (0.0, 180.0),
                (300.0, 480.0),
                (600.0, 780.0),
                (900.0, 1080.0)
            ]
        );
        assert_eq!(
            recovery_ranges,
            vec![(180.0, 300.0), (480.0, 600.0), (780.0, 900.0)]
        );
        assert_eq!(
            result.reasons,
            vec![
                "4 regular work/recovery cycles detected",
                "work-duration CV=0.00, recovery-duration CV=0.00",
            ]
        );
    }

    fn build_irregular_fartlek_raw() -> RawStream {
        let blocks: Vec<(f64, f64, f64, f64)> = vec![
            (20.0, 6.0, 178.0, 320.0),
            (250.0, 3.0, 145.0, 150.0),
            (90.0, 6.5, 182.0, 340.0),
            (50.0, 2.8, 140.0, 130.0),
            (40.0, 5.5, 172.0, 300.0),
        ];
        let mut time_s = Vec::new();
        let mut speed = Vec::new();
        let mut hr = Vec::new();
        let mut power = Vec::new();
        let mut t = 0.0f64;
        for (dur, spd, h, pw) in blocks {
            for _ in 0..dur as usize {
                time_s.push(t);
                speed.push(spd);
                hr.push(h);
                power.push(pw);
                t += 1.0;
            }
        }
        build_raw(time_s, speed, hr, power)
    }

    #[test]
    fn irregular_surges_are_fartlek_not_structured_intervals() {
        let raw = build_irregular_fartlek_raw();
        let result = detect_intervals(&raw);
        assert_eq!(result.session_kind, SessionKind::Fartlek);
        assert!(result.work_segments.is_empty());
        assert!(result.recovery_segments.is_empty());
        assert_eq!(
            result.reasons,
            vec!["irregular surges detected; not a structured interval set"]
        );
    }

    #[test]
    fn fartlek_retains_candidate_surges_without_claiming_structured_work() {
        let raw = build_irregular_fartlek_raw();
        let result = detect_intervals(&raw);

        assert_eq!(result.session_kind, SessionKind::Fartlek);
        assert!(result.work_segments.is_empty());
        assert!(result.recovery_segments.is_empty());

        let series = result
            .fartlek_series
            .as_ref()
            .expect("fartlek candidates must be retained");
        let surge_ranges = series
            .effort_segments
            .iter()
            .map(|segment| (segment.range.start, segment.range.end))
            .collect::<Vec<_>>();
        assert_eq!(
            surge_ranges,
            vec![(0.0, 20.0), (270.0, 360.0), (410.0, 450.0)]
        );
        assert_eq!(series.recovery_segments.len(), 2);
    }

    #[test]
    fn recording_gap_does_not_stall_or_join_adjacent_runs() {
        let mut time_s: Vec<f64> = (0..30).map(|time| time as f64).collect();
        time_s.extend((60..90).map(|time| time as f64));
        let raw = build_raw(
            time_s,
            constant_n(60, 3.0),
            constant_n(60, 140.0),
            constant_n(60, 180.0),
        );

        let result = detect_intervals(&raw);
        assert_eq!(result.session_kind, SessionKind::Other);
        assert!(result.work_segments.is_empty());
    }

    #[test]
    fn speed_and_power_are_not_mixed_at_stops() {
        let mut time_s = Vec::new();
        let mut speed = Vec::new();
        let mut hr = Vec::new();
        let mut power = Vec::new();
        for rep in 0..3 {
            for _ in 0..60 {
                time_s.push(time_s.len() as f64);
                speed.push(6.0);
                hr.push(175.0);
                power.push(300.0);
            }
            if rep < 2 {
                for _ in 0..30 {
                    time_s.push(time_s.len() as f64);
                    speed.push(0.0);
                    hr.push(140.0);
                    power.push(120.0);
                }
            }
        }

        let result = detect_intervals(&build_raw(time_s, speed, hr, power));
        assert_eq!(result.session_kind, SessionKind::StructuredIntervals);
        assert_eq!(result.work_segments.len(), 3);
    }
}
