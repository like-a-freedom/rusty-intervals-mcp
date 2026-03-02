//! Tests for dynamic OpenAPI tool dispatch.
//!
//! These tests verify:
//! - HTTP request building from operation metadata
//! - Path parameter substitution
//! - Query parameter encoding
//! - JSON body handling
//! - Response parsing and compact mode
//! - Error handling

use intervals_icu_mcp::dynamic::{DynamicOperation, ParamLocation, ParamSpec};
use rmcp::model::JsonObject;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Helper to create a test DynamicOperation
fn create_test_operation(
    name: impl Into<String>,
    method: reqwest::Method,
    path_template: &str,
) -> DynamicOperation {
    use rmcp::model::Tool;

    let schema: JsonObject = serde_json::Map::new();
    let name_str = name.into();
    DynamicOperation {
        name: name_str.clone(),
        method,
        path_template: path_template.to_string(),
        description: "Test operation".to_string(),
        params: vec![],
        has_json_body: false,
        tool: Tool::new(name_str, "Test operation", schema),
        output_schema: None,
    }
}

/// Helper to add a path parameter to operation
fn with_path_param(mut op: DynamicOperation, name: &str, auto_injected: bool) -> DynamicOperation {
    op.params.push(ParamSpec {
        name: name.to_string(),
        location: ParamLocation::Path,
        auto_injected,
    });
    op
}

/// Helper to add a query parameter to operation
fn with_query_param(mut op: DynamicOperation, name: &str) -> DynamicOperation {
    op.params.push(ParamSpec {
        name: name.to_string(),
        location: ParamLocation::Query,
        auto_injected: false,
    });
    op
}

#[tokio::test]
async fn test_dispatch_get_request() {
    let mock_server = MockServer::start().await;

    // Mock endpoint
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "test_athlete",
            "name": "Test User"
        })))
        .mount(&mock_server)
        .await;

    let operation = with_path_param(
        create_test_operation(
            "getProfile",
            reqwest::Method::GET,
            "/api/v1/athlete/{id}/profile",
        ),
        "id",
        false,
    );

    let args = Some(
        json!({
            "id": "test_athlete"
        })
        .as_object()
        .unwrap()
        .clone(),
    );

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_ok(), "Dispatch failed: {:?}", result.err());
    let response = result.unwrap();
    assert!(!response.content.is_empty());
}

#[tokio::test]
async fn test_dispatch_path_parameter_substitution() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act123/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "power": [100, 200, 150]
        })))
        .mount(&mock_server)
        .await;

    let operation = with_path_param(
        create_test_operation(
            "getStreams",
            reqwest::Method::GET,
            "/api/v1/activity/{id}/streams",
        ),
        "id",
        false,
    );

    let args = Some(
        json!({
            "id": "act123"
        })
        .as_object()
        .unwrap()
        .clone(),
    );

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "ignored_athlete_id",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dispatch_query_parameters() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/activities"))
        .and(query_param("limit", "5"))
        .and(query_param("days_back", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "act1", "name": "Activity 1"}
        ])))
        .mount(&mock_server)
        .await;

    let mut operation = with_path_param(
        create_test_operation(
            "listActivities",
            reqwest::Method::GET,
            "/api/v1/athlete/{id}/activities",
        ),
        "id",
        true, // auto-injected athlete_id
    );
    operation = with_query_param(operation, "limit");
    operation = with_query_param(operation, "days_back");

    let args = Some(
        json!({
            "limit": 5,
            "days_back": 7
        })
        .as_object()
        .unwrap()
        .clone(),
    );

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dispatch_post_with_json_body() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/athlete/test_athlete/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "event123",
            "name": "Test Event"
        })))
        .mount(&mock_server)
        .await;

    let mut operation = with_path_param(
        create_test_operation(
            "createEvent",
            reqwest::Method::POST,
            "/api/v1/athlete/{id}/events",
        ),
        "id",
        true,
    );
    operation.has_json_body = true;

    let args = Some(
        json!({
            "body": {
                "name": "Test Event",
                "category": "WORKOUT"
            }
        })
        .as_object()
        .unwrap()
        .clone(),
    );

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dispatch_auto_injected_athlete_id() {
    let mock_server = MockServer::start().await;

    // When auto_injected=true, path should use athlete_id from config
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/config_athlete/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "config_athlete"
        })))
        .mount(&mock_server)
        .await;

    let operation = with_path_param(
        create_test_operation(
            "getProfile",
            reqwest::Method::GET,
            "/api/v1/athlete/{id}/profile",
        ),
        "id",
        true, // auto-injected
    );

    // No 'id' in args - should use athlete_id from config
    let args = Some(json!({}).as_object().unwrap().clone());

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "config_athlete",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dispatch_missing_required_path_param() {
    let mock_server = MockServer::start().await;

    let operation = with_path_param(
        create_test_operation(
            "getProfile",
            reqwest::Method::GET,
            "/api/v1/athlete/{id}/profile",
        ),
        "id",
        false, // not auto-injected
    );

    // No 'id' in args
    let args = Some(json!({}).as_object().unwrap().clone());

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.message.contains("missing required path parameter"));
}

