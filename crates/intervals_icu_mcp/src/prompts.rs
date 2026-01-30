use rmcp::model::{GetPromptResult, PromptMessage, PromptMessageRole};

pub fn analyze_recent_training_prompt(days_back: u32) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!("Training analysis: {} days", days_back)),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Analyze training over {} days: volume, load distribution, CTL/ATL/TSB trends, recovery. Use get_recent_activities(days_back={}), get_fitness_summary, get_wellness. Give actionable insights.",
                days_back, days_back
            ),
        )],
    }
}

pub fn performance_analysis_prompt(metric: &str, days_back: u32) -> GetPromptResult {
    let metric_lower = metric.to_ascii_lowercase();
    let tool_hint = match metric_lower.as_str() {
        "power" | "ride" | "cycling" => "get_power_curves",
        "hr" | "heart rate" | "heart_rate" => "get_hr_curves",
        _ => "get_pace_curves",
    };

    GetPromptResult {
        description: Some(format!("{} analysis: {} days", metric, days_back)),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Analyze {} performance over {} days: best efforts, threshold estimation, zones, trends. Use {}. Provide training recommendations.",
                metric, days_back, tool_hint
            ),
        )],
    }
}

pub fn activity_deep_dive_prompt(activity_id: &str) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!("Activity {} analysis", activity_id)),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Analyze activity {}: metrics (distance, time, power, HR), intervals, best efforts, load. Use get_activity_details, get_activity_intervals, get_best_efforts. Compare with recent activities.",
                activity_id
            ),
        )],
    }
}

pub fn recovery_check_prompt(days_back: u32) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!("Recovery check: {} days", days_back)),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Assess recovery over {} days: wellness (HRV, resting HR, sleep), TSB, fatigue. Use get_wellness, get_fitness_summary. Recommend training intensity.",
                days_back
            ),
        )],
    }
}

pub fn training_plan_review_prompt(start_date: &str) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!("Plan review from {}", start_date)),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Review training plan from {}: upcoming workouts, planned vs current fitness, recovery balance. Use get_upcoming_workouts, get_fitness_summary. Suggest adjustments.",
                start_date
            ),
        )],
    }
}

pub fn plan_training_week_prompt(start_date: &str, focus: &str) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!("Plan week {} ({})", start_date, focus)),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Plan training week from {} with '{}' focus: check fitness (get_fitness_summary), recent load (get_recent_activities), recovery (get_wellness). Create daily schedule with workout types, intensities, recovery days. Use create_event to add approved workouts.",
                start_date, focus
            ),
        )],
    }
}

pub fn analyze_and_adapt_plan_prompt(period_label: &str, focus: &str) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!("Adapt plan ({} focus)", focus)),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Analyze {} and adapt plan for '{}': compare planned vs actual load (get_recent_activities, get_fitness_summary, get_wellness, get_events). Highlight gaps, suggest adjustments. Use create_event/update_event for changes.",
                period_label, focus
            ),
        )],
    }
}
