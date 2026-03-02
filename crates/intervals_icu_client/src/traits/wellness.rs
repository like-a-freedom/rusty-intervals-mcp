//! Wellness service trait for wellness data management.

use crate::Result;

/// Service for wellness data operations.
#[async_trait::async_trait]
pub trait WellnessService: Send + Sync + 'static {
    /// Get wellness data for recent days.
    async fn get_wellness(&self, days_back: Option<i32>) -> Result<serde_json::Value>;

    /// Get wellness data for a specific date.
    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value>;

    /// Update wellness data for a specific date.
    async fn update_wellness(
        &self,
        date: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value>;
}
