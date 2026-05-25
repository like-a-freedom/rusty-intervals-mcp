use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use intervals_icu_client::ActivitySummary;
use serde_json::Value;

use crate::domains::coach::AnalysisWindow;
use crate::domains::progress::{
    HypothesisDomain, ProgressHypothesis, ProgressReport, TidDriftMetrics, TidDriftState,
};
use crate::engines::analysis_fetch::build_daily_load_series;
use crate::engines::changepoint::detect_trailing_ctl_plateau;
use crate::engines::coach_metrics::{
    compute_acwr, compute_lnrmssd_rollup, compute_monotony, compute_strain, compute_tid_entropy,
    extract_ctl_series, extract_hrv_series, parse_wellness_metrics,
};

const DEFAULT_TID_DRIFT_DELTA_THRESHOLD: f64 = 0.15;

fn parse_activity_date(value: &str) -> Option<NaiveDate> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|dt| dt.date())
        .or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
}

fn extract_zone_distribution(detail: &Value) -> Option<(f64, f64, f64)> {
    let zone_times = detail.get("icu_zone_times")?.as_array()?;
    let mut z1 = 0.0;
    let mut z2 = 0.0;
    let mut z3 = 0.0;

    for entry in zone_times {
        let id = entry.get("id")?.as_str()?;
        let secs = entry
            .get("secs")
            .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
            .unwrap_or(0.0);
        match id {
            "Z1" | "Z2" => z1 += secs,
            "Z3" => z2 += secs,
            "Z4" | "Z5" | "Z6" | "Z7" => z3 += secs,
            _ => {}
        }
    }

    let total = z1 + z2 + z3;
    if total <= f64::EPSILON {
        return None;
    }

    Some((z1 / total, z2 / total, z3 / total))
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

        let Some((z1, z2, z3)) = extract_zone_distribution(detail) else {
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
    if weekly.len() < 4 {
        return TidDriftMetrics::default();
    }

    let entropies = weekly
        .iter()
        .filter_map(|(z1, z2, z3)| compute_tid_entropy(*z1, *z2, *z3))
        .collect::<Vec<_>>();

    if entropies.len() < 4 {
        return TidDriftMetrics::default();
    }

    let recent = &entropies[entropies.len().saturating_sub(4)..];
    let prior = &entropies[..entropies.len().saturating_sub(4)];
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
            report.monotony.map(|value| value < 1.5).unwrap_or(false),
            matches!(report.tid_drift.drift_state, TidDriftState::Stable),
        ],
        &[0.35, 0.25, 0.20, 0.20],
    );
    if volume_confidence >= 0.50 {
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
            report.monotony.map(|value| value >= 1.5).unwrap_or(false),
            matches!(report.tid_drift.drift_state, TidDriftState::Converging),
            report.tid_drift.dominant_zone == Some(2),
        ],
        &[0.30, 0.25, 0.25, 0.20],
    );
    if intensity_confidence >= 0.50 {
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
    if recovery_confidence >= 0.50 {
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
    let mut report = ProgressReport::default();

    if let Some((dates, ctl_values)) = extract_ctl_series(Some(wellness)) {
        report.plateau = detect_trailing_ctl_plateau(&dates, &ctl_values);
    } else {
        report
            .warnings
            .push("Wellness CTL/fitness history unavailable; plateau detection skipped.".into());
    }

    let activity_refs = activities.iter().collect::<Vec<_>>();
    let daily_load_series = build_daily_load_series(&activity_refs, activity_details, window);
    let daily_load_values = daily_load_series
        .iter()
        .map(|(_, load)| *load)
        .collect::<Vec<_>>();
    let trailing_7d = if daily_load_values.len() >= 7 {
        &daily_load_values[daily_load_values.len() - 7..]
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
        report.lnrmssd = compute_lnrmssd_rollup(&hrv_values);
    } else {
        report
            .warnings
            .push("Wellness HRV history unavailable; lnRMSSD rollup skipped.".into());
    }

    // Interpretation rule: if enough athlete-specific history exists, downstream
    // hypothesis logic should treat HRV and load features as deviations from that
    // athlete's own baseline before falling back to generic heuristic states.

    let weekly_zones = build_weekly_zone_distributions(activities, activity_details);
    report.tid_drift = compute_tid_drift(&weekly_zones);
    if !report.tid_drift.supported {
        report
            .warnings
            .push("TID drift unavailable; not enough activity details with zone data.".into());
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
}
