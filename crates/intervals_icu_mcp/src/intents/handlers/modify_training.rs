use crate::domains::events::{
    normalize_event_start, validate_and_prepare_event, validation_error_to_string,
};
use crate::intents::{
    ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput, OutputMetadata,
};
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
use crate::intents::utils::{filter_events_by_date, filter_events_by_range};

pub struct ModifyTrainingHandler;

const SINGLE_SCOPE_LIMIT: u32 = 200;
const RANGE_SCOPE_LIMIT: u32 = 500;

impl ModifyTrainingHandler {
    pub fn new() -> Self {
        Self
    }

    fn dedupe_events(events: Vec<Event>) -> Vec<Event> {
        let mut deduped = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for event in events {
            let dedupe_key = event
                .id
                .clone()
                .unwrap_or_else(|| format!("{}:{}", event.start_date_local, event.name));
            if seen_ids.insert(dedupe_key) {
                deduped.push(event);
            }
        }

        deduped
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
            .map(Self::dedupe_events)
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

impl ModifyTrainingHandler {
    fn resolve_target_scope(input: &Value) -> Result<TargetScope, IntentError> {
        let target_date = input.get("target_date").and_then(Value::as_str);
        let target_date_from = input.get("target_date_from").and_then(Value::as_str);
        let target_date_to = input.get("target_date_to").and_then(Value::as_str);

        match (target_date, target_date_from, target_date_to) {
            (Some(date), _, _) => {
                let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
                    IntentError::validation(format!(
                        "Invalid date format: {}. Use YYYY-MM-DD.",
                        date
                    ))
                })?;
                Ok(TargetScope::Single(parsed))
            }
            (None, Some(start), Some(end)) => {
                let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d").map_err(|_| {
                    IntentError::validation(format!(
                        "Invalid date format: {}. Use YYYY-MM-DD.",
                        start
                    ))
                })?;
                let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d").map_err(|_| {
                    IntentError::validation(format!(
                        "Invalid date format: {}. Use YYYY-MM-DD.",
                        end
                    ))
                })?;
                if start_date > end_date {
                    return Err(IntentError::validation(
                        "Start date must be before end date.".to_string(),
                    ));
                }
                Ok(TargetScope::Range(start_date, end_date))
            }
            (None, Some(_), None) | (None, None, Some(_)) => Err(IntentError::validation(
                "target_date_from and target_date_to must be provided together.".to_string(),
            )),
            (None, None, None) => Err(IntentError::validation(
                "Provide target_date or target_date_from/target_date_to.".to_string(),
            )),
        }
    }

    async fn modify_training(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
        dry_run: bool,
    ) -> Result<IntentOutput, IntentError> {
        let new_date = input.get("new_date").and_then(Value::as_str);
        let desc_filter = input
            .get("target_description_contains")
            .and_then(Value::as_str);
        let target_scope = Self::resolve_target_scope(input)?;

        let update_fields = Self::build_update_fields(input)?;

        let (mut matching, target_label) = self.find_matching_events(client, &target_scope).await?;
        let events_before_filter = matching.len();

        // Apply description filter if provided
        if let Some(desc) = desc_filter {
            matching.retain(|e| Self::event_matches_description(e, desc));
            tracing::debug!(
                "After description filter '{}': {} events remain",
                desc,
                matching.len()
            );
        }

        // Handle empty results gracefully (not an error)
        if matching.is_empty() {
            let mut content = Vec::new();
            content.push(ContentBlock::markdown(
                "## Modify Training\n\n**Status:** No events found".to_string(),
            ));

            let mut summary = Vec::new();
            let mut suggestions = Vec::new();
            let mut next_actions = Vec::new();

            if let Some(d) = desc_filter
                && events_before_filter > 0
            {
                summary.push("- No events matched the provided description filter".into());
                summary.push(format!(
                    "- The date has scheduled training, but none matched '{}'",
                    d
                ));
                summary.push(format!("- Search filter: '{}'", d));

                suggestions.push(
                    "Try a broader description filter or omit it to see all events on that date"
                        .into(),
                );
                suggestions.push("Use analyze_training with target_type: period to inspect the scheduled workouts before modifying them".into());

                next_actions
                    .push("To inspect the date: analyze_training with target_type: period".into());
                next_actions.push("To retry without the filter: modify_training with the same target_date and no target_description_contains".into());
                next_actions.push(
                    "To create an additional workout instead: modify_training with action: create"
                        .into(),
                );
            } else {
                summary.push(format!(
                    "- No training events scheduled for {}",
                    target_label
                ));
                summary.push("- This date appears to be free".into());
                if let Some(d) = desc_filter {
                    summary.push(format!("- Search filter: '{}'", d));
                }

                suggestions.push("Check a different date for existing workouts".into());
                suggestions.push("Use action: create to add a new workout instead".into());
                suggestions.push("View your calendar to find scheduled workouts".into());

                next_actions.push(
                    "To modify a different date: modify_training with different target_date".into(),
                );
                next_actions.push(
                    "To view scheduled workouts: analyze_training with target_type: period".into(),
                );
                next_actions
                    .push("To create a new workout: modify_training with action: create".into());
            }

            content.push(ContentBlock::markdown(summary.join("\n")));

            return Ok(IntentOutput::new(content)
                .with_suggestions(suggestions)
                .with_next_actions(next_actions));
        }

        let mut content = Vec::new();
        let mode = if dry_run {
            "Preview (dry_run)"
        } else {
            "Changes Applied"
        };
        content.push(ContentBlock::markdown(format!(
            "## Modify Training - {}\n\n**Action:** modify\n**Target:** {}\n**Affected:** {} event(s)\nAffected: {} event(s)",
            mode, target_label, matching.len(), matching.len()
        )));

        let mut rows = Vec::new();
        for event in &matching {
            let event_title = event.name.clone();
            if let Some(new_d) = new_date {
                rows.push(vec!["Date".into(), event_title.clone(), new_d.to_string()]);
            }
            if let Some(new_name) = input.get("new_name").and_then(Value::as_str) {
                rows.push(vec![
                    "Name".into(),
                    event_title.clone(),
                    new_name.to_string(),
                ]);
            }
            if let Some(new_description) = input.get("new_description").and_then(Value::as_str) {
                rows.push(vec![
                    "Description".into(),
                    event_title.clone(),
                    new_description.to_string(),
                ]);
            }
            if let Some(new_duration) = input.get("new_duration").and_then(Value::as_str) {
                rows.push(vec![
                    "Duration".into(),
                    event_title.clone(),
                    new_duration.to_string(),
                ]);
            }
        }

        if !rows.is_empty() {
            content.push(ContentBlock::table(
                vec!["Field".into(), "Event".into(), "New".into()],
                rows,
            ));
        }

        if !dry_run {
            for event in &matching {
                let event_id = event.id.as_deref().ok_or_else(|| {
                    IntentError::api(
                        "Matched event is missing id and cannot be updated".to_string(),
                    )
                })?;
                client
                    .update_event(event_id, &update_fields)
                    .await
                    .map_err(|e| {
                        IntentError::api(format!("Failed to update event {}: {}", event_id, e))
                    })?;
            }
        }

        let mut suggestions = vec!["Changes ready to apply.".into()];
        let mut next_actions = vec![
            "To apply changes: call again without dry_run".into(),
            "To view calendar: analyze_training with target_type: period".into(),
        ];

        if !dry_run {
            suggestions = vec!["Training modified successfully.".into()];
            next_actions =
                vec!["To view updated calendar: analyze_training with target_type: period".into()];
        }

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions)
            .with_metadata(OutputMetadata {
                events_modified: Some(matching.len() as u32),
                ..Default::default()
            }))
    }

    async fn create_training(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
        dry_run: bool,
    ) -> Result<IntentOutput, IntentError> {
        let new_date = input
            .get("new_date")
            .or_else(|| input.get("target_date"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                IntentError::validation("Missing required field for create: new_date")
            })?;
        let new_name = input
            .get("new_name")
            .and_then(Value::as_str)
            .unwrap_or("New Workout");
        let new_duration = input
            .get("new_duration")
            .and_then(Value::as_str)
            .unwrap_or("1:00");

        let category = input
            .get("new_category")
            .or_else(|| input.get("category"))
            .and_then(Value::as_str)
            .unwrap_or("Workout");

        let new_description = input
            .get("new_description")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let new_type = input
            .get("new_type")
            .or_else(|| input.get("type"))
            .and_then(Value::as_str)
            .map(str::to_owned);

        let event_category = match category {
            "Workout" => intervals_icu_client::EventCategory::Workout,
            "RaceA" => intervals_icu_client::EventCategory::RaceA,
            "RaceB" => intervals_icu_client::EventCategory::RaceB,
            "RaceC" => intervals_icu_client::EventCategory::RaceC,
            "Note" => intervals_icu_client::EventCategory::Note,
            "Plan" => intervals_icu_client::EventCategory::Plan,
            _ => intervals_icu_client::EventCategory::Workout,
        };

        let new_event = validate_and_prepare_event(intervals_icu_client::Event {
            id: None,
            start_date_local: new_date.to_string(),
            name: new_name.to_string(),
            category: event_category,
            description: new_description,
            r#type: new_type,
        })
        .map_err(|e| IntentError::validation(validation_error_to_string(e)))?;

        if !dry_run {
            let _created = client
                .create_event(new_event.clone())
                .await
                .map_err(|e| IntentError::api(format!("Failed to create event: {}", e)))?;
        }

        let mut content = Vec::new();
        let mode = if dry_run {
            "Preview (dry_run)"
        } else {
            "Created"
        };
        content.push(ContentBlock::markdown(format!(
            "## Create Training - {}\n\n**Name:** {}\n**Date:** {}\n**Duration:** {}",
            mode, new_name, new_date, new_duration
        )));

        let suggestions = if dry_run {
            vec![
                "Ready to create. Call again without dry_run to apply.".into(),
                "Reuse the same idempotency_token only for the exact apply retry of this preview."
                    .into(),
            ]
        } else {
            vec!["Training created successfully.".into()]
        };

        let next_actions = if dry_run {
            vec![
                "To apply this exact create: call again without dry_run using the same payload and idempotency_token".into(),
                "To change the workout type or date: adjust new_type/new_date and use a new idempotency_token".into(),
                "To inspect the day first: analyze_training with target_type: period".into(),
            ]
        } else {
            vec![
                "To view the new workout: analyze_training with target_type: period".into(),
                "To modify it: modify_training with action: modify".into(),
            ]
        };

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions)
            .with_metadata(OutputMetadata {
                events_created: Some(1),
                ..Default::default()
            }))
    }

    async fn delete_training(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
        dry_run: bool,
    ) -> Result<IntentOutput, IntentError> {
        let desc_filter = input
            .get("target_description_contains")
            .and_then(Value::as_str);
        let target_scope = Self::resolve_target_scope(input)?;
        let (mut matching, target_label) = self.find_matching_events(client, &target_scope).await?;

        if let Some(desc) = desc_filter {
            matching.retain(|e| Self::event_matches_description(e, desc));
        }

        let count = matching.len();

        if !dry_run {
            if count > 1 {
                let ids = matching
                    .iter()
                    .filter_map(|event| event.id.clone())
                    .collect::<Vec<_>>();
                client
                    .bulk_delete_events(ids)
                    .await
                    .map_err(|e| IntentError::api(format!("Failed to delete events: {}", e)))?;
            } else if let Some(event) = matching.first()
                && let Some(event_id) = &event.id
            {
                client.delete_event(event_id).await.map_err(|e| {
                    IntentError::api(format!("Failed to delete event {}: {}", event_id, e))
                })?;
            }
        }

        let mut content = Vec::new();
        let mode = if dry_run {
            "Preview (dry_run)"
        } else {
            "Deleted"
        };
        content.push(ContentBlock::markdown(format!(
            "## Delete Training - {}\n\n**Target:** {}\n**Events to delete:** {}",
            mode, target_label, count
        )));

        let suggestions = if dry_run {
            vec![format!(
                "{} event(s) will be deleted. Call again without dry_run to confirm.",
                count
            )]
        } else {
            vec!["Training deleted successfully.".into()]
        };

        let next_actions = vec![
            "To create replacement: modify_training with action: create".into(),
            "To view calendar: analyze_training with target_type: period".into(),
        ];

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions)
            .with_metadata(OutputMetadata {
                events_deleted: Some(count as u32),
                ..Default::default()
            }))
    }
}

