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

/// Handler must surface validation errors when required fields are missing
/// (per input_schema's `required` list).
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

/// Handler must reject input where a period's start is after its end
/// before any engine work runs.
#[tokio::test]
async fn compare_periods_handler_validates_date_range() {
    let handler = ComparePeriodsHandler::new();
    let client = Arc::new(MockIntervalsClient::default());
    let input = json!({
        "period_a_start": "2026-02-01",
        "period_a_end": "2026-01-01",
        "period_b_start": "2026-02-01",
        "period_b_end": "2026-02-28",
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_err(), "expected validation error");
}

/// Handler must reject input missing multiple required fields (only the
/// first period start present) before any engine work runs.
#[tokio::test]
async fn compare_periods_handler_missing_several_required_fields() {
    let handler = ComparePeriodsHandler::new();
    let client = Arc::new(MockIntervalsClient::default());
    let input = json!({
        "period_a_start": "2026-01-01",
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_err(), "expected validation error");
}
