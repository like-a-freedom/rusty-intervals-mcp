use intervals_icu_mcp::prompts::{
    activity_deep_dive_prompt, analyze_and_adapt_plan_prompt, analyze_recent_training_prompt,
    performance_analysis_prompt, plan_training_week_prompt, recovery_check_prompt,
    training_plan_review_prompt,
};
use rmcp::model::{PromptMessageContent, PromptMessageRole};

fn prompt_text(result: &rmcp::model::GetPromptResult) -> &str {
    assert_eq!(
        result.messages.len(),
        1,
        "helper prompts should emit one user message"
    );
    let message = &result.messages[0];
    assert_eq!(message.role, PromptMessageRole::User);

    match &message.content {
        PromptMessageContent::Text { text } => text,
        other => panic!("expected text prompt, got {other:?}"),
    }
}

#[test]
fn analyze_recent_training_prompt_mentions_expected_tools() {
    let result = analyze_recent_training_prompt(14);
    let text = prompt_text(&result);

    assert_eq!(
        result.description.as_deref(),
        Some("Training analysis: 14 days")
    );
    assert!(text.contains("Analyze training over 14 days"));
    assert!(text.contains("get_recent_activities(days_back=14)"));
    assert!(text.contains("get_fitness_summary"));
    assert!(text.contains("get_wellness"));
}

#[test]
fn performance_analysis_prompt_selects_metric_specific_tools() {
    let power = performance_analysis_prompt("Power", 30);
    let hr = performance_analysis_prompt("heart rate", 21);
    let pace = performance_analysis_prompt("run", 10);

    assert_eq!(
        power.description.as_deref(),
        Some("Power analysis: 30 days")
    );
    assert!(prompt_text(&power).contains("Use get_power_curves"));
    assert!(prompt_text(&hr).contains("Use get_hr_curves"));
    assert!(prompt_text(&pace).contains("Use get_pace_curves"));
}

#[test]
fn activity_and_recovery_prompts_reference_expected_context() {
    let activity = activity_deep_dive_prompt("act-42");
    let recovery = recovery_check_prompt(7);

    assert_eq!(
        activity.description.as_deref(),
        Some("Activity act-42 analysis")
    );
    assert!(prompt_text(&activity).contains("Analyze activity act-42"));
    assert!(prompt_text(&activity).contains("get_activity_details"));
    assert!(prompt_text(&activity).contains("get_activity_intervals"));
    assert!(prompt_text(&activity).contains("get_best_efforts"));

    assert_eq!(
        recovery.description.as_deref(),
        Some("Recovery check: 7 days")
    );
    assert!(prompt_text(&recovery).contains("Assess recovery over 7 days"));
    assert!(prompt_text(&recovery).contains("get_wellness"));
    assert!(prompt_text(&recovery).contains("get_fitness_summary"));
}

#[test]
fn planning_prompts_reference_planning_tools_and_focus() {
    let review = training_plan_review_prompt("2026-03-10");
    let week = plan_training_week_prompt("2026-03-10", "threshold");
    let adapt = analyze_and_adapt_plan_prompt("last 4 weeks", "race prep");

    assert_eq!(
        review.description.as_deref(),
        Some("Plan review from 2026-03-10")
    );
    assert!(prompt_text(&review).contains("Review training plan from 2026-03-10"));
    assert!(prompt_text(&review).contains("get_upcoming_workouts"));

    assert_eq!(
        week.description.as_deref(),
        Some("Plan week 2026-03-10 (threshold)")
    );
    assert!(prompt_text(&week).contains("'threshold' focus"));
    assert!(prompt_text(&week).contains("create_event"));

    assert_eq!(
        adapt.description.as_deref(),
        Some("Adapt plan (race prep focus)")
    );
    assert!(prompt_text(&adapt).contains("Analyze last 4 weeks"));
    assert!(prompt_text(&adapt).contains("'race prep'"));
    assert!(prompt_text(&adapt).contains("get_events"));
    assert!(prompt_text(&adapt).contains("create_event/update_event"));
}