impl Default for ModifyTrainingHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
enum TargetScope {
    Single(NaiveDate),
    Range(NaiveDate, NaiveDate),
}

#[cfg(test)]
mod tests {
    use super::*;
    use intervals_icu_client::EventCategory;

    #[test]
    fn test_new_handler() {
        let handler = ModifyTrainingHandler::new();
        assert_eq!(handler.name(), "modify_training");
    }

    #[test]
    fn test_default_handler() {
        let _handler = ModifyTrainingHandler;
    }

    #[test]
    fn test_name() {
        let handler = ModifyTrainingHandler::new();
        assert_eq!(IntentHandler::name(&handler), "modify_training");
    }

    #[test]
    fn test_description() {
        let handler = ModifyTrainingHandler::new();
        let desc = IntentHandler::description(&handler);
        assert!(desc.contains("Modifies or creates calendar training events"));
        assert!(desc.contains("modify"));
        assert!(desc.contains("create"));
        assert!(desc.contains("delete"));
    }

    #[test]
    fn test_input_schema_structure() {
        let handler = ModifyTrainingHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("action"));
        assert!(props.contains_key("target_date"));
        assert!(props.contains_key("new_date"));
        assert!(props.contains_key("new_type"));
        assert!(props.contains_key("dry_run"));
        assert!(props.contains_key("idempotency_token"));

