//! Pure, deterministic calculation of evidence-gated cycling endurance
//! metrics. This engine never touches I/O — it accepts parsed
//! `MetricStreams` and emits raw, traceable observations and eligibility
//! statuses. The renderer is responsible for wording.
//!
//! # Protocol constants
//!
//! All acquired constants are product-level. They are not physiological
//! good/bad thresholds; they define what counts as a "control window"
//! or a "matched prolonged ride" for this MCP feature.
//!
//! - Control window: 600 s, stride 60 s.
//! - Eligibility: mean power in `[0.55, 0.80] * eFTP`, power CV ≤ 5 %, and
//!   HR + power coverage ≥ 90 %.
//! - Longitudinal cohort split: recent 0–14 days, reference 15–90 days
//!   before `as_of`. Both must contain ≥ 2 unique activity ids, and all
//!   accepted windows must match anchor power within 5 %.
//! - Prolonged response: one ride with an early window ending 20–60 min
//!   and a late window ending ≥ 120 min, both matching the same power
//!   within 5 %. The pair with the largest HR delta wins tiebreaks.
//!
//! # Algorithm shape
//!
//! Per-session control windows are computed with a two-pointer sliding
//! iterator — O(n) over the time array — instead of resampling the
//! window for every stride. The pair matching for the prolonged ride
//! compares all eligible early/late pairs and selects the most informative
//! one (largest HR delta at the closest power match).

use chrono::NaiveDate;

use crate::domains::endurance_evidence::{
    EnduranceEvidenceMetrics, EnduranceEvidenceStatus, ProlongedRideResponseMetrics,
    SubmaximalHrPowerMetrics,
};
use crate::domains::interval_detection::TimeRange;
use crate::domains::interval_segment::MetricStreams;

// ── Constants ─────────────────────────────────────────────────────────

/// One control window is exactly 600 seconds.
pub const CONTROL_WINDOW_S: f64 = 600.0;

/// Snap control-window statistics every 60 seconds.
pub const CONTROL_STRIDE_S: f64 = 60.0;

/// Minimum time coverage for both HR and power inside a control window.
pub const CONTROL_MIN_COVERAGE: f64 = 0.90;

/// Power CV must not exceed 5% within a control window.
pub const CONTROL_MAX_POWER_CV_PCT: f64 = 5.0;

/// Minimum sustained power as a fraction of eFTP to qualify as a steady
/// submaximal window.
pub const CONTROL_MIN_EFTP_FRACTION: f64 = 0.55;

/// Maximum sustained power as a fraction of eFTP.
pub const CONTROL_MAX_EFTP_FRACTION: f64 = 0.80;

/// Fractional tolerance for power match between paired observations.
pub const POWER_MATCH_TOLERANCE: f64 = 0.05;

/// Age of "recent" cohort in days.
pub const RECENT_DAYS: i64 = 14;

/// Minimum age of "reference" cohort in days.
pub const REFERENCE_MIN_AGE_DAYS: i64 = 15;

/// Maximum age of "reference" cohort in days.
pub const REFERENCE_MAX_AGE_DAYS: i64 = 90;

/// Each longitudinal cohort must contain at least this many accepted
/// activity windows.
pub const MIN_COHORT_WINDOWS: usize = 2;

/// Earliest acceptable end of an early control window, in seconds.
pub const EARLY_END_MIN_S: f64 = 20.0 * 60.0;

/// Latest acceptable end of an early control window, in seconds.
pub const EARLY_END_MAX_S: f64 = 60.0 * 60.0;

/// Earliest acceptable end of a late control window, in seconds.
pub const LATE_END_MIN_S: f64 = 120.0 * 60.0;

// ── Input ─────────────────────────────────────────────────────────────

/// One parsed cycling session fed into the engine.
#[derive(Clone, Debug)]
pub struct CyclingSessionInput {
    pub activity_id: String,
    pub date: NaiveDate,
    pub streams: MetricStreams,
}

// ── Internal data ─────────────────────────────────────────────────────

/// One eligible control window inside a session with all derived metrics.
#[derive(Clone, Debug, PartialEq)]
struct ControlWindow {
    range: TimeRange,
    avg_power_w: f64,
    avg_hr_bpm: f64,
    efficiency_w_per_bpm: f64,
    power_cv_pct: f64,
    activity_id: String,
}

// ── Public entry point ────────────────────────────────────────────────

