use crate::domains::progress::ProgressReport;
use crate::intents::ContentBlock;

pub(crate) fn render_progress_report(
    report: &ProgressReport,
    hypothesis_mode: bool,
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
        "## Progress Tracking Report\n\n### Plateau Detection\n{}\n\n### Load Context\n- ACWR: {}\n- Monotony: {}\n- Strain: {}\n\n### HRV Context\n- HRV ratio: {}\n- HRV trend: {}\n- HRV suppressed: {}\n\n### TID Drift\n- Supported: {}\n- Drift state: {:?}\n- Dominant zone: {}",
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
        report.tid_drift.supported,
        report.tid_drift.drift_state,
        report.tid_drift.dominant_zone.map(|value| value.to_string()).unwrap_or_else(|| "unavailable".into()),
    )));

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

        let blocks = render_progress_report(&report, true);
        let markdown = format!("{:?}", blocks);
        assert!(markdown.contains("Plateau Detection"));
        assert!(markdown.contains("TID drift unavailable"));
    }
}
