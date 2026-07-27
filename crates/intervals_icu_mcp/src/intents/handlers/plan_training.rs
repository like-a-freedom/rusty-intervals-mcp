use crate::intents::{
    ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput, OutputMetadata,
};
use async_trait::async_trait;
use chrono::Datelike;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
/// Plan Training Intent Handler
///
/// Plans training across various horizons (microcycle to annual plan).
use std::sync::Arc;

use crate::domains::events::validate_and_prepare_event;
use crate::engines::fitness_context::FitnessContext;
use crate::engines::forecast::{
    TAPER_ACTUAL_REDUCTION_PCT, TAPER_TARGET_REDUCTION_PCT, parameterized_load, project_tsb,
};
use crate::intents::utils::parse_date;

pub struct PlanTrainingHandler;
impl PlanTrainingHandler {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Parse and validate the JSON input for the plan_training intent.
    ///
    /// Extracts the required `period_start` / `period_end` strings, parses
    /// them into `NaiveDate` values, and resolves the optional `focus`,
    /// `max_hours_per_week`, and `adaptive` fields. Returns a fully-typed
    /// `PlanInputs` value or an `IntentError::validation` describing the
    /// first problem found.
    fn parse_plan_inputs(input: &Value) -> Result<PlanInputs, IntentError> {
        let period_start = input
            .get("period_start")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: period_start"))?;
        let period_end = input
            .get("period_end")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: period_end"))?;
        let focus = TrainingFocus::parse(input.get("focus").and_then(Value::as_str));
        let max_hours = input
            .get("max_hours_per_week")
            .and_then(Value::as_f64)
            .unwrap_or(10.0);
        let adaptive = input
            .get("adaptive")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let start_date = parse_date(period_start, "period_start")?;
        let end_date = parse_date(period_end, "period_end")?;

        if start_date > end_date {
            return Err(IntentError::validation(
                "Start date must be before end date.".to_string(),
            ));
        }

        let weeks: u32 = u32::try_from((end_date - start_date).num_days() / 7 + 1).unwrap_or(0);

        Ok(PlanInputs {
            period_start: period_start.to_string(),
            period_end: period_end.to_string(),
            start_date,
            end_date,
            focus,
            max_hours,
            adaptive,
            weeks,
        })
    }
}

#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrainingFocus {
    AerobicBase,
    Intensity,
    Specific,
    Taper,
    Recovery,
}

impl TrainingFocus {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("aerobic_base") {
            "intensity" => Self::Intensity,
            "specific" => Self::Specific,
            "taper" => Self::Taper,
            "recovery" => Self::Recovery,
            _ => Self::AerobicBase,
        }
    }

    fn to_load_label(self) -> &'static str {
        match self {
            Self::AerobicBase | Self::Recovery => "easy",
            Self::Intensity => "tempo",
            Self::Specific => "hard",
            Self::Taper => "easy",
        }
    }

    #[must_use]
    fn as_str(self) -> &'static str {
        match self {
            Self::AerobicBase => "aerobic_base",
            Self::Intensity => "intensity",
            Self::Specific => "specific",
            Self::Taper => "taper",
            Self::Recovery => "recovery",
        }
    }
}

