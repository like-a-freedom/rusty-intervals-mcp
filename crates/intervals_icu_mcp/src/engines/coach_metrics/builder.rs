use crate::domains::coach::{AnalysisKind, AnalysisWindow, CoachContext};
use crate::engines::traits::RecoveryMetricsBuilder;
use intervals_icu_client::IntervalsError;
use serde_json::Value;

use super::{parse_fitness_metrics, parse_wellness_metrics};

pub struct CoachMetricsBuilder;

impl RecoveryMetricsBuilder for CoachMetricsBuilder {
    fn build_all(
        &self,
        fitness: Option<&Value>,
        wellness: Option<&Value>,
    ) -> Result<CoachContext, IntervalsError> {
        let window = AnalysisWindow::new(
            chrono::Utc::now().date_naive() - chrono::Duration::days(7),
            chrono::Utc::now().date_naive(),
        );
        let mut context = CoachContext::new(AnalysisKind::RecoveryAssessment, window);

        if let Some(fitness_value) = fitness {
            context.metrics.fitness = parse_fitness_metrics(Some(fitness_value));
        }

        if let Some(wellness_value) = wellness {
            context.metrics.wellness = parse_wellness_metrics(Some(wellness_value));
        }

        Ok(context)
    }
}
