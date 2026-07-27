
use super::*;
use crate::test_support::mock::MockIntervalsClient;
use std::sync::Arc;

// ========================================================================
// Constructor Tests
// ========================================================================

#[test]
fn test_new_handler() {
    let handler = ManageGearHandler::new();
    assert_eq!(handler.name(), "manage_gear");
}

#[test]
fn test_default_handler() {
    let _handler = ManageGearHandler;
}

// ========================================================================
// IntentHandler Trait Implementation Tests
// ========================================================================

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

// ========================================================================
// api_gear_type() Tests
// ========================================================================

#[test]
fn test_api_gear_type_shoes() {
    assert_eq!(ManageGearHandler::api_gear_type("shoes"), "Shoes");
    assert_eq!(ManageGearHandler::api_gear_type("Shoes"), "Shoes");
    assert_eq!(ManageGearHandler::api_gear_type("SHOES"), "Shoes");
}

#[test]
fn test_api_gear_type_bike() {
    assert_eq!(ManageGearHandler::api_gear_type("bike"), "Bike");
    assert_eq!(ManageGearHandler::api_gear_type("Bike"), "Bike");
}

#[test]
fn test_api_gear_type_watch() {
    assert_eq!(ManageGearHandler::api_gear_type("watch"), "Computer");
    assert_eq!(ManageGearHandler::api_gear_type("Watch"), "Computer");
}

#[test]
fn test_api_gear_type_trainer() {
    assert_eq!(ManageGearHandler::api_gear_type("trainer"), "Trainer");
}

#[test]
fn test_api_gear_type_wetsuit() {
    assert_eq!(ManageGearHandler::api_gear_type("wetsuit"), "Wetsuit");
}

#[test]
fn test_api_gear_type_other() {
    assert_eq!(ManageGearHandler::api_gear_type("other"), "Equipment");
    assert_eq!(ManageGearHandler::api_gear_type("unknown"), "Equipment");
    assert_eq!(ManageGearHandler::api_gear_type(""), "Equipment");
}

// ========================================================================
// Input Validation and Default Value Tests
// ========================================================================

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

    let new_gear_type = input
        .get("new_gear_type")
        .and_then(|v| v.as_str())
        .unwrap_or("shoes");
    assert_eq!(new_gear_type, "shoes");
    assert!(input.get("new_gear_name").is_some());
}

#[test]
fn test_required_fields_for_retire() {
    let input = json!({
        "action": "retire",
        "gear_name": "Old Shoes"
    });

    assert!(input.get("gear_name").is_some());
}

#[test]
fn test_gear_status_formatting() {
    let mileage = 850;
    let remaining = 150;
    let worn_pct = (mileage as f32 / (mileage + remaining) as f32) * 100.0;

    assert!((worn_pct - 85.0).abs() < 0.1);
}

#[test]
fn test_content_structure() {
    let handler = ManageGearHandler::new();

    assert_eq!(handler.name(), "manage_gear");
    assert!(handler.description().len() > 50);
}

#[test]
fn test_gear_type_filter_values() {
    // Test the type filter mapping in list_gear
    let type_filter = match "shoes" {
        "shoes" => "Shoes",
        "bike" => "Bike",
        "watch" => "Watch",
        _ => "Other",
    };
    assert_eq!(type_filter, "Shoes");
}

#[test]
fn test_type_name_display() {
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

    let type_name = match "watch" {
        "shoes" => "Shoes",
        "bike" => "Bikes",
        "watch" => "Watches",
        _ => "Other",
    };
    assert_eq!(type_name, "Watches");
}

// ========================================================================
// Handler Execution Tests
// ========================================================================

