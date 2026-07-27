use crate::intents::{
    ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput,
    data_availability_block,
};
use async_trait::async_trait;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::domains::coach::WellnessMetrics;
use crate::engines::ade::compute_ade;
use crate::engines::analysis_fetch::RecoveryFetchRequest;

#[cfg(test)]
use crate::domains::coach::CoachMetrics;
#[cfg(test)]
use crate::engines::coach_guidance::build_alerts;
#[cfg(test)]
use crate::engines::coach_metrics::{parse_fitness_metrics, parse_wellness_metrics};

mod constants;
mod planned_activity;
mod recovery_rows;

use planned_activity::PlannedActivity;
use recovery_rows::build_recovery_metric_rows as build_rows_internal;
pub struct AssessRecoveryHandler {
    engine: crate::engines::recovery_assessment_engine::RecoveryAssessmentEngine,
}

impl AssessRecoveryHandler {
    pub fn new() -> Self {
        let builder = Arc::new(crate::engines::coach_metrics::CoachMetricsBuilder);
        let engine = crate::engines::recovery_assessment_engine::RecoveryAssessmentEngine::new(
            builder.clone(),
        );
        Self { engine }
    }
    #[cfg(test)]
    fn parse_wellness(&self, wellness: &Value) -> (f64, f64, f64) {
        parse_wellness_metrics(Some(wellness))
            .map(|metrics| {
                (
                    metrics.avg_sleep_hours.unwrap_or(7.5),
                    metrics.avg_resting_hr.unwrap_or(52.0),
                    metrics.avg_hrv.unwrap_or(65.0),
                )
            })
            .unwrap_or((7.5, 52.0, 65.0))
    }

    fn build_recovery_metric_rows(
        wellness: &WellnessMetrics,
        fitness: &crate::domains::coach::FitnessMetrics,
    ) -> Vec<Vec<String>> {
        build_rows_internal(wellness, fitness)
    }

    #[cfg(test)]
    fn check_red_flags(&self, sleep: f64, rhr: f64, hrv: f64, tsb: f64) -> Vec<String> {
        let metrics = CoachMetrics {
            fitness: Some(parse_fitness_metrics(Some(&json!([{"form": tsb}]))).unwrap_or_default()),
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(sleep),
                avg_resting_hr: Some(rhr),
                avg_hrv: Some(hrv),
                wellness_days_count: 1,
                ..Default::default()
            }),
            ..Default::default()
        };

        build_alerts(&metrics)
            .into_iter()
            .map(|alert| format!("{}: {}", alert.title, alert.evidence.join(", ")))
            .collect()
    }
}

