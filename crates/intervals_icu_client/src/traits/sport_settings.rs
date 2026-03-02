//! Sport settings service trait for sport configuration.

use crate::Result;

/// Service for sport settings management.
#[async_trait::async_trait]
pub trait SportSettingsService: Send + Sync + 'static {
    /// Get sport settings.
    async fn get_sport_settings(&self) -> Result<serde_json::Value>;

    /// Update sport settings.
    async fn update_sport_settings(
        &self,
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// Apply sport settings to historical activities.
    async fn apply_sport_settings(&self, sport_type: &str) -> Result<serde_json::Value>;

    /// Create new sport settings.
    async fn create_sport_settings(&self, settings: &serde_json::Value) -> Result<serde_json::Value>;

    /// Delete sport settings.
    async fn delete_sport_settings(&self, sport_type: &str) -> Result<()>;
}
