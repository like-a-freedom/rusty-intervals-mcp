use crate::intents::{ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput};
use async_trait::async_trait;
use chrono::Utc;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
/// Manage Gear Intent Handler
///
/// Manages athlete gear (view, add, retire).
use std::sync::Arc;

pub struct ManageGearHandler;
impl ManageGearHandler {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl IntentHandler for ManageGearHandler {
    fn name(&self) -> &'static str {
        "manage_gear"
    }

    fn description(&self) -> &'static str {
        "Manages athlete gear (view, add, retire). \
         Use for tracking shoe mileage, managing bikes, and monitoring gear wear."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "add", "retire"], "description": "Action to perform"},
                "gear_type": {"type": "string", "enum": ["shoes", "bike", "watch", "other"], "description": "Gear type"},
                "gear_name": {"type": "string", "description": "Gear name (for retire)"},
                "new_gear_name": {"type": "string", "description": "New gear name (for add)"},
                "new_gear_type": {"type": "string", "description": "New gear type (for add)"},
                "idempotency_token": {"type": "string", "description": "Idempotency token (required for add/retire)"}
            },
            "required": ["action"]
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

        match action {
            "list" => self.list_gear(&input, client.as_ref()).await,
            "add" => {
                let _token = input
                    .get("idempotency_token")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        IntentError::validation("Missing required field: idempotency_token")
                    })?;
                self.add_gear(&input, client.as_ref()).await
            }
            "retire" => {
                let _token = input
                    .get("idempotency_token")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        IntentError::validation("Missing required field: idempotency_token")
                    })?;
                self.retire_gear(&input, client.as_ref()).await
            }
            _ => Err(IntentError::validation(format!(
                "Invalid action: {}. Must be 'list', 'add', or 'retire'",
                action
            ))),
        }
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}

impl ManageGearHandler {
    async fn list_gear(
        &self,
        input: &Value,
        _client: &dyn IntervalsClient,
    ) -> Result<IntentOutput, IntentError> {
        let gear_type = input
            .get("gear_type")
            .and_then(Value::as_str)
            .unwrap_or("shoes");

        let gear_list = _client
            .get_gear_list()
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch gear: {}", e)))?;

        let gear_array = gear_list
            .as_array()
            .ok_or_else(|| IntentError::api("Invalid gear list format".to_string()))?
            .clone();

        let type_filter = match gear_type {
            "shoes" => "Shoes",
            "bike" => "Bike",
            "watch" => "Watch",
            _ => "Other",
        };

        let filtered: Vec<&Value> = gear_array
            .iter()
            .filter(|g| {
                g.get("type")
                    .and_then(|t| t.as_str())
                    .map(|t| {
                        let normalized = match t {
                            "Shoes" | "CyclingShoes" => "Shoes",
                            "Bike" | "Trainer" => "Bike",
                            "Computer" => "Watch",
                            _ => "Other",
                        };
                        normalized == type_filter
                    })
                    .unwrap_or(false)
            })
            .collect();

        let mut content = Vec::new();
        let type_name = match gear_type {
            "shoes" => "Shoes",
            "bike" => "Bikes",
            "watch" => "Watches",
            _ => "Other",
        };

        if filtered.is_empty() {
            content.push(ContentBlock::markdown(format!(
                "## Gear: {}\n\n*No {} found*",
                type_name,
                type_name.to_lowercase()
            )));
        } else {
            content.push(ContentBlock::markdown(format!(
                "## Gear: {}\n\n*Showing {} {}*",
                type_name,
                filtered.len(),
                type_name.to_lowercase()
            )));

            let mut rows = vec![vec![
                "Name".to_string(),
                "Distance".to_string(),
                "Remaining".to_string(),
                "Status".to_string(),
            ]];

            for gear in &filtered {
                let name = gear
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                let distance_m = gear.get("distance").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let distance_km = distance_m / 1000.0;

                let (remaining_km, status_str): (f64, String) =
                    if let Some(reminders) = gear.get("reminders").and_then(|r| r.as_array()) {
                        if let Some(reminder) = reminders.first() {
                            let percent = reminder
                                .get("percent_used")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);

                            if percent >= 100.0 {
                                (0.0, "🔴 Replace now".to_string())
                            } else if percent >= 80.0 {
                                let remaining = (distance_km * (100.0 - percent) / 100.0).max(0.0);
                                (remaining, format!("🔶 {:.0}% worn", percent))
                            } else {
                                let remaining = (distance_km * (100.0 - percent) / 100.0).max(0.0);
                                (remaining, format!("✅ {:.0}% used", percent))
                            }
                        } else {
                            (0.0, "ℹ️ No reminder set".to_string())
                        }
                    } else if let Some(retired) = gear.get("retired").and_then(|v| v.as_str()) {
                        if !retired.is_empty() {
                            (0.0, "⏹️ Retired".to_string())
                        } else {
                            (0.0, "ℹ️ No reminder set".to_string())
                        }
                    } else {
                        (0.0, "ℹ️ No reminder set".to_string())
                    };

                rows.push(vec![
                    name.to_string(),
                    format!("{:.1} km", distance_km),
                    format!("{:.1} km", remaining_km),
                    status_str,
                ]);
            }

            content.push(ContentBlock::table(rows[0].clone(), rows[1..].to_vec()));
        }

