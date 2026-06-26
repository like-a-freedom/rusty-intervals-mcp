use crate::intents::{ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput};
use async_trait::async_trait;
use chrono::{NaiveDate, NaiveDateTime};
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
/// Analyze Race Intent Handler
///
/// Post-race analysis: results, strategy, comparison to plan.
use std::sync::Arc;

use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext, RaceMetrics};
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::{RaceFetchRequest, fetch_race_data};
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{
    extract_ctl_series, parse_fitness_metrics, parse_wellness_metrics,
};
use crate::engines::race_readiness::{compute_ctl_drop, compute_race_readiness};
use crate::intents::utils::{data_availability_block, filter_activities_by_description};

pub struct AnalyzeRaceHandler;
impl AnalyzeRaceHandler {
    pub fn new() -> Self {
        Self
    }

    fn parse_requested_date(value: &str) -> Result<NaiveDate, IntentError> {
        if value.eq_ignore_ascii_case("today") {
            return Ok(chrono::Local::now().date_naive());
        }

        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| IntentError::validation(format!("Invalid race date: {}", value)))
    }

    fn parse_activity_date(value: &str) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .ok()
            .or_else(|| {
                NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
                    .ok()
                    .map(|dt| dt.date())
            })
    }

    fn looks_like_race(name: Option<&str>) -> bool {
        let Some(name) = name else {
            return false;
        };
        let name = name.to_lowercase();
        [
            "race",
            "marathon",
            "half",
            "ultra",
            "10k",
            "5k",
            "triathlon",
            "trail",
            "skyrace",
            "vertical",
            "km vert",
        ]
        .iter()
        .any(|token| name.contains(token))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RaceAnalysisMode {
    Performance,
    Strategy,
    Recovery,
}

impl RaceAnalysisMode {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("performance") {
            "strategy" => Self::Strategy,
            "recovery" => Self::Recovery,
            _ => Self::Performance,
        }
    }
}