/// Compute both endurance-evidence reports from a set of sessions.
///
/// `as_of` is the end-date for cohort assignment. `eftp_w` is the
/// athlete's cycling eFTP (used to define the submaximal band), or
/// `None` if the athlete profile lacks an eFTP — in which case both
/// reports degrade to deterministic `MissingEftp` outcomes.
pub fn compute_endurance_evidence(
    sessions: &[CyclingSessionInput],
    as_of: NaiveDate,
    eftp_w: Option<f64>,
) -> EnduranceEvidenceMetrics {
    if eftp_w.is_none_or(|w| w <= 0.0) {
        return EnduranceEvidenceMetrics {
            submaximal: SubmaximalHrPowerMetrics {
                status: EnduranceEvidenceStatus::MissingEftp,
                ..SubmaximalHrPowerMetrics::default()
            },
            prolonged_response: ProlongedRideResponseMetrics::unavailable(
                EnduranceEvidenceStatus::MissingEftp,
            ),
        };
    }
    let eftp_w = eftp_w.unwrap();

    EnduranceEvidenceMetrics {
        submaximal: submaximal_report(sessions, as_of, eftp_w),
        prolonged_response: prolonged_report(sessions, eftp_w),
    }
}

// ── Submaximal longitudinal report ────────────────────────────────────

fn status_only_submaximal(
    status: EnduranceEvidenceStatus,
    considered: usize,
    accepted: usize,
    source_ids: Vec<String>,
) -> SubmaximalHrPowerMetrics {
    SubmaximalHrPowerMetrics {
        status,
        activities_considered: considered,
        activities_accepted: accepted,
        source_activity_ids: source_ids,
        ..SubmaximalHrPowerMetrics::default()
    }
}

fn submaximal_report(
    sessions: &[CyclingSessionInput],
    as_of: NaiveDate,
    eftp_w: f64,
) -> SubmaximalHrPowerMetrics {
    let considered = sessions.len();

    let mut sorted_sessions: Vec<&CyclingSessionInput> = sessions.iter().collect();
    sorted_sessions.sort_by(|left, right| {
        right
            .date
            .cmp(&left.date)
            .then_with(|| right.activity_id.cmp(&left.activity_id))
    });

    let mut recent: Vec<ControlWindow> = Vec::new();
    let mut reference: Vec<ControlWindow> = Vec::new();

    for session in sorted_sessions {
        let age_days = (as_of - session.date).num_days();
        let windows = collect_session_windows(session, eftp_w);
        let Some(best) = pick_best_window(windows) else {
            continue;
        };
        if (0..=RECENT_DAYS).contains(&age_days) {
            recent.push(best);
        } else if (REFERENCE_MIN_AGE_DAYS..=REFERENCE_MAX_AGE_DAYS).contains(&age_days) {
            reference.push(best);
        }
    }

    let accepted_total = unique_activity_count(&recent) + unique_activity_count(&reference);
    let mut all_ids: Vec<String> = recent
        .iter()
        .chain(reference.iter())
        .map(|w| w.activity_id.clone())
        .collect();
    all_ids.sort();
    all_ids.dedup();

    if recent.len() < MIN_COHORT_WINDOWS || reference.len() < MIN_COHORT_WINDOWS {
        return status_only_submaximal(
            EnduranceEvidenceStatus::InsufficientCandidateSessions,
            considered,
            accepted_total,
            all_ids,
        );
    }

    // Anchor: median power across recent matched-cohort windows. Using
    // a robust central value (not a single earliest/latest window)
    // gives stable reporting when individual rides fluctuate.
    reference.sort_by(|left, right| {
        left.power_cv_pct
            .total_cmp(&right.power_cv_pct)
            .then_with(|| left.range.start.total_cmp(&right.range.start))
    });
    let tentative_anchor = recent
        .iter()
        .min_by(|a, b| {
            a.power_cv_pct
                .total_cmp(&b.power_cv_pct)
                .then_with(|| a.range.start.total_cmp(&b.range.start))
        })
        .map(|w| w.avg_power_w)
        .unwrap_or(0.0);

    let anchor_filter = |w: &&ControlWindow| power_match(w.avg_power_w, tentative_anchor);
    let recent_matched: Vec<&ControlWindow> = recent.iter().filter(anchor_filter).collect();
    let reference_matched: Vec<&ControlWindow> = reference.iter().filter(anchor_filter).collect();

    let anchor_power = median_of_refs(recent_matched.iter().copied().map(|w| w.avg_power_w))
        .unwrap_or(tentative_anchor);

    let matched_recent: Vec<&ControlWindow> = recent_matched
        .into_iter()
        .filter(|w| power_match(w.avg_power_w, anchor_power))
        .collect();
    let matched_reference: Vec<&ControlWindow> = reference_matched
        .into_iter()
        .filter(|w| power_match(w.avg_power_w, anchor_power))
        .collect();

    if matched_recent.len() < MIN_COHORT_WINDOWS || matched_reference.len() < MIN_COHORT_WINDOWS {
        let matched_total =
            unique_ref_count(&matched_recent) + unique_ref_count(&matched_reference);
        return status_only_submaximal(
            EnduranceEvidenceStatus::NoComparableControlWindows,
            considered,
            matched_total,
            collect_source_ids_refs(
                matched_recent
                    .iter()
                    .chain(matched_reference.iter())
                    .copied(),
            ),
        );
    }

    let recent_hr = median_of_refs(matched_recent.iter().copied().map(|w| w.avg_hr_bpm));
    let reference_hr = median_of_refs(matched_reference.iter().copied().map(|w| w.avg_hr_bpm));
    let recent_eff = median_of_refs(
        matched_recent
            .iter()
            .copied()
            .map(|w| w.efficiency_w_per_bpm),
    );
    let reference_eff = median_of_refs(
        matched_reference
            .iter()
            .copied()
            .map(|w| w.efficiency_w_per_bpm),
    );

    let recent_hr = recent_hr.expect("non-empty");
    let reference_hr = reference_hr.expect("non-empty");
    let recent_eff = recent_eff.expect("non-empty");
    let reference_eff = reference_eff.expect("non-empty");

    let hr_delta = recent_hr - reference_hr;
    let eff_delta_pct = if reference_eff.abs() > f64::EPSILON {
        100.0 * (recent_eff - reference_eff) / reference_eff
    } else {
        0.0
    };

    let mut source_ids: Vec<String> = matched_recent
        .iter()
        .chain(matched_reference.iter())
        .map(|w| w.activity_id.clone())
        .collect();
    source_ids.sort();
    source_ids.dedup();

    SubmaximalHrPowerMetrics {
        status: EnduranceEvidenceStatus::Available,
        activities_considered: considered,
        activities_accepted: unique_ref_count(&matched_recent)
            + unique_ref_count(&matched_reference),
        source_activity_ids: source_ids,
        anchor_power_w: Some(anchor_power),
        recent_median_hr_bpm: Some(recent_hr),
        reference_median_hr_bpm: Some(reference_hr),
        hr_delta_bpm: Some(hr_delta),
        recent_efficiency_w_per_bpm: Some(recent_eff),
        reference_efficiency_w_per_bpm: Some(reference_eff),
        efficiency_delta_pct: Some(eff_delta_pct),
    }
}

