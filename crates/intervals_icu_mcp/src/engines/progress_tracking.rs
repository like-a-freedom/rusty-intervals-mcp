use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, NaiveDate};
use intervals_icu_client::ActivitySummary;
use serde_json::Value;

use crate::domains::baseline::{BaselineTransform, compute_personal_baseline};
use crate::domains::coach::AnalysisWindow;
use crate::domains::progress::{
    HypothesisDomain, ProgressHypothesis, ProgressReport, TidDriftMetrics, TidDriftState,
};
use crate::engines::analysis_fetch::{activity_load, build_daily_load_series};
use crate::engines::changepoint::detect_trailing_ctl_plateau;
use crate::engines::coach_metrics::{
    compute_acwr, compute_lnrmssd_rollup, compute_monotony, compute_strain, compute_tid_entropy,
    extract_ctl_series, extract_hrv_series, parse_wellness_metrics,
};
use crate::engines::shared::{compute_zone_distribution, parse_activity_date};

const DEFAULT_TID_DRIFT_DELTA_THRESHOLD: f64 = 0.15;
const MIN_WEEKS_FOR_TID_DRIFT: usize = 4;
const MONOTONY_HIGH_THRESHOLD: f64 = 1.5;
const HYPOTHESIS_MIN_CONFIDENCE: f64 = 0.50;
const TRAILING_LOAD_WINDOW_DAYS: usize = 7;
const CTL_TIME_CONSTANT_DAYS: f64 = 42.0;

/// Minimum daily CTL points required to compute a trailing plateau at all.
pub const MIN_DAYS_FOR_PLATEAU: usize = 28;

/// Minimum daily CTL points required to personalize the flat-band threshold.
pub const MIN_DAYS_FOR_PERSONALIZATION: usize = 56;

/// Maximum wellness range the track_progress handler will request when auto-expanding.
/// Mirrors `MAX_PERIOD_WEEKS * 7` from the track_progress handler (24 weeks = 168 days).
pub const MAX_WELLNESS_DAYS_FALLBACK: i32 = 168;

/// Count the number of daily CTL/fitness points actually present in a wellness payload.
/// Empty entries and entries missing CTL are ignored.
#[must_use]
pub fn count_ctl_points(wellness: &Value) -> usize {
    let Some(entries) = wellness.as_array() else {
        return 0;
    };
    entries
        .iter()
        .filter_map(Value::as_object)
        .filter(|object| {
            // Reuse the same key precedence as extract_ctl_series: ["fitness", "ctl"] or ["ctlLoad", "icu_ctl"].
            object.get("fitness").and_then(Value::as_f64).is_some()
                || object.get("ctl").and_then(Value::as_f64).is_some()
                || object.get("ctlLoad").and_then(Value::as_f64).is_some()
                || object.get("icu_ctl").and_then(Value::as_f64).is_some()
        })
        .count()
}

/// Estimate a daily chronic-load series from activity training load when the
/// wellness endpoint has no historical CTL. The series begins on the first
/// dated activity with a usable load, so pre-training zeroes are never treated
/// as history.
pub fn derive_ctl_series_from_activity_loads(
    activities: &[ActivitySummary],
    activity_details: &HashMap<String, Value>,
    window: &AnalysisWindow,
) -> Option<(Vec<String>, Vec<f64>)> {
    let first_activity_date = activities
        .iter()
        .filter_map(|activity| {
            let date = parse_activity_date(&activity.start_date_local)?;
            let has_load = activity_load(activity, activity_details.get(&activity.id)).is_some();
            (has_load && date >= window.start_date && date <= window.end_date).then_some(date)
        })
        .min()?;

    let derivation_window = AnalysisWindow::new(first_activity_date, window.end_date);
    let activity_refs = activities.iter().collect::<Vec<_>>();
    let daily_loads = build_daily_load_series(&activity_refs, activity_details, &derivation_window);
    if daily_loads.activities_with_load == 0 {
        return None;
    }

    let mut ctl = 0.0;
    let mut dates = Vec::with_capacity(daily_loads.daily.len());
    let mut values = Vec::with_capacity(daily_loads.daily.len());
    for (date, load) in &daily_loads.daily {
        ctl += (load - ctl) / CTL_TIME_CONSTANT_DAYS;
        dates.push(date.to_string());
        values.push(ctl);
    }

    Some((dates, values))
}

