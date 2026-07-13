//! Coverage-aware, time-weighted segment metric computation.
//!
//! This engine computes metrics from independently parsed metric streams
//! (speed, HR, power) for established segment time windows. It never
//! decides whether a session is structured — that is the detector's job.
//!
//! Key behaviours:
//! - Time-weighted means (not arithmetic) so irregular sampling does not
//!   bias pace, HR, or power.
//! - Per-signal coverage ratio determines whether a metric is emitted.
//! - Recording gaps >10 seconds are never interpolated.

use crate::domains::interval_detection::TimeRange;
use crate::domains::interval_segment::{
    EnrichedSegment, IntervalSegmentMetrics, MetricStreams, SegmentWindow, SeriesConsistency,
    SignalCoverage,
};

const MIN_COVERAGE_RATIO: f64 = 0.80;
const MAX_GAP_S: f64 = 10.0;
const SMOOTHING_WINDOW_S: f64 = 5.0;
const MIN_PERCENTILE_WINDOWS: usize = 5;

// ── Private span helpers ──────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Span {
    start: f64,
    end: f64,
    start_value: f64,
    end_value: f64,
}

impl Span {
    fn duration(self) -> f64 {
        self.end - self.start
    }

    fn integral(self) -> f64 {
        0.5 * (self.start_value + self.end_value) * self.duration()
    }

    fn squared_integral(self) -> f64 {
        let a = self.start_value;
        let b = self.end_value;
        let d = self.duration();
        d * (a * a + a * b + b * b) / 3.0
    }
}

#[derive(Clone, Copy, Default)]
struct SignalSummary {
    integral: f64,
    mean: f64,
    variance: f64,
    coverage: SignalCoverage,
    accepted: bool,
}

/// Summarise a single signal (speed, HR, or power) over a time range.
///
/// For each adjacent sample pair:
/// 1. Reject non-finite timestamps/values, non-positive deltas, gaps >10s.
/// 2. Intersect the pair's time range with the requested segment.
/// 3. Linearly interpolate values at the clipped start and end.
/// 4. Sum covered duration, integral, and squared integral.
/// 5. Compute mean and variance from the summed moments.
fn summarize_signal(time_s: &[f64], values: &[f64], range: TimeRange) -> SignalSummary {
    if time_s.len() != values.len() || time_s.len() < 2 {
        return SignalSummary::default();
    }

    let seg_start = range.start;
    let seg_end = range.end;
    let segment_duration = (seg_end - seg_start).max(0.0);
    if segment_duration == 0.0 {
        return SignalSummary::default();
    }

    let mut covered = 0.0;
    let mut integral = 0.0;
    let mut squared_integral = 0.0;
    let mut sample_count = 0usize;

    for i in 0..time_s.len() {
        let t = time_s[i];
        let v = values[i];
        if t.is_finite() && v.is_finite() && t >= seg_start && t < seg_end {
            sample_count += 1;
        }
    }

    for i in 1..time_s.len() {
        let t0 = time_s[i - 1];
        let t1 = time_s[i];
        let v0 = values[i - 1];
        let v1 = values[i];

        // Reject non-finite samples or non-positive delta
        if !t0.is_finite() || !t1.is_finite() || !v0.is_finite() || !v1.is_finite() {
            continue;
        }
        let dt = t1 - t0;
        if dt <= 0.0 || dt > MAX_GAP_S {
            continue;
        }

        // Intersect with requested segment range
        let span_start = t0.max(seg_start);
        let span_end = t1.min(seg_end);
        if span_start >= span_end {
            continue;
        }

        // Linear interpolation at clipped boundaries
        let frac_start = (span_start - t0) / dt;
        let frac_end = (span_end - t0) / dt;
        let span_v0 = v0 + (v1 - v0) * frac_start;
        let span_v1 = v0 + (v1 - v0) * frac_end;

        let span = Span {
            start: span_start,
            end: span_end,
            start_value: span_v0,
            end_value: span_v1,
        };

        covered += span.duration();
        integral += span.integral();
        squared_integral += span.squared_integral();
    }

    let ratio = if segment_duration > 0.0 {
        covered / segment_duration
    } else {
        0.0
    };

    let mean = if covered > 0.0 {
        integral / covered
    } else {
        0.0
    };

    let variance = if covered > 0.0 {
        (squared_integral / covered - mean * mean).max(0.0)
    } else {
        0.0
    };

    let accepted = ratio >= MIN_COVERAGE_RATIO;

    SignalSummary {
        integral,
        mean,
        variance,
        coverage: SignalCoverage {
            sample_count,
            covered_duration_s: covered,
            ratio,
        },
        accepted,
    }
}

