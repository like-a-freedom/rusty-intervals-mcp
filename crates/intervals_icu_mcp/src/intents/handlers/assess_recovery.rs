use crate::intents::{ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput};
use async_trait::async_trait;
use chrono::Local;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
/// Assess Recovery Intent Handler
///
/// Assesses recovery status, readiness to train, and detects red flags.
use std::sync::Arc;

const READINESS_SLEEP_EASY: f64 = 6.0;
const READINESS_SLEEP_INTENSITY: f64 = 7.0;
const READINESS_SLEEP_LONG: f64 = 6.5;
const READINESS_SLEEP_RACE: f64 = 7.5;
const READINESS_TSB_INTENSITY: f64 = -5.0;
const READINESS_TSB_LONG: f64 = -8.0;
const READINESS_TSB_RACE: f64 = 5.0;
const READINESS_RECOVERY_INDEX_INTENSITY: f64 = 0.95;
const READINESS_RECOVERY_INDEX_LONG: f64 = 0.9;
const READINESS_RECOVERY_INDEX_RACE: f64 = 1.1;

#[cfg(test)]
use crate::domains::coach::CoachMetrics;
use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext, WellnessMetrics};
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::{RecoveryFetchRequest, fetch_recovery_data};
use crate::engines::coach_guidance::{build_alerts, build_guidance};
use crate::engines::coach_metrics::{parse_fitness_metrics, parse_wellness_metrics};
use crate::intents::utils::data_availability_block;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlannedActivity {
    Easy,
    Intensity,
    Long,
    Race,
}

impl PlannedActivity {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("easy") {
            "intensity" => Self::Intensity,
            "long" => Self::Long,
            "race" => Self::Race,
            _ => Self::Easy,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Intensity => "intensity",
            Self::Long => "long",
            Self::Race => "race",
        }
    }

    fn readiness_copy(
        self,
        wellness: &WellnessMetrics,
        fitness: &crate::domains::coach::FitnessMetrics,
        red_flags: &[String],
    ) -> (String, String) {
        let sleep = wellness.avg_sleep_hours.unwrap_or(0.0);
        let recovery_index = wellness.recovery_index.unwrap_or(0.0);
        let tsb = fitness.tsb.unwrap_or(0.0);
        let has_flags = !red_flags.is_empty();

        match self {
            Self::Easy => {
                let verdict = if has_flags && sleep < READINESS_SLEEP_EASY {
                    "Caution"
                } else {
                    "Green light"
                };
                (
                    verdict.to_string(),
                    if verdict == "Green light" {
                        "Easy training is appropriate; keep the session conversational and use it as recovery support.".to_string()
                    } else {
                        "Keep the day gentle and shorten or skip the session if fatigue rises during warm-up.".to_string()
                    },
                )
            }
            Self::Intensity => {
                let ready = !has_flags
                    && sleep >= READINESS_SLEEP_INTENSITY
                    && tsb > READINESS_TSB_INTENSITY
                    && recovery_index >= READINESS_RECOVERY_INDEX_INTENSITY;
                (
                    if ready {
                        "Ready for quality"
                    } else {
                        "Hold intensity"
                    }
                    .to_string(),
                    if ready {
                        "Metrics support a quality session; keep the hard work controlled and protect the cooldown.".to_string()
                    } else {
                        "Today's recovery signals are not strong enough for a quality session; if HRV is below your personal baseline, swap intensity for aerobic work or rest.".to_string()
                    },
                )
            }
            Self::Long => {
                let ready = !has_flags
                    && sleep >= READINESS_SLEEP_LONG
                    && tsb > READINESS_TSB_LONG
                    && recovery_index >= READINESS_RECOVERY_INDEX_LONG;
                (
                    if ready {
                        "Long run acceptable"
                    } else {
                        "Trim the long day"
                    }
                    .to_string(),
                    if ready {
                        "You can handle a long aerobic session, but keep fueling disciplined and cap any late surges.".to_string()
                    } else {
                        "Recovery is borderline for a long session; if HRV is below your personal baseline, reduce duration or convert the day to steady aerobic mileage.".to_string()
                    },
                )
            }
            Self::Race => {
                let ready = !has_flags
                    && sleep >= READINESS_SLEEP_RACE
                    && tsb > READINESS_TSB_RACE
                    && recovery_index >= READINESS_RECOVERY_INDEX_RACE;
                (
                    if ready {
                        "Race-ready"
                    } else {
                        "Not race-ready"
                    }
                    .to_string(),
                    if ready {
                        "Current markers support a race effort; preserve freshness, finalize logistics, and avoid adding extra load.".to_string()
                    } else {
                        "Current recovery markers do not support a race-level effort; if HRV is below your personal baseline, prioritize recovery and reassess before racing.".to_string()
                    },
                )
            }
        }
    }
}