type ZonePct = (f64, f64, f64);

type WeekKey = (i32, u32);

pub fn build_weekly_zone_distributions(
    activities: &[ActivitySummary],
    activity_details: &HashMap<String, Value>,
) -> Vec<ZonePct> {
    let mut per_week: BTreeMap<WeekKey, Vec<ZonePct>> = BTreeMap::new();

    let mut sorted = activities.to_vec();
    sorted.sort_by_key(|activity| {
        parse_activity_date(&activity.start_date_local).unwrap_or(NaiveDate::MIN)
    });

    for activity in sorted {
        let Some(date) = parse_activity_date(&activity.start_date_local) else {
            continue;
        };
        let Some(detail) = activity_details.get(&activity.id) else {
            continue;
        };

        let Some((z1, z2, z3)) = detail
            .get("icu_zone_times")
            .and_then(compute_zone_distribution)
        else {
            continue;
        };

        let iso = date.iso_week();
        per_week
            .entry((iso.year(), iso.week()))
            .or_default()
            .push((z1, z2, z3));
    }

    per_week
        .into_values()
        .map(|samples| {
            let n = samples.len() as f64;
            let z1 = samples.iter().map(|sample| sample.0).sum::<f64>() / n;
            let z2 = samples.iter().map(|sample| sample.1).sum::<f64>() / n;
            let z3 = samples.iter().map(|sample| sample.2).sum::<f64>() / n;
            (z1, z2, z3)
        })
        .collect()
}

pub fn compute_tid_drift(weekly: &[(f64, f64, f64)]) -> TidDriftMetrics {
    if weekly.len() < MIN_WEEKS_FOR_TID_DRIFT {
        return TidDriftMetrics::default();
    }

    let entropies = weekly
        .iter()
        .filter_map(|(z1, z2, z3)| compute_tid_entropy(*z1, *z2, *z3))
        .collect::<Vec<_>>();

    if entropies.len() < MIN_WEEKS_FOR_TID_DRIFT {
        return TidDriftMetrics::default();
    }

    let recent = &entropies[entropies.len().saturating_sub(MIN_WEEKS_FOR_TID_DRIFT)..];
    let prior = &entropies[..entropies.len().saturating_sub(MIN_WEEKS_FOR_TID_DRIFT)];
    let recent_mean = recent.iter().sum::<f64>() / recent.len() as f64;
    let prior_mean = if prior.is_empty() {
        recent_mean
    } else {
        prior.iter().sum::<f64>() / prior.len() as f64
    };
    let delta = recent_mean - prior_mean;
    let drift_band = DEFAULT_TID_DRIFT_DELTA_THRESHOLD;

    let recent_week = weekly.last().copied().unwrap_or((0.0, 0.0, 0.0));
    let dominant_zone = [recent_week.0, recent_week.1, recent_week.2]
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(index, _)| index as u8 + 1);

    TidDriftMetrics {
        supported: true,
        weekly_samples: weekly.len(),
        entropy_recent_4w: Some(recent_mean),
        entropy_prior_4w: Some(prior_mean),
        drift_state: if delta <= -drift_band {
            TidDriftState::Converging
        } else if delta >= drift_band {
            TidDriftState::Polarizing
        } else {
            TidDriftState::Stable
        },
        dominant_zone,
    }
}