#[async_trait]
impl IntentHandler for PlanTrainingHandler {
    fn name(&self) -> &'static str {
        "plan_training"
    }

    fn description(&self) -> &'static str {
        "Plans training across various horizons (week to annual plan). Returns \
         periodized phases with volume/focus, sample week with HR zones, race \
         anchors from calendar, conflict detection against existing events, and \
         Banister TSB forecast (CTL/ATL/TSB projection with fatigue class per \
         milestone). Adaptive mode uses current fitness (TSB, CTL, ATL) and \
         wellness (readiness, HRV, sleep) to calibrate volume and detect overshoot.

         Use this tool when: you need to create a race preparation plan, periodize \
         training for a target event, or generate structured weekly workouts. \
         Implements +7-10% weekly progression, recovery weeks every 3-4 weeks. \
         Do NOT use when: you need to analyze existing training (use analyze_training) \
         or assess recovery (use assess_recovery). Requires idempotency_token."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period_start": {"type": "string", "description": "Period start (YYYY-MM-DD or 'next_monday')"},
                "period_end": {"type": "string", "description": "Period end (YYYY-MM-DD or '12weeks')"},
                "focus": {"type": "string", "enum": ["aerobic_base", "intensity", "specific", "taper", "recovery"], "description": "Period focus"},
                "target_race": {"type": "string", "description": "Target race (description)"},
                "max_hours_per_week": {"type": "number", "description": "Maximum hours per week"},
                "adaptive": {"type": "boolean", "default": true, "description": "Adaptive planning based on current state"},
                "idempotency_token": {"type": "string", "description": "Idempotency token (required)"}
            },
            "required": ["period_start", "period_end", "idempotency_token"]
        })
    }

    #[allow(clippy::too_many_lines)]
    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let plan_inputs = Self::parse_plan_inputs(&input)?;
        let PlanInputs {
            period_start,
            period_end,
            start_date,
            end_date,
            focus,
            max_hours,
            adaptive,
            weeks,
        } = plan_inputs.clone();

        // --- Required fetches ---
        let profile = client
            .get_athlete_profile()
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch profile: {}", e)))?;

        let sport_settings = client.get_sport_settings().await.ok();

        let fitness_context = if adaptive {
            FitnessContext::load(client.as_ref()).await
        } else {
            FitnessContext::empty()
        };
        let fitness_metrics = fitness_context.metrics().cloned();

        // --- Task 2: Wellness ---
        let wellness = if adaptive {
            client.get_wellness(Some(14)).await.ok()
        } else {
            None
        };

        // --- Past events (for context and race anchors) ---
        let past_horizon =
            ((chrono::Utc::now().date_naive() - start_date).num_days() as i32).max(0) + 30;
        let past_events = client
            .get_events(Some(past_horizon), None)
            .await
            .ok()
            .unwrap_or_default();

        // --- Upcoming workouts (future conflicts + display) ---
        let upcoming = client
            .get_upcoming_workouts(Some(weeks * 7), Some(100), None)
            .await
            .ok();

        // --- Historical volume (actual weeks, not hardcoded) ---
        let (historical_avg_hours, historical_weeks) = if adaptive {
            client
                .get_recent_activities(Some(60), Some(56))
                .await
                .ok()
                .and_then(|activities| {
                    let dated: Vec<(chrono::NaiveDate, f64, f64)> = activities
                        .iter()
                        .filter_map(|a| {
                            let date =
                                chrono::NaiveDate::parse_from_str(&a.start_date_local, "%Y-%m-%d")
                                    .ok()?;
                            let moving_secs = a.moving_time? as f64;
                            let elapsed_secs = a.elapsed_time? as f64;
                            Some((date, moving_secs, elapsed_secs))
                        })
                        .collect();
                    if dated.is_empty() {
                        return None;
                    }
                    let oldest = dated.iter().map(|(d, _, _)| *d).min()?;
                    let newest = dated.iter().map(|(d, _, _)| *d).max()?;
                    let weeks = ((newest - oldest).num_days() as f64 / 7.0).max(1.0);
                    let total_moving_seconds: f64 = dated.iter().map(|(_, s, _)| s).sum();
                    let total_elapsed_seconds: f64 = dated.iter().map(|(_, _, e)| e).sum();
                    Some((
                        (
                            total_moving_seconds / 3600.0 / weeks,
                            total_elapsed_seconds / 3600.0 / weeks,
                        ),
                        weeks,
                    ))
                })
                .unzip()
        } else {
            (None, None)
        };

        // --- Parse extracted data ---
        let sport_info = sport_settings
            .as_ref()
            .map(ExtractedSportSettings::from_sport_settings)
            .unwrap_or_default();

        let wellness_snapshot = wellness
            .as_ref()
            .map(WellnessSnapshot::from_value)
            .unwrap_or_default();

        // --- Conflict detection (Fix 1+2: exclude RaceA/B, check upcoming) ---
        // Non-race existing events in period
        let existing_conflicts: Vec<&intervals_icu_client::Event> = past_events
            .iter()
            .filter(|e| {
                !matches!(
                    e.category,
                    intervals_icu_client::EventCategory::RaceA
                        | intervals_icu_client::EventCategory::RaceB
                )
            })
            .filter(|e| {
                chrono::NaiveDate::parse_from_str(&e.start_date_local, "%Y-%m-%d")
                    .map(|d| d >= start_date && d <= end_date)
                    .unwrap_or(false)
            })
            .collect();

        // Upcoming workout dates in period (future conflicts)
        let upcoming_conflict_dates: Vec<String> = upcoming
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|w| {
                        let date_str = w.get("start_date_local").and_then(|v| v.as_str())?;
                        let date = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
                        if date >= start_date && date <= end_date {
                            let name = w.get("name").and_then(|v| v.as_str()).unwrap_or("Workout");
                            Some(format!("{} ({})", name, date_str))
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !existing_conflicts.is_empty() || !upcoming_conflict_dates.is_empty() {
            let mut conflict_content = Vec::new();

            if !existing_conflicts.is_empty() {
                conflict_content.push(ContentBlock::markdown(
                    "# Conflict Detected\nExisting events overlap with the planned period. \
                     Remove or reschedule them before creating a new plan."
                        .to_string(),
                ));
                let mut conflict_rows = vec![vec!["Date".into(), "Name".into(), "Category".into()]];
                for c in &existing_conflicts {
                    conflict_rows.push(vec![
                        c.start_date_local.clone(),
                        c.name.clone(),
                        format!("{:?}", c.category),
                    ]);
                }
                conflict_content.push(ContentBlock::table(
                    conflict_rows[0].clone(),
                    conflict_rows[1..].to_vec(),
                ));
            }

            if !upcoming_conflict_dates.is_empty() {
                conflict_content.push(ContentBlock::markdown(format!(
                    "# Existing Plan Detected\n{} workouts already scheduled in this period. \
                     Remove existing plan before creating a new one.",
                    upcoming_conflict_dates.len()
                )));
            }

            return Ok(IntentOutput::new(conflict_content)
                .with_suggestions(vec![
                    "Events or workouts found in planning period.".into(),
                    "Remove conflicts or adjust period_start/period_end.".into(),
                ])
                .with_next_actions(vec![
                    "To delete conflicts: modify_training with action: delete".into(),
                    "To reschedule: modify_training with action: modify".into(),
                ])
                .with_metadata(OutputMetadata {
                    events_created: Some(0),
                    ..Default::default()
                }));
        }

        // --- Race anchors (from past_events and upcoming) ---
        let mut race_anchors: Vec<(String, String, String)> = past_events
            .iter()
            .filter(|e| {
                matches!(
                    e.category,
                    intervals_icu_client::EventCategory::RaceA
                        | intervals_icu_client::EventCategory::RaceB
                )
            })
            .filter(|e| {
                chrono::NaiveDate::parse_from_str(&e.start_date_local, "%Y-%m-%d")
                    .map(|d| d >= start_date && d <= end_date)
                    .unwrap_or(false)
            })
            .map(|e| {
                (
                    e.start_date_local.clone(),
                    e.name.clone(),
                    format!("{:?}", e.category),
                )
            })
            .collect();

        // Also check upcoming workouts for race events
        if let Some(ref up) = upcoming
            && let Some(arr) = up.as_array()
        {
            for w in arr {
                let is_race = w
                    .get("category")
                    .and_then(|c| c.as_str())
                    .map(|c| c == "RaceA" || c == "RaceB")
                    .unwrap_or(false);
                if !is_race {
                    continue;
                }
                if let Some(date_str) = w.get("start_date_local").and_then(|v| v.as_str())
                    && let Ok(d) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                    && d >= start_date
                    && d <= end_date
                {
                    let name = w
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Race")
                        .to_string();
                    let category = w
                        .get("category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("RaceA")
                        .to_string();
                    race_anchors.push((date_str.to_string(), name, category));
                }
            }
        }

        // --- Build output ---
        let athlete_name = profile.name.as_deref().unwrap_or("Athlete");
        let mut content = Vec::new();
        let race_info = input
            .get("target_race")
            .and_then(|r| r.as_str())
            .map(|r| format!(" - {}", r))
            .unwrap_or_default();

        // --- Task 6: Enhanced header ---
        let sport_line = sport_info
            .sport_name
            .as_ref()
            .map(|n| format!("\nSport: {}", n))
            .unwrap_or_default();
        let ftp_line = sport_info
            .ftp
            .map(|v| format!("\nFTP: {:.0}W", v))
            .unwrap_or_default();
        let lthr_line = sport_info
            .lthr
            .map(|v| format!(" | LTHR: {:.0} bpm", v))
            .unwrap_or_default();
        let historical_line = historical_avg_hours
            .map(|(moving_avg, elapsed_avg)| {
                let wk_label = historical_weeks
                    .map(|w| format!("{:.0}wk", w))
                    .unwrap_or_else(|| "8wk".into());
                format!(
                    "\nHistorical Avg ({}): {:.1} hrs/wk (moving), {:.1} hrs/wk (elapsed)",
                    wk_label, moving_avg, elapsed_avg
                )
            })
            .unwrap_or_default();
        let tsb_line = fitness_metrics
            .as_ref()
            .and_then(|f| f.tsb)
            .map(|tsb| {
                let state = if tsb > 10.0 {
                    "Fresh"
                } else if tsb < -10.0 {
                    "Fatigued"
                } else {
                    "Balanced"
                };
                format!("\nCurrent TSB: {:.0} ({})", tsb, state)
            })
            .unwrap_or_default();
        let ctl_line = fitness_metrics
            .as_ref()
            .and_then(|f| f.ctl)
            .map(|ctl| format!("\nCurrent CTL: {:.0}", ctl))
            .unwrap_or_default();
        let atl_line = fitness_metrics
            .as_ref()
            .and_then(|f| f.atl)
            .map(|atl| format!("\nCurrent ATL: {:.0}", atl))
            .unwrap_or_default();
        let ramp_rate_line = fitness_metrics
            .as_ref()
            .and_then(|f| f.ramp_rate)
            .map(|rr| format!("\nRamp Rate: {:+.1}/wk", rr))
            .unwrap_or_default();
        let readiness_line = wellness_snapshot
            .readiness
            .map(|r| {
                let state = if r >= 7.0 {
                    "Good"
                } else if r >= 5.0 {
                    "Fair"
                } else {
                    "Low"
                };
                format!("\nReadiness: {:.1} ({})", r, state)
            })
            .unwrap_or_default();

        content.push(ContentBlock::markdown(format!(
            "# Training Plan: {}{}\n\
             Athlete: {}{}{}{}{}{}{}{}{}{}\n\
             Period: {} to {} ({} weeks)\n\
             Focus: {}\n\
             Max Hours/Week: {:.1}",
            focus.as_str().replace('_', " ").to_uppercase(),
            race_info,
            athlete_name,
            sport_line,
            ftp_line,
            lthr_line,
            historical_line,
            tsb_line,
            ctl_line,
            atl_line,
            ramp_rate_line,
            readiness_line,
            period_start,
            period_end,
            weeks,
            focus.as_str(),
            max_hours
        )));

        // --- Race anchors section ---
        if !race_anchors.is_empty() {
            let mut anchor_rows = vec![vec!["Date".into(), "Event".into(), "Category".into()]];
            for (date, name, category) in &race_anchors {
                anchor_rows.push(vec![date.clone(), name.clone(), category.clone()]);
            }
            content.push(ContentBlock::markdown("Race Anchors".to_string()));
            content.push(ContentBlock::table(
                anchor_rows[0].clone(),
                anchor_rows[1..].to_vec(),
            ));
        }

        // --- Periodization ---
        let (phases, structure) = self.build_periodization(weeks, focus, max_hours);

        let mut phase_rows = vec![vec![
            "Phase".into(),
            "Weeks".into(),
            "Volume".into(),
            "Focus".into(),
        ]];
        for phase in &phases {
            phase_rows.push(vec![
                phase.name.clone(),
                phase.weeks.clone(),
                phase.volume.clone(),
                phase.focus.clone(),
            ]);
        }
        content.push(ContentBlock::table(
            phase_rows[0].clone(),
            phase_rows[1..].to_vec(),
        ));

        content.push(ContentBlock::markdown(format!("Structure\n{}", structure)));

        // --- TSB Forecast ---
        if let Some(ref f) = fitness_metrics
            && let (Some(current_ctl), Some(current_atl)) = (f.ctl, f.atl)
        {
            let daily_tss = parameterized_load(focus.to_load_label());
            let daily_loads =
                std::iter::repeat_n(daily_tss, (weeks as usize) * 7).collect::<Vec<_>>();
            let projection = project_tsb(current_ctl, current_atl, &daily_loads);

            let mut forecast_rows = vec![vec![
                "Day".into(),
                "CTL".into(),
                "ATL".into(),
                "TSB".into(),
                "Status".into(),
            ]];
            for entry in &projection {
                if entry.day == 1
                    || entry.day % 7 == 0
                    || entry.day == projection.last().map(|l| l.day).unwrap_or(0)
                {
                    forecast_rows.push(vec![
                        entry.day.to_string(),
                        format!("{:.1}", entry.ctl),
                        format!("{:.1}", entry.atl),
                        format!("{:.1}", entry.tsb),
                        entry.fatigue_class.replace('_', " ").to_string(),
                    ]);
                }
            }
            content.push(ContentBlock::markdown("TSB Forecast".to_string()));
            content.push(ContentBlock::table(
                forecast_rows[0].clone(),
                forecast_rows[1..].to_vec(),
            ));

            // Taper efficiency (only relevant when focus is taper)
            if focus == TrainingFocus::Taper {
                let first_tsb = projection.first().map(|p| p.tsb).unwrap_or(0.0);
                let last_tsb = projection.last().map(|p| p.tsb).unwrap_or(0.0);
                let tsb_gain = last_tsb - first_tsb;
                let reduction_pct = TAPER_ACTUAL_REDUCTION_PCT;
                let target_pct = TAPER_TARGET_REDUCTION_PCT;
                let (efficiency, tsb_response) = crate::engines::forecast::compute_taper_efficiency(
                    reduction_pct,
                    target_pct,
                    tsb_gain,
                );
                let efficiency_label = if efficiency >= 1.0 {
                    "effective"
                } else if efficiency >= 0.7 {
                    "moderate"
                } else {
                    "ineffective"
                };
                content.push(ContentBlock::markdown(format!(
                    "Taper Efficiency\n\
                     ️ Efficiency Ratio: {:.2} ({})\n\
                     ️ TSB Response: {:.1} pts per % volume reduced",
                    efficiency, efficiency_label, tsb_response
                )));
            }
        }

        // --- Sample week with HR zones ---
        content.push(ContentBlock::markdown(self.build_sample_week(
            focus,
            max_hours,
            sport_info.lthr,
        )));

        // --- Task 5: Generate and create events ---
        let events_to_create = generate_events(&phases, start_date, focus, weeks);
        let events_count = u32::try_from(events_to_create.len()).unwrap_or(0);

        let validated_events: Result<Vec<_>, _> = events_to_create
            .into_iter()
            .map(validate_and_prepare_event)
            .collect();

        let validated_events = validated_events
            .map_err(|e| IntentError::api(format!("Failed to validate events: {}", e)))?;

        let created_events = client
            .bulk_create_events(validated_events)
            .await
            .map_err(|e| IntentError::api(format!("Failed to create events: {}", e)))?;

        content.push(ContentBlock::markdown(format!(
            "Created Events: {} workouts\nEvents successfully created in Intervals.icu.",
            created_events.len()
        )));

        // --- Suggestions ---
        let mut suggestions = vec![
            format!(
                "Weeks 1-{}: {} - focus on aerobic base, 85-95% Z1-Z2",
                weeks.min(4),
                phases[0].name
            ),
            "Volume progression: max +7-10% per week".into(),
        ];

        if let Some(ref fm) = fitness_metrics
            && let Some(tsb) = fm.tsb
        {
            if tsb > 10.0 {
                suggestions.push("TSB positive - good base for training load.".into());
            } else if tsb < -10.0 {
                suggestions.push("TSB negative - consider starting with recovery week.".into());
            }
        }

        // Task 2: Wellness suggestions
        if let Some(readiness) = wellness_snapshot.readiness {
            if readiness < 5.0 {
                suggestions.push(format!(
                    "Low readiness ({:.1}) - consider starting with a recovery week.",
                    readiness
                ));
            } else if readiness >= 7.0 {
                suggestions.push(format!(
                    "Readiness {:.1} (Good) - safe to progress load.",
                    readiness
                ));
            }
        }
        if let Some(hrv) = wellness_snapshot.hrv
            && hrv < 40.0
        {
            suggestions.push(format!(
                "HRV very low ({:.0} ms) - monitor recovery before increasing intensity.",
                hrv
            ));
        }
        if let Some(sleep) = wellness_snapshot.sleep_avg
            && sleep < 6.5
        {
            suggestions.push(format!(
                "Sleep average {:.1}h below threshold - prioritize rest.",
                sleep
            ));
        }

        // Task 4: Volume overshoot warning
        if let Some((moving_avg, _elapsed_avg)) = historical_avg_hours
            && max_hours > moving_avg * 1.3
        {
            suggestions.push(format!(
                    "Requested {:.1} hrs/wk exceeds your 8-week average ({:.1} hrs/wk) by {:.0}% - consider a more gradual increase.",
                    max_hours,
                    moving_avg,
                    ((max_hours - moving_avg) / moving_avg * 100.0)
                ));
        }

        let next_actions = vec![
            "To view details after creation: analyze_training with target_type: period".into(),
            "After period: assess_recovery for state evaluation".into(),
            "To modify plan: modify_training with action: modify".into(),
        ];

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions)
            .with_metadata(OutputMetadata {
                events_created: Some(events_count),
                ..Default::default()
            }))
    }

    fn requires_idempotency_token(&self) -> bool {
        true
    }
}