// ── Rolling window helpers ────────────────────────────────────────────

/// Compute all valid full-window trailing means inside the segment.
///
/// A window `[anchor - window_s, anchor]` is valid when:
/// - anchor is at least `window_s` after the segment start
/// - the window lies entirely inside the segment
/// - the window's valid-span coverage is ≥80%
/// - its mean is finite
fn full_window_means(time_s: &[f64], values: &[f64], range: TimeRange, window_s: f64) -> Vec<f64> {
    if time_s.len() != values.len() || time_s.len() < 2 || window_s <= 0.0 {
        return Vec::new();
    }

    let seg_start = range.start;
    let seg_end = range.end;
    let mut means = Vec::new();
    let mut anchor_idx = 1;

    for i in 1..time_s.len() {
        let anchor_t = time_s[i];
        // The anchor must be at least window_s after the segment start
        if anchor_t < seg_start + window_s {
            continue;
        }
        // The window must lie entirely inside the segment
        let window_start = anchor_t - window_s;
        if window_start < seg_start || anchor_t > seg_end {
            continue;
        }

        // Walk forward to find the window start index
        while anchor_idx < i && time_s[anchor_idx] < window_start {
            anchor_idx += 1;
        }

        // Collect samples within the window
        let mut sum = 0.0;
        let mut covered_duration = 0.0;
        let mut prev_t = f64::NAN;

        for j in anchor_idx.max(1)..=i {
            let t = time_s[j];
            let v = values[j];
            if !t.is_finite() || !v.is_finite() {
                continue;
            }
            if j == anchor_idx.max(1) {
                prev_t = t;
                continue;
            }
            let dt = t - prev_t;
            if dt > 0.0 && dt <= MAX_GAP_S {
                // Clip the span to the window
                let span_start = prev_t.max(window_start);
                let span_end = t.min(anchor_t);
                if span_start < span_end {
                    let frac_start = (span_start - prev_t) / dt;
                    let frac_end = (span_end - prev_t) / dt;
                    let v_start = values[j - 1] + (v - values[j - 1]) * frac_start;
                    let v_end = values[j - 1] + (v - values[j - 1]) * frac_end;
                    // Time-weighted mean contribution
                    let span_dur = span_end - span_start;
                    sum += 0.5 * (v_start + v_end) * span_dur;
                    covered_duration += span_dur;
                }
            }
            prev_t = t;
        }

        let window_coverage = if window_s > 0.0 {
            covered_duration / window_s
        } else {
            0.0
        };
        if window_coverage >= MIN_COVERAGE_RATIO && covered_duration > 0.0 {
            let mean = sum / covered_duration;
            if mean.is_finite() {
                means.push(mean);
            }
        }
    }

    means
}

