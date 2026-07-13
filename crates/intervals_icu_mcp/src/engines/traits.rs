use crate::domains::coach::CoachContext;
use intervals_icu_client::IntervalsError;
use serde_json::Value;

pub trait RecoveryMetricsBuilder: Send + Sync {
    fn build_all(
        &self,
        fitness: Option<&Value>,
        wellness: Option<&Value>,
    ) -> Result<CoachContext, IntervalsError>;
}
