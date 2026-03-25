//! Baseline token measurement for intent outputs.
//! Run BEFORE compact markdown changes to establish baseline.

use intervals_icu_mcp::intents::{ContentBlock, IntentOutput, OutputMetadata};

fn serialized_chars(output: &IntentOutput) -> usize {
    serde_json::to_string(output).unwrap().len()
}

#[test]
fn baseline_assess_recovery() {
    let output = IntentOutput::new(vec![
        ContentBlock::markdown(
            "## Recovery Assessment (01 Jan - 07 Jan)\n\n**Readiness for:** interval",
        ),
        ContentBlock::table(
            vec!["Metric".into(), "Value".into(), "Status".into()],
            vec![
                vec!["HRV".into(), "65 ms".into(), "✅ Fresh".into()],
                vec!["Resting HR".into(), "48 bpm".into(), "✅ Normal".into()],
            ],
        ),
        ContentBlock::markdown(
            "**Red Flags:** None detected ✅\n\n**Recommendation:** Ready for key workout",
        ),
        ContentBlock::markdown(
            "### Activity-Specific Readiness\n\n- **GO** for interval work.\n- HRV trending above baseline.",
        ),
    ])
    .with_suggestions(vec![
        "Good recovery window for intensity.".into(),
        "Consider adding a tempo block.".into(),
    ])
    .with_next_actions(vec!["To analyze today's workout: analyze_training".into()]);

    let chars = serialized_chars(&output);
    // chars/4 is a proxy metric, not exact token count.
    // Markdown symbols (##, **, \n\n) tokenize differently: "##" = 1 token, "**" = 1 token.
    // Real tokens ≈ chars/3.5 for verbose markdown, chars/4 for compact text.
    // This proxy is sufficient for relative before/after comparison.
    eprintln!(
        "assess_recovery baseline: {} chars (~{} tokens)",
        chars,
        chars / 4
    );
    assert!(chars > 0);
}

#[test]
fn baseline_analyze_training_single_activity() {
    let output = IntentOutput::new(vec![
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
                vec!["PACE".into(), "5:00 /km".into(), "available".into()],
            ],
        ),
        ContentBlock::markdown("Quality Findings"),
        ContentBlock::markdown("  Average power tracked at 220 W."),
    ])
    .with_suggestions(vec!["Consistent pacing throughout.".into()]);

    let chars = serialized_chars(&output);
    eprintln!(
        "analyze_training single baseline: {} chars (~{} tokens)",
        chars,
        chars / 4
    );
    assert!(chars > 0);
}

#[test]
fn baseline_plan_training() {
    let output = IntentOutput::new(vec![
        ContentBlock::markdown(
            "## Training Plan: Spring Build\n\n**Athlete:** John\n**Period:** 2026-03-25 to 2026-04-25\n**Focus:** threshold\n**Max Hours/Week:** 10",
        ),
        ContentBlock::markdown("### Race Anchors"),
        ContentBlock::table(
            vec!["Date".into(), "Race".into(), "Priority".into()],
            vec![vec!["2026-04-20".into(), "Marathon".into(), "A".into()]],
        ),
        ContentBlock::markdown("### Structure"),
        ContentBlock::markdown(
            "- Monday: REST\n- Tuesday: Easy Run 45min\n- Wednesday: Intervals 6x1000m\n- Thursday: Recovery Run 30min\n- Friday: REST\n- Saturday: Long Run 90min\n- Sunday: Easy Ride 60min",
        ),
    ])
    .with_suggestions(vec!["Build phase starts next week.".into()])
    .with_metadata(OutputMetadata {
        events_created: Some(7),
        ..Default::default()
    });

    let chars = serialized_chars(&output);
    eprintln!(
        "plan_training baseline: {} chars (~{} tokens)",
        chars,
        chars / 4
    );
    assert!(chars > 0);
}

#[test]
fn baseline_manage_profile() {
    let output = IntentOutput::new(vec![
        ContentBlock::markdown("## Athlete Profile"),
        ContentBlock::markdown("### Overview"),
        ContentBlock::table(
            vec!["Parameter".into(), "Value".into()],
            vec![
                vec!["Name".into(), "John Doe".into()],
                vec!["Weight".into(), "75 kg".into()],
            ],
        ),
        ContentBlock::markdown("### Thresholds (run)"),
        ContentBlock::table(
            vec!["Type".into(), "Value".into()],
            vec![
                vec!["FTP".into(), "280 W".into()],
                vec!["LTHR".into(), "172 bpm".into()],
            ],
        ),
    ]);

    let chars = serialized_chars(&output);
    eprintln!(
        "manage_profile baseline: {} chars (~{} tokens)",
        chars,
        chars / 4
    );
    assert!(chars > 0);
}