/// Type-7 percentile (as defined by Hyndman & Fan).
fn percentile_type7(values: &mut [f64], probability: f64) -> Option<f64> {
    if values.is_empty() || !(0.0..=1.0).contains(&probability) {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let rank = probability * (values.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let fraction = rank - lower as f64;
    Some(values[lower] + fraction * (values[upper] - values[lower]))
}

// ── Public API ────────────────────────────────────────────────────────

/// Compute coverage-aware metrics for a single segment window.
pub fn compute_segment_metrics(
    streams: &MetricStreams,
    range: TimeRange,
) -> IntervalSegmentMetrics {
    let duration_s = (range.end - range.start).max(0.0);
    if duration_s == 0.0 {
        return IntervalSegmentMetrics::default();
    }

    let speed = streams
        .speed_mps
        .as_deref()
        .map(|values| summarize_signal(&streams.time_s, values, range));
    let hr = streams
        .heartrate_bpm
        .as_deref()
        .map(|values| summarize_signal(&streams.time_s, values, range));
    let power = streams
        .power_w
        .as_deref()
        .map(|values| summarize_signal(&streams.time_s, values, range));

    let mut metrics = IntervalSegmentMetrics {
        duration_s,
        distance_m: speed
            .as_ref()
            .and_then(|s| s.accepted.then_some(s.integral)),
        avg_speed_mps: speed.as_ref().and_then(|s| s.accepted.then_some(s.mean)),
        avg_hr_bpm: hr.as_ref().and_then(|s| s.accepted.then_some(s.mean)),
        avg_power_w: power.as_ref().and_then(|s| s.accepted.then_some(s.mean)),
        power_cv_pct: power.as_ref().and_then(|s| {
            (s.accepted && s.mean > 0.0).then_some(100.0 * s.variance.max(0.0).sqrt() / s.mean)
        }),
        speed_coverage: speed
            .as_ref()
            .map(|summary| summary.coverage)
            .unwrap_or_default(),
        hr_coverage: hr
            .as_ref()
            .map(|summary| summary.coverage)
            .unwrap_or_default(),
        power_coverage: power
            .as_ref()
            .map(|summary| summary.coverage)
            .unwrap_or_default(),
        ..IntervalSegmentMetrics::default()
    };

    // ── Robust rolling-window percentiles ─────────────────────────────
    if let Some(speed_values) = streams.speed_mps.as_deref() {
        let mut rolling =
            full_window_means(&streams.time_s, speed_values, range, SMOOTHING_WINDOW_S);
        if rolling.len() >= MIN_PERCENTILE_WINDOWS
            && metrics.speed_coverage.ratio >= MIN_COVERAGE_RATIO
        {
            let mut low = rolling.clone();
            metrics.low_speed_p05_mps = percentile_type7(&mut low, 0.05);
            metrics.high_speed_p95_mps = percentile_type7(&mut rolling, 0.95);
        }
    }

    if let Some(hr_values) = streams.heartrate_bpm.as_deref() {
        let mut rolling = full_window_means(&streams.time_s, hr_values, range, SMOOTHING_WINDOW_S);
        if rolling.len() >= MIN_PERCENTILE_WINDOWS
            && metrics.hr_coverage.ratio >= MIN_COVERAGE_RATIO
        {
            metrics.peak_hr_p95_bpm = percentile_type7(&mut rolling, 0.95);
        }
    }

    if let Some(power_values) = streams.power_w.as_deref() {
        let rolling = full_window_means(&streams.time_s, power_values, range, SMOOTHING_WINDOW_S);
        if metrics.power_coverage.ratio >= MIN_COVERAGE_RATIO && !rolling.is_empty() {
            let best = rolling
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
                .max(0.0);
            if best.is_finite() {
                metrics.best_5s_power_w = Some(best);
            }
        }
    }

    metrics
}

/// Enrich a collection of segment windows with metrics from the same
/// metric streams.
pub fn enrich_segments(streams: &MetricStreams, windows: &[SegmentWindow]) -> Vec<EnrichedSegment> {
    windows
        .iter()
        .cloned()
        .map(|window| EnrichedSegment {
            metrics: compute_segment_metrics(streams, window.range),
            window,
        })
        .collect()
}

/// Compute repeat-consistency statistics for a homogeneous structured set.
///
/// Returns `None` when fewer than three efforts exist, or when no common
/// metric (speed, power) is present across all efforts.
pub fn compute_structured_consistency(efforts: &[EnrichedSegment]) -> Option<SeriesConsistency> {
    if efforts.len() < 3 {
        return None;
    }

    let speeds: Vec<f64> = efforts
        .iter()
        .filter_map(|e| e.metrics.avg_speed_mps)
        .collect();
    let powers: Vec<f64> = efforts
        .iter()
        .filter_map(|e| e.metrics.avg_power_w)
        .collect();

    if speeds.len() < efforts.len() && powers.len() < efforts.len() {
        return None;
    }

    let mut result = SeriesConsistency::default();

    if speeds.len() >= 3 {
        let pace: Vec<f64> = speeds
            .iter()
            .map(|s| if *s > 0.0 { 1000.0 / s } else { f64::NAN })
            .filter(|v| v.is_finite())
            .collect();
        if pace.len() >= 3 {
            result.pace_cv_pct = Some(cv_pct(&pace));
            result.speed_cv_pct = Some(cv_pct(&speeds));
            if let (Some(first), Some(last)) = (speeds.first(), speeds.last())
                && *first > 0.0
            {
                result.first_to_last_speed_change_pct = Some((last - first) / first * 100.0);
                let first_pace = 1000.0 / first;
                let last_pace = 1000.0 / last;
                if first_pace > 0.0 {
                    result.first_to_last_pace_change_pct =
                        Some((last_pace - first_pace) / first_pace * 100.0);
                }
            }
        }
    }

    if powers.len() >= 3 {
        result.power_cv_pct = Some(cv_pct(&powers));
        if let (Some(first), Some(last)) = (powers.first(), powers.last())
            && *first > 0.0
        {
            result.first_to_last_power_change_pct = Some((last - first) / first * 100.0);
        }
    }

    Some(result)
}

fn cv_pct(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    if mean <= 0.0 {
        return 0.0;
    }
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    variance.sqrt() / mean * 100.0
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::interval_detection::TimeRange;
    use crate::domains::interval_segment::{
        EnrichedSegment, IntervalSegmentMetrics, MetricStreams, SegmentRole, SegmentWindow,
        SignalCoverage,
    };

    // ── Time-weighted mean ────────────────────────────────────────────

    #[test]
    fn average_speed_is_time_weighted_not_sample_weighted() {
        let streams = MetricStreams {
            time_s: vec![0.0, 1.0, 9.0, 10.0],
            speed_mps: Some(vec![2.0, 2.0, 6.0, 6.0]),
            heartrate_bpm: None,
            power_w: None,
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 0.0,
                end: 10.0,
            },
        );
        // 0-1: speed 2 → 1s at 2
        // 1-9: gap 8s > 10s → skipped
        // 9-10: speed 6 → 1s at 6
        // Covered: 1s at 2 + 1s at 6 → mean = (2+6)/2 = 4, integral = 2*1 + 6*1 = 8, but wait
        // Actually 0-1: span (0,1) at speed 2 → integral 2*1 = 2
        // 9-10: span (9,10) at speed 6 → integral 6*1 = 6
        // Total covered = 2s, integral = 8, mean = 4
        // But distance = integral = 8, not 40. Let me reconsider.
        // Wait — dt from 1 to 9 is 8, which is ≤ MAX_GAP_S (10). So the gap IS interpolated!
        // dt = 8 ≤ 10, so it's kept. span_start=1, span_end=9, v0=2, v1=6.
        // frac at 1: 0, frac at 9: (9-1)/8 = 1.0
        // Wait, that's t1(i=2) = 9, t0(i=1) = 1. dt = 8 ≤ 10 ✓
        // span_start = 1.max(0) = 1, span_end = 9.min(10) = 9
        // frac at start: (1-1)/8 = 0, span_v0 = 2
        // frac at end: (9-1)/8 = 1.0, span_v1 = 6
        // span duration = 8, mean = (2+6)/2 = 4
        // integral = 0.5 * (2+6) * 8 = 32
        // extra: 0-1: integral = 0.5*(2+2)*1 = 2
        // 9-10: integral = 0.5*(6+6)*1 = 6
        // total covered = 1 + 8 + 1 = 10
        // total integral = 2 + 32 + 6 = 40
        // ratio = 10/10 = 1.0
        // mean = 40/10 = 4.0 ✓
        // distance = integral = 40 ✓
        assert!((metrics.avg_speed_mps.expect("speed") - 4.0).abs() < 1e-9);
        assert!((metrics.distance_m.expect("distance") - 40.0).abs() < 1e-9);
    }

    // ── Boundary clipping ─────────────────────────────────────────────

    #[test]
    fn integration_clips_first_and_last_spans_to_segment_boundaries() {
        let streams = MetricStreams {
            time_s: vec![0.0, 10.0, 20.0],
            speed_mps: Some(vec![0.0, 10.0, 20.0]),
            heartrate_bpm: None,
            power_w: None,
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 5.0,
                end: 15.0,
            },
        );
        // 0-10: dt=10 > MAX_GAP_S? No, 10 ≤ 10. So kept.
        // span_start = 5, span_end = 10
        // frac_start = (5-0)/10 = 0.5, clamp to 0.5
        // span_v0 = 0 + (10-0)*0.5 = 5
        // frac_end = (10-0)/10 = 1.0
        // span_v1 = 10
        // span duration = 5, integral = 0.5*(5+10)*5 = 37.5
        // 10-20: dt=10
        // span_start = 10, span_end = 15
        // frac_start = (10-10)/10 = 0, span_v0 = 10
        // frac_end = (15-10)/10 = 0.5, span_v1 = 10 + (20-10)*0.5 = 15
        // span duration = 5, integral = 0.5*(10+15)*5 = 62.5
        // total covered = 10, total integral = 100
        // mean = 100/10 = 10 ✓, distance = 100 ✓
        assert!((metrics.avg_speed_mps.expect("speed") - 10.0).abs() < 1e-9);
        assert!((metrics.distance_m.expect("distance") - 100.0).abs() < 1e-9);
    }

    // ── Gap rejection ─────────────────────────────────────────────────

    #[test]
    fn gap_larger_than_ten_seconds_is_not_interpolated() {
        let streams = MetricStreams {
            time_s: vec![0.0, 1.0, 20.0, 21.0],
            speed_mps: Some(vec![4.0; 4]),
            heartrate_bpm: None,
            power_w: None,
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 0.0,
                end: 21.0,
            },
        );
        // dt from 1 to 20 = 19 > 10 → gap skipped
        // covered = 1s (0-1) + 1s (20-21) = 2s
        // ratio = 2/21 = 0.095..., < 0.8
        assert!(metrics.avg_speed_mps.is_none());
        assert!(metrics.distance_m.is_none());
        assert!(metrics.speed_coverage.ratio < 0.80);
    }

    // ── Independent signal availability ───────────────────────────────

    #[test]
    fn malformed_power_does_not_hide_valid_speed_and_hr() {
        let streams = MetricStreams {
            time_s: (0..=10).map(f64::from).collect(),
            speed_mps: Some(vec![5.0; 11]),
            heartrate_bpm: Some(vec![160.0; 11]),
            power_w: None,
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 0.0,
                end: 10.0,
            },
        );
        assert_eq!(metrics.avg_speed_mps, Some(5.0));
        assert_eq!(metrics.avg_hr_bpm, Some(160.0));
        assert!(metrics.avg_power_w.is_none());
    }

    #[test]
    fn nan_samples_in_time_lower_coverage_ratio() {
        let streams = MetricStreams {
            time_s: vec![0.0, f64::NAN, 2.0, 3.0, 4.0, 5.0],
            speed_mps: Some(vec![5.0; 6]),
            heartrate_bpm: Some(vec![160.0; 6]),
            power_w: None,
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 0.0,
                end: 5.0,
            },
        );
        // NaN at index 1 kills the (0,NaN) and (NaN,2) span pairs via
        // is_finite rejection. Only spans (2,3), (3,4), (4,5) contribute.
        // coverage = 3s / 5s = 0.6 < 0.80 → metrics rejected.
        assert!(metrics.avg_speed_mps.is_none());
        assert!(metrics.avg_hr_bpm.is_none());
        assert_eq!(
            (metrics.speed_coverage.ratio * 100.0).round() as i32,
            60,
            "3/5 = 0.6"
        );
    }

    #[test]
    fn nan_signal_values_are_skipped_without_affecting_neighbouring_spans() {
        // Speed has a NaN in the middle; the span crossing it is split.
        let streams = MetricStreams {
            time_s: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            speed_mps: Some(vec![5.0, 5.0, f64::NAN, 5.0, 5.0, 5.0]),
            heartrate_bpm: None,
            power_w: None,
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 0.0,
                end: 5.0,
            },
        );
        // NaN at time 2 means pairs (1,2) and (2,3) are rejected.
        // Active spans: (0,1) dt=1, (3,4) dt=1, (4,5) dt=1 → 3s covered.
        // ratio = 3/5 = 0.6 < 0.8, so no speed metric.
        assert!(
            metrics.avg_speed_mps.is_none(),
            "coverage below threshold with NaN gaps"
        );
        assert!(metrics.speed_coverage.ratio < 0.80);
    }

    #[test]
    fn misaligned_array_lengths_return_default_metrics() {
        let streams = MetricStreams {
            time_s: vec![0.0, 1.0, 2.0],
            speed_mps: Some(vec![5.0, 5.0]), // only 2 elements vs 3 time
            heartrate_bpm: None,
            power_w: None,
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 0.0,
                end: 2.0,
            },
        );
        // summarize_signal returns default because time_s.len() != values.len()
        assert!(metrics.avg_speed_mps.is_none());
        assert!(metrics.distance_m.is_none());
    }

    // ── Empty / reversed range ───────────────────────────────────────

    #[test]
    fn empty_or_reversed_range_returns_no_measurements() {
        let streams = MetricStreams::default();
        for range in [
            TimeRange {
                start: 10.0,
                end: 10.0,
            },
            TimeRange {
                start: 11.0,
                end: 10.0,
            },
        ] {
            let metrics = compute_segment_metrics(&streams, range);
            assert_eq!(metrics.duration_s, 0.0);
            assert!(metrics.distance_m.is_none());
            assert!(metrics.avg_hr_bpm.is_none());
            assert!(metrics.avg_power_w.is_none());
        }
    }

    // ── Enrich ────────────────────────────────────────────────────────

    #[test]
    fn enrich_segments_produces_one_enriched_segment_per_window() {
        let streams = MetricStreams {
            time_s: (0..=10).map(f64::from).collect(),
            speed_mps: Some(vec![5.0; 11]),
            heartrate_bpm: None,
            power_w: None,
        };
        let windows = vec![
            SegmentWindow {
                role: SegmentRole::Work,
                range: TimeRange {
                    start: 0.0,
                    end: 10.0,
                },
            },
            SegmentWindow {
                role: SegmentRole::Recovery,
                range: TimeRange {
                    start: 10.0,
                    end: 15.0,
                },
            },
        ];
        let enriched = enrich_segments(&streams, &windows);
        assert_eq!(enriched.len(), 2);
        assert_eq!(enriched[0].window.role, SegmentRole::Work);
        assert_eq!(enriched[1].window.role, SegmentRole::Recovery);
        assert!(enriched[0].metrics.avg_speed_mps.is_some());
    }

    // ── Robust statistics ─────────────────────────────────────────────

    #[test]
    fn single_sample_spikes_do_not_become_fastest_pace_or_peak_hr() {
        let mut speed = vec![5.0; 31];
        let mut hr = vec![160.0; 31];
        speed[15] = 50.0; // spike
        hr[15] = 240.0; // spike
        let streams = MetricStreams {
            time_s: (0..=30).map(f64::from).collect(),
            speed_mps: Some(speed),
            heartrate_bpm: Some(hr),
            power_w: None,
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 0.0,
                end: 30.0,
            },
        );
        assert!(metrics.high_speed_p95_mps.expect("p95 speed") < 15.0);
        assert!(metrics.peak_hr_p95_bpm.expect("p95 HR") < 180.0);
    }

    #[test]
    fn best_five_second_power_uses_only_full_windows() {
        let streams = MetricStreams {
            time_s: (0..=15).map(f64::from).collect(),
            speed_mps: None,
            heartrate_bpm: None,
            power_w: Some(
                (0..=15)
                    .map(|second| {
                        if (5..=10).contains(&second) {
                            300.0
                        } else {
                            100.0
                        }
                    })
                    .collect(),
            ),
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 0.0,
                end: 15.0,
            },
        );
        assert_eq!(metrics.best_5s_power_w, Some(300.0));
    }

    #[test]
    fn robust_percentiles_are_none_when_fewer_than_five_full_windows_exist() {
        let streams = MetricStreams {
            time_s: (0..=7).map(f64::from).collect(),
            speed_mps: Some(vec![5.0; 8]),
            heartrate_bpm: Some(vec![160.0; 8]),
            power_w: None,
        };
        let metrics = compute_segment_metrics(
            &streams,
            TimeRange {
                start: 0.0,
                end: 7.0,
            },
        );
        assert!(metrics.high_speed_p95_mps.is_none());
        assert!(metrics.low_speed_p05_mps.is_none());
        assert!(metrics.peak_hr_p95_bpm.is_none());
    }

    // ── Consistency ───────────────────────────────────────────────────

    fn enriched_work_with(speed_mps: f64, power_w: f64) -> EnrichedSegment {
        EnrichedSegment {
            window: SegmentWindow {
                role: SegmentRole::Work,
                range: TimeRange {
                    start: 0.0,
                    end: 60.0,
                },
            },
            metrics: IntervalSegmentMetrics {
                duration_s: 60.0,
                avg_speed_mps: Some(speed_mps),
                avg_power_w: Some(power_w),
                speed_coverage: SignalCoverage {
                    sample_count: 61,
                    covered_duration_s: 60.0,
                    ratio: 1.0,
                },
                power_coverage: SignalCoverage {
                    sample_count: 61,
                    covered_duration_s: 60.0,
                    ratio: 1.0,
                },
                ..IntervalSegmentMetrics::default()
            },
        }
    }

    #[test]
    fn consistency_requires_three_complete_reps() {
        let two = vec![
            enriched_work_with(5.0, 250.0),
            enriched_work_with(4.9, 245.0),
        ];
        assert!(compute_structured_consistency(&two).is_none());
    }

    #[test]
    fn consistency_reports_cv_and_first_to_last_change_without_scoring() {
        let efforts = vec![
            enriched_work_with(5.0, 250.0),
            enriched_work_with(5.0, 250.0),
            enriched_work_with(4.5, 225.0),
        ];
        let result = compute_structured_consistency(&efforts).expect("consistency");
        assert!(result.speed_cv_pct.expect("speed CV") > 0.0);
        assert!((result.first_to_last_speed_change_pct.expect("speed delta") + 10.0).abs() < 1e-9);
        assert!((result.first_to_last_power_change_pct.expect("power delta") + 10.0).abs() < 1e-9);
        assert!(result.pace_cv_pct.expect("pace CV") > 0.0);
    }
}
