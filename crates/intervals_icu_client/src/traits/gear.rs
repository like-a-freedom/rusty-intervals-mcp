//! Gear service trait for equipment management.

use crate::Result;

/// Service for gear/equipment management.
#[async_trait::async_trait]
pub trait GearService: Send + Sync + 'static {
    /// Get list of gear items.
    async fn get_gear_list(&self) -> Result<serde_json::Value>;

    /// Create a new gear item.
    async fn create_gear(&self, gear: &serde_json::Value) -> Result<serde_json::Value>;

    /// Update an existing gear item.
    async fn update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// Delete a gear item.
    async fn delete_gear(&self, gear_id: &str) -> Result<()>;

    /// Create a gear maintenance reminder.
    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// Update a gear maintenance reminder.
    async fn update_gear_reminder(
        &self,
        gear_id: &str,
        reminder_id: &str,
        reset: bool,
        snooze_days: u32,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;
}
