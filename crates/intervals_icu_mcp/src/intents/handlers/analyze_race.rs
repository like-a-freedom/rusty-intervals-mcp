use crate::intents::{ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput};
use async_trait::async_trait;
use chrono::NaiveDate;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
/// Analyze Race Intent Handler
///
/// Post-race analysis: results, strategy, comparison to plan.
use std::sync::Arc;

use crate::content::date::{data_availability_block, filter_activities_by_description};
use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext, RaceMetrics};
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::{RaceFetchRequest, fetch_race_data};
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{extract_ctl_series, parse_wellness_metrics};
use crate::engines::fitness_context::FitnessContext;
use crate::engines::race_readiness::{compute_ctl_drop, compute_race_readiness};
use crate::engines::shared::parse_activity_date;

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
                .max_by_key(|activity| parse_activity_date(&activity.start_date_local))
        } else if let Some(date) = date {
            let target_date = Self::parse_requested_date(date)?;
            activities.iter().find(|activity| {
                parse_activity_date(&activity.start_date_local) == Some(target_date)
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
            .await
            .map_err(|e| IntentError::api(e.to_string()))?;
            fetched.activities = vec![race.clone()];
            let fitness_context = FitnessContext::load(client.as_ref()).await;
            fetched.wellness = client.get_wellness(Some(7)).await.ok();

            let race_date = parse_activity_date(&race.start_date_local)
                .unwrap_or_else(|| chrono::Local::now().date_naive());
            let mut race_context = CoachContext::new(
                AnalysisKind::RaceAnalysis,
                AnalysisWindow::new(race_date, race_date),
            );
            race_context.audit = build_data_audit(&fetched);
            race_context.metrics.fitness = fitness_context.metrics().cloned();
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
            let ctl_drop_val = compute_ctl_drop(
                extract_ctl_series(fetched.wellness.as_ref())
                    .map(|(_, values)| values)
                    .unwrap_or_default()
                    .as_slice(),
                race_context.metrics.fitness.as_ref().and_then(|f| f.ctl),
            );
            let race_readiness = compute_race_readiness(
                race_context.metrics.fitness.as_ref().and_then(|f| f.tsb),
                durability_drifting,
                false,
                system_mismatch,
                ctl_drop_val,
            );
            // Persist the readiness breakdown onto the metrics bag so downstream
            // guidance/alert rules and rendering can consume it.
            race_context.metrics.race_readiness = Some(race_readiness.to_metrics());

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
            let ctl_drop_line = if let Some(drop) = ctl_drop_val {
                format!("  CTL Drop: {:.1} pts (taper/shedding penalty)", drop)
            } else {
                String::from("  CTL Drop: none (no detraining detected)")
            };
            content.push(ContentBlock::markdown(format!(
                "Race Readiness\n  Score: {}/100\n  Tier: {}\n{}",
                race_readiness.score, tier, ctl_drop_line
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
                let race_day = parse_activity_date(&race.start_date_local);
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
#[path = "analyze_race/tests.rs"]
mod tests;
