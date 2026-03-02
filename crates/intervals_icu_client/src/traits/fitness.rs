//! Fitness service trait for fitness metrics and summaries.

use crate::Result;

/// Service for fitness metrics and CTL/ATL/TSB analysis.
#[async_trait::async_trait]
pub trait FitnessService: Send + Sync + 'static {
    /// Get athlete fitness summary (CTL, ATL, TSB).
    async fn get_fitness_summary(&self) -> Result<serde_json::Value>;

    /// Get power curves for a sport.
    async fn get_power_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value>;

    /// Get heart rate curves for a sport.
    async fn get_hr_curves(&self, days_back: Option<i32>, sport: &str)
    -> Result<serde_json::Value>;

    /// Get pace curves for a sport.
    async fn get_pace_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value>;
}
