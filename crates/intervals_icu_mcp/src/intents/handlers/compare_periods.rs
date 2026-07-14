use async_trait::async_trait;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::engines::analyze_training::compare_periods;
use crate::intents::{IdempotencyCache, IntentError, IntentHandler, IntentOutput};

pub struct ComparePeriodsHandler;

impl ComparePeriodsHandler {
    pub fn new() -> Self {
        Self
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
         Supports filtering by workout type and requesting specific metrics (volume, pace, hr, tss, intensity, zones, etvs)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period_a_start": {
                    "type": "string",
                    "description": "Period A start date (YYYY-MM-DD)"
                },
                "period_a_end": {
                    "type": "string",
                    "description": "Period A end date (YYYY-MM-DD)"
                },
                "period_b_start": {
                    "type": "string",
                    "description": "Period B start date (YYYY-MM-DD)"
                },
                "period_b_end": {
                    "type": "string",
                    "description": "Period B end date (YYYY-MM-DD)"
                },
                "period_a_label": {
                    "type": "string",
                    "description": "Label for Period A (default: 'Period A')"
                },
                "period_b_label": {
                    "type": "string",
                    "description": "Label for Period B (default: 'Period B')"
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
        compare_periods(&input, client.as_ref())
            .await
            .map(|r| r.into_intent_output())
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}
