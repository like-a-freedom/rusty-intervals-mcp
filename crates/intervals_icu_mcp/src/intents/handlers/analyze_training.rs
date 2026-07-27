use async_trait::async_trait;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::engines::analyze_training::{analyze_period, analyze_single};
use crate::intents::{IdempotencyCache, IntentError, IntentHandler, IntentOutput};

pub struct AnalyzeTrainingHandler;

impl AnalyzeTrainingHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AnalyzeTrainingHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IntentHandler for AnalyzeTrainingHandler {
    fn name(&self) -> &'static str {
        "analyze_training"
    }

    fn description(&self) -> &'static str {
        "Analyzes training sessions — single workout or period. Returns power-duration \
         anchors (eFTP, W', pMax), ESPE-derived metrics (aerobic durability, glycolytic \
         bias), W' depletion (WDRM), signed aerobic decoupling (ISDM) with durability \
         state, Z2 HR stability, terrain context (index, VAM), nutrition demand (carb, \
         protein), and running/cycling curve profile. Period analysis adds heat stress \
         context, TID model (pyramidal/threshold/polarized), NDLI (neural density load), \
         power curve comparison (2-window deltas), ultra-specific tokens (back-to-back \
         load, vert/week), and load management (ACWR, monotony, strain). Also retrieves \
         calendar events (races, sick days, injuries, notes, planned workouts). \
         Includes a Fitness Snapshot with current CTL, ATL, TSB, and ramp rate when \
         athlete-summary data is available.

         Use this tool when: you need to review a completed workout's quality, assess \
         aerobic/neural fatigue, check pacing distribution, examine period trends, or \
         compare evolution. Do NOT use when: you need to plan future training (use \
         plan_training), assess recovery readiness (use assess_recovery), or perform \
         post-race debrief (use analyze_race).

         For a single workout call target_type=\"single\" with a date. \
         For a date range call target_type=\"period\" with period_start=\"2025-04-01\" \
         and period_end=\"2025-06-30\". Do not use start_date/end_date.

         analysis_type controls depth: summary (basic metrics), detailed (+execution \
         context, Z2, terrain, nutrition, profile), intervals (+interval breakdown), \
         streams (+stream insights)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_type": {
                    "type": "string",
                    "enum": ["single", "period"],
                    "description": "Analysis type: single workout or period"
                },
                "date": {
                    "type": "string",
                    "description": "Workout date (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday') for single analysis"
                },
                "period_start": {
                    "type": "string",
                    "description": "Period start (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday') for period analysis and calendar-event context"
                },
                "period_end": {
                    "type": "string",
                    "description": "Period end (YYYY-MM-DD, 'today', 'tomorrow', or 'yesterday') for period analysis and calendar-event context"
                },
                "description_contains": {
                    "type": "string",
                    "description": "Filter activities by name/description (case-insensitive substring match). Works with target_type: single only. Examples: 'long run', 'tempo', 'intervals', 'threshold'"
                },
                "analysis_type": {
                    "type": "string",
                    "enum": ["summary", "detailed", "intervals", "streams"],
                    "default": "summary",
                    "description": "Analysis depth for single workouts: summary (basic metrics table), detailed (+execution context, Z2 HR stability, terrain context, nutrition demand, curve profile, quality findings), intervals (+structured interval breakdown with HR/power/pace per rep), streams (+raw stream min/max/points insights). For period analysis: always returns trend, load management, NDLI. Add streams for daily load series or intervals for interval session listing."
                },
                "include_best_efforts": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include best efforts comparison"
                },
                "include_histograms": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include power/HR/pace histograms. Only valid when target_type is 'single'."
                },
                "metrics": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Requested metrics: time, distance, vertical, tss, pace, hr, etvs. ETVS is returned in weighted minutes with model and coverage context in the analysis body. Unavailable exact metrics are marked unavailable instead of being silently ignored."
                }
            },
            "required": ["target_type"],
            "allOf": [
                {
                    "if": {
                        "properties": {
                            "target_type": { "const": "single" }
                        }
                    },
                    "then": {
                        "required": ["date"]
                    },
                    "else": {
                        "required": ["period_start", "period_end"]
                    }
                },
                {
                    "if": {
                        "properties": {
                            "target_type": { "const": "period" }
                        }
                    },
                    "then": {
                        "properties": {
                            "include_histograms": { "const": false },
                            "description_contains": false
                        }
                    }
                }
            ]
        })
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let target_type = input
            .get("target_type")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: target_type"))?;

        match target_type {
            "single" => analyze_single(&input, client.as_ref())
                .await
                .map(|r| r.into_intent_output()),
            "period" => analyze_period(&input, client.as_ref())
                .await
                .map(|r| r.into_intent_output()),
            _ => Err(IntentError::validation(format!(
                "Invalid target_type: {}. Must be 'single' or 'period'",
                target_type
            ))),
        }
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "analyze_training/tests.rs"]
mod tests;
