use crate::domains::coach::FitnessMetrics;
use crate::engines::coach_metrics::parse_fitness_metrics;
use intervals_icu_client::IntervalsClient;

/// Loads fitness data once and exposes typed accessors.
/// Replaces the duplicated `client.get_fitness_summary().await.ok()` + `parse_fitness_metrics()` pattern.
pub(crate) struct FitnessContext {
    metrics: Option<FitnessMetrics>,
}

impl FitnessContext {
    /// Load fitness summary from the client. Errors are swallowed (fitness is optional).
    pub async fn load(client: &dyn IntervalsClient) -> Self {
        let raw = client.get_fitness_summary().await.ok();
        let metrics = parse_fitness_metrics(raw.as_ref());
        Self { metrics }
    }

    /// Create an empty context (no fitness data).
    pub fn empty() -> Self {
        Self { metrics: None }
    }

    pub fn metrics(&self) -> Option<&FitnessMetrics> {
        self.metrics.as_ref()
    }
}