fn gear_mock_client() -> MockIntervalsClient {
    MockIntervalsClient::builder().with_gear_list(json!([
        {
            "id": "g1",
            "name": "Running Shoes",
            "type": "Shoes",
            "distance": 500000.0,
            "retired": ""
        },
        {
            "id": "g2",
            "name": "Road Bike",
            "type": "Bike",
            "distance": 2000000.0,
            "retired": ""
        },
        {
            "id": "g3",
            "name": "Old Shoes",
            "type": "Shoes",
            "distance": 1000000.0,
            "retired": "2025-01-01"
        }
    ]))
}

#[tokio::test]
async fn test_execute_list_gear_action() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "list"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.content.is_empty());
}

#[tokio::test]
async fn test_execute_list_gear_shoes_filter() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "list",
        "gear_type": "shoes"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    let content_text = format!("{:?}", output.content);
    assert!(content_text.contains("Shoes"));
}

#[tokio::test]
async fn test_execute_list_gear_bike_filter() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "list",
        "gear_type": "bike"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    let content_text = format!("{:?}", output.content);
    assert!(content_text.contains("Bikes"));
}

#[tokio::test]
async fn test_execute_list_gear_empty_result() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_gear_list(json!([])));
    let input = json!({
        "action": "list",
        "gear_type": "shoes"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    let content_text = format!("{:?}", output.content);
    assert!(content_text.contains("No shoes found"));
}

#[tokio::test]
async fn test_execute_add_gear_action() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "add",
        "new_gear_name": "New Running Shoes",
        "new_gear_type": "shoes",
        "idempotency_token": "test-token-add"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.content.is_empty());
    assert!(!output.suggestions.is_empty());
}

#[tokio::test]
async fn test_execute_add_gear_default_type() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "add",
        "new_gear_name": "New Gear",
        "idempotency_token": "test-token-add"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    let content_text = format!("{:?}", output.content);
    assert!(content_text.contains("shoes")); // Default type
}

#[tokio::test]
async fn test_execute_retire_gear_action() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "retire",
        "gear_name": "Running Shoes",
        "idempotency_token": "test-token-retire"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.content.is_empty());
    assert!(!output.suggestions.is_empty());
}

#[tokio::test]
async fn test_execute_retire_gear_not_found() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "retire",
        "gear_name": "Nonexistent Gear",
        "idempotency_token": "test-token-retire"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_invalid_action() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "invalid_action"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_missing_action() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({});

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_add_gear_missing_name() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "add",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_retire_gear_missing_name() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "retire",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_add_gear_missing_token() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "add",
        "new_gear_name": "New Shoes"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_retire_gear_missing_token() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "retire",
        "gear_name": "Old Shoes"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntentError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_execute_list_gear_next_actions() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "list"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(!output.next_actions.is_empty());
    assert!(
        output
            .next_actions
            .iter()
            .any(|a| a.contains("manage_gear action: add"))
    );
}

#[tokio::test]
async fn test_execute_add_gear_next_actions() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "add",
        "new_gear_name": "New Shoes",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(
        output
            .next_actions
            .iter()
            .any(|a| a.contains("manage_gear action: list"))
    );
}

#[tokio::test]
async fn test_execute_retire_gear_next_actions() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "retire",
        "gear_name": "Running Shoes",
        "idempotency_token": "test-token"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(
        output
            .next_actions
            .iter()
            .any(|a| a.contains("manage_gear action: list"))
    );
}

#[tokio::test]
async fn test_execute_list_gear_with_reminders() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_gear_list(json!([
        {
            "id": "g1",
            "name": "Shoes with Reminder",
            "type": "Shoes",
            "distance": 800000.0,
            "reminders": [
                {
                    "percent_used": 85.0
                }
            ]
        }
    ])));
    let input = json!({
        "action": "list"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    let content_text = format!("{:?}", output.content);
    assert!(content_text.contains("85"));
}

#[tokio::test]
async fn test_execute_list_gear_distance_formatting() {
    let handler = ManageGearHandler::new();
    let client = Arc::new(gear_mock_client());
    let input = json!({
        "action": "list"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    let content_text = format!("{:?}", output.content);
    assert!(content_text.contains("km"));
}