fn unique_activity_count(windows: &[ControlWindow]) -> usize {
    let mut ids: Vec<&String> = windows.iter().map(|w| &w.activity_id).collect();
    ids.sort();
    ids.dedup();
    ids.len()
}

fn unique_ref_count(windows: &[&ControlWindow]) -> usize {
    let mut ids: Vec<&String> = windows.iter().map(|w| &w.activity_id).collect();
    ids.sort();
    ids.dedup();
    ids.len()
}

fn collect_source_ids_refs<'a, I>(windows: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a ControlWindow>,
{
    let mut ids: Vec<String> = windows.into_iter().map(|w| w.activity_id.clone()).collect();
    ids.sort();
    ids.dedup();
    ids
}

fn power_match(power_w: f64, anchor_power_w: f64) -> bool {
    if anchor_power_w.abs() <= f64::EPSILON {
        return false;
    }
    ((power_w - anchor_power_w).abs() / anchor_power_w) <= POWER_MATCH_TOLERANCE
}

fn median_of_refs<I>(values: I) -> Option<f64>
where
    I: IntoIterator<Item = f64>,
{
    let mut collected: Vec<f64> = values.into_iter().collect();
    median(&mut collected)
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let n = values.len();
    if n % 2 == 1 {
        Some(values[n / 2])
    } else {
        let left = values[n / 2 - 1];
        let right = values[n / 2];
        Some((left + right) / 2.0)
    }
}

// ── Per-session sliding window collector (O(n)) ──────────────────────

