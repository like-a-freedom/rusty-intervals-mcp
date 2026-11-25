mod analyze_compare;
mod analyze_period;
mod analyze_single;
pub(crate) mod presentation;
pub mod progress;

pub(crate) fn analyze_report(
    report: crate::engines::analyze_training::AnalyzeReport,
) -> crate::intents::IntentOutput {
    use self::presentation as render;
    use crate::engines::analyze_training::AnalyzeReport;
    use crate::intents::IntentOutput;
    match report {
        AnalyzeReport::Single(report) => analyze_single::render_single(report),
        AnalyzeReport::Period(report) => analyze_period::render_period(report),
        AnalyzeReport::Comparison(report) => analyze_compare::render_comparison(report),
        AnalyzeReport::SingleEmpty { date, filter } => IntentOutput::new(render::single_no_activities_blocks(&date, filter.as_deref()))
            .with_suggestions(vec![
                "Check if activities are synced from your fitness device".into(),
                "Verify the date - did you train on this day?".into(),
                "Try expanding the date range to include nearby days".into(),
            ])
            .with_next_actions(vec![
                "To view recent activities: analyze_training with target_type: period and wider date range".into(),
                "To check athlete profile: manage_profile with action: get".into(),
            ]),
        AnalyzeReport::SingleMultiple { date, filter, activities } => {
            let matching: Vec<_> = activities.iter().collect();
            let content = render::single_multiple_activities_blocks(&date, filter.as_deref(), &matching);
            let first_key_phrase = matching[0].name.as_deref().unwrap_or("Workout")
                .split(['-', '—', ':']).next().unwrap_or("Workout").trim();
            let mut next_actions = vec![
                format!("Retry with `description_contains` from the list above (e.g., `description_contains: \"{}\"`)", first_key_phrase),
                "Use `analyze_training` with `target_type: period` to see all activities".into(),
            ];
            if matching.len() <= 3 {
                next_actions.push(format!("Or specify activity ID directly if your MCP client supports it (e.g., `{}`)", matching[0].id));
            }
            IntentOutput::new(content).with_suggestions(vec![
                "Choose one activity from the list and retry with its `description_contains` value".into(),
                "For interval analysis, look for keywords like 'tempo', 'threshold', 'intervals', 'repeats', 'VO2'".into(),
                "Note: Only workouts created with structured intervals will show interval data".into(),
            ]).with_next_actions(next_actions)
        }
        AnalyzeReport::PeriodEmpty { start, end, events } => {
            let events: Vec<_> = events.iter().collect();
            IntentOutput::new(render::period_no_activities_blocks(&start, &end, &events))
                .with_suggestions(vec![
                    "Verify the date range - did you train during this period?".into(),
                    "Try a wider date range to capture recent or upcoming workouts".into(),
                    "Use compare_periods to contrast with a nearby range that has activity".into(),
                ])
                .with_next_actions(vec![
                    "To analyze a different period: analyze_training with wider period_start/period_end".into(),
                    "To compare with another period: compare_periods".into(),
                ])
        }
    }
}
