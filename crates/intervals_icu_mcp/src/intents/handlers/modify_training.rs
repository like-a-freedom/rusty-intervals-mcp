use crate::domains::events::normalize_event_start;
use crate::intents::{IdempotencyCache, IntentError, IntentHandler, IntentOutput};
use async_trait::async_trait;
use chrono::NaiveDate;
use intervals_icu_client::Event;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
/// Modify Training Intent Handler
///
/// Modifies existing training (CRUD: modify, create, delete).
use std::sync::Arc;

use crate::engines::analysis_fetch::fetch_calendar_events_between;
use crate::engines::dedupe::dedupe_and_sort_events;
use crate::intents::utils::{filter_events_by_date, filter_events_by_range};

pub struct ModifyTrainingHandler;

mod actions;

const SINGLE_SCOPE_LIMIT: u32 = 200;
const RANGE_SCOPE_LIMIT: u32 = 500;

impl ModifyTrainingHandler {
    pub fn new() -> Self {
        Self
    }

    async fn fetch_events_between(
        &self,
        client: &dyn IntervalsClient,
        start_date: &NaiveDate,
        end_date: &NaiveDate,
        limit: u32,
    ) -> Result<Vec<Event>, IntentError> {
        fetch_calendar_events_between(client, start_date, end_date, limit)
            .await
            .map_err(|e| IntentError::api(e.to_string()))
            .map(dedupe_and_sort_events)
    }

    async fn fetch_events_for_date(
        &self,
        client: &dyn IntervalsClient,
        target_date: &NaiveDate,
    ) -> Result<Vec<Event>, IntentError> {
        self.fetch_events_between(client, target_date, target_date, SINGLE_SCOPE_LIMIT)
            .await
    }

    async fn fetch_events_for_range(
        &self,
        client: &dyn IntervalsClient,
        start_date: &NaiveDate,
        end_date: &NaiveDate,
    ) -> Result<Vec<Event>, IntentError> {
        if start_date > end_date {
            return Err(IntentError::validation(
                "Start date must be before end date.".to_string(),
            ));
        }

        self.fetch_events_between(client, start_date, end_date, RANGE_SCOPE_LIMIT)
            .await
    }

    async fn find_matching_events(
        &self,
        client: &dyn IntervalsClient,
        target_scope: &TargetScope,
    ) -> Result<(Vec<Event>, String), IntentError> {
        match target_scope {
            TargetScope::Single(target_date) => {
                let events = self.fetch_events_for_date(client, target_date).await?;
                let matching = filter_events_by_date(&events, target_date)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>();
                Ok((matching, target_date.to_string()))
            }
            TargetScope::Range(start_date, end_date) => {
                let events = self
                    .fetch_events_for_range(client, start_date, end_date)
                    .await?;
                let matching = filter_events_by_range(&events, start_date, end_date)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>();
                Ok((matching, format!("{} to {}", start_date, end_date)))
            }
        }
    }

    fn event_matches_description(event: &Event, filter: &str) -> bool {
        let needle = filter.to_lowercase();
        event.name.to_lowercase().contains(&needle)
            || event
                .description
                .as_ref()
                .map(|description| description.to_lowercase().contains(&needle))
                .unwrap_or(false)
    }

    fn parse_duration_to_seconds(duration: &str) -> Result<i64, IntentError> {
        let parts = duration.split(':').collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err(IntentError::validation(format!(
                "Invalid duration format: {}. Use H:MM.",
                duration
            )));
        }

        let hours = parts[0].parse::<i64>().map_err(|_| {
            IntentError::validation(format!("Invalid duration hours: {}", duration))
        })?;
        let minutes = parts[1].parse::<i64>().map_err(|_| {
            IntentError::validation(format!("Invalid duration minutes: {}", duration))
        })?;

        if !(0..60).contains(&minutes) || hours < 0 {
            return Err(IntentError::validation(format!(
                "Invalid duration value: {}. Use H:MM.",
                duration
            )));
        }