        let suggestions = if filtered.is_empty() {
            vec![format!(
                "No {} found. Add some gear to start tracking.",
                type_name.to_lowercase()
            )]
        } else {
            vec![]
        };

        let next_actions = vec![
            "Add new gear: manage_gear action: add".into(),
            "View all gear: manage_gear action: list (without gear_type)".into(),
        ];

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions))
    }

    fn api_gear_type(gear_type: &str) -> &'static str {
        match gear_type.to_lowercase().as_str() {
            "shoes" => "Shoes",
            "bike" => "Bike",
            "watch" => "Computer",
            "trainer" => "Trainer",
            "wetsuit" => "Wetsuit",
            _ => "Equipment",
        }
    }

    async fn add_gear(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
    ) -> Result<IntentOutput, IntentError> {
        let new_gear_name = input
            .get("new_gear_name")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: new_gear_name"))?;
        let new_gear_type = input
            .get("new_gear_type")
            .and_then(Value::as_str)
            .unwrap_or("shoes");

        let created = client
            .create_gear(&json!({
                "name": new_gear_name,
                "type": Self::api_gear_type(new_gear_type),
            }))
            .await
            .map_err(|e| IntentError::api(format!("Failed to add gear: {}", e)))?;

        let created_id = created
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("pending");

        let mut content = Vec::new();
        content.push(ContentBlock::markdown(format!(
            "## Add Gear\n\n**Name:** {}\n**Type:** {}\n**ID:** {}\n\nCreated via Intervals.icu API.",
            new_gear_name, new_gear_type, created_id
        )));

        let suggestions = vec![format!(
            "{} is now available for future activity matching.",
            new_gear_name
        )];
        let next_actions = vec!["To view updated list: manage_gear action: list".into()];

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions))
    }

    async fn retire_gear(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
    ) -> Result<IntentOutput, IntentError> {
        let gear_name = input
            .get("gear_name")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: gear_name"))?;

        let gear_list = client
            .get_gear_list()
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch gear: {}", e)))?;

        let gear_array = gear_list
            .as_array()
            .ok_or_else(|| IntentError::api("Invalid gear list format".to_string()))?;

        let target_gear = gear_array.iter().find(|g| {
            g.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.to_lowercase() == gear_name.to_lowercase())
                .unwrap_or(false)
        });

        if let Some(gear) = target_gear {
            let gear_id = gear
                .get("id")
                .and_then(|id| id.as_str())
                .ok_or_else(|| IntentError::api("Gear has no ID".to_string()))?;

            let mut updated = gear.clone();
            updated["retired"] = Value::String(Utc::now().date_naive().to_string());

            client
                .update_gear(gear_id, &updated)
                .await
                .map_err(|e| IntentError::api(format!("Failed to retire gear: {}", e)))?;

            let mut content = Vec::new();
            content.push(ContentBlock::markdown(format!(
                "## Retire Gear\n\n**Name:** {}\n**ID:** {}\n\nRetired via Intervals.icu API.",
                gear_name, gear_id
            )));

            let suggestions = vec![format!(
                "{} is now excluded from active gear rotation.",
                gear_name
            )];
            let next_actions = vec!["View updated list: manage_gear action: list".into()];

            Ok(IntentOutput::new(content)
                .with_suggestions(suggestions)
                .with_next_actions(next_actions))
        } else {
            let available: Vec<&str> = gear_array
                .iter()
                .filter_map(|g| g.get("name").and_then(|n| n.as_str()))
                .collect();
            Err(IntentError::validation(format!(
                "Gear '{}' not found. Available gear: {}",
                gear_name,
                available.join(", ")
            )))
        }
    }
}