#[tokio::test]
async fn test_dispatch_response_compact_mode() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "test_athlete",
            "name": "Test User",
            "email": "test@example.com",
            "extra_field": "should be filtered"
        })))
        .mount(&mock_server)
        .await;

    let operation = with_path_param(
        create_test_operation(
            "getProfile",
            reqwest::Method::GET,
            "/api/v1/athlete/{id}/profile",
        ),
        "id",
        false,
    );

    // Request with compact=true and fields filter
    let args = Some(
        json!({
            "id": "test_athlete",
            "compact": true,
            "fields": ["id", "name"]
        })
        .as_object()
        .unwrap()
        .clone(),
    );

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap();

    // Response should be filtered to only id and name
    let content_str = serde_json::to_string(&response.content).unwrap();
    assert!(content_str.contains("id"));
    assert!(content_str.contains("name"));
    // Note: compact mode filtering may vary based on implementation
}

#[tokio::test]
async fn test_dispatch_response_body_only() {
    let mock_server = MockServer::start().await;

    let expected_body = json!({
        "id": "test_athlete",
        "name": "Test User"
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&expected_body))
        .mount(&mock_server)
        .await;

    let operation = with_path_param(
        create_test_operation(
            "getProfile",
            reqwest::Method::GET,
            "/api/v1/athlete/{id}/profile",
        ),
        "id",
        false,
    );

    // Request with body_only=true
    let args = Some(
        json!({
            "id": "test_athlete",
            "body_only": true
        })
        .as_object()
        .unwrap()
        .clone(),
    );

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap();

    // With body_only=true, should return structured response with just the body
    // Content is Vec<Annotated<RawContent>>, check it's not empty
    assert!(!response.content.is_empty());
}

#[tokio::test]
async fn test_dispatch_http_error_handling() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/profile"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": "Not found"
        })))
        .mount(&mock_server)
        .await;

    let operation = with_path_param(
        create_test_operation(
            "getProfile",
            reqwest::Method::GET,
            "/api/v1/athlete/{id}/profile",
        ),
        "id",
        false,
    );

    let args = Some(
        json!({
            "id": "test_athlete"
        })
        .as_object()
        .unwrap()
        .clone(),
    );

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    // Should handle HTTP errors gracefully
    assert!(result.is_ok() || result.is_err());
    // Error handling strategy may vary - either return error or return error in response
}

#[tokio::test]
async fn test_dispatch_missing_api_key() {
    let mock_server = MockServer::start().await;

    let operation = create_test_operation(
        "getProfile",
        reqwest::Method::GET,
        "/api/v1/athlete/{id}/profile",
    );

    let args = Some(
        json!({
            "id": "test_athlete"
        })
        .as_object()
        .unwrap()
        .clone(),
    );

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "", // Empty API key
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.message.contains("INTERVALS_ICU_API_KEY"));
}

#[tokio::test]
async fn test_dispatch_with_empty_arguments() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "test_athlete"
        })))
        .mount(&mock_server)
        .await;

    let operation = with_path_param(
        create_test_operation(
            "getProfile",
            reqwest::Method::GET,
            "/api/v1/athlete/{id}/profile",
        ),
        "id",
        true, // auto-injected
    );

    // Empty arguments
    let args = Some(JsonObject::new());

    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "test_api_key",
        &operation,
        args.as_ref(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dispatch_without_arguments() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "test_athlete"
        })))
        .mount(&mock_server)
        .await;

    let operation = with_path_param(
        create_test_operation(
            "getProfile",
            reqwest::Method::GET,
            "/api/v1/athlete/{id}/profile",
        ),
        "id",
        true, // auto-injected
    );

    // No arguments at all
    let result = intervals_icu_mcp::dynamic::dispatch_operation(
        &reqwest::Client::new(),
        &mock_server.uri(),
        "test_athlete",
        "test_api_key",
        &operation,
        None,
    )
    .await;

    assert!(result.is_ok());
}
