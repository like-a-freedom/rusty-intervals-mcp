use super::*;
use crate::content::ContentBlock;
use crate::domains::interval_segment::{SegmentSeriesReport, SportPresentation};
use crate::engines::analysis::WorkoutGrade;
use crate::engines::trail_execution::TerrainContext;
use intervals_icu_client::ActivitySummary;

pub(crate) fn single_no_activities_blocks(
    date: &str,
    desc_filter: Option<&str>,
) -> Vec<ContentBlock> {
    let mut content = Vec::new();
    content.push(ContentBlock::markdown(format!(
        "# Analysis: {}\nStatus: No activities found",
        date
    )));

    let mut summary = vec![
        format!("  No training activities recorded for {}", date),
        "  This could be a rest day or activities haven't been synced yet".into(),
    ];
    if let Some(d) = desc_filter {
        summary.push(format!("  Search filter: '{}'", d));
    }
    content.push(ContentBlock::markdown(summary.join("\n")));
    content
}

pub(crate) fn single_multiple_activities_blocks(
    date: &str,
    desc_filter: Option<&str>,
    matching: &[&ActivitySummary],
) -> Vec<ContentBlock> {
    let mut content = Vec::new();
    content.push(ContentBlock::markdown(format!(
        "# Analysis: {}\nStatus: Multiple activities found",
        date
    )));

    let mut summary = vec![format!(
        "  Found {} activities for {}",
        matching.len(),
        date
    )];
    if let Some(d) = desc_filter {
        summary.push(format!(
            "  Search filter: '{}' matched {} activities",
            d,
            matching.len()
        ));
    }
    summary.push("  Please be more specific with your search.".into());
    content.push(ContentBlock::markdown(summary.join("\n")));

    let mut activities_list = vec!["Found activities:".into()];
    for (i, a) in matching.iter().enumerate() {
        activities_list.push(format!(
            "{}. {} (ID: {})",
            i + 1,
            a.name.as_deref().unwrap_or("Unknown"),
            a.id
        ));
    }
    content.push(ContentBlock::markdown(activities_list.join("\n")));

    let mut retry_examples = vec!["To analyze a specific activity, retry with:".into()];
    for (i, a) in matching.iter().enumerate() {
        let name = a.name.as_deref().unwrap_or("Unknown");
        let id = &a.id;
        let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();

        retry_examples.push(format!(
            "{}. {} → `description_contains: \"{}\"` or ID: `{}`",
            i + 1,
            name,
            key_phrase,
            id
        ));
    }
    content.push(ContentBlock::markdown(retry_examples.join("\n")));
    content
}

pub(crate) fn single_header_block(
    activity_name: &str,
    date: &str,
    activity_id: &str,
    analysis_type: &str,
) -> ContentBlock {
    ContentBlock::markdown(format!(
        "# Analysis: {}\nDate: {}\nID: {}\nType: {}",
        activity_name, date, activity_id, analysis_type
    ))
}

pub(crate) fn workout_grade_block(grade: &WorkoutGrade) -> ContentBlock {
    ContentBlock::markdown(format!("Workout Grade: {:?}", grade))
}

pub(crate) fn insights_block(insights: &[String]) -> Option<ContentBlock> {
    if insights.is_empty() {
        return None;
    }
    let mut insight_lines = vec!["Insights".to_string()];
    for insight in insights {
        insight_lines.push(format!("  {}", insight));
    }
    Some(ContentBlock::markdown(insight_lines.join("\n")))
}

pub(crate) fn basic_metrics_table_block(rows: Vec<Vec<String>>) -> ContentBlock {
    ContentBlock::table(vec!["Metric".into(), "Value".into()], rows)
}

pub(crate) fn requested_single_metrics_blocks(rows: Vec<Vec<String>>) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("Requested Metrics".to_string()),
        ContentBlock::table(vec!["Metric".into(), "Value".into(), "Status".into()], rows),
    ]
}

pub(crate) fn detailed_breakdown_blocks(rows: Vec<Vec<String>>) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("Detailed Breakdown".to_string()),
        ContentBlock::table(vec!["Metric".into(), "Value".into()], rows),
    ]
}

pub(crate) fn workout_comments_blocks(rows: Vec<Vec<String>>) -> Vec<ContentBlock> {
    vec![
        ContentBlock::markdown("Workout Comments".to_string()),
        ContentBlock::table(
            vec![
                "When".into(),
                "Author".into(),
                "Type".into(),
                "Comment".into(),
            ],
            rows,
        ),
    ]
}

pub(crate) fn execution_context_block(lines: &[String]) -> ContentBlock {
    ContentBlock::markdown(format!("Execution Context\n  {}", lines.join("\n  ")))
}

pub(crate) fn stream_metrics_block(stream_metrics: &[String]) -> Option<ContentBlock> {
    if stream_metrics.is_empty() {
        return None;
    }
    Some(ContentBlock::markdown(format!(
        "Stream Metrics\n  {}",
        stream_metrics.join("\n  ")
    )))
}