#[must_use]
struct Phase {
    name: String,
    weeks: String,
    volume: String,
    focus: String,
}

/// Parsed and validated input for the `plan_training` intent.
///
/// Built by [`PlanTrainingHandler::parse_plan_inputs`] from the raw JSON
/// `Value` produced by the MCP layer.
#[must_use]
#[derive(Clone, Debug)]
struct PlanInputs {
    period_start: String,
    period_end: String,
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
    focus: TrainingFocus,
    max_hours: f64,
    adaptive: bool,
    weeks: u32,
}

// --- Task 1: Sport settings extraction ---

#[must_use]
#[derive(Default)]
struct ExtractedSportSettings {
    sport_name: Option<String>,
    ftp: Option<f64>,
    lthr: Option<f64>,
}

impl ExtractedSportSettings {
    fn from_sport_settings(
        settings: &intervals_icu_client::domains::workout::SportSettings,
    ) -> Self {
        let first = settings.sports.first();
        Self {
            sport_name: first.and_then(|s| s.name.clone()),
            ftp: first.and_then(|s| s.ftp),
            lthr: first.and_then(|s| s.lthr),
        }
    }
}

// --- Task 2: Wellness snapshot ---

#[must_use]
#[derive(Default)]
struct WellnessSnapshot {
    readiness: Option<f64>,
    hrv: Option<f64>,
    sleep_avg: Option<f64>,
}