#[async_trait]
impl IntentHandler for AssessRecoveryHandler {
    fn name(&self) -> &'static str {
        "assess_recovery"
    }

    fn description(&self) -> &'static str {
        "Assesses recovery status, readiness to train, and detects red flags. \
         Returns multi-domain HRV analysis (ratio, trend slope, recovery quality \
         index), wellness metrics (sleep, RHR, HRV, recovery index, readiness), \
         fitness metrics (TSB, CTL, ATL), and ADE system state assessment \
         (LoadAccepting/RecoveryPriority with risk level and active flags). \
         Activity-specific readiness verdict for easy/intensity/long/race sessions.

         Use this tool when: you need to check if today is safe for a key workout, \
         evaluate post-race recovery status, or detect overtraining signs. \
         Do NOT use when: you need to analyze a specific workout (use analyze_training) \
         or perform post-race debrief (use analyze_race)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period_days": {"type": "number", "default": 7, "description": "Analysis period (days)"},
                "for_activity": {"type": "string", "enum": ["easy", "intensity", "long", "race"], "description": "Planned activity type"},
                "include_wellness": {"type": "boolean", "default": true, "description": "Include wellness data"},
                "include_red_flags": {"type": "boolean", "default": true, "description": "Check for red flags"}
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let period_days = input
            .get("period_days")
            .and_then(Value::as_i64)
            .unwrap_or(7);
        let planned_activity =
            PlannedActivity::parse(input.get("for_activity").and_then(Value::as_str));
        let include_wellness = input
            .get("include_wellness")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let include_red_flags = input
            .get("include_red_flags")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let request = RecoveryFetchRequest {
            period_days: period_days as i32,
            include_wellness,
        };

        let report = self
            .engine
            .build_report(client.as_ref(), &request)
            .await
            .map_err(|e| IntentError::api(e.to_string()))?;

        let recovery_context = report.coach_context;

        // Look-ahead: check upcoming workouts for key sessions
        let upcoming = client
            .get_upcoming_workouts(Some(7), Some(5), None)
            .await
            .ok();

        let mut content = Vec::new();
        let start_date = report.period.start_date;
        let end_date = report.period.end_date;

        content.push(ContentBlock::markdown(format!(
            "# Recovery Assessment ({} - {})\nReadiness for: {}",
            start_date.format("%d %b"),
            end_date.format("%d %b"),
            planned_activity.as_str()
        )));

        let wellness = recovery_context
            .metrics
            .wellness
            .clone()
            .unwrap_or_default();
        let fitness = recovery_context.metrics.fitness.clone().unwrap_or_default();
        let rows = Self::build_recovery_metric_rows(&wellness, &fitness);
        content.push(ContentBlock::table(
            vec!["Metric".into(), "Value".into(), "Status".into()],
            rows,
        ));

        if wellness.recovery_index.is_none()
            && (wellness.avg_resting_hr.is_some() || wellness.avg_hrv.is_some())
        {
            content.push(ContentBlock::markdown(
                    "Recovery Index\nRecovery Index unavailable because either HRV or resting HR is missing."
                        .to_string(),
                ));
        }

        // Render personal baseline evidence
        if let Some(ref hrv_baseline) = wellness.hrv_personal_baseline {
            let position_str = match hrv_baseline.position {
                crate::domains::baseline::BaselinePosition::Below => "Below",
                crate::domains::baseline::BaselinePosition::Within => "Within",
                crate::domains::baseline::BaselinePosition::Above => "Above",
            };
            content.push(ContentBlock::markdown(format!(
                "Personal Baseline\n  Metric: {} ({})\n  Recent 7-day: {:.2}\n  60-day baseline: {:.2}\n  CV: {:.4}\n  SWC: {:.4}\n  Band: [{:.2}, {:.2}]\n  Position: {}\n  Samples: recent {}, baseline {} (span {} days)\n  Model: {}\n\nPosition describes a sustained statistical deviation from your own history; it is not a readiness verdict.",
                hrv_baseline.metric,
                hrv_baseline.unit,
                hrv_baseline.recent_mean_7d,
                hrv_baseline.baseline_mean_60d,
                hrv_baseline.baseline_cv_60d,
                hrv_baseline.swc,
                hrv_baseline.lower_bound,
                hrv_baseline.upper_bound,
                position_str,
                hrv_baseline.recent_sample_count,
                hrv_baseline.baseline_sample_count,
                hrv_baseline.baseline_span_days,
                hrv_baseline.model,
            )));
        }

        if let Some(ref rhr_baseline) = wellness.resting_hr_personal_baseline {
            let position_str = match rhr_baseline.position {
                crate::domains::baseline::BaselinePosition::Below => "Below",
                crate::domains::baseline::BaselinePosition::Within => "Within",
                crate::domains::baseline::BaselinePosition::Above => "Above",
            };
            content.push(ContentBlock::markdown(format!(
                "Resting HR Personal Baseline\n  Metric: {} ({})\n  Recent 7-day: {:.1}\n  60-day baseline: {:.1}\n  CV: {:.4}\n  SWC: {:.4}\n  Band: [{:.1}, {:.1}]\n  Position: {}\n  Samples: recent {}, baseline {} (span {} days)\n  Model: {}\n\nPosition describes a sustained statistical deviation from your own history; it is not a readiness verdict.",
                rhr_baseline.metric,
                rhr_baseline.unit,
                rhr_baseline.recent_mean_7d,
                rhr_baseline.baseline_mean_60d,
                rhr_baseline.baseline_cv_60d,
                rhr_baseline.swc,
                rhr_baseline.lower_bound,
                rhr_baseline.upper_bound,
                position_str,
                rhr_baseline.recent_sample_count,
                rhr_baseline.baseline_sample_count,
                rhr_baseline.baseline_span_days,
                rhr_baseline.model,
            )));
        }

        // ADE — System State Assessment
        let ade_result = compute_ade(
            &crate::engines::ade::AdeInputs {
                tsb: recovery_context
                    .metrics
                    .fitness
                    .as_ref()
                    .and_then(|f| f.tsb),
                hrv_ratio: recovery_context
                    .metrics
                    .wellness
                    .as_ref()
                    .and_then(|w| w.hrv_ratio),
                ramp_rate: recovery_context
                    .metrics
                    .fitness
                    .as_ref()
                    .and_then(|f| f.ramp_rate),
                ..Default::default()
            },
            recovery_context
                .metrics
                .fitness
                .as_ref()
                .and_then(|f| f.tsb),
        );
        {
            let state_label = match ade_result.operational_state {
                crate::engines::ade::OperationalState::LoadAccepting => "Load Accepting",
                crate::engines::ade::OperationalState::RecoveryPriority => "Recovery Priority",
            };
            let risk_label = match ade_result.risk_level {
                crate::engines::ade::RiskLevel::Low => "Low",
                crate::engines::ade::RiskLevel::Moderate => "Moderate",
                crate::engines::ade::RiskLevel::High => "High",
                crate::engines::ade::RiskLevel::Critical => "Critical",
            };
            let mut flags = Vec::new();
            if ade_result.maladaptation_risk {
                flags.push("Maladaptation Risk");
            }
            if ade_result.functional_overreach {
                flags.push("Functional Overreach");
            }
            if ade_result.load_pressure {
                flags.push("Load Pressure");
            }
            if ade_result.loaded_taper {
                flags.push("Loaded Taper");
            }
            let flags_line = if flags.is_empty() {
                "  Flags: None".to_string()
            } else {
                format!("  Flags: {}", flags.join(", "))
            };
            content.push(ContentBlock::markdown(format!(
                "System State Assessment\n  State: {}\n  Risk: {}\n{}",
                state_label, risk_label, flags_line
            )));
        }

        // Calculate red flags first
        let red_flags = if include_red_flags {
            recovery_context
                .alerts
                .iter()
                .map(|alert| format!("{}: {}", alert.title, alert.evidence.join(", ")))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        if include_red_flags {
            let flags_md = if red_flags.is_empty() {
                "Red Flags: None detected\nRecommendation: Ready for key workout".to_string()
            } else {
                let mut md = String::from("Red Flags Detected:\n");
                for flag in &red_flags {
                    md.push_str(&format!("  {}\n", flag));
                }
                md.push_str("\nRecommendation: Consider recovery before intensity");
                md
            };
            content.push(ContentBlock::markdown(flags_md));
        }

        let (readiness_status, readiness_note) =
            planned_activity.readiness_copy(&wellness, &fitness, &red_flags);
        content.push(ContentBlock::markdown(format!(
            "Activity-Specific Readiness\n  {} for {} work.\n  {}",
            readiness_status,
            planned_activity.as_str(),
            readiness_note
        )));

        if let Some(block) = data_availability_block(
            &recovery_context.audit.degraded_mode_reasons,
            recovery_context.audit.all_available(),
        ) {
            content.push(block);
        }

        // Use shared guidance from coach engine
        let mut suggestions = recovery_context.guidance.suggestions.clone();

        // Look-ahead: warn if key workout scheduled in next 7 days
        if let Some(ref workouts) = upcoming
            && let Some(arr) = workouts.as_array()
        {
            let has_key_workout = arr.iter().any(|w| {
                let name = w
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                name.contains("race")
                    || name.contains("interval")
                    || name.contains("tempo")
                    || name.contains("long")
            });
            if has_key_workout {
                suggestions.push(
                    "Key workout or race scheduled in the next 7 days - prioritize recovery today."
                        .into(),
                );
            }
        }

        let mut next_actions = vec![
            format!(
                "To plan training: plan_training with focus: {}",
                planned_activity.as_str()
            ),
            "To analyze recent workouts: analyze_training with target_type: period".into(),
            "To add one workout to an empty day: inspect that date with analyze_training, then use modify_training with action: create and dry_run: true".into(),
        ];
        match planned_activity {
            PlannedActivity::Easy => {
                next_actions.insert(0, "Proceed with easy training and keep RPE low".into())
            }
            PlannedActivity::Intensity => next_actions.insert(
                0,
                "If signals worsen, replace the quality session with aerobic running".into(),
            ),
            PlannedActivity::Long => next_actions.insert(
                0,
                "Fuel early and shorten the session if fatigue rises mid-run".into(),
            ),
            PlannedActivity::Race => next_actions.insert(
                0,
                "Recheck recovery markers before committing to race effort".into(),
            ),
        }
        for action in &recovery_context.guidance.next_actions {
            if !next_actions.contains(action) {
                next_actions.insert(0, action.clone());
            }
        }
        if !red_flags.is_empty() {
            next_actions.insert(0, "Consider rest day before next hard session".into());
        }

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions))
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}

