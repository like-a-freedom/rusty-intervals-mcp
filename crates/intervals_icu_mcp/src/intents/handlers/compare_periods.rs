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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use std::sync::Arc;

    /// Handler must delegate the comparison work to `compare_periods` and
    /// surface a successful `IntentOutput` for valid input. Confirms the
    /// wiring (Arc<dyn IntervalsClient>, IdempotencyCache None) works.
    #[tokio::test]
    async fn compare_periods_handler_delegates_successfully() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockIntervalsClient::default());
        let input = json!({
            "period_a_start": "2026-03-01",
            "period_a_end": "2026-03-07",
            "period_b_start": "2026-03-08",
            "period_b_end": "2026-03-14",
        });
        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    /// Handler must surface validation errors from the underlying engine when
    /// required fields are missing (per input_schema's `required` list).
    #[tokio::test]
    async fn compare_periods_handler_missing_required_field() {
        let handler = ComparePeriodsHandler::new();
        let client = Arc::new(MockIntervalsClient::default());
        // period_b_start omitted on purpose
        let input = json!({
            "period_a_start": "2026-03-01",
            "period_a_end": "2026-03-07",
            "period_b_end": "2026-03-14",
        });
        let result = handler.execute(input, client, None).await;
        assert!(result.is_err(), "expected validation error");
    }
}
