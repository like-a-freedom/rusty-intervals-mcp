use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext};
use crate::engines::ade::compute_ade;
use crate::engines::analysis_audit::build_data_audit;
use crate::engines::analysis_fetch::RecoveryFetchRequest;
use crate::engines::analysis_fetch::fetch_recovery_data;
use crate::engines::coach_guidance::build_alerts;
use crate::engines::coach_guidance::build_guidance;
use crate::engines::traits::RecoveryMetricsBuilder;
use intervals_icu_client::{ApiError, IntervalsClient, IntervalsError};
use std::sync::Arc;

pub struct RecoveryAssessmentEngine {
    metrics_builder: Arc<dyn RecoveryMetricsBuilder>,
}

impl RecoveryAssessmentEngine {
    pub fn new(metrics_builder: Arc<dyn RecoveryMetricsBuilder>) -> Self {
        Self { metrics_builder }
    }

    pub async fn build_report(
        &self,
        client: &dyn IntervalsClient,
        request: &RecoveryFetchRequest,
    ) -> Result<RecoveryReport, IntervalsError> {
        let period_days = request.period_days as i64;
        let end_date = chrono::Local::now().date_naive();
        let start_date = end_date - chrono::Duration::days(period_days);
        let window = AnalysisWindow::new(start_date, end_date);

        let fetched_data = fetch_recovery_data(client, request)
            .await
            .map_err(|e| IntervalsError::Api(ApiError::new(500, e.to_string(), "")))?;

        let mut coach_context = self.metrics_builder.build_all(
            fetched_data.fitness.as_ref(),
            fetched_data.wellness.as_ref(),
        )?;

        coach_context.meta =
            crate::domains::coach::CoachMeta::new(AnalysisKind::RecoveryAssessment, &window);
        coach_context.audit = build_data_audit(&fetched_data);

        coach_context.alerts = build_alerts(&coach_context.metrics);
        coach_context.guidance = build_guidance(&coach_context.metrics, &coach_context.alerts);

        let ade_outputs = compute_ade(
            &crate::engines::ade::AdeInputs {
                tsb: coach_context.metrics.fitness.as_ref().and_then(|f| f.tsb),
                hrv_ratio: coach_context
                    .metrics
                    .wellness
                    .as_ref()
                    .and_then(|w| w.hrv_ratio),
                ramp_rate: coach_context
                    .metrics
                    .fitness
                    .as_ref()
                    .and_then(|f| f.ramp_rate),
                ..Default::default()
            },
            coach_context.metrics.fitness.as_ref().and_then(|f| f.tsb),
        );

        Ok(RecoveryReport {
            period: window.clone(),
            coach_context,
            ade_outputs: Some(ade_outputs),
        })
    }
}

#[derive(Debug, Clone)]
pub struct RecoveryReport {
    pub period: AnalysisWindow,
    pub coach_context: CoachContext,
    pub ade_outputs: Option<crate::engines::ade::AdeOutput>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use serde_json::Value;

    struct FakeMetricsBuilder;

    impl RecoveryMetricsBuilder for FakeMetricsBuilder {
        fn build_all(
            &self,
            _fitness: Option<&Value>,
            _wellness: Option<&Value>,
        ) -> Result<CoachContext, intervals_icu_client::IntervalsError> {
            Ok(CoachContext::new(
                AnalysisKind::RecoveryAssessment,
                AnalysisWindow::new(
                    chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                    chrono::NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
                ),
            ))
        }
    }

    #[tokio::test]
    async fn engine_instantiates() {
        let builder = Arc::new(FakeMetricsBuilder);
        let _engine = RecoveryAssessmentEngine::new(builder);
    }

    #[tokio::test]
    async fn engine_returns_error_on_missing_data() {
        let builder = Arc::new(FakeMetricsBuilder);
        let engine = RecoveryAssessmentEngine::new(builder);

        let mock_client = MockIntervalsClient::builder().with_activities(vec![]);

        let request = RecoveryFetchRequest {
            period_days: 7,
            include_wellness: true,
        };

        let result = engine.build_report(&mock_client, &request).await;

        assert!(result.is_err());
    }
}