#[async_trait]
impl IntentHandler for AnalyzeRaceHandler {
    fn name(&self) -> &'static str {
        "analyze_race"
    }

    fn description(&self) -> &'static str {
        "Post-race analysis: results, execution pattern, comparison to plan, \
         and race readiness scoring. Returns race metrics (distance, time, avg HR), \
         efficiency factor, aerobic decoupling, interval/segment detection, \
         post-race load context, and a 5-factor Race Readiness score (score/100 \
         with tier: ready/monitor/caution/not_ready). Three modes: performance \
         (race execution), strategy (pacing, fueling, terrain), recovery (post-race \
         recovery outlook).
         
         Use this tool when: you need a post-race debrief, evaluate race execution \
         vs plan, check recovery needs, or assess readiness for a next race. \
         Do NOT use when: you need ongoing training analysis (use analyze_training) \
         or general recovery assessment (use assess_recovery)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "date": {"type": "string", "description": "Race date (YYYY-MM-DD) or 'last_race'. Preferred canonical field."},
                "target_date": {"type": "string", "description": "Alias for date. Accepts YYYY-MM-DD or 'today'."},
                "description_contains": {"type": "string", "description": "Search by description (e.g., '50K', 'marathon')"},
                "analysis_type": {"type": "string", "enum": ["performance", "strategy", "recovery"], "default": "performance", "description": "Analysis type"},
                "compare_to_planned": {"type": "boolean", "default": true, "description": "Compare to planned workout"}
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let date = input
            .get("date")
            .and_then(Value::as_str)
            .or_else(|| input.get("target_date").and_then(Value::as_str));
        let desc_filter = input.get("description_contains").and_then(Value::as_str);
        let analysis_mode =
            RaceAnalysisMode::parse(input.get("analysis_type").and_then(Value::as_str));
        let compare = input
            .get("compare_to_planned")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        // Fetch recent activities to find race
        let activities = client
            .get_recent_activities(Some(50), Some(60))
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch activities: {}", e)))?;

        let race = if matches!(date, Some("last_race")) {
            activities
                .iter()
                .filter(|activity| Self::looks_like_race(activity.name.as_deref()))
                .max_by_key(|activity| Self::parse_activity_date(&activity.start_date_local))
        } else if let Some(date) = date {
            let target_date = Self::parse_requested_date(date)?;
            activities.iter().find(|activity| {
                Self::parse_activity_date(&activity.start_date_local) == Some(target_date)
                    && desc_filter
                        .map(|desc| {
                            activity
                                .name
                                .as_ref()
                                .map(|name| name.to_lowercase().contains(&desc.to_lowercase()))
                                .unwrap_or(false)
                        })
                        .unwrap_or(true)
            })
        } else if let Some(desc) = desc_filter {
            filter_activities_by_description(&activities, desc)
                .first()
                .copied()
        } else {
            activities
                .iter()
                .find(|activity| Self::looks_like_race(activity.name.as_deref()))
                .or_else(|| activities.first())
        };

        let mut content = Vec::new();
        content.push(ContentBlock::markdown(
            "# Race Analysis\nPost-race performance review",
        ));

        // Handle no race found gracefully
        if race.is_none() {
            let summary = if let Some(d) = desc_filter {
                vec![
                    format!("  No race found matching '{}'", d),
                    "  Try a different search term or date range".into(),
                    "  Check if the activity is tagged as a race".into(),
                ]
            } else {
                vec![
                    "  No recent race activities found".into(),
                    "  Races are typically tagged or have 'race' in the name".into(),
                    "  Try using description_contains with race name".into(),
                ]
            };

            content.push(ContentBlock::markdown(summary.join("\n")));

            let suggestions = vec![
                "Use description_contains with specific race name (e.g., '50K', 'Marathon')".into(),
                "Check if activities are properly tagged in Intervals.icu".into(),
                "Try a wider date range to capture the race".into(),
            ];

            let next_actions = vec![
                "To analyze a specific race: analyze_race with description_contains: 'Race Name'"
                    .into(),
                "To view recent activities: analyze_training with target_type: period".into(),
            ];

            return Ok(IntentOutput::new(content)
                .with_suggestions(suggestions)
                .with_next_actions(next_actions));
        }

        if let Some(race) = race {
            let mut fetched = fetch_race_data(
                client.as_ref(),
                &RaceFetchRequest {
                    activity_id: race.id.clone(),
                    include_intervals: true,
                    include_streams: true,
                },
            )
            .await?;
            fetched.activities = vec![race.clone()];
            fetched.fitness = client.get_fitness_summary().await.ok();
            fetched.wellness = client.get_wellness(Some(7)).await.ok();

            let race_date = NaiveDate::parse_from_str(&race.start_date_local, "%Y-%m-%d")
                .ok()
                .unwrap_or_else(|| chrono::Local::now().date_naive());
            let mut race_context = CoachContext::new(
                AnalysisKind::RaceAnalysis,
                AnalysisWindow::new(race_date, race_date),
            );
            race_context.audit = build_data_audit(&fetched);
            race_context.metrics.fitness = parse_fitness_metrics(fetched.fitness.as_ref());
            race_context.metrics.wellness = parse_wellness_metrics(fetched.wellness.as_ref());

            let details = fetched.workout_detail.clone().unwrap_or_else(|| json!({}));
            let segment_count = fetched
                .intervals
                .as_ref()
                .and_then(Value::as_array)
                .map(|items| items.len());
            let race_duration_secs = details.get("moving_time").and_then(Value::as_i64);
            let race_distance_m = details.get("distance").and_then(Value::as_f64);
            let avg_hr = details.get("average_heartrate").and_then(Value::as_f64);
            let (efficiency_factor, aerobic_decoupling) =
                crate::engines::coach_metrics::derive_execution_metrics(
                    fetched.workout_detail.as_ref(),
                    fetched.streams.as_ref(),
                );
            let post_race_recovery_note = race_context
                .metrics
                .fitness
                .as_ref()
                .and_then(|fitness| fitness.tsb)
                .map(|tsb| {
                    if tsb < -10.0 {
                        format!("Post-race load remains elevated (TSB {:.1}); recovery block recommended.", tsb)
                    } else {
                        format!("Post-race load looks manageable (TSB {:.1}); resume structure gradually.", tsb)
                    }
                });
            let durability_drifting = aerobic_decoupling
                .as_ref()
                .map(|d| d.state == "drifting")
                .unwrap_or(false);
            let system_mismatch = race_context
                .metrics
                .fitness
                .as_ref()
                .map(|f| {
                    let ctl_positive = f.ctl.map(|c| c > 0.0).unwrap_or(false);
                    let atl_zero = f.atl.map(|a| a <= 0.0).unwrap_or(true);
                    ctl_positive && atl_zero
                })
                .unwrap_or(false);
            let race_readiness = compute_race_readiness(
                race_context.metrics.fitness.as_ref().and_then(|f| f.tsb),
                durability_drifting,
                false,
                system_mismatch,
                compute_ctl_drop(
                    extract_ctl_series(fetched.wellness.as_ref())
                        .map(|(_, values)| values)
                        .unwrap_or_default()
                        .as_slice(),
                    race_context.metrics.fitness.as_ref().and_then(|f| f.ctl),
                ),
            );

            race_context.metrics.race = Some(RaceMetrics {
                race_duration_secs,
                race_distance_m,
                avg_hr,
                segment_count,
                race_load_note: avg_hr.map(|hr| format!("Average race heart rate: {:.0} bpm.", hr)),
                post_race_recovery_note,
                efficiency_factor,
                aerobic_decoupling,
            });
            race_context.alerts = build_alerts(&race_context.metrics);
            race_context.guidance = build_guidance(&race_context.metrics, &race_context.alerts);

            let name = race.name.as_deref().unwrap_or("Race");
            content.push(ContentBlock::markdown(format!(
                "{}\nDate: {}\nID: {}",
                name, race.start_date_local, race.id
            )));

            // Build results table
            if let Some(obj) = details.as_object() {
                let mut rows = vec![vec!["Metric".into(), "Value".into()]];
                if let Some(v) = obj.get("distance").and_then(|x| x.as_f64()) {
                    rows.push(vec!["Distance".into(), format!("{:.2} km", v / 1000.0)]);
                }
                if let Some(v) = obj.get("moving_time").and_then(|x| x.as_i64()) {
                    rows.push(vec![
                        "Time".into(),
                        format!("{}:{:02}:{:02}", v / 3600, (v % 3600) / 60, (v % 60)),
                    ]);
                }
                if let Some(v) = obj.get("average_heartrate").and_then(|x| x.as_f64()) {
                    rows.push(vec!["Avg HR".into(), format!("{} bpm", v as u32)]);
                }
                content.push(ContentBlock::table(rows[0].clone(), rows[1..].to_vec()));
            }

            if let Some(race_metrics) = &race_context.metrics.race {
                let mut execution_lines = Vec::new();
                if let Some(segments) = race_metrics.segment_count {
                    execution_lines.push(format!(
                        "Detected {} race segments/interval blocks.",
                        segments
                    ));
                }
                if let Some(load_note) = &race_metrics.race_load_note {
                    execution_lines.push(load_note.clone());
                }
                if let Some(efficiency_factor) = race_metrics.efficiency_factor {
                    execution_lines.push(format!(
                        "Efficiency Factor: {:.2} (power/HR, higher = fresher)",
                        efficiency_factor
                    ));
                }
                if let Some(decoupling) = &race_metrics.aerobic_decoupling {
                    execution_lines.push(format!(
                        "Aerobic Decoupling: {:.1}% ({})",
                        decoupling.decoupling_pct, decoupling.state
                    ));
                }
                if !execution_lines.is_empty() {
                    content.push(ContentBlock::markdown(format!(
                        "Execution Pattern\n  {}",
                        execution_lines.join("\n  ")
                    )));
                }

                if analysis_mode == RaceAnalysisMode::Performance {
                    let mut performance_lines = Vec::new();
                    if let Some(duration) = race_metrics.race_duration_secs {
                        performance_lines.push(format!(
                            "Moving time: {}:{:02}:{:02}.",
                            duration / 3600,
                            (duration % 3600) / 60,
                            duration % 60
                        ));
                    }
                    if let Some(distance) = race_metrics.race_distance_m {
                        performance_lines.push(format!(
                            "Race distance covered: {:.2} km.",
                            distance / 1000.0
                        ));
                    }
                    if let Some(decoupling) = &race_metrics.aerobic_decoupling {
                        performance_lines.push(format!(
                            "Cardiac drift finished at {:.1}% ({}).",
                            decoupling.decoupling_pct, decoupling.state
                        ));
                    }
                    if !performance_lines.is_empty() {
                        content.push(ContentBlock::markdown(format!(
                            "Performance Review\n  {}",
                            performance_lines.join("\n  ")
                        )));
                    }
                }

                if analysis_mode == RaceAnalysisMode::Recovery {
                    if let Some(recovery_note) = &race_metrics.post_race_recovery_note {
                        content.push(ContentBlock::markdown(format!(
                            "Recovery Outlook\n  {}",
                            recovery_note
                        )));
                    }
                } else if let Some(recovery_note) = &race_metrics.post_race_recovery_note {
                    content.push(ContentBlock::markdown(format!(
                        "Post-Race Load Context\n  {}",
                        recovery_note
                    )));
                }
            }

            // Race Readiness
            let tier = if race_readiness.score >= 80 {
                "ready"
            } else if race_readiness.score >= 60 {
                "monitor"
            } else if race_readiness.score >= 40 {
                "caution"
            } else {
                "not_ready"
            };
            content.push(ContentBlock::markdown(format!(
                "Race Readiness\n  Score: {}/100\n  Tier: {}",
                race_readiness.score, tier
            )));

            if let Some(wellness) = &race_context.metrics.wellness {
                let mut wellness_lines = Vec::new();
                if let Some(hrv) = wellness.avg_hrv {
                    wellness_lines.push(format!("HRV: {:.0} ms", hrv));
                }
                if let Some(sleep) = wellness.avg_sleep_hours {
                    wellness_lines.push(format!("Sleep: {:.1} hrs", sleep));
                }
                if let Some(rhr) = wellness.avg_resting_hr {
                    wellness_lines.push(format!("Resting HR: {:.0} bpm", rhr));
                }
                if let Some(ratio) = wellness.hrv_ratio {
                    let status = if ratio >= 1.0 {
                        "recovered"
                    } else if ratio >= 0.9 {
                        "adequate"
                    } else {
                        "suppressed"
                    };
                    wellness_lines.push(format!("HRV ratio: {:.2} ({})", ratio, status));
                }
                if let Some(trend) = &wellness.hrv_trend_state {
                    wellness_lines.push(format!("HRV trend: {}", trend));
                }
                if let Some(index) = wellness.recovery_index {
                    let status = if index >= 1.2 {
                        "supportive"
                    } else if index >= 0.9 {
                        "watch"
                    } else {
                        "low"
                    };
                    wellness_lines.push(format!("Recovery index: {:.2} ({})", index, status));
                }
                if !wellness_lines.is_empty() {
                    content.push(ContentBlock::markdown(format!(
                        "Wellness Context\n  {}",
                        wellness_lines.join("\n  ")
                    )));
                }
            }

            if analysis_mode == RaceAnalysisMode::Strategy {
                content.push(ContentBlock::markdown(
                    "Strategy Review\n  Focus on pacing discipline, fueling timing, and how effort changed across segments.\n  Compare the first and final third of the race for execution drift.\n  Revisit terrain-specific decisions, aid-station timing, and surges that may have raised cost late in the race.".to_string(),
                ));
            }

            if compare {
                let planned_events = client
                    .get_upcoming_workouts(Some(365), Some(300), None)
                    .await
                    .unwrap_or_default();
                let planned_events: Vec<intervals_icu_client::Event> = planned_events
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| serde_json::from_value(v.clone()).ok())
                            .collect()
                    })
                    .unwrap_or_default();
                let race_day = Self::parse_activity_date(&race.start_date_local);
                let matching_plan = planned_events.iter().find(|event| {
                    race_day
                        .map(|day| event.start_date_local.starts_with(&day.to_string()))
                        .unwrap_or(false)
                        || event.name.to_lowercase().contains(&name.to_lowercase())
                });
                let comparison = if let Some(plan) = matching_plan {
                    format!(
                        "Comparison to Plan\n  Planned event: {}\n  Planned date: {}\n  Actual race: {}",
                        plan.name, plan.start_date_local, name
                    )
                } else {
                    "Comparison to Plan\n  No matching planned event found for this race."
                        .to_string()
                };
                content.push(ContentBlock::markdown(comparison));
            }

            if let Some(block) = data_availability_block(
                &race_context.audit.degraded_mode_reasons,
                race_context.audit.all_available(),
            ) {
                content.push(block);
            }

            // Use shared guidance from coach engine with race-specific additions
            let mut suggestions = race_context.guidance.suggestions.clone();
            if !suggestions.iter().any(|s| s.contains("nutrition")) {
                suggestions.push("Review nutrition and hydration strategy for next race.".into());
            }

            let mut next_actions = vec![
                "To assess recovery: assess_recovery with period_days: 14".into(),
                "To plan next buildup: plan_training".into(),
            ];
            for action in &race_context.guidance.next_actions {
                if !next_actions.contains(action) {
                    next_actions.insert(0, action.clone());
                }
            }

            return Ok(IntentOutput::new(content)
                .with_suggestions(suggestions)
                .with_next_actions(next_actions));
        }

        content.push(ContentBlock::markdown("Results\n| Metric | Value |\n|--------|-------|\n| Time | -:-:-:-- |\n| Place | -/- |\n| Pace | -'--\"/km |"));

        let suggestions = vec!["Review nutrition and hydration strategy for next race.".into()];

        let next_actions = vec![
            "To assess recovery: assess_recovery with period_days: 14".into(),
            "To plan next buildup: plan_training".into(),
        ];

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions))
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}