        Ok(hours * 3600 + minutes * 60)
    }

    fn build_update_fields(input: &Value) -> Result<Value, IntentError> {
        let mut fields = serde_json::Map::new();

        if let Some(new_date) = input.get("new_date").and_then(Value::as_str) {
            let normalized = normalize_event_start(new_date).ok_or_else(|| {
                IntentError::validation(format!(
                    "Invalid date format: {}. Use YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS.",
                    new_date
                ))
            })?;
            fields.insert("start_date_local".to_string(), Value::String(normalized));
        }
        if let Some(new_name) = input.get("new_name").and_then(Value::as_str) {
            fields.insert("name".to_string(), Value::String(new_name.to_string()));
        }
        if let Some(new_description) = input.get("new_description").and_then(Value::as_str) {
            fields.insert(
                "description".to_string(),
                Value::String(new_description.to_string()),
            );
        }
        if let Some(new_category) = input
            .get("new_category")
            .or_else(|| input.get("category"))
            .and_then(Value::as_str)
        {
            fields.insert(
                "category".to_string(),
                Value::String(new_category.to_string()),
            );
        }
        if let Some(new_type) = input
            .get("new_type")
            .or_else(|| input.get("type"))
            .and_then(Value::as_str)
        {
            fields.insert("type".to_string(), Value::String(new_type.to_string()));
        }
        if let Some(new_duration) = input.get("new_duration").and_then(Value::as_str) {
            fields.insert(
                "moving_time".to_string(),
                Value::from(Self::parse_duration_to_seconds(new_duration)?),
            );
        }

        if fields.is_empty() {
            return Err(IntentError::validation(
                "Modify action requires at least one new_* field to change.".to_string(),
            ));
        }

        Ok(Value::Object(fields))
    }
}

#[async_trait]
impl IntentHandler for ModifyTrainingHandler {
    fn name(&self) -> &'static str {
        "modify_training"
    }

    fn description(&self) -> &'static str {
        "Modifies or creates calendar training events (modify, create, delete). \
            Use this tool to reschedule workouts, change their details, create a new workout or calendar event \
            on a specific date, or delete planned sessions and other calendar events such as races, sick days, \
            injuries, notes, and plan markers. For create operations, use `new_category` for the calendar category \
            (usually `Workout`) and `new_type` for the workout or sport type (for example `Run` or `WeightTraining`). \
            `target_date` is accepted as an alias for `new_date` when creating. Prefer `dry_run: true` before applying. \
            Requires idempotency token for all operations."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["modify", "create", "delete"], "description": "Action to perform"},
                "target_date": {"type": "string", "description": "Target workout date (YYYY-MM-DD)"},
                "target_description_contains": {"type": "string", "description": "Search by description"},
                "target_date_from": {"type": "string", "description": "Range start for batch operations"},
                "target_date_to": {"type": "string", "description": "Range end for batch operations"},
                "new_date": {"type": "string", "description": "New date for modify"},
                "new_name": {"type": "string", "description": "New name"},
                "new_description": {"type": "string", "description": "New description"},
                "new_duration": {"type": "string", "description": "New duration (e.g., '1:30')"},
                "new_category": {"type": "string", "description": "Calendar event category (usually 'Workout'; other values include RaceA, RaceB, RaceC, Sick, Injured, Note, Holiday, Plan, Target)"},
                "new_type": {"type": "string", "description": "Workout or sport type for Workout events (e.g., 'Run', 'Ride', 'Swim', 'WeightTraining'). If omitted for category 'Workout', defaults to 'Run'."},
                "dry_run": {"type": "boolean", "default": false, "description": "Preview changes only"},
                "idempotency_token": {"type": "string", "description": "Idempotency token (required)"}
            },
            "required": ["action", "idempotency_token"]
        })
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: action"))?;
        let dry_run = input
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        match action {
            "modify" => self.modify_training(&input, client.as_ref(), dry_run).await,
            "create" => self.create_training(&input, client.as_ref(), dry_run).await,
            "delete" => self.delete_training(&input, client.as_ref(), dry_run).await,
            _ => Err(IntentError::validation(format!(
                "Invalid action: {}. Must be 'modify', 'create', or 'delete'",
                action
            ))),
        }
    }

    fn requires_idempotency_token(&self) -> bool {
        true
    }
}

impl Default for ModifyTrainingHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub(super) enum TargetScope {
    Single(NaiveDate),
    Range(NaiveDate, NaiveDate),
}

#[cfg(test)]
#[path = "modify_training/tests.rs"]
mod tests;