fn collect_session_windows(session: &CyclingSessionInput, eftp_w: f64) -> Vec<ControlWindow> {
    let time = &session.streams.time_s;
    if time.len() < 2 {
        return Vec::new();
    }

    let mut right_end = time.len();
    while right_end > 0 && !time[right_end - 1].is_finite() {
        right_end -= 1;
    }
    if right_end < 2 {
        return Vec::new();
    }

    let last_time = time[right_end - 1];
    if !last_time.is_finite() || last_time < CONTROL_WINDOW_S {
        return Vec::new();
    }

    // Window semantics: samples at indices [left, right) (exclusive right),
    // time span = time[right-1] - time[left]. A 600s window contains 600
    // 1-Hz samples.
    let mut left: usize = 0;
    let mut right: usize = 0;
    let mut power_sum = 0.0_f64;
    let mut power_sq_sum = 0.0_f64;
    let mut hr_sum = 0.0_f64;
    let mut power_count: u64 = 0;
    let mut hr_count: u64 = 0;
    let mut next_stride: f64 = CONTROL_STRIDE_S;
    let mut emitted: Vec<ControlWindow> = Vec::new();

    while right < right_end {
        let t_right = time[right];
        if !t_right.is_finite() {
            right += 1;
            continue;
        }

        add_sample(
            &session.streams,
            right,
            &mut power_sum,
            &mut power_sq_sum,
            &mut hr_sum,
            &mut power_count,
            &mut hr_count,
        );

        // Increment right to make it exclusive before shrinking.
        right += 1;
        let t_window_end = time[right - 1];

        // Shrink from the left until the window covers <= CONTROL_WINDOW_S.
        while left < right {
            let t_left = time[left];
            if t_left.is_finite() && (t_window_end - t_left) <= CONTROL_WINDOW_S {
                break;
            }
            remove_sample(
                &session.streams,
                left,
                &mut power_sum,
                &mut power_sq_sum,
                &mut hr_sum,
                &mut power_count,
                &mut hr_count,
            );
            left += 1;
        }

        if right <= left {
            continue;
        }

        let window_span = t_window_end - time[left];
        if window_span <= 0.0 {
            continue;
        }

        // Emit at every CONTROL_STRIDE_S boundary if the window covers
        // exactly CONTROL_WINDOW_S and we have signal.
        if t_window_end >= next_stride
            && (window_span - CONTROL_WINDOW_S).abs() < 1e-9
            && power_count > 0
            && hr_count > 0
        {
            if let Some(window) = build_window(
                session,
                left,
                right - 1,
                t_window_end,
                power_sum,
                power_sq_sum,
                hr_sum,
                power_count,
                hr_count,
                eftp_w,
            ) {
                emitted.push(window);
            }
            next_stride = t_window_end + CONTROL_STRIDE_S;
        }
    }

    emitted
}