impl WellnessSnapshot {
    fn from_value(value: &Value) -> Self {
        let entries = value.as_array().cloned().unwrap_or_default();
        // Use the latest entry (most recent reading)
        let latest = match entries.last() {
            Some(e) => e,
            None => return Self::default(),
        };

        Self {
            readiness: latest.get("readiness").and_then(|v| v.as_f64()),
            hrv: latest.get("hrv").and_then(|v| v.as_f64()),
            sleep_avg: latest.get("sleep").and_then(|v| v.as_f64()),
        }
    }
}

// --- Task 5: Event generation ---

#[must_use]
fn generate_events(
    _phases: &[Phase],
    start_date: chrono::NaiveDate,
    focus: TrainingFocus,
    weeks: u32,
) -> Vec<intervals_icu_client::Event> {
    let mut events = Vec::new();
    let workout_names: Vec<(&str, &str)> = match focus {
        TrainingFocus::AerobicBase => vec![
            ("Easy Run Z1-Z2", "Easy aerobic run, conversational pace"),
            ("Endurance Run Z2", "Steady aerobic effort"),
            ("Recovery Run Z1", "Very easy, active recovery"),
            ("Long Run Z2", "Progressive long aerobic run"),
        ],
        TrainingFocus::Intensity => vec![
            ("Threshold Session", "Zone 3-4 intervals"),
            ("VO2max Intervals", "Short, hard intervals Z4-Z5"),
            ("Easy Aerobic", "Recovery between sessions"),
            (
                "Long Aerobic + Strides",
                "Aerobic with neuromuscular finish",
            ),
        ],
        TrainingFocus::Specific => vec![
            ("Race-Pace Intervals", "Sustained race-specific effort"),
            ("Specific Workout", "Terrain and fueling rehearsal"),
            ("Easy Maintenance", "Aerobic maintenance, low load"),
            ("Long Race-Specific", "Full dress rehearsal"),
        ],
        TrainingFocus::Taper => vec![
            ("Sharpening", "Short pickups, maintain sharpness"),
            ("Race-Pace Activation", "Brief race-pace effort"),
            ("Easy Aerobic", "Very easy, preserve freshness"),
            ("Pre-Race Opener", "Short leg opener"),
        ],
        TrainingFocus::Recovery => vec![
            ("Easy Aerobic", "Gentle aerobic, no intensity"),
            ("Mobility + Strength", "Maintenance strength work"),
            ("Easy Run + Strides", "Light jog with optional strides"),
            ("Cross-Training", "Low-impact activity"),
        ],
    };

    let days_of_week = [
        chrono::Weekday::Tue,
        chrono::Weekday::Thu,
        chrono::Weekday::Sat,
        chrono::Weekday::Sun,
    ];

    for week in 0..weeks {
        // Skip recovery weeks only for long-term periodization focuses
        let skip_recovery = matches!(
            focus,
            TrainingFocus::AerobicBase | TrainingFocus::Intensity | TrainingFocus::Specific
        );
        if skip_recovery && week > 0 && (week + 1) % 4 == 0 {
            continue;
        }

        let week_start = start_date + chrono::Duration::weeks(week as i64);
        for (i, day_offset) in days_of_week.iter().enumerate() {
            let mut current = week_start;
            while current.weekday() != *day_offset {
                current += chrono::Duration::days(1);
            }
            let (name, description) = &workout_names[i % workout_names.len()];
            events.push(intervals_icu_client::Event {
                id: None,
                start_date_local: current.format("%Y-%m-%d").to_string(),
                name: name.to_string(),
                category: intervals_icu_client::EventCategory::Workout,
                description: Some(description.to_string()),
                r#type: None,
            });
        }
    }

    events
}

