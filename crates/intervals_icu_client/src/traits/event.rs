//! Event service trait for calendar/event-related operations.

use crate::{Event, Result};

/// Service for event/calendar operations.
#[async_trait::async_trait]
pub trait EventService: Send + Sync + 'static {
    /// Create a new event.
    async fn create_event(&self, event: Event) -> Result<Event>;

    /// Get a specific event by ID.
    async fn get_event(&self, event_id: &str) -> Result<Event>;

    /// Delete an event.
    async fn delete_event(&self, event_id: &str) -> Result<()>;

    /// Get events with optional filtering.
    async fn get_events(
        &self,
        days_back: Option<i32>,
        limit: Option<u32>,
    ) -> Result<Vec<Event>>;

    /// Create multiple events in bulk.
    async fn bulk_create_events(&self, events: Vec<Event>) -> Result<Vec<Event>>;

    /// Get upcoming workouts and events.
    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
        limit: Option<u32>,
        category: Option<String>,
    ) -> Result<serde_json::Value>;

    /// Update an existing event.
    async fn update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// Delete multiple events by IDs.
    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<()>;

    /// Duplicate an event.
    async fn duplicate_event(
        &self,
        event_id: &str,
        num_copies: Option<u32>,
        weeks_between: Option<u32>,
    ) -> Result<Vec<Event>>;
}
