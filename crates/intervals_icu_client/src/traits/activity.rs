//! Activity service trait for activity-related operations.

use crate::{ActivityMessage, ActivitySummary, BestEffortsOptions, Result};

/// Service for activity-related operations.
#[async_trait::async_trait]
#[allow(clippy::too_many_arguments)]
pub trait ActivityService: Send + Sync + 'static {
    /// Get recent activities for the athlete.
    async fn get_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<i32>,
    ) -> Result<Vec<ActivitySummary>>;

    /// Get detailed information about a specific activity.
    async fn get_activity_details(&self, activity_id: &str) -> Result<serde_json::Value>;

    /// Get read-only messages/comments for a specific activity.
    async fn get_activity_messages(&self, _activity_id: &str) -> Result<Vec<ActivityMessage>> {
        Ok(Vec::new())
    }

    /// Get activity stream data (power, heart rate, etc.).
    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value>;

    /// Get structured intervals from an activity.
    async fn get_activity_intervals(&self, activity_id: &str) -> Result<serde_json::Value>;

    /// Get best efforts for an activity.
    async fn get_best_efforts(
        &self,
        activity_id: &str,
        options: Option<BestEffortsOptions>,
    ) -> Result<serde_json::Value>;

    /// Search activities by query.
    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<ActivitySummary>>;

    /// Search activities with full details.
    async fn search_activities_full(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<serde_json::Value>;

    /// Download activities as CSV.
    async fn get_activities_csv(&self) -> Result<String>;

    /// Update an activity's fields.
    async fn update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// Delete an activity.
    async fn delete_activity(&self, activity_id: &str) -> Result<()>;

    /// Get activities around a specific activity.
    async fn get_activities_around(
        &self,
        activity_id: &str,
        limit: Option<u32>,
        route_id: Option<i64>,
    ) -> Result<serde_json::Value>;

    /// Download activity file.
    async fn download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>>;

    /// Download activity file with progress tracking.
    async fn download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<crate::DownloadProgress>,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>>;

    /// Download FIT file.
    async fn download_fit_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>>;

    /// Download GPX file.
    async fn download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>>;

    /// Get GAP (Grade Adjusted Pace) histogram.
    async fn get_gap_histogram(&self, activity_id: &str) -> Result<serde_json::Value>;

    /// Search intervals within activities.
    async fn search_intervals(
        &self,
        min_secs: u32,
        max_secs: u32,
        min_intensity: u32,
        max_intensity: u32,
        interval_type: Option<String>,
        min_reps: Option<u32>,
        max_reps: Option<u32>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value>;

    /// Get power histogram for an activity.
    async fn get_power_histogram(&self, activity_id: &str) -> Result<serde_json::Value>;

    /// Get heart rate histogram for an activity.
    async fn get_hr_histogram(&self, activity_id: &str) -> Result<serde_json::Value>;

    /// Get pace histogram for an activity.
    async fn get_pace_histogram(&self, activity_id: &str) -> Result<serde_json::Value>;
}
