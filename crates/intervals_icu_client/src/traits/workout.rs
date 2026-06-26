//! Workout service trait for workout library operations.

use crate::Result;
use crate::domains::workout::{Folder, WorkoutItem};

/// Service for workout library and training plan operations.
#[async_trait::async_trait]
pub trait WorkoutService: Send + Sync + 'static {
    /// Get workout library folders and plans.
    async fn get_workout_library(&self) -> Result<Vec<WorkoutItem>>;

    /// Get workouts in a specific folder.
    async fn get_workouts_in_folder(&self, folder_id: &str) -> Result<Vec<WorkoutItem>>;

    /// Create a new folder (training plan).
    async fn create_folder(&self, folder: &serde_json::Value) -> Result<Folder>;

    /// Update an existing folder.
    async fn update_folder(
        &self,
        folder_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// Delete a folder.
    async fn delete_folder(&self, folder_id: &str) -> Result<()>;
}