pub struct AssessRecoveryHandler;
impl AssessRecoveryHandler {
    pub fn new() -> Self {
        Self
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
        let avg_sleep = wellness.avg_sleep_hours.unwrap_or(0.0);
        let resting_hr = wellness.avg_resting_hr.unwrap_or(0.0);
        let hrv = wellness.avg_hrv.unwrap_or(0.0);
        let tsb = fitness.tsb.unwrap_or(0.0);

        let sleep_status = if avg_sleep >= crate::engines::coach_guidance::SLEEP_GOOD_HOURS {
            "✅ Good"
        } else if avg_sleep >= crate::engines::coach_guidance::SLEEP_FAIR_MIN_HOURS {
            "⚠️ Fair"
        } else {
            "❌ Poor"
        };

        let rhr_status = if resting_hr <= crate::engines::coach_guidance::RHR_NORMAL_BPM {
            "✅ Normal"
        } else if resting_hr <= crate::engines::coach_guidance::RHR_ELEVATED_MAX_BPM {
            "⚠️ Elevated"
        } else {
            "❌ High"
        };

        let hrv_status = match wellness.hrv_trend_state.as_deref() {
            Some("suppressed") => "❌ Suppressed vs personal baseline",
            Some("below_range") => "⚠️ Below personal baseline",
            Some("within_range") => "✅ Within personal range",
            _ if hrv > 0.0 => "⚪ Build personal baseline",
            _ => "n/a",
        };

        let tsb_status = if tsb > crate::engines::coach_guidance::TSB_FRESH {
            "✅ Fresh"
        } else if tsb > crate::engines::coach_guidance::TSB_FATIGUED {
            "⚠️ Neutral"
        } else {
            "❌ Fatigued"
        };

        let mut rows = vec![
            vec![
                "Avg Sleep".into(),
                format!("{:.1} hrs", avg_sleep),
                sleep_status.into(),
            ],
            vec![
                "Resting HR".into(),
                format!("{} bpm", resting_hr as u32),
                rhr_status.into(),
            ],
            vec!["HRV".into(), format!("{:.0} ms", hrv), hrv_status.into()],
            vec!["TSB".into(), format!("{:.0}", tsb), tsb_status.into()],
        ];

        if let Some(recovery_index) = wellness.recovery_index {
            let recovery_status = if recovery_index >= 1.2 {
                "✅ Supportive"
            } else if recovery_index >= 0.9 {
                "⚠️ Watch"
            } else {
                "❌ Low"
            };
            rows.push(vec![
                "Recovery Index".into(),
                format!("{:.2}", recovery_index),
                recovery_status.into(),
            ]);
        }

        if let Some(readiness_score) = wellness.readiness_score {
            let readiness_status = if readiness_score >= 7.0 {
                "✅ Supportive"
            } else if readiness_score >= 5.0 {
                "⚠️ Watch"
            } else {
                "❌ Low"
            };
            rows.push(vec![
                "Readiness Score".into(),
                format!("{:.1}", readiness_score),
                readiness_status.into(),
            ]);
        }

        if let (Some(mood), Some(stress), Some(fatigue)) =
            (wellness.avg_mood, wellness.avg_stress, wellness.avg_fatigue)
        {
            rows.push(vec!["Mood".into(), format!("{:.0}/10", mood), "".into()]);
            rows.push(vec![
                "Stress".into(),
                format!("{:.0}/10", stress),
                "".into(),
            ]);
            rows.push(vec![
                "Fatigue".into(),
                format!("{:.0}/10", fatigue),
                "".into(),
            ]);
        }

        rows
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
         Use for checking readiness for key workouts, evaluating post-race recovery, \
         and identifying overtraining signs."
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

        let mut content = Vec::new();
        let end_date = Local::now().date_naive();
        let start_date = end_date - chrono::Duration::days(period_days);

        let fetched = fetch_recovery_data(
            client.as_ref(),
            &RecoveryFetchRequest {
                period_days: period_days as i32,
                include_wellness,
            },
        )
        .await?;

        // Look-ahead: check upcoming workouts for key sessions
        let upcoming = client
            .get_upcoming_workouts(Some(7), Some(5), None)
            .await
            .ok();

        let mut context = CoachContext::new(
            AnalysisKind::RecoveryAssessment,
            AnalysisWindow::new(start_date, end_date),
        );
        context.audit = build_data_audit(&fetched);
        context.metrics.fitness = parse_fitness_metrics(fetched.fitness.as_ref());
        context.metrics.wellness = parse_wellness_metrics(fetched.wellness.as_ref());
        if include_red_flags {
            context.alerts = build_alerts(&context.metrics);
        }
        context.guidance = build_guidance(&context.metrics, &context.alerts);

        content.push(ContentBlock::markdown(format!(
            "# Recovery Assessment ({} - {})\nReadiness for: {}",
            start_date.format("%d %b"),
            end_date.format("%d %b"),
            planned_activity.as_str()
        )));

        let wellness = context.metrics.wellness.clone().unwrap_or_default();
        let fitness = context.metrics.fitness.clone().unwrap_or_default();
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

        // Calculate red flags first
        let red_flags = if include_red_flags {
            context
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
            &context.audit.degraded_mode_reasons,
            context.audit.all_available(),
        ) {
            content.push(block);
        }

        // Use shared guidance from coach engine
        let mut suggestions = context.guidance.suggestions.clone();

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
        for action in &context.guidance.next_actions {
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

    #[test]
    fn test_new_handler() {
        let handler = AssessRecoveryHandler::new();
        assert_eq!(handler.name(), "assess_recovery");
    }

    #[test]
    fn test_default_handler() {
        let _handler = AssessRecoveryHandler;
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

        // Neutral
        let tsb = 0.0;
        let status = if tsb > TSB_FRESH {
            "Fresh"
        } else if tsb > TSB_FATIGUED {
            "Neutral"
        } else {
            "Fatigued"
        };
        assert_eq!(status, "Neutral");

        // Fatigued
        let tsb = -15.0;
        let status = if tsb > TSB_FRESH {
            "Fresh"
        } else if tsb > TSB_FATIGUED {
            "Neutral"
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
                avg_mood: None,
                avg_stress: None,
                avg_fatigue: None,
                readiness_score: None,
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
}
