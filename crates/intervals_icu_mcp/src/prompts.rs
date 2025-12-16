use rmcp::model::{GetPromptResult, PromptMessage, PromptMessageRole};

pub fn analyze_recent_training_prompt(days_back: u32) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!(
            "Training analysis over the past {} days",
            days_back
        )),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Analyze my Intervals.icu training over the past {} days.\n\nFocus on:\n1. Training volume (distance, time, elevation, training load)\n2. Training distribution by activity type\n3. Fitness trends (CTL/ATL/TSB)\n4. Recovery metrics (HRV, sleep, wellness)\n5. Key insights and recommendations\n\nUse get_recent_activities with days_back={}, get_fitness_summary for CTL/ATL/TSB analysis, and get_wellness_data to assess recovery. Present findings in a clear, actionable format.",
                days_back, days_back
            ),
        )],
    }
}

pub fn performance_analysis_prompt(metric: &str, days_back: u32) -> GetPromptResult {
    let metric_lower = metric.to_ascii_lowercase();
    let body = match metric_lower.as_str() {
        "power" | "ride" | "cycling" => {
            "Include:\n1. Power curve with best efforts (5s, 1m, 5m, 20m, 1h)\n2. Estimated FTP from 20-minute power\n3. Power zones and training recommendations\n4. Trends and recent improvements\n\nUse get_power_curves to get the data, then provide detailed analysis with training suggestions."
        }
        "hr" | "heart rate" | "heart_rate" => {
            "Include:\n1. HR curve with best efforts across durations\n2. Max HR and FTHR estimation\n3. HR zones based on max HR\n4. Cardiac fitness trends\n\nUse get_hr_curves to get HR curve data, then provide detailed analysis with zone recommendations."
        }
        _ => {
            "Include:\n1. Best pace efforts across distances\n2. Threshold pace estimation from curve\n3. Pace zones for different training intensities\n4. Recent running trends\n\nUse get_pace_curves to get pace curve data (optionally with GAP for trail running), then provide detailed analysis with training recommendations."
        }
    };

    GetPromptResult {
        description: Some(format!("{} focus over the past {} days", metric, days_back)),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!("{}\n\nAnalyze the last {} days.", body, days_back),
        )],
    }
}

pub fn activity_deep_dive_prompt(activity_id: &str) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!(
            "Comprehensive analysis of activity {}",
            activity_id
        )),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Provide a comprehensive analysis of activity {}.\n\nInclude:\n1. Basic metrics (distance, time, pace/speed, elevation)\n2. Power and heart rate data (if available)\n3. Training load and intensity\n4. Interval structure and workout compliance (if structured)\n5. Best efforts found in this activity\n6. Subjective metrics (feel, RPE)\n7. Performance insights and comparison to recent activities\n\nUse get_activity_details for basic info, get_activity_intervals for workout structure, get_best_efforts for peak performances, and optionally get_activity_streams for time-series visualization. Compare with similar recent activities to provide context.",
                activity_id
            ),
        )],
    }
}

pub fn recovery_check_prompt(days_back: u32) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!(
            "Recovery assessment for the last {} days",
            days_back
        )),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Assess my current recovery status and readiness for training.\n\nInclude:\n1. Recent wellness metrics (HRV, resting HR, sleep quality)\n2. Training stress balance (TSB, CTL/ATL)\n3. Subjective metrics (fatigue, soreness, mood)\n4. Recovery trends over past week\n5. Training recommendations\n\nUse get_wellness_data for recent wellness, get_fitness_summary for TSB analysis, then provide clear guidance on training intensity. Review the last {} days for context.",
                days_back
            ),
        )],
    }
}

pub fn training_plan_review_prompt(start_date: &str) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!("Review training plan starting from {}", start_date)),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Review my upcoming training plan and provide feedback starting {}.\n\nInclude:\n1. Upcoming workouts from calendar\n2. Planned training load vs current fitness\n3. Recovery days and intensity distribution\n4. Workout library structure (if using a training plan)\n5. Recommendations for adjustments\n\nUse get_upcoming_workouts to see the plan, get_fitness_summary for current form, and optionally get_workout_library to see available training plans, then evaluate if the plan is appropriate and suggest any modifications.",
                start_date
            ),
        )],
    }
}

pub fn plan_training_week_prompt(start_date: &str, focus: &str) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!(
            "Plan training week from {} with '{}' focus",
            start_date, focus
        )),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Help me plan my training week with a '{}' focus starting {}.\n\nSteps:\n1. Check current fitness status (CTL/ATL/TSB) using get_fitness_summary\n2. Review recent training load and patterns with get_recent_activities\n3. Check recovery markers with get_wellness_data\n4. Review workout library for appropriate sessions with get_workout_library\n5. Create planned workouts for the week using create_event\n\nProvide a structured weekly plan with:\n- Workout types and intensities for each day\n- Recovery days placement\n- Expected weekly training load\n- Reasoning for the schedule based on current form\n\nThen offer to create the events in my calendar if I approve the plan.",
                focus, start_date
            ),
        )],
    }
}

pub fn analyze_and_adapt_plan_prompt(period_label: &str, focus: &str) -> GetPromptResult {
    GetPromptResult {
        description: Some(format!(
            "Analyze recent training and adapt plan ({} focus)",
            focus
        )),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!(
                "Analyze my recent training over {} and adapt my training plan accordingly.\n\nDo the following:\n1. Summarize recent training load and distribution (use get_recent_activities with appropriate days_back, get_fitness_summary for CTL/ATL/TSB).\n2. Check recovery markers and readiness (get_wellness_data).\n3. Inspect current/upcoming plan (get_upcoming_workouts or get_events) and see if any plan is already applied.\n4. Compare planned vs actual load; highlight gaps or overreach.\n5. Suggest concrete adjustments (swap/add/remove workouts, tweak intensity/volume) to align plan with actuals and focus on '{}'.\n6. Provide a short rationale and optional calendar changes (create_event / update_event) if needed.\n\nPrefer concise, actionable steps and ensure adjustments respect recovery and progression.",
                period_label, focus
            ),
        )],
    }
}
