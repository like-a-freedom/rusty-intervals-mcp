//! Weather service trait for athlete weather configuration.

use crate::Result;

/// Service for weather forecast configuration operations.
#[async_trait::async_trait]
pub trait WeatherService: Send + Sync + 'static {
    /// Get the athlete's weather forecast configuration.
    async fn get_weather_config(&self) -> Result<serde_json::Value>;

    /// Update the athlete's weather forecast configuration.
    async fn update_weather_config(&self, config: &serde_json::Value) -> Result<serde_json::Value>;
}
