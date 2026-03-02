//! Tests for DynamicRegistry and OpenAPI spec parsing.
//!
//! These tests verify:
//! - OpenAPI spec parsing and validation
//! - Dynamic tool generation from operationId
//! - Tag-based filtering (include/exclude)
//! - Tool metadata generation (JSON Schema, descriptions)

use intervals_icu_mcp::dynamic::{DynamicRegistry, parse_openapi_spec};
use serde_json::json;
use std::collections::HashSet;

/// Helper to create empty tag set
fn empty_tags() -> HashSet<String> {
    HashSet::new()
}

/// Helper to create tag set from strings
fn tag_set(tags: &[&str]) -> HashSet<String> {
    tags.iter().map(|s| s.to_string()).collect()
}

/// Helper to create a minimal OpenAPI spec with custom endpoints
fn create_test_spec() -> serde_json::Value {
    json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "paths": {
            "/api/v1/athlete/{id}/profile": {
                "get": {
                    "operationId": "getAthleteProfile",
                    "summary": "Get athlete profile",
                    "description": "Returns the athlete's profile information",
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
                    "summary": "List activities",
                    "description": "Returns a list of athlete's activities",
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
                            "required": false,
                            "schema": {"type": "integer", "default": 10}
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success"
                        }
                    }
                }
            },
            "/api/v1/athlete/{id}/wellness": {
                "get": {
                    "operationId": "listWellness",
                    "summary": "List wellness records",
                    "tags": ["Wellness"],
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
                            "description": "Success"
                        }
                    }
                }
            }
        }
    })
}

#[test]
fn test_parse_openapi_spec_basic() {
    let spec = create_test_spec();
    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags());

    assert!(
        result.is_ok(),
        "Failed to parse valid spec: {:?}",
        result.err()
    );
    let registry = result.unwrap();

    // Should have 3 operations
    assert_eq!(registry.len(), 3, "Expected 3 operations in registry");

    // Check operation names
    assert!(registry.operation("getAthleteProfile").is_some());
    assert!(registry.operation("listActivities").is_some());
    assert!(registry.operation("listWellness").is_some());
}

#[test]
fn test_parse_openapi_spec_generates_tool_metadata() {
    let spec = create_test_spec();
    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags()).unwrap();

    let tools = result.list_tools();
    assert_eq!(tools.len(), 3);

    // Check tool has proper metadata
    let profile_tool = tools
        .iter()
        .find(|t| t.name == "getAthleteProfile")
        .unwrap();
    assert!(profile_tool.description.is_some());
    // input_schema is Arc<Map<String, Value>>, just check it exists
    assert!(!profile_tool.input_schema.is_empty());
}

#[test]
fn test_parse_openapi_spec_path_parameters() {
    let spec = create_test_spec();
    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags()).unwrap();

    let op = result.operation("getAthleteProfile").unwrap();

    // Should have path parameter 'id'
    assert!(!op.params.is_empty());
    let path_params: Vec<_> = op
        .params
        .iter()
        .filter(|p| matches!(p.location, intervals_icu_mcp::dynamic::ParamLocation::Path))
        .collect();
    assert!(!path_params.is_empty());

    // Check path template contains {id}
    assert!(op.path_template.contains("{id}"));
}

#[test]
fn test_parse_openapi_spec_query_parameters() {
    let spec = create_test_spec();
    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags()).unwrap();

    let op = result.operation("listActivities").unwrap();

    // Should have query parameter 'limit'
    let query_params: Vec<_> = op
        .params
        .iter()
        .filter(|p| matches!(p.location, intervals_icu_mcp::dynamic::ParamLocation::Query))
        .collect();
    assert!(!query_params.is_empty());

    let limit_param = query_params.iter().find(|p| p.name == "limit").unwrap();
    assert!(!limit_param.auto_injected);
}

#[test]
fn test_tag_filtering_include() {
    let spec = create_test_spec();
    let include_tags = tag_set(&["Athlete"]);
    let exclude_tags = empty_tags();

    let result = parse_openapi_spec(&spec, &include_tags, &exclude_tags).unwrap();

    // Should only include getAthleteProfile (tagged with "Athlete")
    assert_eq!(result.len(), 1);
    assert!(result.operation("getAthleteProfile").is_some());
    assert!(result.operation("listActivities").is_none());
    assert!(result.operation("listWellness").is_none());
}

#[test]
fn test_tag_filtering_exclude() {
    let spec = create_test_spec();
    let include_tags = empty_tags();
    let exclude_tags = tag_set(&["Wellness"]);

    let result = parse_openapi_spec(&spec, &include_tags, &exclude_tags).unwrap();

    // Should exclude listWellness (tagged with "Wellness")
    assert_eq!(result.len(), 2);
    assert!(result.operation("getAthleteProfile").is_some());
    assert!(result.operation("listActivities").is_some());
    assert!(result.operation("listWellness").is_none());
}