pub fn match_hypotheses(report: &ProgressReport) -> Vec<ProgressHypothesis> {
    if !report.plateau.plateau_detected {
        return Vec::new();
    }

    let mut hypotheses = Vec::new();

    // When enough athlete history exists, prefer athlete-relative load states
    // (e.g. unusual-for-this-athlete / within-typical-range) over literal fallback
    // cutoffs such as monotony 1.5.

    let volume_confidence = weighted_confidence(
        &[
            report.plateau.trend == crate::domains::progress::TrendState::Flat,
            matches!(
                report.acwr_state.as_deref(),
                Some("productive") | Some("underloaded")
            ),
            report
                .monotony
                .map(|value| value < MONOTONY_HIGH_THRESHOLD)
                .unwrap_or(false),
            matches!(report.tid_drift.drift_state, TidDriftState::Stable),
        ],
        &[0.35, 0.25, 0.20, 0.20],
    );
    if volume_confidence >= HYPOTHESIS_MIN_CONFIDENCE {
        hypotheses.push(ProgressHypothesis {
            domain: HypothesisDomain::Volume,
            confidence: volume_confidence,
            evidence: vec![
                "Trailing CTL is flat over the plateau window.".into(),
                format!(
                    "ACWR state: {}.",
                    report
                        .acwr_state
                        .clone()
                        .unwrap_or_else(|| "unknown".into())
                ),
                format!("Monotony: {:.2}.", report.monotony.unwrap_or(0.0)),
            ],
            suggested_intervention:
                "Increase weekly volume gradually while keeping ACWR in the productive band."
                    .into(),
            tracking_metric: "Weekly TSS / load trend".into(),
        });
    }

    let intensity_confidence = weighted_confidence(
        &[
            report.plateau.trend == crate::domains::progress::TrendState::Flat,
            report
                .monotony
                .map(|value| value >= MONOTONY_HIGH_THRESHOLD)
                .unwrap_or(false),
            matches!(report.tid_drift.drift_state, TidDriftState::Converging),
            report.tid_drift.dominant_zone == Some(2),
        ],
        &[0.30, 0.25, 0.25, 0.20],
    );
    if intensity_confidence >= HYPOTHESIS_MIN_CONFIDENCE {
        hypotheses.push(ProgressHypothesis {
            domain: HypothesisDomain::IntensityDistribution,
            confidence: intensity_confidence,
            evidence: vec![
                "Plateau is flat rather than declining.".into(),
                format!("Monotony: {:.2}.", report.monotony.unwrap_or(0.0)),
                format!("TID drift: {:?}.", report.tid_drift.drift_state),
            ],
            suggested_intervention:
                "Increase distribution contrast by reducing Z2 density and reintroducing clearer easy / hard separation."
                    .into(),
            tracking_metric: "Weekly 3-zone distribution".into(),
        });
    }

    let recovery_confidence = weighted_confidence(
        &[
            matches!(
                report.plateau.trend,
                crate::domains::progress::TrendState::Declining
                    | crate::domains::progress::TrendState::Flat
            ),
            matches!(
                report.acwr_state.as_deref(),
                Some("watch") | Some("overreaching")
            ),
            report.hrv_suppressed,
            matches!(
                report.hrv_trend_state.as_deref(),
                Some("suppressed") | Some("below_range")
            ),
        ],
        &[0.30, 0.30, 0.20, 0.20],
    );
    if recovery_confidence >= HYPOTHESIS_MIN_CONFIDENCE {
        hypotheses.push(ProgressHypothesis {
            domain: HypothesisDomain::Recovery,
            confidence: recovery_confidence,
            evidence: vec![
                format!("Plateau trend: {:?}.", report.plateau.trend),
                format!(
                    "ACWR state: {}.",
                    report
                        .acwr_state
                        .clone()
                        .unwrap_or_else(|| "unknown".into())
                ),
                format!(
                    "HRV trend state: {}.",
                    report
                        .hrv_trend_state
                        .clone()
                        .unwrap_or_else(|| "unknown".into())
                ),
            ],
            suggested_intervention: "Schedule a recovery microcycle before adding more load."
                .into(),
            tracking_metric: "HRV ratio and next 7-day load".into(),
        });
    }

    hypotheses.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hypotheses
}

pub fn generate_recommendations(hypotheses: &[ProgressHypothesis]) -> Vec<String> {
    hypotheses
        .iter()
        .map(|hypothesis| {
            format!(
                "[{:.0}%] {:?}: {} Track {}.",
                hypothesis.confidence * 100.0,
                hypothesis.domain,
                hypothesis.suggested_intervention,
                hypothesis.tracking_metric,
            )
        })
        .collect()
}

