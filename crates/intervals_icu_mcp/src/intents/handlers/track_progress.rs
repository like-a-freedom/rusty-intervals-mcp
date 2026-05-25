use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Duration;
use chrono::Utc;
use intervals_icu_client::{ActivitySummary, IntervalsClient};
use serde_json::Value;
use serde_json::json;

use crate::domains::coach::AnalysisWindow;
use crate::engines::progress_tracking::build_progress_report;
use crate::intents::{IdempotencyCache, IntentError, IntentHandler, IntentOutput, OutputMetadata};

use super::render::progress::render_progress_report;

const DEFAULT_PERIOD_WEEKS: i64 = 12;
const MIN_PERIOD_WEEKS: i64 = 4;
const MAX_PERIOD_WEEKS: i64 = 24;
const MAX_ACTIVITIES_TO_FETCH: u32 = 200;
const ACTIVITY_FETCH_BUFFER_DAYS: i32 = 14;
const TID_SAMPLE_PER_WEEK: usize = 5;
const TID_SAMPLE_MAX: usize = 60;

pub struct TrackProgressHandler;

impl Default for TrackProgressHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackProgressHandler {
    pub fn new() -> Self {
        Self
    }

    fn select_tid_sample(activities: &[ActivitySummary], max_items: usize) -> Vec<ActivitySummary> {
        let mut sampled = activities.to_vec();
        sampled.sort_by(|a, b| a.start_date_local.cmp(&b.start_date_local));
        sampled.into_iter().rev().take(max_items).collect()
    }
}

#[async_trait]
impl IntentHandler for TrackProgressHandler {
    fn name(&self) -> &'static str {
        "track_progress"
    }

    fn description(&self) -> &'static str {
        "Detect trailing progress plateaus and summarize likely root causes from CTL, load context, HRV, and TID drift.

Use this tool when: you need to understand whether training is stalled, identify why progress has flattened, and get evidence-backed coaching hypotheses with recommended actions. Helps answer 'why am I not improving?' or 'is my training working?'.

Do NOT use when: you need to analyze a specific workout in detail (use analyze_training), or assess current recovery readiness (use assess_recovery), or plan future training (use plan_training).

Arguments:
- period_weeks (integer, 4–24, default 12): How far back to analyze.
- hypothesis_mode (boolean, default true): Whether to compute coaching hypotheses (volume, intensity distribution, recovery) and recommendations.

Returns: Progress Tracking Report with plateau detection, load context (ACWR, monotony, strain), HRV context, TID drift analysis, coaching hypotheses with confidence scores, recommendations, and warnings when data is insufficient.
On error: API or validation errors with descriptive messages."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period_weeks": {
                    "type": "integer",
                    "minimum": MIN_PERIOD_WEEKS,
                    "maximum": MAX_PERIOD_WEEKS,
                    "default": DEFAULT_PERIOD_WEEKS
                },
                "hypothesis_mode": {
                    "type": "boolean",
                    "default": true
                }
            },
            "required": []
        })
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _idempotency_cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let period_weeks = input
            .get("period_weeks")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_PERIOD_WEEKS as u64) as i64;
        let hypothesis_mode = input
            .get("hypothesis_mode")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let period_days = (period_weeks * 7) as i32;
        let end_date = Utc::now().date_naive();
        let start_date = end_date - Duration::days((period_days as i64) - 1);
        let window = AnalysisWindow::new(start_date, end_date);

        let wellness = client
            .get_wellness(Some(period_days))
            .await
            .map_err(|error| IntentError::api(format!("Failed to fetch wellness: {error}")))?;

        let activities = client
            .get_recent_activities(
                Some(MAX_ACTIVITIES_TO_FETCH),
                Some(period_days + ACTIVITY_FETCH_BUFFER_DAYS),
            )
            .await
            .map_err(|error| IntentError::api(format!("Failed to fetch activities: {error}")))?;

        let tid_sample = Self::select_tid_sample(
            &activities,
            (period_weeks as usize * TID_SAMPLE_PER_WEEK).min(TID_SAMPLE_MAX),
        );
        let mut activity_details = HashMap::new();
        for activity in tid_sample {
            if let Ok(detail) = client.get_activity_details(&activity.id).await {
                activity_details.insert(activity.id.clone(), detail);
            }
        }

        let report = build_progress_report(&wellness, &activities, &activity_details, &window);
        let content = render_progress_report(&report, hypothesis_mode);

        Ok(IntentOutput::new(content)
            .with_suggestions(report.recommendations.clone())
            .with_next_actions(vec![
                "analyze_training for workout-level detail".into(),
                "assess_recovery for readiness context".into(),
                "plan_training after confirming the diagnosis".into(),
            ])
            .with_metadata(OutputMetadata::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use intervals_icu_client::ActivitySummary;
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn execute_returns_markdown_report() {
        let client = MockIntervalsClient::builder()
            .with_wellness(json!([
                {"date": "2026-01-01", "ctl": 60.0, "hrv": 65.0},
                {"date": "2026-01-02", "ctl": 60.2, "hrv": 64.0},
                {"date": "2026-01-03", "ctl": 60.1, "hrv": 63.0},
                {"date": "2026-01-04", "ctl": 60.0, "hrv": 63.0},
                {"date": "2026-01-05", "ctl": 60.1, "hrv": 62.0},
                {"date": "2026-01-06", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-07", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-08", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-09", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-10", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-11", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-12", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-13", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-14", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-15", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-16", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-17", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-18", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-19", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-20", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-21", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-22", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-23", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-24", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-25", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-26", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-27", "ctl": 60.0, "hrv": 62.0},
                {"date": "2026-01-28", "ctl": 60.0, "hrv": 62.0}
            ]))
            .with_activities(vec![
                ActivitySummary {
                    id: "act-1".into(),
                    start_date_local: "2026-01-27".into(),
                    training_load: Some(80),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "act-2".into(),
                    start_date_local: "2026-01-28".into(),
                    training_load: Some(75),
                    ..Default::default()
                },
            ])
            .with_activity_detail("act-1", json!({"icu_zone_times": [{"id": "Z1", "secs": 1800}, {"id": "Z2", "secs": 900}, {"id": "Z3", "secs": 300}], "icu_training_load": 80, "polarization_index": 0.82}))
            .with_activity_detail("act-2", json!({"icu_zone_times": [{"id": "Z1", "secs": 1700}, {"id": "Z2", "secs": 1000}, {"id": "Z3", "secs": 200}], "icu_training_load": 75, "polarization_index": 0.79}));

        let handler = TrackProgressHandler::new();
        let output = handler
            .execute(
                json!({"period_weeks": 4, "hypothesis_mode": true}),
                Arc::new(client),
                None,
            )
            .await
            .unwrap();

        let rendered = format!("{:?}", output.content);
        assert!(rendered.contains("Progress Tracking Report"));
    }
}