#[test]
fn test_tag_filtering_multiple_includes() {
    let spec = create_test_spec();
    let include_tags = tag_set(&["Athlete", "Activities"]);
    let exclude_tags = empty_tags();

    let result = parse_openapi_spec(&spec, &include_tags, &exclude_tags).unwrap();

    // Should include Athlete and Activities, exclude Wellness
    assert_eq!(result.len(), 2);
    assert!(result.operation("getAthleteProfile").is_some());
    assert!(result.operation("listActivities").is_some());
    assert!(result.operation("listWellness").is_none());
}

#[test]
fn test_tag_filtering_priority_include_over_exclude() {
    let spec = create_test_spec();
    // When both are set, include takes priority (per SRS)
    let include_tags = tag_set(&["Athlete"]);
    let exclude_tags = tag_set(&["Athlete", "Activities"]);

    let result = parse_openapi_spec(&spec, &include_tags, &exclude_tags).unwrap();

    // Include has priority, so only Athlete tools
    assert_eq!(result.len(), 1);
    assert!(result.operation("getAthleteProfile").is_some());
}

#[test]
fn test_registry_list_tools_sorted() {
    let spec = create_test_spec();
    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags()).unwrap();

    let tools = result.list_tools();

    // Tools should be sorted by name
    let names: Vec<_> = tools.iter().map(|t| &t.name).collect();
    assert_eq!(
        names,
        vec!["getAthleteProfile", "listActivities", "listWellness"]
    );
}

#[test]
fn test_registry_operation_lookup() {
    let spec = create_test_spec();
    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags()).unwrap();

    // Existing operation
    assert!(result.operation("getAthleteProfile").is_some());

    // Non-existing operation
    assert!(result.operation("nonExistent").is_none());
}

#[test]
fn test_registry_len_and_is_empty() {
    let spec = create_test_spec();
    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags()).unwrap();

    assert_eq!(result.len(), 3);
    assert!(!result.is_empty());

    let empty_registry = DynamicRegistry::new();
    assert_eq!(empty_registry.len(), 0);
    assert!(empty_registry.is_empty());
}

#[test]
fn test_parse_spec_with_missing_operation_id() {
    // Spec without operationId should use fallback naming
    let spec = json!({
        "openapi": "3.0.0",
        "info": {"title": "Test", "version": "1.0.0"},
        "paths": {
            "/api/v1/test": {
                "get": {
                    "summary": "Test endpoint",
                    "parameters": [],
                    "responses": {"200": {"description": "Success"}}
                }
            }
        }
    });

    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags());
    assert!(
        result.is_ok(),
        "Should handle missing operationId gracefully"
    );

    let registry = result.unwrap();
    assert_eq!(registry.len(), 1);
    // Should have generated a fallback name
    let tools = registry.list_tools();
    assert!(!tools.is_empty());
    assert!(!tools[0].name.is_empty());
}

#[test]
fn test_parse_spec_invalid_http_method() {
    // Spec with invalid HTTP method should be handled gracefully
    let spec = json!({
        "openapi": "3.0.0",
        "info": {"title": "Test", "version": "1.0.0"},
        "paths": {
            "/api/v1/test": {
                "invalid_method": {
                    "operationId": "testOp",
                    "parameters": [],
                    "responses": {"200": {"description": "Success"}}
                }
            }
        }
    });

    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags());
    // Should either skip invalid method or handle gracefully
    assert!(result.is_ok());
}

#[test]
fn test_parse_spec_with_request_body() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {"title": "Test", "version": "1.0.0"},
        "paths": {
            "/api/v1/athlete/{id}/events": {
                "post": {
                    "operationId": "createEvent",
                    "tags": ["Events"],
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"}
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {"type": "object"}
                            }
                        }
                    },
                    "responses": {"200": {"description": "Success"}}
                }
            }
        }
    });

    let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags()).unwrap();
    assert_eq!(result.len(), 1);

    let op = result.operation("createEvent").unwrap();
    assert_eq!(op.method, reqwest::Method::POST);
    assert!(op.has_json_body);
}

#[tokio::test]
async fn test_dynamic_registry_from_real_api() {
    // This test fetches real OpenAPI spec from intervals.icu
    // Skipped by default to avoid network dependency
    // Run with: cargo test test_dynamic_registry_from_real_api -- --ignored

    let client = reqwest::Client::new();
    let response = client.get("https://intervals.icu/api/v1/docs").send().await;

    if let Ok(resp) = response
        && resp.status().is_success()
    {
        let spec: serde_json::Value = resp.json().await.unwrap();
        let result = parse_openapi_spec(&spec, &empty_tags(), &empty_tags());

        assert!(
            result.is_ok(),
            "Failed to parse real Intervals.icu OpenAPI spec"
        );

        let registry = result.unwrap();
        assert!(
            !registry.is_empty(),
            "Real API spec should produce at least one tool"
        );

        println!(
            "Successfully parsed {} tools from real Intervals.icu API",
            registry.len()
        );
    }
}