impl PlanTrainingHandler {
    fn build_periodization(
        &self,
        weeks: u32,
        focus: TrainingFocus,
        max_hours: f64,
    ) -> (Vec<Phase>, String) {
        let mut phases = Vec::new();

        let structure = match focus {
            TrainingFocus::AerobicBase => {
                let base_weeks = weeks.min(8);
                phases.push(Phase {
                    name: "Base Period".into(),
                    weeks: format!("1-{}", base_weeks),
                    volume: format!("{:.0}-{:.0} hrs", max_hours * 0.6, max_hours * 0.8),
                    focus: "Z1-Z2 85-95%".into(),
                });
                if weeks > 8 {
                    phases.push(Phase {
                        name: "Build Period".into(),
                        weeks: format!("{}-{}", base_weeks + 1, weeks),
                        volume: format!("{:.0}-{:.0} hrs", max_hours * 0.8, max_hours),
                        focus: "Z3 introduction".into(),
                    });
                }
                format!(
                    "  Weeks 1-{}: Base Period (aerobic base, {:.0}-{:.0} hrs/week)\n  Recovery weeks: every 3-4 weeks (-40-60% volume)",
                    base_weeks,
                    max_hours * 0.6,
                    max_hours * 0.8
                )
            }
            TrainingFocus::Intensity => {
                phases.push(Phase {
                    name: "Intensity Block".into(),
                    weeks: format!("1-{}", weeks),
                    volume: format!("{:.0}-{:.0} hrs", max_hours * 0.75, max_hours * 0.95),
                    focus: "Threshold + VO2".into(),
                });
                format!(
                    "  Weeks 1-{}: Intensity development with 1-2 quality sessions each week\n  Keep easy days truly easy to absorb the work\n  Recovery weeks every 2-3 weeks or after stacked high-intensity sessions",
                    weeks
                )
            }
            TrainingFocus::Specific => {
                let taper_start = weeks.saturating_sub(1).max(1);
                phases.push(Phase {
                    name: "Specific Preparation".into(),
                    weeks: if taper_start > 1 {
                        format!("1-{}", taper_start)
                    } else {
                        "1".into()
                    },
                    volume: format!("{:.0}-{:.0} hrs", max_hours * 0.8, max_hours),
                    focus: "Race-specific sessions".into(),
                });
                if weeks >= 3 {
                    phases.push(Phase {
                        name: "Specific Taper".into(),
                        weeks: format!("{}-{}", taper_start + 1, weeks),
                        volume: format!("{:.0}-{:.0} hrs", max_hours * 0.5, max_hours * 0.7),
                        focus: "Sharpen + absorb".into(),
                    });
                }
                format!(
                    "  Weeks 1-{}: Race-specific preparation with terrain, fueling, and pace specificity\n  Rehearse key race demands in long sessions\n  Final days emphasize sharpening, logistics, and freshness",
                    weeks
                )
            }
            TrainingFocus::Taper => {
                phases.push(Phase {
                    name: "Taper Period".into(),
                    weeks: format!("1-{}", weeks),
                    volume: format!("{:.0}-{:.0} hrs", max_hours * 0.4, max_hours * 0.6),
                    focus: "Race-specific, reduced volume".into(),
                });
                format!(
                    "  Weeks 1-{}: Taper (volume -50-60%, maintain intensity)\n  Race readiness focus",
                    weeks
                )
            }
            TrainingFocus::Recovery => {
                phases.push(Phase {
                    name: "Recovery Block".into(),
                    weeks: format!("1-{}", weeks),
                    volume: format!("{:.0}-{:.0} hrs", max_hours * 0.4, max_hours * 0.6),
                    focus: "Freshen up".into(),
                });
                format!(
                    "  Weeks 1-{}: Recovery emphasis with low stress and reduced volume\n  Optional strides or drills only if freshness is improving\n  Use the block to restore motivation, sleep quality, and musculoskeletal resilience",
                    weeks
                )
            }
        };

        (phases, structure)
    }

