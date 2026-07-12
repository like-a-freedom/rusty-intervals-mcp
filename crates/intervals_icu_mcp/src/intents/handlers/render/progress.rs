use crate::domains::coach::FitnessMetrics;
use crate::domains::progress::ProgressReport;
use crate::intents::ContentBlock;

pub(crate) fn render_progress_report(
    report: &ProgressReport,
    hypothesis_mode: bool,
    fitness: Option<&FitnessMetrics>,
) -> Vec<ContentBlock> {
    let mut sections = Vec::new();

    let plateau_text = if report.plateau.plateau_detected {
        format!(
            "Plateau detected from {} ({} days, slope {:+.2}/week).",
            report
                .plateau
                .plateau_start_date
                .as_deref()
                .unwrap_or("unknown date"),
            report.plateau.plateau_duration_days.unwrap_or(0),
            report.plateau.trailing_slope_per_week.unwrap_or(0.0),
        )
    } else if report.plateau.supported {
        format!(
            "No trailing plateau detected. Current CTL trend is {:?}.",
            report.plateau.trend
        )
    } else {
        "Plateau detection unavailable because CTL history is insufficient.".into()
    };

    sections.push(ContentBlock::markdown(format!(
        "## Progress Tracking Report\n\n### Plateau Detection\n{}\n\n### Load Context\n- ACWR: {}\n- Monotony: {} (higher = samey)\n- Strain: {} (total load × monotony)\n\n### HRV Context\n- HRV ratio: {} (1.0 = recovered)\n- HRV trend: {}\n- HRV suppressed: {}\n\n### lnRMSSD 7-day Rollup\n- Supported: {}\n- Recent mean: {} (ln-scale)\n- Recent CV: {} (variability)\n- Trend slope: {} (per day)\n- Sample count: {}\n\n### TID Drift\n- Supported: {}\n- Weekly samples: {}\n- Entropy recent 4w: {} (lower = concentrated)\n- Entropy prior 4w: {} (reference)\n- Drift state: {:?}\n- Dominant zone: {}",
        plateau_text,
        report
            .acwr_ratio
            .map(|value| format!("{value:.2} ({})", report.acwr_state.clone().unwrap_or_else(|| "unknown".into())))
            .unwrap_or_else(|| "unavailable".into()),
        report.monotony.map(|value| format!("{value:.2}")).unwrap_or_else(|| "unavailable".into()),
        report.strain.map(|value| format!("{value:.0}")).unwrap_or_else(|| "unavailable".into()),
        report.hrv_ratio.map(|value| format!("{value:.2}")).unwrap_or_else(|| "unavailable".into()),
        report.hrv_trend_state.clone().unwrap_or_else(|| "unavailable".into()),
        report.hrv_suppressed,
        report.lnrmssd.supported,
        report.lnrmssd.recent_mean_7d.map(|value| format!("{value:.2}")).unwrap_or_else(|| "unavailable".into()),
        report.lnrmssd.recent_cv_7d.map(|value| format!("{value:.4}")).unwrap_or_else(|| "unavailable".into()),
        report.lnrmssd.trend_slope.map(|value| format!("{value:.4}")).unwrap_or_else(|| "unavailable".into()),
        report.lnrmssd.sample_count,
        report.tid_drift.supported,
        report.tid_drift.weekly_samples,
        report.tid_drift.entropy_recent_4w.map(|value| format!("{value:.3}")).unwrap_or_else(|| "unavailable".into()),
        report.tid_drift.entropy_prior_4w.map(|value| format!("{value:.3}")).unwrap_or_else(|| "unavailable".into()),
        report.tid_drift.drift_state,
        report.tid_drift.dominant_zone.map(|value| value.to_string()).unwrap_or_else(|| "unavailable".into()),
    )));

    // Personal baseline evidence
    {
        let mut baseline_parts = Vec::new();
        if let Some(ref hrv_baseline) = report.hrv_personal_baseline {
            let pos = match hrv_baseline.position {
                crate::domains::baseline::BaselinePosition::Below => "Below",
                crate::domains::baseline::BaselinePosition::Within => "Within",
                crate::domains::baseline::BaselinePosition::Above => "Above",
            };
            baseline_parts.push(format!(
                "  Metric: {} ({})\n  Recent 7-day: {:.2}\n  60-day baseline: {:.2}\n  CV: {:.4}\n  SWC: {:.4}\n  Band: [{:.2}, {:.2}]\n  Position: {}\n  Samples: recent {}, baseline {} (span {} days)\n  Model: {}",
                hrv_baseline.metric,
                hrv_baseline.unit,
                hrv_baseline.recent_mean_7d,
                hrv_baseline.baseline_mean_60d,
                hrv_baseline.baseline_cv_60d,
                hrv_baseline.swc,
                hrv_baseline.lower_bound,
                hrv_baseline.upper_bound,
                pos,
                hrv_baseline.recent_sample_count,
                hrv_baseline.baseline_sample_count,
                hrv_baseline.baseline_span_days,
                hrv_baseline.model,
            ));
        }
        if let Some(ref rhr_baseline) = report.resting_hr_personal_baseline {
            let pos = match rhr_baseline.position {
                crate::domains::baseline::BaselinePosition::Below => "Below",
                crate::domains::baseline::BaselinePosition::Within => "Within",
                crate::domains::baseline::BaselinePosition::Above => "Above",
            };
            baseline_parts.push(format!(
                "  Metric: {} ({})\n  Recent 7-day: {:.1}\n  60-day baseline: {:.1}\n  CV: {:.4}\n  SWC: {:.4}\n  Band: [{:.1}, {:.1}]\n  Position: {}\n  Samples: recent {}, baseline {} (span {} days)\n  Model: {}",
                rhr_baseline.metric,
                rhr_baseline.unit,
                rhr_baseline.recent_mean_7d,
                rhr_baseline.baseline_mean_60d,
                rhr_baseline.baseline_cv_60d,
                rhr_baseline.swc,
                rhr_baseline.lower_bound,
                rhr_baseline.upper_bound,
                pos,
                rhr_baseline.recent_sample_count,
                rhr_baseline.baseline_sample_count,
                rhr_baseline.baseline_span_days,
                rhr_baseline.model,
            ));
        }
        if !baseline_parts.is_empty() {
            sections.push(ContentBlock::markdown(format!(
                "### Personal Baselines\n{}\n\nPosition describes a sustained statistical deviation from your own history; it is not a readiness verdict.",
                baseline_parts.join("\n\n"),
            )));
        }
    }

    if let Some(fm) = fitness {
        let mut fit_lines = vec!["### Fitness Snapshot".to_string()];
        if let Some(ctl) = fm.ctl {
            fit_lines.push(format!("- CTL: {:.0}", ctl));
        }
        if let Some(atl) = fm.atl {
            fit_lines.push(format!("- ATL: {:.0}", atl));
        }
        if let Some(tsb) = fm.tsb {
            let state = if tsb > 10.0 {
                "Fresh"
            } else if tsb < -10.0 {
                "Fatigued"
            } else {
                "Balanced"
            };
            fit_lines.push(format!("- TSB: {:.0} ({})", tsb, state));
        }
        if let Some(rr) = fm.ramp_rate {
            fit_lines.push(format!("- Ramp Rate: {:+.1}/wk", rr));
        }
        sections.push(ContentBlock::markdown(fit_lines.join("\n")));
    }

    if hypothesis_mode && !report.hypotheses.is_empty() {
        let body = report
            .hypotheses
            .iter()
            .map(|hypothesis| {
                format!(
                    "- {:?} ({:.0}%): {}\n  - Evidence: {}\n  - Track: {}",
                    hypothesis.domain,
                    hypothesis.confidence * 100.0,
                    hypothesis.suggested_intervention,
                    hypothesis.evidence.join("; "),
                    hypothesis.tracking_metric,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        sections.push(ContentBlock::markdown(format!(
            "### Hypotheses\n{}\n\n### Recommendations\n{}",
            body,
            if report.recommendations.is_empty() {
                "- none".into()
            } else {
                report
                    .recommendations
                    .iter()
                    .map(|item| format!("- {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        )));
    }

    if !report.warnings.is_empty() {
        sections.push(ContentBlock::markdown(format!(
            "### Warnings\n{}",
            report
                .warnings
                .iter()
                .map(|warning| format!("- {warning}"))
                .collect::<Vec<_>>()
                .join("\n")
        )));
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::baseline::{BaselinePosition, PersonalBaselineDeviation};
    use crate::domains::progress::{ChangepointResult, ProgressReport};

    #[test]
    fn render_progress_report_mentions_plateau_and_warnings() {
        let report = ProgressReport {
            plateau: ChangepointResult {
                supported: true,
                plateau_detected: true,
                plateau_start_date: Some("2026-01-29".into()),
                plateau_duration_days: Some(28),
                ..Default::default()
            },
            warnings: vec!["TID drift unavailable.".into()],
            ..Default::default()
        };

        let blocks = render_progress_report(&report, true, None);
        let markdown = format!("{:?}", blocks);
        assert!(markdown.contains("Plateau Detection"));
        assert!(markdown.contains("TID drift unavailable"));
        assert!(markdown.contains("lnRMSSD 7-day Rollup"));
        assert!(markdown.contains("Sample count"));
    }

    #[test]
    fn render_progress_report_shows_personal_baselines() {
        let report = ProgressReport {
            plateau: ChangepointResult::unsupported(),
            hrv_personal_baseline: Some(PersonalBaselineDeviation {
                metric: "lnRMSSD".into(),
                unit: "ln(ms)".into(),
                recent_mean_7d: 4.0,
                baseline_mean_60d: 4.1,
                baseline_cv_60d: 0.05,
                swc: 0.025,
                lower_bound: 4.075,
                upper_bound: 4.125,
                recent_sample_count: 7,
                baseline_sample_count: 40,
                baseline_span_days: 55,
                position: BaselinePosition::Within,
                model: "log-normal SWC".into(),
            }),
            resting_hr_personal_baseline: Some(PersonalBaselineDeviation {
                metric: "Resting HR".into(),
                unit: "bpm".into(),
                recent_mean_7d: 54.0,
                baseline_mean_60d: 55.0,
                baseline_cv_60d: 0.03,
                swc: 0.825,
                lower_bound: 54.175,
                upper_bound: 55.825,
                recent_sample_count: 7,
                baseline_sample_count: 40,
                baseline_span_days: 55,
                position: BaselinePosition::Within,
                model: "raw SWC".into(),
            }),
            ..Default::default()
        };

        let blocks = render_progress_report(&report, false, None);
        let markdown = format!("{:?}", blocks);
        assert!(markdown.contains("Personal Baselines"));
        assert!(markdown.contains("lnRMSSD"));
        assert!(markdown.contains("Resting HR"));
        assert!(markdown.contains("4.0"));
        assert!(markdown.contains("54.0"));
        assert!(markdown.contains("SWC"));
        assert!(markdown.contains("not a readiness verdict"));
    }
}
