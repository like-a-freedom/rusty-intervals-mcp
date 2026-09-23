use async_trait::async_trait;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::content::date::parse_period_range;
use crate::engines::analyze_training::compare_periods;
use crate::intents::{IdempotencyCache, IntentError, IntentHandler, IntentOutput};

pub struct ComparePeriodsHandler;

impl ComparePeriodsHandler {
    pub fn new() -> Self {
        Self
    }

    fn validate(input: &Value) -> Result<(), IntentError> {
        let _ = parse_period_range(input, "period_a")?;
        let _ = parse_period_range(input, "period_b")?;
        Ok(())
    }
}

impl Default for ComparePeriodsHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IntentHandler for ComparePeriodsHandler {
    fn name(&self) -> &'static str {
        "compare_periods"
    }

    fn description(&self) -> &'static str {
        "Compare performance between two periods (like-for-like). \
         Shows volume, time, distance, elevation, workout count, and TSS deltas. \
         Supports filtering by workout type and requesting specific metrics (volume, pace, hr, tss, intensity, zones, etvs). \
         Δ reports (later period - earlier period) / earlier period, so a positive value means the later period grew vs the earlier one. \
         Periods are auto-ordered by start date — either input slot may be the older or newer period."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period_a_start": {
                    "type": "string",
                    "description": "First period start date (YYYY-MM-DD). The two periods are auto-ordered by start date: the later period is always used as the reference for delta computation, so the Δ column reports (later - earlier). Either period may be labelled A or B — labels follow each period into the rendered table."
                },
                "period_a_end": {
                    "type": "string",
                    "description": "First period end date (YYYY-MM-DD)"
                },
                "period_b_start": {
                    "type": "string",
                    "description": "Second period start date (YYYY-MM-DD). The two periods are auto-ordered by start date: the later period is always used as the reference for delta computation, so the Δ column reports (later - earlier). Either period may be labelled A or B — labels follow each period into the rendered table."
                },
                "period_b_end": {
                    "type": "string",
                    "description": "Second period end date (YYYY-MM-DD)"
                },
                "period_a_label": {
                    "type": "string",
                    "description": "Label for the first period (default: 'Period A'). The label appears in the rendered table regardless of whether this period ends up as the earlier or later one."
                },
                "period_b_label": {
                    "type": "string",
                    "description": "Label for the second period (default: 'Period B'). The label appears in the rendered table regardless of whether this period ends up as the earlier or later one."
                },
                "workout_type": {
                    "type": "string",
                    "description": "Filter by workout type: intervals, tempo, long_run, etc."
                },
                "metrics": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Requested metrics: volume, pace, hr, tss, intensity, zones, etvs"
                }
            },
            "required": ["period_a_start", "period_a_end", "period_b_start", "period_b_end"]
        })
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        Self::validate(&input)?;
        compare_periods(&input, client.as_ref())
            .await
            .map(|r| r.into_intent_output())
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "compare_periods/tests.rs"]
mod tests;
