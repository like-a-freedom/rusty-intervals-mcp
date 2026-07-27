use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Duration;
use chrono::Utc;
use intervals_icu_client::{ActivitySummary, IntervalsClient};
use serde_json::Value;
use serde_json::json;

use crate::domains::coach::AnalysisWindow;
use crate::engines::progress_tracking::{
    MAX_WELLNESS_DAYS_FALLBACK, MIN_DAYS_FOR_PLATEAU, build_progress_report_with_ctl_fallback,
    count_ctl_points, derive_ctl_series_from_activity_loads,
};
use crate::intents::{IdempotencyCache, IntentError, IntentHandler, IntentOutput, OutputMetadata};

use super::render::progress::render_progress_report;

const DEFAULT_PERIOD_WEEKS: i64 = 12;
const MIN_PERIOD_WEEKS: i64 = 4;
const MAX_PERIOD_WEEKS: i64 = 24;
const ACTIVITY_FETCH_BUFFER_DAYS: i32 = 14;
const CTL_WARMUP_DAYS: i32 = 42;
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

Use only for one trailing 4–24 week window. Do NOT use for YoY or two non-contiguous periods; use `compare_periods`.

Do NOT use when: you need to analyze a specific workout in detail (use analyze_training), or assess current recovery readiness (use assess_recovery), or plan future training (use plan_training).

Arguments:
- period_weeks (integer, 4–24, default 12): How far back to analyze.
- hypothesis_mode (boolean, default true): Whether to compute coaching hypotheses (volume, intensity distribution, recovery) and recommendations.

Returns: Progress Tracking Report with plateau detection, load context (ACWR, monotony, strain), HRV context, TID drift analysis, coaching hypotheses with confidence scores, recommendations, and warnings when data is insufficient. When wellness lacks enough historical CTL, plateau detection uses a clearly marked estimate from activity training-load history. Includes a Fitness Snapshot with current CTL, ATL, TSB, and ramp rate when athlete-summary data is available.
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
        if !(MIN_PERIOD_WEEKS..=MAX_PERIOD_WEEKS).contains(&period_weeks) {
            return Err(IntentError::validation(format!(
                "period_weeks must be in [{MIN_PERIOD_WEEKS}, {MAX_PERIOD_WEEKS}], got {period_weeks}"
            )));
        }
        let hypothesis_mode = input
            .get("hypothesis_mode")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let period_days = (period_weeks * 7) as i32;
        let end_date = Utc::now().date_naive();
        let start_date = end_date - Duration::days((period_days as i64) - 1);
        let window = AnalysisWindow::new(start_date, end_date);

        // Fetch wellness with the requested window, then auto-expand to the API maximum
        // if the initial window is too short for plateau detection. The user-supplied
        // `period_weeks` is treated as a hint, not a hard cap — if it leaves us under
        // 28 days of CTL history we silently retry with the largest range the server can serve.
        let mut wellness = client
            .get_wellness(Some(period_days))
            .await
            .map_err(|error| IntentError::api(format!("Failed to fetch wellness: {error}")))?;

        let initial_ctl_points = count_ctl_points(&wellness);
        let max_days = MAX_WELLNESS_DAYS_FALLBACK;
        if initial_ctl_points < MIN_DAYS_FOR_PLATEAU
            && period_days < max_days
            && period_weeks < MAX_PERIOD_WEEKS
        {
            tracing::info!(
                requested_days = period_days,
                available_ctl_points = initial_ctl_points,
                min_required = MIN_DAYS_FOR_PLATEAU,
                max_days,
                "initial wellness window too short for plateau detection; retrying with max range"
            );
            wellness = client.get_wellness(Some(max_days)).await.map_err(|error| {
                IntentError::api(format!("Failed to fetch expanded wellness: {error}"))
            })?;
        }

        let activity_history_days = (period_days + ACTIVITY_FETCH_BUFFER_DAYS)
            .max(MAX_WELLNESS_DAYS_FALLBACK + CTL_WARMUP_DAYS);
        let activities = client
            .get_recent_activities(None, Some(activity_history_days))
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

        let ctl_window = AnalysisWindow::new(
            end_date - Duration::days(i64::from(activity_history_days - 1)),
            end_date,
        );
        let activity_ctl_fallback =
            derive_ctl_series_from_activity_loads(&activities, &activity_details, &ctl_window);
        let report = build_progress_report_with_ctl_fallback(
            &wellness,
            &activities,
            &activity_details,
            &window,
            activity_ctl_fallback,
        );

        let fitness_context =
            crate::engines::fitness_context::FitnessContext::load(client.as_ref()).await;

        let content = render_progress_report(&report, hypothesis_mode, fitness_context.metrics());

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
#[path = "track_progress/tests.rs"]
mod tests;
