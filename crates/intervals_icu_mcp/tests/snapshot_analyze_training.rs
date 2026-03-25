//! Snapshot test for analyze_training output.
//! Captures output shape for regression detection.

use intervals_icu_mcp::intents::{ContentBlock, IntentOutput};

fn sample_single_activity_output() -> IntentOutput {
    IntentOutput::new(vec![
        ContentBlock::markdown("# Analysis: 2026-03-20\nDate: 2026-03-20\nID: 12345\nType: Run"),
        ContentBlock::table(
            vec!["Metric".into(), "Value".into()],
            vec![
                vec!["Distance".into(), "10.50 km".into()],
                vec!["Duration".into(), "0:52:30".into()],
                vec!["Avg HR".into(), "155 bpm".into()],
            ],
        ),
        ContentBlock::markdown("Requested Metrics"),
        ContentBlock::table(
            vec!["Metric".into(), "Value".into(), "Status".into()],
            vec![
                vec!["TIME".into(), "0:52:30".into(), "available".into()],
                vec!["DISTANCE".into(), "10.50 km".into(), "available".into()],
            ],
        ),
        ContentBlock::markdown("Quality Findings"),
        ContentBlock::markdown("  Average power tracked at 220 W."),
        ContentBlock::markdown("Interval Analysis\nDetected Intervals: 6"),
        ContentBlock::table(
            vec!["#".into(), "Duration".into(), "HR".into(), "Power".into()],
            vec![
                vec!["1".into(), "4:00".into(), "165 bpm".into(), "280 W".into()],
                vec!["2".into(), "3:58".into(), "168 bpm".into(), "285 W".into()],
            ],
        ),
    ])
    .with_suggestions(vec!["Strong intervals — power held above target.".into()])
    .with_next_actions(vec![
        "To compare with previous week: compare_periods".into(),
    ])
}

#[test]
fn snapshot_analyze_training_single_activity() {
    let output = sample_single_activity_output();
    let json = serde_json::to_string_pretty(&output).unwrap();

    eprintln!("=== ANALYZE TRAINING SNAPSHOT ===\n{}\n=== END ===", json);

    assert_eq!(output.content.len(), 8);
    assert!(matches!(&output.content[0], ContentBlock::Markdown { .. }));
    assert!(matches!(&output.content[1], ContentBlock::Table { .. }));
    assert_eq!(output.suggestions.len(), 1);
    assert_eq!(output.next_actions.len(), 1);

    if let ContentBlock::Markdown { markdown } = &output.content[0] {
        assert!(markdown.contains("Analysis"));
        assert!(markdown.contains("2026-03-20"));
        assert!(markdown.contains("12345"));
    }
}