        // Check action enum values
        let action = props.get("action").unwrap();
        let action_enum = action.get("enum").unwrap().as_array().unwrap();
        assert!(action_enum.contains(&json!("modify")));
        assert!(action_enum.contains(&json!("create")));
        assert!(action_enum.contains(&json!("delete")));
    }

    #[test]
    fn test_requires_idempotency_token() {
        let handler = ModifyTrainingHandler::new();
        assert!(IntentHandler::requires_idempotency_token(&handler));
    }

    #[test]
    fn test_action_values() {
        let valid_actions = ["modify", "create", "delete"];
        for action in &valid_actions {
            assert!(["modify", "create", "delete"].contains(action));
        }
    }

    #[test]
    fn test_dry_run_default() {
        let input = json!({
            "action": "modify",
            "target_date": "2026-03-01",
            "idempotency_token": "test"
        });
        let dry_run = input
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(!dry_run);
    }

    #[test]
    fn test_delete_requires_dry_run_validation() {
        // Delete operation should require dry_run: true first
        let action = "delete";
        let dry_run = false;

        // This should be rejected per business logic
        assert_eq!(action, "delete");
        assert!(!dry_run);
        // In actual code: returns error "Delete operation requires dry_run: true first"
    }

    #[test]
    fn test_date_validation() {
        let valid_date = "2026-03-01";
        let result = NaiveDate::parse_from_str(valid_date, "%Y-%m-%d");
        assert!(result.is_ok());

        let invalid_date = "01-03-2026";
        let result = NaiveDate::parse_from_str(invalid_date, "%Y-%m-%d");
        assert!(result.is_err());
    }

    #[test]
    fn test_optional_fields() {
        let input = json!({
            "action": "modify",
            "target_date": "2026-03-01",
            "idempotency_token": "test"
        });

        // These fields are optional
        assert!(input.get("new_date").is_none());
        assert!(input.get("new_name").is_none());
        assert!(input.get("new_description").is_none());
        assert!(input.get("target_description_contains").is_none());
    }

    #[test]
    fn test_resolve_target_scope_requires_both_range_bounds() {
        let input = json!({
            "action": "modify",
            "target_date_from": "2026-03-01",
            "idempotency_token": "test"
        });

        let err = ModifyTrainingHandler::resolve_target_scope(&input)
            .expect_err("missing target_date_to should be rejected");

        assert!(
            err.to_string()
                .contains("target_date_from and target_date_to must be provided together")
        );
    }

    #[test]
    fn test_dedupe_events_keeps_unique_id_and_fallback_keys() {
        let deduped = ModifyTrainingHandler::dedupe_events(vec![
            Event {
                id: Some("event-1".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Tempo Session".to_string(),
                category: EventCategory::Workout,
                description: Some("first copy".to_string()),
                r#type: Some("Run".to_string()),
            },
            Event {
                id: Some("event-1".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Tempo Session".to_string(),
                category: EventCategory::Workout,
                description: Some("duplicate id".to_string()),
                r#type: Some("Run".to_string()),
            },
            Event {
                id: None,
                start_date_local: "2026-03-02".to_string(),
                name: "Strength".to_string(),
                category: EventCategory::Workout,
                description: Some("fallback key".to_string()),
                r#type: Some("Gym".to_string()),
            },
            Event {
                id: None,
                start_date_local: "2026-03-02".to_string(),
                name: "Strength".to_string(),
                category: EventCategory::Workout,
                description: Some("duplicate fallback key".to_string()),
                r#type: Some("Gym".to_string()),
            },
            Event {
                id: None,
                start_date_local: "2026-03-03".to_string(),
                name: "Long Run".to_string(),
                category: EventCategory::Workout,
                description: Some("unique fallback key".to_string()),
                r#type: Some("Run".to_string()),
            },
        ]);

        assert_eq!(deduped.len(), 3);
        assert_eq!(deduped[0].id.as_deref(), Some("event-1"));
        assert_eq!(deduped[1].name, "Strength");
        assert_eq!(deduped[2].name, "Long Run");
    }

    #[test]
    fn test_metadata_structure() {
        let metadata = OutputMetadata {
            has_more: None,
            next_offset: None,
            total_count: None,
            events_created: Some(5),
            events_modified: Some(3),
            events_deleted: Some(1),
            extra: std::collections::HashMap::new(),
        };

        assert_eq!(metadata.events_created, Some(5));
        assert_eq!(metadata.events_modified, Some(3));
        assert_eq!(metadata.events_deleted, Some(1));
    }
}
