//! Integration tests using wiremock for HTTP mocking.
//!
//! These tests verify the full MCP tool dispatch pipeline:
//! - OpenAPI spec loading
//! - Dynamic tool generation
//! - HTTP request dispatch
//! - Response parsing
//!
//! Unlike `integration_tools.rs`, these tests do NOT require real API credentials
//! and can run in CI on every commit.

use intervals_icu_mcp::dynamic::{DynamicRuntime, DynamicRuntimeConfig};
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

/// Setup mock server with OpenAPI spec and common endpoints
async fn setup_mock_api() -> MockServer {
    let mock_server = MockServer::start().await;

    // Mock OpenAPI spec
    let openapi_spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Intervals.icu API",
            "version": "1.0.0"
        },
        "paths": {
            "/api/v1/athlete/{id}/profile": {
                "get": {
                    "operationId": "getAthleteProfile",
                    "summary": "Get athlete profile",
                    "tags": ["Athlete"],
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"}
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "id": {"type": "string"},
                                            "name": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/api/v1/athlete/{id}/activities": {
                "get": {
                    "operationId": "listActivities",
                    "summary": "List recent activities",
                    "tags": ["Activities"],
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"}
                        },
                        {
                            "name": "limit",
                            "in": "query",
                            "schema": {"type": "integer", "default": 10}
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "id": {"type": "string"},
                                                "name": {"type": "string"}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/docs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openapi_spec))
        .mount(&mock_server)
        .await;

    // Mock athlete profile endpoint
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "test_athlete",
            "name": "Test Athlete"
        })))
        .mount(&mock_server)
        .await;

    // Mock activities endpoint
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_athlete/activities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "act1", "name": "Morning Run"},
            {"id": "act2", "name": "Bike Ride"}
        ])))
        .mount(&mock_server)
        .await;

    mock_server
}

#[tokio::test]
async fn test_dynamic_runtime_loads_openapi_spec() {
    let mock_server = setup_mock_api().await;

    let config = DynamicRuntimeConfig::builder()
        .base_url(&mock_server.uri())
        .athlete_id("test_athlete")
        .api_key("test_key")
        .spec_source(format!("{}/api/v1/docs", mock_server.uri()))
        .build();

    let runtime = DynamicRuntime::new(config);

    // Should successfully load the registry
    let registry_result = runtime.ensure_registry().await;
    assert!(registry_result.is_ok(), "Failed to load OpenAPI spec");

    let registry = registry_result.unwrap();
    assert_eq!(registry.len(), 2, "Should have 2 operations");
    assert!(registry.operation("getAthleteProfile").is_some());
    assert!(registry.operation("listActivities").is_some());
}

#[tokio::test]
async fn test_dynamic_runtime_tool_dispatch() {
    let mock_server = setup_mock_api().await;

    let config = DynamicRuntimeConfig::builder()
        .base_url(&mock_server.uri())
        .athlete_id("test_athlete")
        .api_key("test_key")
        .spec_source(format!("{}/api/v1/docs", mock_server.uri()))
        .build();

    let runtime = DynamicRuntime::new(config);

    // Load registry
    let registry = runtime.ensure_registry().await.unwrap();

    // Dispatch getAthleteProfile tool
    let op = registry.operation("getAthleteProfile").unwrap();
    let result = runtime
        .dispatch_openapi(op, None)
        .await;

    assert!(result.is_ok(), "Dispatch failed: {:?}", result.err());
    let response = result.unwrap();
    assert!(!response.content.is_empty());

    // Verify response contains expected data
    let content_str = serde_json::to_string(&response.content).unwrap();
    assert!(content_str.contains("test_athlete"));
    assert!(content_str.contains("Test Athlete"));
}

#[tokio::test]
async fn test_dynamic_runtime_tool_with_arguments() {
    let mock_server = setup_mock_api().await;

    let config = DynamicRuntimeConfig::builder()
        .base_url(&mock_server.uri())
        .athlete_id("test_athlete")
        .api_key("test_key")
        .spec_source(format!("{}/api/v1/docs", mock_server.uri()))
        .build();

    let runtime = DynamicRuntime::new(config);
    let registry = runtime.ensure_registry().await.unwrap();

    // Dispatch listActivities with limit argument
    let op = registry.operation("listActivities").unwrap();
    let args = Some(json!({
        "limit": 5
    }).as_object().unwrap().clone());

    let result = runtime.dispatch_openapi(op, args.as_ref()).await;

    assert!(result.is_ok());
    let response = result.unwrap();

    // Should return array of activities
    let content_str = serde_json::to_string(&response.content).unwrap();
    assert!(content_str.contains("act1"));
    assert!(content_str.contains("act2"));
}

#[tokio::test]
async fn test_dynamic_runtime_tool_not_found() {
    let mock_server = setup_mock_api().await;

    let config = DynamicRuntimeConfig::builder()
        .base_url(&mock_server.uri())
        .athlete_id("test_athlete")
        .api_key("test_key")
        .spec_source(format!("{}/api/v1/docs", mock_server.uri()))
        .build();

    let runtime = DynamicRuntime::new(config);
    let registry = runtime.ensure_registry().await.unwrap();

    // Try to call non-existent tool
    let result = registry.operation("nonExistentTool");

    assert!(result.is_none(), "Should not find non-existent tool");
}

#[tokio::test]
async fn test_dynamic_runtime_tag_filtering() {
    let mock_server = setup_mock_api().await;

    // Only include Athlete tag
    let config = DynamicRuntimeConfig::builder()
        .base_url(&mock_server.uri())
        .athlete_id("test_athlete")
        .api_key("test_key")
        .spec_source(format!("{}/api/v1/docs", mock_server.uri()))
        .include_tag("Athlete")
        .build();

    let runtime = DynamicRuntime::new(config);
    let registry = runtime.ensure_registry().await.unwrap();

    // Should only have getAthleteProfile (tagged with "Athlete")
    assert_eq!(registry.len(), 1);
    assert!(registry.operation("getAthleteProfile").is_some());
    assert!(registry.operation("listActivities").is_none());
}

#[tokio::test]
async fn test_dynamic_runtime_cache_refresh() {
    let mock_server = setup_mock_api().await;

    let config = DynamicRuntimeConfig::builder()
        .base_url(&mock_server.uri())
        .athlete_id("test_athlete")
        .api_key("test_key")
        .spec_source(format!("{}/api/v1/docs", mock_server.uri()))
        .refresh_interval(std::time::Duration::from_secs(1))
        .build();

    let runtime = DynamicRuntime::new(config);

    // First load
    let registry1 = runtime.ensure_registry().await.unwrap();
    let count1 = registry1.len();

    // Second load (should use cache)
    let registry2 = runtime.ensure_registry().await.unwrap();
    let count2 = registry2.len();

    assert_eq!(count1, count2, "Cache should return same tool count");
}