#[allow(clippy::too_many_arguments)]
fn add_sample(
    streams: &MetricStreams,
    index: usize,
    power_sum: &mut f64,
    power_sq_sum: &mut f64,
    hr_sum: &mut f64,
    power_count: &mut u64,
    hr_count: &mut u64,
) {
    if let Some(power) = streams.power_w.as_ref()
        && let Some(value) = power.get(index)
        && value.is_finite()
    {
        *power_sum += value;
        *power_sq_sum += value * value;
        *power_count += 1;
    }
    if let Some(hr) = streams.heartrate_bpm.as_ref()
        && let Some(value) = hr.get(index)
        && value.is_finite()
        && *value > 0.0
    {
        *hr_sum += value;
        *hr_count += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn remove_sample(
    streams: &MetricStreams,
    index: usize,
    power_sum: &mut f64,
    power_sq_sum: &mut f64,
    hr_sum: &mut f64,
    power_count: &mut u64,
    hr_count: &mut u64,
) {
    if let Some(power) = streams.power_w.as_ref()
        && let Some(value) = power.get(index)
        && value.is_finite()
    {
        *power_sum -= value;
        *power_sq_sum -= value * value;
        *power_count = power_count.saturating_sub(1);
    }
    if let Some(hr) = streams.heartrate_bpm.as_ref()
        && let Some(value) = hr.get(index)
        && value.is_finite()
        && *value > 0.0
    {
        *hr_sum -= value;
        *hr_count = hr_count.saturating_sub(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_window(
    session: &CyclingSessionInput,
    left: usize,
    _last_index: usize,
    t_window_end: f64,
    power_sum: f64,
    power_sq_sum: f64,
    hr_sum: f64,
    power_count: u64,
    hr_count: u64,
    eftp_w: f64,
) -> Option<ControlWindow> {
    if power_count == 0 || hr_count == 0 {
        return None;
    }

    let window_duration = t_window_end - session.streams.time_s[left];
    if window_duration <= 0.0 {
        return None;
    }

    let power_coverage_ratio = (power_count as f64 / window_duration.max(1.0)).min(1.0);
    let hr_coverage_ratio = (hr_count as f64 / window_duration.max(1.0)).min(1.0);

    if power_coverage_ratio < CONTROL_MIN_COVERAGE || hr_coverage_ratio < CONTROL_MIN_COVERAGE {
        return None;
    }

    let avg_power = power_sum / power_count as f64;
    if avg_power <= 0.0 {
        return None;
    }

    let power_variance = (power_sq_sum / power_count as f64) - avg_power * avg_power;
    if power_variance < 0.0 {
        return None;
    }
    let power_cv = 100.0 * power_variance.sqrt() / avg_power;
    if power_cv > CONTROL_MAX_POWER_CV_PCT {
        return None;
    }

    let min_p = CONTROL_MIN_EFTP_FRACTION * eftp_w;
    let max_p = CONTROL_MAX_EFTP_FRACTION * eftp_w;
    if !(min_p..=max_p).contains(&avg_power) {
        return None;
    }

    let avg_hr = hr_sum / hr_count as f64;
    if !(avg_hr > 0.0 && avg_hr.is_finite()) {
        return None;
    }

    Some(ControlWindow {
        range: TimeRange {
            start: session.streams.time_s[left],
            end: t_window_end,
        },
        avg_power_w: avg_power,
        avg_hr_bpm: avg_hr,
        efficiency_w_per_bpm: avg_power / avg_hr,
        power_cv_pct: power_cv,
        activity_id: session.activity_id.clone(),
    })
}

fn pick_best_window(mut windows: Vec<ControlWindow>) -> Option<ControlWindow> {
    windows.sort_by(|left, right| {
        left.power_cv_pct
            .total_cmp(&right.power_cv_pct)
            .then_with(|| left.range.start.total_cmp(&right.range.start))
    });
    windows.into_iter().next()
}

// ── Prolonged-ride response (pair matching) ───────────────────────────

fn prolonged_report(sessions: &[CyclingSessionInput], eftp_w: f64) -> ProlongedRideResponseMetrics {
    // Pick the most recent session deterministically.
    let mut sorted_sessions: Vec<&CyclingSessionInput> = sessions.iter().collect();
    sorted_sessions.sort_by(|left, right| {
        right
            .date
            .cmp(&left.date)
            .then_with(|| right.activity_id.cmp(&left.activity_id))
    });
    let Some(target) = sorted_sessions.into_iter().next() else {
        return ProlongedRideResponseMetrics::unavailable(
            EnduranceEvidenceStatus::NoEligibleProlongedRide,
        );
    };

    let windows = collect_session_windows(target, eftp_w);
    if windows.is_empty() {
        return ProlongedRideResponseMetrics::unavailable(
            EnduranceEvidenceStatus::NoEligibleProlongedRide,
        );
    }

    let mut early: Vec<&ControlWindow> = windows
        .iter()
        .filter(|w| w.range.end >= EARLY_END_MIN_S && w.range.end <= EARLY_END_MAX_S)
        .collect();
    let mut late: Vec<&ControlWindow> = windows
        .iter()
        .filter(|w| w.range.end >= LATE_END_MIN_S)
        .collect();

    if early.is_empty() || late.is_empty() {
        return ProlongedRideResponseMetrics::unavailable(
            EnduranceEvidenceStatus::NoEligibleProlongedRide,
        );
    }

    // Tentative pair: lowest combined power CV first, latest end time,
    // used to short-circuit if no better info is available. We still
    // explicitly run the pair match below.
    early.sort_by(|a, b| a.power_cv_pct.total_cmp(&b.power_cv_pct));
    late.sort_by(|a, b| a.power_cv_pct.total_cmp(&b.power_cv_pct));

    let mut best_pair: Option<(&ControlWindow, &ControlWindow, f64)> = None;

    for early_window in early.iter() {
        for late_window in late.iter() {
            let early_power = early_window.avg_power_w;
            let late_power = late_window.avg_power_w;
            if early_power.abs() <= f64::EPSILON {
                continue;
            }
            let diff_frac = ((late_power - early_power).abs() / early_power).abs();
            if diff_frac > POWER_MATCH_TOLERANCE {
                continue;
            }
            let hr_delta = late_window.avg_hr_bpm - early_window.avg_hr_bpm;
            match best_pair {
                Some((_, _, current_hr)) if hr_delta.abs() <= current_hr.abs() => {}
                _ => best_pair = Some((*early_window, *late_window, hr_delta)),
            }
        }
    }

    let Some((early_window, late_window, hr_delta)) = best_pair else {
        return ProlongedRideResponseMetrics::unavailable(
            EnduranceEvidenceStatus::NoMatchedEarlyLateWindows,
        );
    };

    let matched_power = (early_window.avg_power_w + late_window.avg_power_w) / 2.0;
    let early_eff = early_window.efficiency_w_per_bpm;
    let late_eff = late_window.efficiency_w_per_bpm;
    let eff_delta_pct = if early_eff.abs() > f64::EPSILON {
        100.0 * (late_eff - early_eff) / early_eff
    } else {
        0.0
    };

    ProlongedRideResponseMetrics {
        status: EnduranceEvidenceStatus::Available,
        source_activity_id: Some(target.activity_id.clone()),
        early_window_end_min: Some(early_window.range.end / 60.0),
        late_window_end_min: Some(late_window.range.end / 60.0),
        matched_power_w: Some(matched_power),
        early_hr_bpm: Some(early_window.avg_hr_bpm),
        late_hr_bpm: Some(late_window.avg_hr_bpm),
        hr_delta_bpm: Some(hr_delta),
        early_efficiency_w_per_bpm: Some(early_eff),
        late_efficiency_w_per_bpm: Some(late_eff),
        efficiency_delta_pct: Some(eff_delta_pct),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(
        id: &str,
        date: &str,
        power: f64,
        hr: f64,
        duration_s: usize,
    ) -> CyclingSessionInput {
        CyclingSessionInput {
            activity_id: id.into(),
            date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            streams: MetricStreams {
                time_s: (0..=duration_s).map(|v| v as f64).collect(),
                speed_mps: None,
                heartrate_bpm: Some(vec![hr; duration_s + 1]),
                power_w: Some(vec![power; duration_s + 1]),
            },
        }
    }

    #[test]
    fn matched_control_cohorts_report_raw_hr_and_efficiency_deltas() {
        let sessions = vec![
            session("b1", "2026-04-20", 200.0, 150.0, 1_800),
            session("b2", "2026-04-28", 202.0, 151.0, 1_800),
            session("r1", "2026-07-03", 201.0, 145.0, 1_800),
            session("r2", "2026-07-08", 199.0, 144.0, 1_800),
        ];
        let report = compute_endurance_evidence(
            &sessions,
            NaiveDate::from_ymd_opt(2026, 7, 13).unwrap(),
            Some(300.0),
        );

        assert_eq!(report.submaximal.status, EnduranceEvidenceStatus::Available);
        assert!((report.submaximal.anchor_power_w.unwrap() - 200.0).abs() < 1e-9);
        assert_eq!(report.submaximal.recent_median_hr_bpm, Some(144.5));
        assert_eq!(report.submaximal.reference_median_hr_bpm, Some(150.5));
        assert_eq!(report.submaximal.hr_delta_bpm, Some(-6.0));
        assert!(report.submaximal.efficiency_delta_pct.unwrap() > 3.0);
    }

    #[test]
    fn variable_or_gapped_power_never_becomes_a_control_window() {
        let mut variable = session("v", "2026-07-08", 200.0, 145.0, 600);
        for (index, value) in variable
            .streams
            .power_w
            .as_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            if index % 2 == 0 {
                *value = 320.0;
            }
        }
        let mut gapped = session("g", "2026-07-09", 200.0, 145.0, 600);
        gapped.streams.power_w.as_mut().unwrap().fill(f64::NAN);
        let report = compute_endurance_evidence(
            &[variable, gapped],
            NaiveDate::from_ymd_opt(2026, 7, 13).unwrap(),
            Some(300.0),
        );
        assert_ne!(report.submaximal.status, EnduranceEvidenceStatus::Available);
        assert_eq!(report.submaximal.activities_accepted, 0);
    }

    #[test]
    fn prolonged_ride_pairs_all_eligible_early_and_late_windows() {
        let mut long = session("long", "2026-07-10", 200.0, 145.0, 8_100);
        // Extend HR=155 over the full late-window span (including the
        // boundary sample at index 7200) so the closed-interval window
        // average is exactly 155, not 154.98.
        long.streams.heartrate_bpm.as_mut().unwrap()[6_600..7_201].fill(155.0);
        let report = compute_endurance_evidence(
            &[long],
            NaiveDate::from_ymd_opt(2026, 7, 13).unwrap(),
            Some(300.0),
        );
        assert_eq!(
            report.prolonged_response.status,
            EnduranceEvidenceStatus::Available
        );
        assert_eq!(report.prolonged_response.hr_delta_bpm, Some(10.0));
        assert!(report.prolonged_response.late_window_end_min.unwrap() >= 120.0);
    }

    #[test]
    fn missing_eftp_and_non_matching_cohorts_are_explicitly_unavailable() {
        let sessions = vec![session("a", "2026-07-08", 200.0, 145.0, 1_800)];
        let date = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
        let report_none = compute_endurance_evidence(&sessions, date, None);
        assert_eq!(
            report_none.submaximal.status,
            EnduranceEvidenceStatus::MissingEftp
        );
        assert_eq!(
            report_none.prolonged_response.status,
            EnduranceEvidenceStatus::MissingEftp
        );

        let far_apart = vec![
            session("old-a", "2026-05-01", 180.0, 150.0, 1_800),
            session("old-b", "2026-05-05", 180.0, 151.0, 1_800),
            session("new-a", "2026-07-05", 220.0, 145.0, 1_800),
            session("new-b", "2026-07-08", 220.0, 144.0, 1_800),
        ];
        assert_eq!(
            compute_endurance_evidence(&far_apart, date, Some(300.0))
                .submaximal
                .status,
            EnduranceEvidenceStatus::NoComparableControlWindows,
        );
    }

    #[test]
    fn empty_sessions_or_short_sessions_are_explicitly_unavailable() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
        let no_data = compute_endurance_evidence(&[], date, Some(300.0));
        assert_eq!(
            no_data.submaximal.status,
            EnduranceEvidenceStatus::InsufficientCandidateSessions
        );
        assert_eq!(
            no_data.prolonged_response.status,
            EnduranceEvidenceStatus::NoEligibleProlongedRide
        );

        let short = vec![session("short", "2026-07-08", 200.0, 145.0, 30)];
        let short_report = compute_endurance_evidence(&short, date, Some(300.0));
        assert_eq!(
            short_report.submaximal.status,
            EnduranceEvidenceStatus::InsufficientCandidateSessions
        );
    }

    #[test]
    fn same_date_sessions_are_split_by_activity_id_in_anchor_selection() {
        // Two reference sessions on 2026-05-01 and two recent sessions on
        // 2026-07-08 — all four should contribute, and the recent
        // cohort's median HR must reflect both same-date ids.
        let mut older_a = session("old-z", "2026-05-01", 200.0, 152.0, 1_800);
        let mut older_b = session("old-a", "2026-05-05", 200.0, 148.0, 1_800);
        let mut newer_a = session("new-z", "2026-07-08", 200.0, 144.0, 1_800);
        let mut newer_b = session("new-a", "2026-07-08", 200.0, 146.0, 1_800);
        older_a.streams.heartrate_bpm.as_mut().unwrap().fill(152.0);
        older_b.streams.heartrate_bpm.as_mut().unwrap().fill(148.0);
        newer_a.streams.heartrate_bpm.as_mut().unwrap().fill(144.0);
        newer_b.streams.heartrate_bpm.as_mut().unwrap().fill(146.0);
        let date = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
        let report =
            compute_endurance_evidence(&[older_a, older_b, newer_a, newer_b], date, Some(300.0));
        // Median of recent HRs (144, 146) = 145.0; reference HRs (148,
        // 152) median = 150.0. Delta -5 bpm.
        assert_eq!(report.submaximal.status, EnduranceEvidenceStatus::Available);
        assert_eq!(report.submaximal.recent_median_hr_bpm, Some(145.0));
        assert_eq!(report.submaximal.reference_median_hr_bpm, Some(150.0));
        assert_eq!(report.submaximal.hr_delta_bpm, Some(-5.0));
        let mut ids = report.submaximal.source_activity_ids.clone();
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "new-a".to_string(),
                "new-z".to_string(),
                "old-a".to_string(),
                "old-z".to_string(),
            ],
        );
    }

    // ── numerical fixture regression ──────────────────────────────────

    use chrono::NaiveDate as _NaiveDate;
    use serde::Deserialize as _Deserialize;
    use serde_json::Value as _Value;

    const STABLE_FIXTURE: &str =
        include_str!("../../tests/fixtures/endurance_evidence/stable-control.json");
    const GAPPED_FIXTURE: &str =
        include_str!("../../tests/fixtures/endurance_evidence/gapped-control.json");
    const LONG_FIXTURE: &str =
        include_str!("../../tests/fixtures/endurance_evidence/matched-prolonged-ride.json");

    #[derive(Debug, _Deserialize)]
    struct Fixture {
        as_of: _NaiveDate,
        eftp_w: Option<f64>,
        sessions: Vec<FixtureSession>,
        expected: FixtureExpected,
    }

    #[derive(Debug, _Deserialize)]
    #[serde(untagged)]
    enum FixtureSession {
        Constant {
            id: String,
            date: String,
            duration_s: usize,
            power_w: f64,
            hr_bpm: f64,
            #[serde(default)]
            gaps: Vec<Gap>,
        },
        Piecewise {
            id: String,
            date: String,
            duration_s: usize,
            power_w: f64,
            hr_segments: Vec<HrSegment>,
        },
    }

    /// Fixture-only struct. Deserialised from JSON regression fixtures;
    /// only the fields touched by `fixture_to_sessions` are read, the rest
    /// are kept to mirror the on-disk schema. Per ADR-0002 test-fixture
    /// exemption.
    #[derive(Debug, _Deserialize, Default)]
    #[allow(dead_code)]
    struct Gap {
        start_s: usize,
        end_s: usize,
        #[serde(default)]
        nullify_power: bool,
    }

    /// Fixture-only struct. Deserialised from JSON regression fixtures;
    /// fields are present to mirror the on-disk schema even when the
    /// piecewise builder only consumes a subset. Per ADR-0002 test-fixture
    /// exemption.
    #[derive(Debug, _Deserialize)]
    #[allow(dead_code)]
    struct HrSegment {
        start_s: usize,
        end_s: usize,
        hr_bpm: f64,
    }

    #[derive(Debug, _Deserialize)]
    struct FixtureExpected {
        submax_status: EnduranceEvidenceStatus,
        submax_hr_delta_bpm: Option<f64>,
        prolonged_status: EnduranceEvidenceStatus,
        prolonged_hr_delta_bpm: Option<f64>,
    }

    fn load_endurance_evidence_fixtures() -> Vec<Fixture> {
        [
            ("stable-control", STABLE_FIXTURE),
            ("gapped-control", GAPPED_FIXTURE),
            ("matched-prolonged-ride", LONG_FIXTURE),
        ]
        .into_iter()
        .map(|(name, raw)| {
            serde_json::from_str(raw)
                .unwrap_or_else(|_| panic!("parametric fixture {name} must parse"))
        })
        .collect()
    }

    fn fixture_to_sessions(fixture: &Fixture) -> Vec<CyclingSessionInput> {
        let mut out = Vec::new();
        for raw in &fixture.sessions {
            match raw {
                FixtureSession::Constant {
                    id,
                    date,
                    duration_s,
                    power_w,
                    hr_bpm,
                    gaps,
                } => {
                    let mut power = vec![*power_w; *duration_s + 1];
                    let hr = vec![*hr_bpm; *duration_s + 1];
                    for gap in gaps {
                        if gap.nullify_power {
                            for slot in power.iter_mut().take(gap.end_s + 1).skip(gap.start_s) {
                                *slot = f64::NAN;
                            }
                        }
                    }
                    out.push(CyclingSessionInput {
                        activity_id: id.clone(),
                        date: _NaiveDate::parse_from_str(date, "%Y-%m-%d")
                            .expect("fixture date must be YYYY-MM-DD"),
                        streams: MetricStreams {
                            time_s: (0..=*duration_s).map(|v| v as f64).collect(),
                            speed_mps: None,
                            heartrate_bpm: Some(hr),
                            power_w: Some(power),
                        },
                    });
                }
                FixtureSession::Piecewise {
                    id,
                    date,
                    duration_s,
                    power_w,
                    hr_segments,
                } => {
                    let mut hr = vec![0.0_f64; *duration_s + 1];
                    for segment in hr_segments {
                        let start = segment.start_s.min(hr.len() - 1);
                        let end = segment.end_s.min(hr.len() - 1);
                        for slot in hr.iter_mut().take(end + 1).skip(start) {
                            *slot = segment.hr_bpm;
                        }
                    }
                    let power = vec![*power_w; *duration_s + 1];
                    out.push(CyclingSessionInput {
                        activity_id: id.clone(),
                        date: _NaiveDate::parse_from_str(date, "%Y-%m-%d")
                            .expect("fixture date must be YYYY-MM-DD"),
                        streams: MetricStreams {
                            time_s: (0..=*duration_s).map(|v| v as f64).collect(),
                            speed_mps: None,
                            heartrate_bpm: Some(hr),
                            power_w: Some(power),
                        },
                    });
                }
            }
        }
        out
    }

    fn assert_close(actual: Option<f64>, expected: Option<f64>, tolerance: f64, label: &str) {
        match (actual, expected) {
            (Some(a), Some(e)) => {
                assert!(
                    (a - e).abs() <= tolerance,
                    "{label}: actual {a} vs expected {e}"
                );
            }
            (None, None) => {}
            (None, Some(e)) => panic!("{label}: expected {e} but got None"),
            (Some(a), None) => panic!("{label}: actual {a} but expected None"),
        }
    }

    #[test]
    fn numerical_fixtures_preserve_control_and_prolonged_response_semantics() {
        for fixture in load_endurance_evidence_fixtures() {
            let sessions = fixture_to_sessions(&fixture);
            let actual = compute_endurance_evidence(&sessions, fixture.as_of, fixture.eftp_w);
            assert_eq!(
                actual.submaximal.status, fixture.expected.submax_status,
                "fixture as_of={} submax status",
                fixture.as_of,
            );
            assert_close(
                actual.submaximal.hr_delta_bpm,
                fixture.expected.submax_hr_delta_bpm,
                3.0,
                "submax HR delta",
            );
            assert_eq!(
                actual.prolonged_response.status, fixture.expected.prolonged_status,
                "fixture as_of={} prolonged status",
                fixture.as_of,
            );
            assert_close(
                actual.prolonged_response.hr_delta_bpm,
                fixture.expected.prolonged_hr_delta_bpm,
                3.0,
                "prolonged HR delta",
            );
        }
    }

    #[test]
    fn regression_fixture_schema_is_analytically_documented() {
        let raw_fixtures: &[(&str, &str)] = &[
            ("stable-control", STABLE_FIXTURE),
            ("gapped-control", GAPPED_FIXTURE),
            ("matched-prolonged-ride", LONG_FIXTURE),
        ];
        for (name, raw) in raw_fixtures {
            let parsed: _Value = serde_json::from_str(raw)
                .unwrap_or_else(|_| panic!("fixture {name} must stay valid JSON"));
            assert!(parsed.is_object(), "fixture {name} must be an object");
            let object = parsed.as_object().expect("object");
            assert!(
                object.contains_key("as_of"),
                "fixture {name} requires as_of"
            );
            assert!(
                object.contains_key("sessions"),
                "fixture {name} requires sessions"
            );
            assert!(
                object.contains_key("expected"),
                "fixture {name} requires expected"
            );
        }
    }
}