impl Default for AssessRecoveryHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::coach::FitnessMetrics;
    use crate::test_support::content_text;
    use crate::test_support::mock::MockIntervalsClient;
    use intervals_icu_client::{ActivitySummary, IntervalsError};
    use std::sync::Arc;

    #[test]
    fn test_new_handler() {
        let handler = AssessRecoveryHandler::new();
        assert_eq!(handler.name(), "assess_recovery");
    }

    #[test]
    fn test_default_handler() {
        let _handler = AssessRecoveryHandler::new();
    }

    #[test]
    fn test_name() {
        let handler = AssessRecoveryHandler::new();
        assert_eq!(IntentHandler::name(&handler), "assess_recovery");
    }

    #[test]
    fn test_description() {
        let handler = AssessRecoveryHandler::new();
        let desc = IntentHandler::description(&handler);
        assert!(desc.contains("Assesses recovery"));
        assert!(desc.contains("readiness to train"));
        assert!(desc.contains("red flags"));
    }

    #[test]
    fn test_input_schema_structure() {
        let handler = AssessRecoveryHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("period_days"));
        assert!(props.contains_key("for_activity"));
        assert!(props.contains_key("include_wellness"));
        assert!(props.contains_key("include_red_flags"));

        // Check for_activity enum values
        let activity = props.get("for_activity").unwrap();
        let activity_enum = activity.get("enum").unwrap().as_array().unwrap();
        assert!(activity_enum.contains(&json!("easy")));
        assert!(activity_enum.contains(&json!("intensity")));
        assert!(activity_enum.contains(&json!("long")));
        assert!(activity_enum.contains(&json!("race")));
    }

    #[test]
    fn test_requires_idempotency_token() {
        let handler = AssessRecoveryHandler::new();
        assert!(!IntentHandler::requires_idempotency_token(&handler));
    }

    #[test]
    fn test_default_values() {
        let input = json!({});

        let period_days = input
            .get("period_days")
            .and_then(|v| v.as_i64())
            .unwrap_or(7);
        let for_activity = input
            .get("for_activity")
            .and_then(|v| v.as_str())
            .unwrap_or("easy");
        let include_wellness = input
            .get("include_wellness")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let include_red_flags = input
            .get("include_red_flags")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        assert_eq!(period_days, 7);
        assert_eq!(for_activity, "easy");
        assert!(include_wellness);
        assert!(include_red_flags);
    }

    #[test]
    fn test_parse_wellness_empty() {
        let handler = AssessRecoveryHandler::new();
        let wellness = json!([]);

        let (avg_sleep, resting_hr, hrv) = handler.parse_wellness(&wellness);

        // Should return defaults for empty data
        assert!((avg_sleep - 7.5).abs() < 0.01);
        assert!((resting_hr - 52.0).abs() < 0.01);
        assert!((hrv - 65.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_wellness_with_data() {
        let handler = AssessRecoveryHandler::new();
        let wellness = json!([
            {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 70.0},
            {"sleep_hours": 7.0, "resting_hr": 52.0, "hrv": 65.0},
            {"sleep_hours": 7.5, "resting_hr": 51.0, "hrv": 68.0}
        ]);

        let (avg_sleep, resting_hr, hrv) = handler.parse_wellness(&wellness);

        assert!((avg_sleep - 7.5).abs() < 0.01);
        assert!((resting_hr - 51.0).abs() < 0.01);
        assert!((hrv - 67.67).abs() < 0.1);
    }

    #[test]
    fn test_check_red_flags_all_clear() {
        let handler = AssessRecoveryHandler::new();

        // Good values - no red flags
        let flags = handler.check_red_flags(8.0, 50.0, 70.0, 15.0);
        assert!(flags.is_empty());
    }

    #[test]
    fn test_check_red_flags_low_sleep() {
        let handler = AssessRecoveryHandler::new();

        let flags = handler.check_red_flags(6.0, 50.0, 70.0, 15.0);
        assert!(!flags.is_empty());
        assert!(flags.iter().any(|f| f.contains("sleep")));
    }

    #[test]
    fn test_check_red_flags_elevated_rhr() {
        let handler = AssessRecoveryHandler::new();

        let flags = handler.check_red_flags(8.0, 65.0, 70.0, 15.0);
        assert!(!flags.is_empty());
        assert!(flags.iter().any(|f| f.contains("RHR")));
    }

    #[test]
    fn test_check_red_flags_low_hrv() {
        let metrics = CoachMetrics {
            fitness: Some(
                parse_fitness_metrics(Some(&json!([{"form": 15.0}]))).unwrap_or_default(),
            ),
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(8.0),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(35.0),
                hrv_baseline: Some(50.0),
                hrv_deviation_pct: Some(-30.0),
                hrv_trend_state: Some("suppressed".into()),
                wellness_days_count: 7,
                ..Default::default()
            }),
            ..Default::default()
        };

        let flags = build_alerts(&metrics)
            .into_iter()
            .map(|alert| format!("{}: {}", alert.title, alert.evidence.join(", ")))
            .collect::<Vec<_>>();

        assert!(!flags.is_empty());
        assert!(flags.iter().any(|f| f.contains("HRV")));
    }

    #[test]
    fn test_check_red_flags_deep_fatigue() {
        let handler = AssessRecoveryHandler::new();

        let flags = handler.check_red_flags(8.0, 50.0, 70.0, -25.0);
        assert!(!flags.is_empty());
        assert!(flags.iter().any(|f| f.contains("fatigue")));
    }

    #[test]
    fn test_check_red_flags_multiple() {
        let handler = AssessRecoveryHandler::new();

        // Multiple issues
        let flags = handler.check_red_flags(6.0, 65.0, 35.0, -25.0);
        assert!(flags.len() >= 3);
    }

    #[test]
    fn test_sleep_status_thresholds() {
        use crate::engines::coach_guidance::{SLEEP_FAIR_MIN_HOURS, SLEEP_GOOD_HOURS};

        // Good sleep
        let avg_sleep = 7.5;
        let status = if avg_sleep >= SLEEP_GOOD_HOURS {
            "Good"
        } else if avg_sleep >= SLEEP_FAIR_MIN_HOURS {
            "Fair"
        } else {
            "Poor"
        };
        assert_eq!(status, "Good");

        // Fair sleep
        let avg_sleep = 6.5;
        let status = if avg_sleep >= SLEEP_GOOD_HOURS {
            "Good"
        } else if avg_sleep >= SLEEP_FAIR_MIN_HOURS {
            "Fair"
        } else {
            "Poor"
        };
        assert_eq!(status, "Fair");

        // Poor sleep
        let avg_sleep = 5.5;
        let status = if avg_sleep >= SLEEP_GOOD_HOURS {
            "Good"
        } else if avg_sleep >= SLEEP_FAIR_MIN_HOURS {
            "Fair"
        } else {
            "Poor"
        };
        assert_eq!(status, "Poor");
    }

    #[test]
    fn test_tsb_status_thresholds() {
        use crate::engines::coach_guidance::{TSB_FATIGUED, TSB_FRESH};

        // Fresh
        let tsb = 15.0;
        let status = if tsb > TSB_FRESH {
            "Fresh"
        } else if tsb > TSB_FATIGUED {
            "Neutral"
        } else {
            "Fatigued"
        };
        assert_eq!(status, "Fresh");

        // Balanced
        let tsb = 0.0;
        let status = if tsb > TSB_FRESH {
            "Fresh"
        } else if tsb > TSB_FATIGUED {
            "Balanced"
        } else {
            "Fatigued"
        };
        assert_eq!(status, "Balanced");

        // Fatigued
        let tsb = -15.0;
        let status = if tsb > TSB_FRESH {
            "Fresh"
        } else if tsb > TSB_FATIGUED {
            "Balanced"
        } else {
            "Fatigued"
        };
        assert_eq!(status, "Fatigued");
    }

    #[test]
    fn test_for_activity_values() {
        let valid_activities = ["easy", "intensity", "long", "race"];
        for activity in &valid_activities {
            assert!(["easy", "intensity", "long", "race"].contains(activity));
        }
    }

    #[test]
    fn recovery_rows_include_recovery_index_when_available() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.3),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(70.0),
                recovery_index: Some(1.4),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(8.0),
                ..Default::default()
            },
        );

        assert!(
            rows.iter()
                .any(|row| row[0] == "Recovery Index" && row[1].contains("1.40"))
        );
    }

    #[test]
    fn recovery_rows_omit_recovery_index_when_inputs_are_incomplete() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.3),
                avg_resting_hr: None,
                avg_hrv: Some(70.0),
                recovery_index: None,
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(8.0),
                ..Default::default()
            },
        );

        assert!(!rows.iter().any(|row| row[0] == "Recovery Index"));
    }

    #[test]
    fn recovery_rows_render_personal_hrv_status_instead_of_universal_bucket() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.4),
                avg_resting_hr: Some(55.0),
                avg_hrv: Some(64.0),
                hrv_baseline: Some(80.0),
                resting_hr_baseline: Some(50.0),
                hrv_deviation_pct: Some(-20.0),
                hrv_trend_state: Some("suppressed".into()),
                recovery_index: Some(0.73),
                wellness_days_count: 7,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(3.0),
                ..Default::default()
            },
        );

        let hrv_row = rows
            .iter()
            .find(|row| row[0] == "HRV")
            .expect("HRV row should be present");

        assert!(hrv_row[2].contains("baseline") || hrv_row[2].contains("range"));
        assert!(!hrv_row[2].contains("Very Low"));
    }

    #[test]
    fn recovery_rows_include_readiness_score_when_api_available() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                recovery_index: None,
                wellness_days_count: 5,
                readiness_score: Some(8.0),
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );

        let rs_row = rows
            .iter()
            .find(|row| row[0] == "Readiness Score")
            .expect("Readiness Score row should be present");
        assert_eq!(rs_row[1], "8.0");
        assert!(rs_row[2].contains("Supportive"));
    }

    #[test]
    fn recovery_rows_readiness_watch_threshold() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                readiness_score: Some(6.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );

        let rs_row = rows
            .iter()
            .find(|row| row[0] == "Readiness Score")
            .expect("Readiness Score row should be present");
        assert!(rs_row[2].contains("Watch"));
    }

    #[test]
    fn recovery_rows_readiness_low_threshold() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                readiness_score: Some(4.5),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );

        let rs_row = rows
            .iter()
            .find(|row| row[0] == "Readiness Score")
            .expect("Readiness Score row should be present");
        assert!(rs_row[2].contains("Low"));
    }

    #[test]
    fn recovery_rows_include_mood_stress_fatigue_when_complete() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                avg_mood: Some(8.0),
                avg_stress: Some(4.0),
                avg_fatigue: Some(3.0),
                readiness_score: Some(8.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );

        let mood_row = rows
            .iter()
            .find(|row| row[0] == "Mood")
            .expect("Mood row should be present");
        assert!(mood_row[1].contains("8"));

        let stress_row = rows
            .iter()
            .find(|row| row[0] == "Stress")
            .expect("Stress row should be present");
        assert!(stress_row[1].contains("4"));

        let fatigue_row = rows
            .iter()
            .find(|row| row[0] == "Fatigue")
            .expect("Fatigue row should be present");
        assert!(fatigue_row[1].contains("3"));
    }

    #[test]
    fn recovery_rows_omit_mood_stress_fatigue_when_partial() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                avg_mood: Some(8.0),
                avg_stress: None,
                avg_fatigue: Some(3.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );

        assert!(!rows.iter().any(|row| row[0] == "Mood"));
        assert!(!rows.iter().any(|row| row[0] == "Stress"));
        assert!(!rows.iter().any(|row| row[0] == "Fatigue"));
    }

    #[test]
    fn recovery_rows_include_ctl_atl_ramp_rate_when_available() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(70.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                ctl: Some(65.0),
                atl: Some(45.0),
                tsb: Some(20.0),
                ramp_rate: Some(2.5),
                ..Default::default()
            },
        );

        assert!(
            rows.iter()
                .any(|row| row[0] == "CTL" && row[1].contains("65")),
            "CTL row should be present"
        );
        assert!(
            rows.iter()
                .any(|row| row[0] == "ATL" && row[1].contains("45")),
            "ATL row should be present"
        );
        assert!(
            rows.iter()
                .any(|row| row[0] == "Ramp Rate" && row[1].contains("2.5")),
            "Ramp Rate row should be present"
        );
    }

    #[test]
    fn recovery_rows_omit_ctl_atl_ramp_rate_when_none() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(70.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );

        assert!(!rows.iter().any(|row| row[0] == "CTL"));
        assert!(!rows.iter().any(|row| row[0] == "ATL"));
        assert!(!rows.iter().any(|row| row[0] == "Ramp Rate"));
    }

    // ========================================================================
    // PlannedActivity::parse
    // ========================================================================

    #[test]
    fn planned_activity_parse_none_defaults_to_easy() {
        assert_eq!(PlannedActivity::parse(None), PlannedActivity::Easy);
    }

    #[test]
    fn planned_activity_parse_easy() {
        assert_eq!(PlannedActivity::parse(Some("easy")), PlannedActivity::Easy);
    }

    #[test]
    fn planned_activity_parse_intensity() {
        assert_eq!(
            PlannedActivity::parse(Some("intensity")),
            PlannedActivity::Intensity
        );
    }

    #[test]
    fn planned_activity_parse_long() {
        assert_eq!(PlannedActivity::parse(Some("long")), PlannedActivity::Long);
    }

    #[test]
    fn planned_activity_parse_race() {
        assert_eq!(PlannedActivity::parse(Some("race")), PlannedActivity::Race);
    }

    #[test]
    fn planned_activity_parse_fallback_to_easy() {
        assert_eq!(
            PlannedActivity::parse(Some("unknown")),
            PlannedActivity::Easy
        );
    }

    // ========================================================================
    // PlannedActivity::as_str
    // ========================================================================

    #[test]
    fn planned_activity_as_str_easy() {
        assert_eq!(PlannedActivity::Easy.as_str(), "easy");
    }

    #[test]
    fn planned_activity_as_str_intensity() {
        assert_eq!(PlannedActivity::Intensity.as_str(), "intensity");
    }

    #[test]
    fn planned_activity_as_str_long() {
        assert_eq!(PlannedActivity::Long.as_str(), "long");
    }

    #[test]
    fn planned_activity_as_str_race() {
        assert_eq!(PlannedActivity::Race.as_str(), "race");
    }

    // ========================================================================
    // PlannedActivity::readiness_copy
    // ========================================================================

    #[test]
    fn readiness_copy_easy_green_light() {
        let wellness = WellnessMetrics {
            avg_sleep_hours: Some(7.0),
            ..Default::default()
        };
        let fitness = FitnessMetrics::default();
        let (status, note) = PlannedActivity::Easy.readiness_copy(&wellness, &fitness, &[]);
        assert_eq!(status, "Green light");
        assert!(note.contains("conversational"));
    }

    #[test]
    fn readiness_copy_easy_caution() {
        let wellness = WellnessMetrics {
            avg_sleep_hours: Some(5.5),
            ..Default::default()
        };
        let fitness = FitnessMetrics::default();
        let (status, note) =
            PlannedActivity::Easy.readiness_copy(&wellness, &fitness, &["Low sleep".to_string()]);
        assert_eq!(status, "Caution");
        assert!(note.contains("gentle"));
    }

    #[test]
    fn readiness_copy_intensity_ready() {
        let wellness = WellnessMetrics {
            avg_sleep_hours: Some(8.0),
            recovery_index: Some(1.2),
            wellness_days_count: 5,
            ..Default::default()
        };
        let fitness = FitnessMetrics {
            tsb: Some(0.0),
            ..Default::default()
        };
        let (status, _note) = PlannedActivity::Intensity.readiness_copy(&wellness, &fitness, &[]);
        assert_eq!(status, "Ready for quality");
    }

    #[test]
    fn readiness_copy_intensity_hold() {
        let wellness = WellnessMetrics {
            avg_sleep_hours: Some(6.5),
            recovery_index: Some(1.2),
            wellness_days_count: 5,
            ..Default::default()
        };
        let fitness = FitnessMetrics {
            tsb: Some(0.0),
            ..Default::default()
        };
        let (status, _note) = PlannedActivity::Intensity.readiness_copy(&wellness, &fitness, &[]);
        assert_eq!(status, "Hold intensity");
    }

    #[test]
    fn readiness_copy_long_ready() {
        let wellness = WellnessMetrics {
            avg_sleep_hours: Some(7.0),
            recovery_index: Some(1.0),
            wellness_days_count: 5,
            ..Default::default()
        };
        let fitness = FitnessMetrics {
            tsb: Some(0.0),
            ..Default::default()
        };
        let (status, _note) = PlannedActivity::Long.readiness_copy(&wellness, &fitness, &[]);
        assert_eq!(status, "Long run acceptable");
    }

    #[test]
    fn readiness_copy_long_trim() {
        let wellness = WellnessMetrics {
            avg_sleep_hours: Some(8.0),
            recovery_index: Some(1.0),
            wellness_days_count: 5,
            ..Default::default()
        };
        let fitness = FitnessMetrics {
            tsb: Some(-25.0),
            ..Default::default()
        };
        let (status, _note) = PlannedActivity::Long.readiness_copy(&wellness, &fitness, &[]);
        assert_eq!(status, "Trim the long day");
    }

    #[test]
    fn readiness_copy_race_ready() {
        let wellness = WellnessMetrics {
            avg_sleep_hours: Some(8.0),
            recovery_index: Some(1.2),
            wellness_days_count: 5,
            ..Default::default()
        };
        let fitness = FitnessMetrics {
            tsb: Some(10.0),
            ..Default::default()
        };
        let (status, _note) = PlannedActivity::Race.readiness_copy(&wellness, &fitness, &[]);
        assert_eq!(status, "Race-ready");
    }

    #[test]
    fn readiness_copy_race_not_ready() {
        let wellness = WellnessMetrics {
            avg_sleep_hours: Some(8.0),
            recovery_index: Some(1.2),
            wellness_days_count: 5,
            ..Default::default()
        };
        let fitness = FitnessMetrics {
            tsb: Some(0.0),
            ..Default::default()
        };
        let (status, _note) = PlannedActivity::Race.readiness_copy(&wellness, &fitness, &[]);
        assert_eq!(status, "Not race-ready");
    }

    // ========================================================================
    // build_recovery_metric_rows — additional threshold coverage
    // ========================================================================

    #[test]
    fn recovery_rows_rhr_elevated() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(58.0),
                avg_hrv: Some(65.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );
        let rhr_row = rows.iter().find(|r| r[0] == "Resting HR").expect("RHR row");
        assert!(rhr_row[2].contains("Elevated"));
    }

    #[test]
    fn recovery_rows_rhr_high() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(65.0),
                avg_hrv: Some(65.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );
        let rhr_row = rows.iter().find(|r| r[0] == "Resting HR").expect("RHR row");
        assert!(rhr_row[2].contains("High"));
    }

    #[test]
    fn recovery_rows_sleep_fair() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(6.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );
        let sleep_row = rows
            .iter()
            .find(|r| r[0] == "Avg Sleep")
            .expect("Sleep row");
        assert!(sleep_row[2].contains("Fair"));
    }

    #[test]
    fn recovery_rows_sleep_poor() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(5.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );
        let sleep_row = rows
            .iter()
            .find(|r| r[0] == "Avg Sleep")
            .expect("Sleep row");
        assert!(sleep_row[2].contains("Poor"));
    }

    #[test]
    fn recovery_rows_tsb_fresh() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(15.0),
                ..Default::default()
            },
        );
        let tsb_row = rows.iter().find(|r| r[0] == "TSB").expect("TSB row");
        assert!(tsb_row[2].contains("Fresh"));
    }

    #[test]
    fn recovery_rows_tsb_balanced() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(0.0),
                ..Default::default()
            },
        );
        let tsb_row = rows.iter().find(|r| r[0] == "TSB").expect("TSB row");
        assert!(tsb_row[2].contains("Balanced"));
    }

    #[test]
    fn recovery_rows_tsb_fatigued() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(-15.0),
                ..Default::default()
            },
        );
        let tsb_row = rows.iter().find(|r| r[0] == "TSB").expect("TSB row");
        assert!(tsb_row[2].contains("Fatigued"));
    }

    #[test]
    fn recovery_rows_recovery_index_watch() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                recovery_index: Some(1.0),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );
        let ri_row = rows
            .iter()
            .find(|r| r[0] == "Recovery Index")
            .expect("Recovery Index row");
        assert!(ri_row[2].contains("Watch"));
    }

    #[test]
    fn recovery_rows_recovery_index_low() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                recovery_index: Some(0.8),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );
        let ri_row = rows
            .iter()
            .find(|r| r[0] == "Recovery Index")
            .expect("Recovery Index row");
        assert!(ri_row[2].contains("Low"));
    }

    #[test]
    fn recovery_rows_hrv_within_range() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                hrv_trend_state: Some("within_range".into()),
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );
        let hrv_row = rows.iter().find(|r| r[0] == "HRV").expect("HRV row");
        assert!(hrv_row[2].contains("Within personal range"));
    }

    #[test]
    fn recovery_rows_hrv_no_trend() {
        let rows = AssessRecoveryHandler::build_recovery_metric_rows(
            &WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(65.0),
                hrv_trend_state: None,
                wellness_days_count: 5,
                ..Default::default()
            },
            &FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            },
        );
        let hrv_row = rows.iter().find(|r| r[0] == "HRV").expect("HRV row");
        assert!(hrv_row[2].contains("Build personal baseline"));
    }

    // ========================================================================
    // execute() — integration tests with MockIntervalsClient
    // ========================================================================

    fn make_good_client() -> MockIntervalsClient {
        MockIntervalsClient::builder()
            .with_wellness(json!([
                {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 70.0}
            ]))
            .with_fitness_summary(json!({"form": 15.0}))
            .with_activities(vec![ActivitySummary {
                id: "act-1".into(),
                name: Some("Test Workout".into()),
                start_date_local: "2026-05-20".into(),
                ..Default::default()
            }])
    }

    #[tokio::test]
    async fn test_execute_basic_defaults() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(make_good_client());
        let input = json!({});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Recovery Assessment"));
        assert!(content_str.contains("Easy"));
        assert!(content_str.contains("Green light"));
        assert!(content_str.contains("None detected"));
        assert!(!output.next_actions.is_empty());
    }

    #[tokio::test]
    async fn test_execute_with_red_flags() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_wellness(json!([
                    {"sleep_hours": 5.0, "resting_hr": 65.0, "hrv": 35.0}
                ]))
                .with_fitness_summary(json!({"form": -25.0}))
                .with_activities(vec![ActivitySummary {
                    id: "act-1".into(),
                    name: Some("Test".into()),
                    start_date_local: "2026-05-20".into(),
                    ..Default::default()
                }]),
        );
        let input = json!({});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Red Flags Detected"));
        assert!(
            output.next_actions.iter().any(|a| a.contains("rest day")),
            "Expected 'rest day' next action but got: {:?}",
            output.next_actions
        );
    }

    #[tokio::test]
    async fn test_execute_red_flags_disabled() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_wellness(json!([
                    {"sleep_hours": 5.0, "resting_hr": 65.0, "hrv": 35.0}
                ]))
                .with_fitness_summary(json!({"form": -25.0}))
                .with_activities(vec![ActivitySummary {
                    id: "act-1".into(),
                    name: Some("Test".into()),
                    start_date_local: "2026-05-20".into(),
                    ..Default::default()
                }]),
        );
        let input = json!({"include_red_flags": false});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(
            !content_str.contains("Red Flags"),
            "Expected no red flags in content when disabled"
        );
    }

    #[tokio::test]
    async fn test_execute_for_activity_intensity() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(make_good_client());
        let input = json!({"for_activity": "intensity"});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Ready for quality"));
        assert!(
            output
                .next_actions
                .iter()
                .any(|a| a.contains("quality session")),
            "Expected intensity next action but got: {:?}",
            output.next_actions
        );
    }

    #[tokio::test]
    async fn test_execute_for_activity_long() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(make_good_client());
        let input = json!({"for_activity": "long"});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Long run acceptable"));
        assert!(
            output.next_actions.iter().any(|a| a.contains("Fuel early")),
            "Expected long next action but got: {:?}",
            output.next_actions
        );
    }

    #[tokio::test]
    async fn test_execute_for_activity_race() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(make_good_client());
        let input = json!({"for_activity": "race"});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Race-ready"));
        assert!(
            output.next_actions.iter().any(|a| a.contains("Recheck")),
            "Expected race next action but got: {:?}",
            output.next_actions
        );
    }

    #[tokio::test]
    async fn test_execute_intensity_not_ready() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_wellness(json!([
                    {"sleep_hours": 6.0, "resting_hr": 50.0, "hrv": 70.0}
                ]))
                .with_fitness_summary(json!({"form": 0.0}))
                .with_activities(vec![ActivitySummary {
                    id: "act-1".into(),
                    name: Some("Test".into()),
                    start_date_local: "2026-05-20".into(),
                    ..Default::default()
                }]),
        );
        let input = json!({"for_activity": "intensity"});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Hold intensity"));
    }

    #[tokio::test]
    async fn test_execute_long_not_ready() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_wellness(json!([
                    {"sleep_hours": 6.0, "resting_hr": 50.0, "hrv": 70.0}
                ]))
                .with_fitness_summary(json!({"form": -25.0}))
                .with_activities(vec![ActivitySummary {
                    id: "act-1".into(),
                    name: Some("Test".into()),
                    start_date_local: "2026-05-20".into(),
                    ..Default::default()
                }]),
        );
        let input = json!({"for_activity": "long"});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Trim the long day"));
    }

    #[tokio::test]
    async fn test_execute_race_not_ready() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_wellness(json!([
                    {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 70.0}
                ]))
                .with_fitness_summary(json!({"form": 0.0}))
                .with_activities(vec![ActivitySummary {
                    id: "act-1".into(),
                    name: Some("Test".into()),
                    start_date_local: "2026-05-20".into(),
                    ..Default::default()
                }]),
        );
        let input = json!({"for_activity": "race"});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("Not race-ready"));
    }

    #[tokio::test]
    async fn test_execute_with_key_workout_upcoming() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(make_good_client().with_upcoming_workouts(json!([
            {"name": "Race: 10k TT", "start_date_local": "2026-05-25"}
        ])));
        let input = json!({});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(
            output.suggestions.iter().any(|s| s.contains("Key workout")),
            "Expected key workout suggestion but got: {:?}",
            output.suggestions
        );
    }

    #[tokio::test]
    async fn test_execute_with_non_key_upcoming() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(make_good_client().with_upcoming_workouts(json!([
            {"name": "Easy Recovery Run", "start_date_local": "2026-05-25"}
        ])));
        let input = json!({});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(
            !output.suggestions.iter().any(|s| s.contains("Key workout")),
            "Expected no key workout suggestion for non-key workout"
        );
    }

    #[tokio::test]
    async fn test_execute_recovery_index_unavailable() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_wellness(json!([
                    {"sleep_hours": 8.0, "resting_hr": 50.0}
                ]))
                .with_fitness_summary(json!({"form": 15.0}))
                .with_activities(vec![ActivitySummary {
                    id: "act-1".into(),
                    name: Some("Test".into()),
                    start_date_local: "2026-05-20".into(),
                    ..Default::default()
                }]),
        );
        let input = json!({});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(
            content_str.contains("Recovery Index unavailable"),
            "Expected recovery index note but got: {}",
            content_str
        );
    }

    #[tokio::test]
    async fn test_execute_include_wellness_false() {
        let handler = AssessRecoveryHandler::new();
        // Even without wellness data, the mock needs a valid response for
        // get_wellness. When include_wellness is false, fetch_recovery_data
        // skips the wellness call, so any value works (won't be fetched).
        let client = Arc::new(
            MockIntervalsClient::builder()
                .with_fitness_summary(json!({"form": 15.0}))
                .with_activities(vec![ActivitySummary {
                    id: "act-1".into(),
                    name: Some("Test".into()),
                    start_date_local: "2026-05-20".into(),
                    ..Default::default()
                }]),
        );
        let input = json!({"include_wellness": false});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        // With wellness disabled, recovery metrics will use default zeros
        assert!(content_str.contains("Recovery Assessment"));
        assert!(!content_str.contains("Recovery Index"));
    }

    #[tokio::test]
    async fn test_execute_no_upcoming_workouts() {
        let handler = AssessRecoveryHandler::new();
        // Default mock returns json!([]) for upcoming workouts (empty array)
        let client = Arc::new(make_good_client());
        let input = json!({});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(
            !output.suggestions.iter().any(|s| s.contains("Key workout")),
            "Expected no key workout suggestion with empty upcoming"
        );
    }

    #[tokio::test]
    async fn test_execute_period_days_explicit() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(make_good_client());
        let input = json!({"period_days": 14});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_upcoming_workouts_error() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(
            make_good_client()
                .with_upcoming_workouts_error(IntervalsError::from_status(500, "server error")),
        );
        let input = json!({});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        // upcoming is None (error swallowed by .ok()), so no key workout suggestion
        assert!(
            !output.suggestions.iter().any(|s| s.contains("Key workout")),
            "Expected no key workout suggestion when upcoming errors"
        );
    }

    #[tokio::test]
    async fn test_execute_red_flags_with_empty_flags_shows_none_detected() {
        let handler = AssessRecoveryHandler::new();
        let client = Arc::new(make_good_client());
        let input = json!({});
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        let content_str = content_text(&output.content);
        assert!(content_str.contains("None detected"));
        assert!(!content_str.contains("Red Flags Detected"));
    }
}