pub(crate) fn append_structured_interval_header(
    content: &mut Vec<ContentBlock>,
    work_count: usize,
    recovery_count: usize,
    work_duration_s: f64,
    mean_work_intensity: f64,
    confidence: f64,
) {
    content.push(ContentBlock::markdown(format!(
        "Interval Analysis\n  Structured session with {} detected work intervals, {} recoveries, {:.0}s work, mean intensity {:.2}, confidence {:.0}%.",
        work_count, recovery_count, work_duration_s, mean_work_intensity, confidence * 100.0,
    )));
}

pub(crate) fn append_fartlek_interval_header(content: &mut Vec<ContentBlock>, rationale: &str) {
    content.push(ContentBlock::markdown(format!(
        "Interval Analysis\n  Session looks like fartlek / non-structured; no structured work count claimed ({rationale})."
    )));
}

pub(crate) fn append_other_interval_header(content: &mut Vec<ContentBlock>, rationale: &str) {
    content.push(ContentBlock::markdown(format!(
        "Interval Analysis\n  Session has intensity but is not structured; no structured work count claimed ({rationale})."
    )));
}

pub(crate) fn append_insufficient_interval_header(
    content: &mut Vec<ContentBlock>,
    rationale: &str,
) {
    content.push(ContentBlock::markdown(format!(
        "Interval Analysis\n  Stream data insufficient for local interval detection ({rationale})."
    )));
}

pub(crate) fn append_upstream_fallback_warning(content: &mut Vec<ContentBlock>) {
    content.push(ContentBlock::markdown(
        "  Warning: upstream interval endpoint unavailable; Local detection completed as fallback."
            .to_string(),
    ));
}

pub(crate) fn append_upstream_reference_section(
    content: &mut Vec<ContentBlock>,
    output_header: &str,
    interval_rows: Vec<Vec<String>>,
) {
    content.push(ContentBlock::markdown(
        "\nInterval Analysis\nUpstream Interval Reference:".to_string(),
    ));
    content.push(ContentBlock::table(
        vec![
            "Rep".into(),
            "Duration".into(),
            "Avg HR".into(),
            output_header.into(),
        ],
        interval_rows,
    ));
}

pub(crate) fn append_interval_unavailable(content: &mut Vec<ContentBlock>) {
    content.push(ContentBlock::markdown(
        "Interval Analysis\n  Interval detection unavailable: stream data not available for local detection."
            .to_string(),
    ));
}

pub(crate) fn append_segment_report(
    content: &mut Vec<ContentBlock>,
    report: &SegmentSeriesReport,
    presentation: SportPresentation,
) {
    content.push(ContentBlock::markdown(
        "Metrics require ≥80% time coverage per signal; lower-coverage values are shown as n/a.",
    ));
    for table in build_segment_tables(report, presentation) {
        content.push(ContentBlock::markdown(table.title));
        content.push(ContentBlock::table(table.headers, table.rows));
    }

    if let Some(ref cons) = report.consistency {
        let cons_rows = build_consistency_rows(cons, presentation);
        if !cons_rows.is_empty() {
            content.push(ContentBlock::markdown("Repeat Consistency".to_string()));
            content.push(ContentBlock::table(
                vec!["Metric".to_string(), "Value".to_string()],
                cons_rows,
            ));
        }
    }
}

pub(crate) fn power_histogram_unavailable_block() -> ContentBlock {
    ContentBlock::markdown(
        "\nPower Histogram\n  Power histogram unavailable - this workout may not have power meter data."
            .to_string(),
    )
}

pub(crate) fn quality_findings_block(findings: &[String]) -> Option<ContentBlock> {
    if findings.is_empty() {
        return None;
    }
    Some(ContentBlock::markdown(format!(
        "Quality Findings\n  {}",
        findings.join("\n  ")
    )))
}

pub(crate) fn terrain_context_block(terrain: &TerrainContext) -> Option<ContentBlock> {
    if !terrain.supported {
        return None;
    }
    let mut t_lines = vec!["Terrain Context".to_string()];
    if let Some(ti) = terrain.terrain_index {
        t_lines.push(format!("  Terrain Index: {:.0} m/km", ti));
    }
    if let Some(vam) = terrain.vam {
        t_lines.push(format!("  VAM: {:.0} m/h", vam));
    }
    if terrain.terrain_induced {
        t_lines.push("  Efficiency drift: terrain-induced".into());
    }
    Some(ContentBlock::markdown(t_lines.join("\n")))
}

pub(crate) fn nutrition_context_block(carb: f64, protein: f64) -> ContentBlock {
    ContentBlock::markdown(format!(
        "Nutrition Context\n  Carb demand: {:.1} g/kg\n  Protein demand: {:.1} g/kg",
        carb, protein
    ))
}

pub(crate) fn curve_profile_block(
    profile: &crate::engines::adaptation::CurveProfile,
) -> ContentBlock {
    ContentBlock::markdown(format!("Power/Running Profile\n  Type: {:?}", profile))
}