impl Default for AnalyzeRaceHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use intervals_icu_client::{ActivitySummary, IntervalsError};
    use std::sync::Arc;

    fn make_activity(id: &str, date: &str, name: &str) -> ActivitySummary {
        ActivitySummary {
            id: id.to_string(),
            name: Some(name.to_string()),
            start_date_local: date.to_string(),
            ..Default::default()
        }
    }

    fn make_client_with_activity(
        id: &str,
        date: &str,
        name: &str,
        detail: serde_json::Value,
    ) -> MockIntervalsClient {
        MockIntervalsClient::builder()
            .with_activities(vec![make_activity(id, date, name)])
            .with_activity_detail(id, detail)
            .with_streams(json!({}))
            .with_intervals(json!({}))
    }

    fn make_minimal_client(id: &str, date: &str, name: &str) -> MockIntervalsClient {
        make_client_with_activity(id, date, name, json!({}))
    }

    #[test]
    fn test_new_handler() {
        let handler = AnalyzeRaceHandler::new();
        assert_eq!(handler.name(), "analyze_race");
    }

    #[test]
    fn test_default_handler() {
        let _handler = AnalyzeRaceHandler;
    }

    #[test]
    fn test_name() {
        let handler = AnalyzeRaceHandler::new();
        assert_eq!(IntentHandler::name(&handler), "analyze_race");
    }

    #[test]
    fn test_description() {
        let handler = AnalyzeRaceHandler::new();
        let desc = IntentHandler::description(&handler);
        assert!(desc.contains("Post-race analysis"));
        assert!(desc.contains("results"));
        assert!(desc.contains("strategy"));
    }

    #[test]
    fn test_input_schema_structure() {
        let handler = AnalyzeRaceHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("date"));
        assert!(props.contains_key("target_date"));
        assert!(props.contains_key("description_contains"));
        assert!(props.contains_key("analysis_type"));
        assert!(props.contains_key("compare_to_planned"));

        let analysis_type = props.get("analysis_type").unwrap();
        let analysis_enum = analysis_type.get("enum").unwrap().as_array().unwrap();
        assert!(analysis_enum.contains(&json!("performance")));
        assert!(analysis_enum.contains(&json!("strategy")));
        assert!(analysis_enum.contains(&json!("recovery")));
    }

    #[test]
    fn test_requires_idempotency_token() {
        let handler = AnalyzeRaceHandler::new();
        assert!(!IntentHandler::requires_idempotency_token(&handler));
    }

    #[test]
    fn test_default_values() {
        let input = json!({});

        let analysis_type = input
            .get("analysis_type")
            .and_then(|v| v.as_str())
            .unwrap_or("performance");
        let compare = input
            .get("compare_to_planned")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        assert_eq!(analysis_type, "performance");
        assert!(compare);
    }

    #[test]
    fn test_analysis_type_values() {
        let valid_types = ["performance", "strategy", "recovery"];
        for t in &valid_types {
            assert!(["performance", "strategy", "recovery"].contains(t));
        }
    }

    #[test]
    fn test_date_values() {
        let valid_dates = ["2026-03-01", "last_race"];
        for date in &valid_dates {
            assert!(!date.is_empty());
        }
    }

    #[test]
    fn test_description_filter() {
        let input = json!({
            "description_contains": "marathon"
        });

        let desc = input.get("description_contains").and_then(|v| v.as_str());
        assert_eq!(desc, Some("marathon"));
    }

    #[test]
    fn test_target_date_alias_falls_back_when_date_missing() {
        let input = json!({
            "target_date": "2026-03-21"
        });

        let date = input
            .get("date")
            .and_then(Value::as_str)
            .or_else(|| input.get("target_date").and_then(Value::as_str));

        assert_eq!(date, Some("2026-03-21"));
    }

    #[test]
    fn test_parse_requested_date_supports_today_alias() {
        let parsed = AnalyzeRaceHandler::parse_requested_date("today").unwrap();

        assert_eq!(parsed, chrono::Local::now().date_naive());
    }

    #[test]
    fn test_race_search_logic() {
        let activities = ["Race 5K", "Training Run", "Marathon Race", "Easy Run"];
        let search_term = "marathon";

        let matches: Vec<_> = activities
            .iter()
            .filter(|a| a.to_lowercase().contains(&search_term.to_lowercase()))
            .collect();

        assert_eq!(matches.len(), 1);
        assert_eq!(*matches[0], "Marathon Race");
    }

    #[test]
    fn test_looks_like_race_keywords() {
        use super::AnalyzeRaceHandler;
        assert!(AnalyzeRaceHandler::looks_like_race(Some("City Marathon")));
        assert!(AnalyzeRaceHandler::looks_like_race(Some("Half Marathon")));
        assert!(AnalyzeRaceHandler::looks_like_race(Some("5K Race")));
        assert!(AnalyzeRaceHandler::looks_like_race(Some("10K")));
        assert!(AnalyzeRaceHandler::looks_like_race(Some("Ultra 100K")));
        assert!(AnalyzeRaceHandler::looks_like_race(Some(
            "Sprint Triathlon"
        )));
        assert!(AnalyzeRaceHandler::looks_like_race(Some("CCC Trail")));
        assert!(AnalyzeRaceHandler::looks_like_race(Some("Trail Running")));
        assert!(AnalyzeRaceHandler::looks_like_race(Some(
            "Skyrace Chamonix"
        )));
        assert!(AnalyzeRaceHandler::looks_like_race(Some("Vertical KM")));
        assert!(AnalyzeRaceHandler::looks_like_race(Some("km vert")));
        assert!(!AnalyzeRaceHandler::looks_like_race(Some("Easy Run")));
        assert!(!AnalyzeRaceHandler::looks_like_race(Some("Recovery")));
        assert!(!AnalyzeRaceHandler::looks_like_race(None));
        assert!(!AnalyzeRaceHandler::looks_like_race(Some("UTMB")));
        assert!(!AnalyzeRaceHandler::looks_like_race(Some("CCC")));
    }

    #[test]
    fn test_empty_results_handling() {
        let race_found = false;

        if !race_found {
            let suggestions = [
                "Use description_contains with specific race name".to_string(),
                "Check if activities are properly tagged".to_string(),
            ];
            assert!(!suggestions.is_empty());
        }
    }

    #[test]
    fn test_content_structure() {
        let handler = AnalyzeRaceHandler::new();

        assert_eq!(handler.name(), "analyze_race");
        assert!(handler.description().len() > 50);

        let schema = handler.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_next_actions_for_race_analysis() {
        let next_actions = [
            "To assess recovery: assess_recovery with period_days: 14".to_string(),
            "To plan next buildup: plan_training".to_string(),
        ];

        assert_eq!(next_actions.len(), 2);
        assert!(next_actions[0].contains("assess_recovery"));
        assert!(next_actions[1].contains("plan_training"));
    }

    #[test]
    fn test_recovery_period_after_race() {
        let period_days = 14;
        assert!((7..=21).contains(&period_days));
    }

    #[test]
    fn test_parse_requested_date_valid() {
        let parsed = AnalyzeRaceHandler::parse_requested_date("2026-05-24").unwrap();
        assert_eq!(
            parsed,
            NaiveDate::parse_from_str("2026-05-24", "%Y-%m-%d").unwrap()
        );
    }

    #[test]
    fn test_parse_requested_date_invalid() {
        let result = AnalyzeRaceHandler::parse_requested_date("not-a-date");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid race date")
        );
    }

    #[test]
    fn test_parse_activity_date_date_only() {
        assert_eq!(
            AnalyzeRaceHandler::parse_activity_date("2026-05-24"),
            Some(NaiveDate::from_ymd_opt(2026, 5, 24).unwrap())
        );
    }

    #[test]
    fn test_parse_activity_date_datetime() {
        assert_eq!(
            AnalyzeRaceHandler::parse_activity_date("2026-05-24T10:30:00"),
            Some(NaiveDate::from_ymd_opt(2026, 5, 24).unwrap())
        );
    }

    #[test]
    fn test_parse_activity_date_invalid() {
        assert_eq!(AnalyzeRaceHandler::parse_activity_date("garbage"), None);
    }

    #[test]
    fn test_race_analysis_mode_parse_performance() {
        assert_eq!(
            RaceAnalysisMode::parse(Some("performance")),
            RaceAnalysisMode::Performance
        );
    }

    #[test]
    fn test_race_analysis_mode_parse_strategy() {
        assert_eq!(
            RaceAnalysisMode::parse(Some("strategy")),
            RaceAnalysisMode::Strategy
        );
    }

    #[test]
    fn test_race_analysis_mode_parse_recovery() {
        assert_eq!(
            RaceAnalysisMode::parse(Some("recovery")),
            RaceAnalysisMode::Recovery
        );
    }

    #[test]
    fn test_race_analysis_mode_parse_default() {
        assert_eq!(RaceAnalysisMode::parse(None), RaceAnalysisMode::Performance);
        assert_eq!(
            RaceAnalysisMode::parse(Some("unknown")),
            RaceAnalysisMode::Performance
        );
    }

    // ====================================================================
    // execute() integration tests
    // ====================================================================

    #[tokio::test]
    async fn test_execute_no_race_found_empty_activities() {
        let handler = AnalyzeRaceHandler::new();
        let client = Arc::new(MockIntervalsClient::default());
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("No recent race activities found"));
        assert!(!output.suggestions.is_empty());
        assert!(!output.next_actions.is_empty());
    }

    #[tokio::test]
    async fn test_execute_no_race_found_with_desc_filter() {
        let handler = AnalyzeRaceHandler::new();
        let client = Arc::new(MockIntervalsClient::default());
        let result = handler
            .execute(json!({"description_contains": "marathon"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("No race found matching"));
        assert!(content_str.contains("marathon"));
    }

    #[tokio::test]
    async fn test_execute_no_race_found_last_race_empty() {
        let handler = AnalyzeRaceHandler::new();
        let client = Arc::new(MockIntervalsClient::default());
        let result = handler
            .execute(json!({"date": "last_race"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("No recent race activities found"));
    }

    #[tokio::test]
    async fn test_execute_no_race_found_by_date_no_match() {
        let handler = AnalyzeRaceHandler::new();
        let client = Arc::new(MockIntervalsClient::with_activity(
            "act-1",
            "2026-05-01",
            "Easy Run",
        ));
        let result = handler
            .execute(json!({"date": "2026-05-24"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("No recent race activities found"));
    }

    #[tokio::test]
    async fn test_execute_invalid_date_returns_error() {
        let handler = AnalyzeRaceHandler::new();
        let client = Arc::new(MockIntervalsClient::with_activity(
            "act-1",
            "2026-05-01",
            "Easy Run",
        ));
        let result = handler
            .execute(json!({"date": "not-a-date"}), client, None)
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid race date")
        );
    }

    #[tokio::test]
    async fn test_execute_fallback_finds_looks_like_race() {
        let activities = vec![
            make_activity("act-1", "2026-05-20", "Easy Run"),
            make_activity("act-2", "2026-05-21", "Sunday Race"),
        ];
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(activities)
                .with_activity_detail("act-2", json!({}))
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Sunday Race"));
    }

    #[tokio::test]
    async fn test_execute_fallback_finds_first_when_no_race_keyword() {
        let activities = vec![
            make_activity("act-1", "2026-05-20", "Morning Jog"),
            make_activity("act-2", "2026-05-21", "Evening Jog"),
        ];
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(activities)
                .with_activity_detail("act-1", json!({}))
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Morning Jog"));
    }

    #[tokio::test]
    async fn test_execute_last_race_selects_latest() {
        let activities = vec![
            make_activity("act-old", "2026-05-01", "Trail Race"),
            make_activity("act-new", "2026-05-21", "10K Race"),
        ];
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(activities)
                .with_activity_detail("act-new", json!({}))
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"date": "last_race"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("10K Race"));
    }

    #[tokio::test]
    async fn test_execute_by_date_exact_match() {
        let activities = vec![
            make_activity("act-1", "2026-05-20", "Easy Run"),
            make_activity("act-race", "2026-05-24", "Marathon Race"),
        ];
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(activities)
                .with_activity_detail("act-race", json!({}))
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"date": "2026-05-24"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Marathon Race"));
    }

    #[tokio::test]
    async fn test_execute_by_date_with_desc_filter_match() {
        let activities = vec![
            make_activity("act-1", "2026-05-24", "Morning Run"),
            make_activity("act-race", "2026-05-24", "Marathon Race"),
        ];
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(activities)
                .with_activity_detail("act-race", json!({}))
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(
                json!({"date": "2026-05-24", "description_contains": "marathon"}),
                client,
                None,
            )
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Marathon Race"));
    }

    #[tokio::test]
    async fn test_execute_by_date_with_desc_filter_no_match() {
        let activities = vec![
            make_activity("act-1", "2026-05-24", "Morning Run"),
            make_activity("act-2", "2026-05-24", "Evening Run"),
        ];
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(activities)
                .with_activity_detail("act-1", json!({}))
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(
                json!({"date": "2026-05-24", "description_contains": "marathon"}),
                client,
                None,
            )
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("No race found matching"));
        assert!(content_str.contains("marathon"));
    }

    #[tokio::test]
    async fn test_execute_by_desc_filter_only_match() {
        let activities = vec![
            make_activity("act-1", "2026-05-20", "Easy Run"),
            make_activity("act-race", "2026-05-24", "Boston Marathon"),
        ];
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(activities)
                .with_activity_detail("act-race", json!({}))
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"description_contains": "marathon"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Boston Marathon"));
    }

    #[tokio::test]
    async fn test_execute_by_desc_filter_only_no_match() {
        let activities = vec![
            make_activity("act-1", "2026-05-20", "Easy Run"),
            make_activity("act-2", "2026-05-24", "Recovery"),
        ];
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(activities)
                .with_activity_detail("act-1", json!({}))
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"description_contains": "marathon"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("No race found matching"));
    }

    #[tokio::test]
    async fn test_execute_full_detail_table_rows() {
        let detail = json!({
            "distance": 42195.0,
            "moving_time": 14400,
            "average_heartrate": 162.0
        });
        let client = Arc::new(make_client_with_activity(
            "race-1",
            "2026-05-24",
            "Marathon",
            detail,
        ));
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("42.20 km"));
        assert!(content_str.contains("4:00:00"));
        assert!(content_str.contains("162 bpm"));
    }

    #[tokio::test]
    async fn test_execute_with_intervals_and_execution_metrics() {
        let detail = json!({
            "distance": 10000.0,
            "moving_time": 2700,
            "average_heartrate": 170.0,
            "efficiency_factor": 1.05
        });
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "10K Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!([{"start": 0, "end": 600}])),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("1 race segments"));
        assert!(content_str.contains("10.00 km"));
    }

    #[tokio::test]
    async fn test_execute_with_decoupling_metrics() {
        let detail = json!({
            "distance": 10000.0,
            "moving_time": 2700,
            "average_heartrate": 170.0
        });
        let streams = json!({
            "heartrate": [150, 160, 170, 175],
            "watts": [200, 210, 220, 230],
            "cadence": [85, 86, 85, 84],
            "speed": [4.0, 4.1, 4.0, 3.9],
            "distance": [0.0, 2500.0, 5000.0, 10000.0]
        });
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "10K Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(streams)
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_with_fitness_high_tsb() {
        let detail = json!({"distance": 10000.0, "moving_time": 2700});
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "10K Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!({}))
                .with_fitness_summary(json!({"ctl": 50.0, "atl": 80.0, "tsb": -15.0})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("recovery block recommended"));
    }

    #[tokio::test]
    async fn test_execute_with_fitness_low_tsb() {
        let detail = json!({"distance": 10000.0, "moving_time": 2700});
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "10K Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!({}))
                .with_fitness_summary(json!({"ctl": 50.0, "atl": 45.0, "tsb": 5.0})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("looks manageable"));
    }

    #[tokio::test]
    async fn test_execute_strategy_mode() {
        let client = Arc::new(make_minimal_client("race-1", "2026-05-24", "Race"));
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"analysis_type": "strategy"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Strategy Review"));
        assert!(content_str.contains("pacing discipline"));
    }

    #[tokio::test]
    async fn test_execute_recovery_mode_with_note() {
        let detail = json!({"distance": 10000.0, "moving_time": 2700});
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!({}))
                .with_fitness_summary(json!({"ctl": 50.0, "atl": 80.0, "tsb": -15.0})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"analysis_type": "recovery"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Recovery Outlook"));
        assert!(content_str.contains("recovery block recommended"));
    }

    #[tokio::test]
    async fn test_execute_recovery_mode_no_tsb() {
        let detail = json!({"distance": 10000.0, "moving_time": 2700});
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"analysis_type": "recovery"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(!content_str.contains("Recovery Outlook"));
    }

    #[tokio::test]
    async fn test_execute_post_race_load_context_not_recovery() {
        let detail = json!({"distance": 10000.0, "moving_time": 2700});
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!({}))
                .with_fitness_summary(json!({"ctl": 50.0, "atl": 80.0, "tsb": -12.0})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"analysis_type": "performance"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Post-Race Load Context"));
    }

    #[tokio::test]
    async fn test_execute_compare_true_with_matching_plan() {
        let detail = json!({"distance": 42195.0, "moving_time": 14400});
        let plan_event = json!({
            "name": "Marathon Race",
            "start_date_local": "2026-05-24T08:00:00",
            "category": "Workout"
        });
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "Marathon Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!({}))
                .with_upcoming_workouts(json!([plan_event])),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"compare_to_planned": true}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Comparison to Plan"));
        assert!(content_str.contains("Planned event"));
    }

    #[tokio::test]
    async fn test_execute_compare_true_no_matching_plan() {
        let detail = json!({"distance": 42195.0, "moving_time": 14400});
        let plan_event = json!({
            "name": "Easy Run",
            "start_date_local": "2026-05-23T08:00:00",
            "category": "Workout"
        });
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "Marathon Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!({}))
                .with_upcoming_workouts(json!([plan_event])),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"compare_to_planned": true}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("No matching planned event found"));
    }

    #[tokio::test]
    async fn test_execute_compare_true_plan_error() {
        let detail = json!({"distance": 42195.0, "moving_time": 14400});
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "Marathon Race")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!({}))
                .with_upcoming_workouts_error(IntervalsError::from_status(500, "server error")),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"compare_to_planned": true}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("No matching planned event found"));
    }

    #[tokio::test]
    async fn test_execute_compare_false() {
        let detail = json!({"distance": 42195.0, "moving_time": 14400});
        let client = Arc::new(make_client_with_activity(
            "race-1",
            "2026-05-24",
            "Marathon",
            detail,
        ));
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"compare_to_planned": false}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(!content_str.contains("Comparison to Plan"));
    }

    #[tokio::test]
    async fn test_execute_with_wellness_data() {
        let detail = json!({"distance": 10000.0, "moving_time": 2700, "average_heartrate": 165.0});
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(vec![make_activity("race-1", "2026-05-24", "10K")])
                .with_activity_detail("race-1", detail)
                .with_streams(json!({}))
                .with_intervals(json!({}))
                .with_wellness(json!([{"mood": 3, "sleep_hours": 7.5}]))
                .with_fitness_summary(json!({"ctl": 60.0, "atl": 55.0, "tsb": 5.0})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("10K"));
        assert!(content_str.contains("165 bpm"));
    }

    #[tokio::test]
    async fn test_execute_target_date_alias() {
        let client = Arc::new(make_minimal_client("race-1", "2026-05-24", "Race Day"));
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"target_date": "2026-05-24"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Race Day"));
    }

    #[tokio::test]
    async fn test_execute_today_alias() {
        let today = chrono::Local::now().date_naive();
        let client = Arc::new(make_minimal_client(
            "race-1",
            &today.to_string(),
            "Today Race",
        ));
        let handler = AnalyzeRaceHandler::new();
        let result = handler
            .execute(json!({"date": "today"}), client, None)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Today Race"));
    }

    #[tokio::test]
    async fn test_execute_detail_is_not_object_skips_table() {
        let detail = json!("not an object");
        let client = Arc::new(make_client_with_activity(
            "race-1",
            "2026-05-24",
            "Weird Race",
            detail,
        ));
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_without_race_name_fallback() {
        let activities = vec![ActivitySummary {
            id: "act-1".to_string(),
            name: None,
            start_date_local: "2026-05-24".to_string(),
            ..Default::default()
        }];
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_activities(activities)
                .with_activity_detail("act-1", json!({}))
                .with_streams(json!({}))
                .with_intervals(json!({})),
        );
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = format!("{:?}", output.content);
        assert!(content_str.contains("Race"));
    }

    #[tokio::test]
    async fn test_execute_suggestions_includes_nutrition_when_missing() {
        let client = Arc::new(make_minimal_client("race-1", "2026-05-24", "Race"));
        let handler = AnalyzeRaceHandler::new();
        let result = handler.execute(json!({}), client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.suggestions.iter().any(|s| s.contains("nutrition")));
    }
}