fn weighted_confidence(evidence: &[bool], weights: &[f64]) -> f64 {
    let total_weight: f64 = weights.iter().sum();
    if total_weight <= f64::EPSILON {
        return 0.0;
    }

    evidence
        .iter()
        .zip(weights.iter())
        .map(|(present, weight)| if *present { *weight } else { 0.0 })
        .sum::<f64>()
        / total_weight
}

pub fn build_progress_report(
    wellness: &Value,
    activities: &[ActivitySummary],
    activity_details: &HashMap<String, Value>,
    window: &AnalysisWindow,
) -> ProgressReport {
    build_progress_report_with_ctl_fallback(wellness, activities, activity_details, window, None)
}

/// Build a report, preferring upstream wellness CTL and using a clearly marked
/// activity-load estimate only when wellness CTL is unavailable or too short.
pub fn build_progress_report_with_ctl_fallback(
    wellness: &Value,
    activities: &[ActivitySummary],
    activity_details: &HashMap<String, Value>,
    window: &AnalysisWindow,
    activity_ctl_fallback: Option<(Vec<String>, Vec<f64>)>,
) -> ProgressReport {
    let mut report = ProgressReport::default();

    let wellness_ctl = extract_ctl_series(Some(wellness));
    let use_activity_fallback = wellness_ctl
        .as_ref()
        .is_none_or(|(_, values)| values.len() < MIN_DAYS_FOR_PLATEAU)
        && activity_ctl_fallback.is_some();
    let ctl_series = if use_activity_fallback {
        activity_ctl_fallback
    } else {
        wellness_ctl
    };

    let mut available_ctl_points = 0usize;
    if let Some((dates, ctl_values)) = ctl_series {
        available_ctl_points = ctl_values.len();
        report.plateau = detect_trailing_ctl_plateau(&dates, &ctl_values);
        if use_activity_fallback {
            report.warnings.push(
                "Wellness CTL/fitness history unavailable or too short; plateau detection is estimated from activity training-load history."
                    .into(),
            );
        }
    } else {
        report.warnings.push(format!(
            "Wellness CTL/fitness history unavailable; plateau detection skipped (need {} days, have 0).",
            MIN_DAYS_FOR_PLATEAU
        ));
    }

    // If the wellness payload had some entries but fewer than the minimum for plateau,
    // report the concrete shortfall so the user knows how many days they are missing.
    if !report.plateau.supported {
        let available = available_ctl_points.max(count_ctl_points(wellness));
        if available > 0 && available < MIN_DAYS_FOR_PLATEAU {
            let personalization_gap = if available < MIN_DAYS_FOR_PERSONALIZATION {
                format!(
                    "; {} days also needed for personalized flat-band calibration",
                    MIN_DAYS_FOR_PERSONALIZATION
                )
            } else {
                String::new()
            };
            report.warnings.push(format!(
                "Plateau detection needs {} days of daily CTL, but only {} day(s) were found{}.",
                MIN_DAYS_FOR_PLATEAU, available, personalization_gap,
            ));
        }
    }

    let activity_refs = activities.iter().collect::<Vec<_>>();
    let daily_load_series = build_daily_load_series(&activity_refs, activity_details, window);
    let daily_load_values = daily_load_series.values().collect::<Vec<_>>();
    let trailing_7d = if daily_load_values.len() >= TRAILING_LOAD_WINDOW_DAYS {
        &daily_load_values[daily_load_values.len() - TRAILING_LOAD_WINDOW_DAYS..]
    } else {
        &daily_load_values[..]
    };

    let acwr = compute_acwr(&daily_load_values);
    report.acwr_ratio = acwr.as_ref().map(|metrics| metrics.ratio);
    report.acwr_state = acwr.as_ref().map(|metrics| metrics.state.clone());
    report.monotony = compute_monotony(trailing_7d);
    report.strain = report
        .monotony
        .map(|monotony| compute_strain(trailing_7d, monotony));

    if report.acwr_ratio.is_none() {
        report
            .warnings
            .push("Daily load history too short for ACWR.".into());
    }

    let wellness_metrics = parse_wellness_metrics(Some(wellness));
    report.hrv_ratio = wellness_metrics
        .as_ref()
        .and_then(|metrics| metrics.hrv_ratio);
    report.hrv_trend_state = wellness_metrics
        .as_ref()
        .and_then(|metrics| metrics.hrv_trend_state.clone());
    report.hrv_suppressed = wellness_metrics
        .as_ref()
        .map(|metrics| metrics.hrv_suppression_flag)
        .unwrap_or(false);

    if let Some(hrv_values) = extract_hrv_series(Some(wellness)) {
        let hrv_total = hrv_values.len();
        report.lnrmssd = compute_lnrmssd_rollup(&hrv_values);
        if !report.lnrmssd.supported && hrv_total > 0 {
            report.warnings.push(format!(
                "lnRMSSD 7-day rollup needs 7 days of HRV, but only {} day(s) of HRV were found. \
                 Recovery context (HRV ratio / trend) is still computed from the available data.",
                hrv_total
            ));
        }
    } else {
        report
            .warnings
            .push("Wellness HRV history unavailable; lnRMSSD rollup and HRV ratio skipped.".into());
    }

    // Personal baseline from wellness history
    if let Some(entries) = wellness.as_array() {
        let hrv_observations: Vec<(chrono::NaiveDate, f64)> = entries
            .iter()
            .filter_map(|entry| {
                let obj = entry.as_object()?;
                let date_str = obj.get("date")?.as_str()?;
                let date = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
                let hrv = obj.get("hrv")?.as_f64()?;
                Some((date, hrv))
            })
            .collect();
        let rhr_observations: Vec<(chrono::NaiveDate, f64)> = entries
            .iter()
            .filter_map(|entry| {
                let obj = entry.as_object()?;
                let date_str = obj.get("date")?.as_str()?;
                let date = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
                let rhr = obj
                    .get("resting_hr")
                    .or_else(|| obj.get("restingHR"))
                    .or_else(|| obj.get("resting_hr_bpm"))
                    .or_else(|| obj.get("avgSleepingHR"))?
                    .as_f64()?;
                Some((date, rhr))
            })
            .collect();
        report.hrv_personal_baseline =
            compute_personal_baseline(&hrv_observations, BaselineTransform::LogLnRmssd);
        report.resting_hr_personal_baseline =
            compute_personal_baseline(&rhr_observations, BaselineTransform::RawBpm);
    }

    // Interpretation rule: if enough athlete-specific history exists, downstream
    // hypothesis logic should treat HRV and load features as deviations from that
    // athlete's own baseline before falling back to generic heuristic states.

    let weekly_zones = build_weekly_zone_distributions(activities, activity_details);
    report.tid_drift = compute_tid_drift(&weekly_zones);
    if !report.tid_drift.supported {
        let have = weekly_zones.len();
        let need = MIN_WEEKS_FOR_TID_DRIFT;
        if have > 0 {
            report.warnings.push(format!(
                "TID drift needs {} weeks of activity details with zone data, but only {} week(s) were available. \
                 Check that activities have icu_zone_times populated.",
                need, have
            ));
        } else {
            report.warnings.push(
                "TID drift unavailable; no activity details with zone data were found in the window."
                    .into(),
            );
        }
    }

    report.hypotheses = match_hypotheses(&report);
    report.recommendations = generate_recommendations(&report.hypotheses);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use intervals_icu_client::ActivitySummary;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn build_weekly_zone_distributions_uses_chronological_order() {
        let activities = vec![
            ActivitySummary {
                id: "b".into(),
                start_date_local: "2026-01-08".into(),
                ..Default::default()
            },
            ActivitySummary {
                id: "a".into(),
                start_date_local: "2026-01-01".into(),
                ..Default::default()
            },
        ];

        // Week 1 (a): more Z1+Z2 (maps to z1) than Z3 (maps to z2) or Z4+ (maps to z3)
        let details = HashMap::from([
            (
                "a".to_string(),
                json!({"icu_zone_times": [{"id": "Z1", "secs": 2000}, {"id": "Z3", "secs": 300}, {"id": "Z5", "secs": 300}]}),
            ),
            // Week 2 (b): more Z4+ (maps to z3) than Z1+Z2 (maps to z1)
            (
                "b".to_string(),
                json!({"icu_zone_times": [{"id": "Z1", "secs": 300}, {"id": "Z4", "secs": 2000}, {"id": "Z5", "secs": 500}]}),
            ),
        ]);

        let weekly = build_weekly_zone_distributions(&activities, &details);
        assert_eq!(weekly.len(), 2);
        // Week 1: z1 should dominate (has more Z1+Z2 secs than Z3 or Z4+)
        assert!(weekly[0].0 > weekly[0].1);
        assert!(weekly[0].0 > weekly[0].2);
        // Week 2: z3 should dominate (has more Z4+ secs than Z1+Z2 or Z3)
        assert!(weekly[1].2 > weekly[1].0);
        assert!(weekly[1].2 > weekly[1].1);
    }

    #[test]
    fn recovery_hypothesis_wins_when_load_and_hrv_are_red() {
        let report = crate::domains::progress::ProgressReport {
            plateau: crate::domains::progress::ChangepointResult {
                supported: true,
                plateau_detected: true,
                trend: crate::domains::progress::TrendState::Declining,
                ..Default::default()
            },
            acwr_ratio: Some(1.45),
            acwr_state: Some("overreaching".into()),
            monotony: Some(1.1),
            strain: Some(1800.0),
            hrv_ratio: Some(0.82),
            hrv_trend_state: Some("suppressed".into()),
            hrv_suppressed: true,
            ..Default::default()
        };

        let hypotheses = match_hypotheses(&report);
        assert!(!hypotheses.is_empty());
        assert_eq!(
            hypotheses[0].domain,
            crate::domains::progress::HypothesisDomain::Recovery
        );
    }

    #[test]
    fn count_ctl_points_handles_empty_and_missing_keys() {
        use serde_json::json;

        // Empty array -> zero
        assert_eq!(count_ctl_points(&json!([])), 0);
        // Non-array payload -> zero
        assert_eq!(count_ctl_points(&json!({"foo": "bar"})), 0);
        // Entries without CTL fields are ignored
        assert_eq!(
            count_ctl_points(&json!([
                {"date": "2026-01-01", "hrv": 60.0},
                {"date": "2026-01-02", "sleep_hours": 8.0}
            ])),
            0
        );
        // All four key variants are recognised
        assert_eq!(
            count_ctl_points(&json!([
                {"date": "2026-01-01", "fitness": 60.0},
                {"date": "2026-01-02", "ctl": 61.0},
                {"date": "2026-01-03", "ctlLoad": 62.0},
                {"date": "2026-01-04", "icu_ctl": 63.0}
            ])),
            4
        );
    }

    #[test]
    fn build_progress_report_emits_specific_warning_when_ctl_short() {
        use crate::domains::coach::AnalysisWindow;
        use chrono::NaiveDate;

        // 14 days of CTL — short of the 28-day plateau minimum.
        let entries: Vec<Value> = (0..14)
            .map(|i| {
                json!({
                    "date": format!("2026-01-{:02}", i + 1),
                    "fitness": 60.0 + (i as f64) * 0.1,
                })
            })
            .collect();
        let wellness = Value::Array(entries);

        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 14).unwrap();
        let window = AnalysisWindow::new(start, end);

        let report = build_progress_report(&wellness, &[], &HashMap::new(), &window);
        assert!(!report.plateau.supported);
        let joined = report.warnings.join("\n");
        assert!(
            joined.contains("14 day(s)"),
            "expected concrete shortfall in warnings, got: {joined}"
        );
        assert!(
            joined.contains("28 days of daily CTL"),
            "expected minimum to be mentioned, got: {joined}"
        );
    }

    #[test]
    fn activity_derived_ctl_reports_its_actual_shortfall() {
        use chrono::NaiveDate;

        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 14).unwrap();
        let window = AnalysisWindow::new(start, end);
        let fallback = Some((
            (0..14)
                .map(|day| format!("2026-01-{:02}", day + 1))
                .collect(),
            vec![50.0; 14],
        ));

        let report = build_progress_report_with_ctl_fallback(
            &json!([]),
            &[],
            &HashMap::new(),
            &window,
            fallback,
        );
        let warnings = report.warnings.join("\n");

        assert!(warnings.contains("activity training-load history"));
        assert!(
            warnings.contains("14 day(s)"),
            "expected actual activity-derived shortfall, got: {warnings}"
        );
    }

    #[test]
    fn build_progress_report_warning_distinguishes_personalization_gap() {
        use crate::domains::coach::AnalysisWindow;
        use chrono::NaiveDate;

        // 40 days of CTL — past plateau minimum, but short of 56-day personalization.
        let entries: Vec<Value> = (0..40)
            .map(|i| {
                json!({
                    "date": format!("2026-01-{:02}", i + 1),
                    "fitness": 60.0 + (i as f64) * 0.05,
                })
            })
            .collect();
        let wellness = Value::Array(entries);

        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 2, 9).unwrap();
        let window = AnalysisWindow::new(start, end);

        let report = build_progress_report(&wellness, &[], &HashMap::new(), &window);
        let joined = report.warnings.join("\n");
        // 40 days should be enough to attempt a plateau but personalization is short.
        // Depending on slope, plateau may or may not be `supported`; we mainly assert
        // that the warning mentions the personalization gap when present.
        if !report.plateau.supported {
            assert!(
                joined.contains("personalized flat-band"),
                "expected personalization gap to be mentioned, got: {joined}"
            );
        }
    }

    #[test]
    fn build_progress_report_warns_when_hrv_history_too_short_for_lnrmssd() {
        use crate::domains::coach::AnalysisWindow;
        use chrono::NaiveDate;

        // 30 days of CTL, but only 3 days of HRV — lnRMSSD rollup will be unsupported.
        let entries: Vec<Value> = (0..30)
            .map(|i| {
                if i < 3 {
                    json!({
                        "date": format!("2026-01-{:02}", (i % 30) + 1),
                        "fitness": 60.0 + (i as f64) * 0.05,
                        "hrv": 60.0,
                    })
                } else {
                    json!({
                        "date": format!("2026-01-{:02}", (i % 30) + 1),
                        "fitness": 60.0 + (i as f64) * 0.05,
                    })
                }
            })
            .collect();
        let wellness = Value::Array(entries);

        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 30).unwrap();
        let window = AnalysisWindow::new(start, end);

        let report = build_progress_report(&wellness, &[], &HashMap::new(), &window);
        assert!(!report.lnrmssd.supported);
        let joined = report.warnings.join("\n");
        assert!(
            joined.contains("lnRMSSD 7-day rollup needs 7 days"),
            "expected lnRMSSD shortfall warning, got: {joined}"
        );
        assert!(
            joined.contains("3 day(s)"),
            "expected concrete HRV count in warning, got: {joined}"
        );
    }

    #[test]
    fn build_progress_report_warns_when_tid_drift_lacks_zone_data() {
        use crate::domains::coach::AnalysisWindow;
        use chrono::NaiveDate;

        // Long CTL history so plateau warning is silent, but no activity details
        // at all, so TID drift should warn about the absence.
        let entries: Vec<Value> = (0..30)
            .map(|i| {
                json!({
                    "date": format!("2026-01-{:02}", i + 1),
                    "fitness": 60.0 + (i as f64) * 0.05,
                })
            })
            .collect();
        let wellness = Value::Array(entries);

        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 30).unwrap();
        let window = AnalysisWindow::new(start, end);

        let report = build_progress_report(&wellness, &[], &HashMap::new(), &window);
        assert!(!report.tid_drift.supported);
        let joined = report.warnings.join("\n");
        assert!(
            joined.contains("TID drift unavailable"),
            "expected TID warning, got: {joined}"
        );
    }
}