    fn build_sample_week(&self, focus: TrainingFocus, max_hours: f64, lthr: Option<f64>) -> String {
        let hr_hint = lthr
            .map(|l| format!(" (HR < {:.0} bpm)", l * 0.85))
            .unwrap_or_default();
        match focus {
            TrainingFocus::AerobicBase => {
                format!(
                    "Sample Week\n  Monday: REST\n  Tuesday: Easy Run {:.0}:{:02.0} (Z1-Z2){}\n  Wednesday: Recovery + Strength\n  Thursday: Easy Run {:.0}:{:02.0} (Z1-Z2){}\n  Friday: REST or cross-training\n  Saturday: Long Run {:.0}:{:02.0} (Z1-Z2){}\n  Sunday: Active Recovery",
                    max_hours / 5.0 * 60.0,
                    (max_hours / 5.0 * 60.0 % 60.0),
                    hr_hint,
                    max_hours / 5.0 * 60.0,
                    (max_hours / 5.0 * 60.0 % 60.0),
                    hr_hint,
                    max_hours / 3.0 * 60.0,
                    (max_hours / 3.0 * 60.0 % 60.0),
                    hr_hint
                )
            }
            TrainingFocus::Intensity => {
                "Sample Week\n  Monday: REST or short recovery jog\n  Tuesday: Threshold / VO2 session\n  Wednesday: Easy aerobic run\n  Thursday: Secondary quality session or hill reps\n  Friday: REST + mobility\n  Saturday: Long aerobic run with controlled finish\n  Sunday: Recovery shuffle or off".to_string()
            }
            TrainingFocus::Specific => {
                "Sample Week\n  Monday: REST\n  Tuesday: Race-pace intervals on target terrain\n  Wednesday: Easy aerobic maintenance\n  Thursday: Specific workout with fueling rehearsal\n  Friday: Recovery or travel/rest logistics\n  Saturday: Long race-specific session\n  Sunday: Short reset run with drills".to_string()
            }
            TrainingFocus::Taper => {
                "Sample Week\n  Monday: REST\n  Tuesday: Sharpening session with short pickups\n  Wednesday: Easy aerobic run\n  Thursday: Brief race-pace activation\n  Friday: REST and logistics\n  Saturday: Pre-race leg opener\n  Sunday: Race / key event".to_string()
            }
            TrainingFocus::Recovery => {
                "Sample Week\n  Monday: REST\n  Tuesday: Easy 30-45 min aerobic session\n  Wednesday: Mobility + strength maintenance\n  Thursday: Easy aerobic run with optional strides\n  Friday: REST\n  Saturday: Short relaxed endurance session\n  Sunday: Off or gentle cross-training".to_string()
            }
        }
    }
}

impl Default for PlanTrainingHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "plan_training/tests.rs"]
mod tests;