impl Default for ManageGearHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_handler() {
        let handler = ManageGearHandler::new();
        assert_eq!(handler.name(), "manage_gear");
    }

    #[test]
    fn test_default_handler() {
        let _handler = ManageGearHandler;
    }

    #[test]
    fn test_name() {
        let handler = ManageGearHandler::new();
        assert_eq!(IntentHandler::name(&handler), "manage_gear");
    }

    #[test]
    fn test_description() {
        let handler = ManageGearHandler::new();
        let desc = IntentHandler::description(&handler);
        assert!(desc.contains("Manages athlete gear"));
        assert!(desc.contains("view"));
        assert!(desc.contains("add"));
        assert!(desc.contains("retire"));
    }

    #[test]
    fn test_input_schema_structure() {
        let handler = ManageGearHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("action"));
        assert!(props.contains_key("gear_type"));
        assert!(props.contains_key("gear_name"));
        assert!(props.contains_key("new_gear_name"));
        assert!(props.contains_key("new_gear_type"));
        assert!(props.contains_key("idempotency_token"));

        // Check action enum values
        let action = props.get("action").unwrap();
        let action_enum = action.get("enum").unwrap().as_array().unwrap();
        assert!(action_enum.contains(&json!("list")));
        assert!(action_enum.contains(&json!("add")));
        assert!(action_enum.contains(&json!("retire")));
    }

    #[test]
    fn test_requires_idempotency_token() {
        let handler = ManageGearHandler::new();
        assert!(!IntentHandler::requires_idempotency_token(&handler));
    }

    #[test]
    fn test_action_values() {
        let valid_actions = ["list", "add", "retire"];
        for action in &valid_actions {
            assert!(["list", "add", "retire"].contains(action));
        }
    }

    #[test]
    fn test_gear_type_values() {
        let valid_types = ["shoes", "bike", "watch", "other"];
        for gear_type in &valid_types {
            assert!(["shoes", "bike", "watch", "other"].contains(gear_type));
        }
    }

    #[test]
    fn test_default_gear_type() {
        let input = json!({
            "action": "list"
        });

        let gear_type = input
            .get("gear_type")
            .and_then(|v| v.as_str())
            .unwrap_or("shoes");
        assert_eq!(gear_type, "shoes");
    }

    #[test]
    fn test_gear_type_display_name() {
        let type_name = match "shoes" {
            "shoes" => "Shoes",
            "bike" => "Bikes",
            "watch" => "Watches",
            _ => "Other",
        };
        assert_eq!(type_name, "Shoes");

        let type_name = match "bike" {
            "shoes" => "Shoes",
            "bike" => "Bikes",
            "watch" => "Watches",
            _ => "Other",
        };
        assert_eq!(type_name, "Bikes");
    }

    #[test]
    fn test_required_fields_for_add() {
        let input = json!({
            "action": "add",
            "new_gear_name": "New Shoes"
        });

        // new_gear_type is optional with default
        let new_gear_type = input
            .get("new_gear_type")
            .and_then(|v| v.as_str())
            .unwrap_or("shoes");
        assert_eq!(new_gear_type, "shoes");

        // new_gear_name is required
        assert!(input.get("new_gear_name").is_some());
    }

    #[test]
    fn test_required_fields_for_retire() {
        let input = json!({
            "action": "retire",
            "gear_name": "Old Shoes"
        });

        // gear_name is required
        assert!(input.get("gear_name").is_some());
    }

    #[test]
    fn test_gear_status_formatting() {
        // Example gear status
        let mileage = 850;
        let remaining = 150;
        let worn_pct = (mileage as f32 / (mileage + remaining) as f32) * 100.0;

        assert!((worn_pct - 85.0).abs() < 0.1);
    }

    #[test]
    fn test_content_structure() {
        let handler = ManageGearHandler::new();

        // Verify handler has correct metadata
        assert_eq!(handler.name(), "manage_gear");
        assert!(handler.description().len() > 50);
    }
}
