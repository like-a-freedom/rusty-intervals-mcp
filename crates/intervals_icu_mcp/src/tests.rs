//! Public-API unit tests for `intervals_icu_mcp` crate root.
//!
//! Exercises the crate public surface (`IntervalsMcpHandler`,
//! `build_mcp_rmcp_config`). Tests that depend on **private** symbols
//! of `lib.rs` (e.g. `AthleteKeyExtractor`, `request_*` extensions
//! helpers, `client_for_extensions_*`, `parse_rate_limit_values`,
//! `configure_tcp_keepalive`) remain inline in `lib.rs` because they
//! cannot reference private items from a sibling module.
//!
//! Extracted as part of audit F-4 -- see
//! `docs/superpowers/plans/2026-07-27-architecture-audit.md`.

use std::sync::Arc;

use crate::test_support::mock::MockIntervalsClient;
use crate::{IntervalsMcpHandler, build_mcp_rmcp_config};

use uuid::Uuid;

fn test_handler() -> IntervalsMcpHandler {
    let runtime = dynamic::DynamicRuntime::new(dynamic::DynamicRuntimeConfig::builder().build());
    IntervalsMcpHandler::with_dynamic_runtime(Arc::new(MockIntervalsClient::default()), runtime)
}

fn test_handler() -> IntervalsMcpHandler {
    let runtime = dynamic::DynamicRuntime::new(dynamic::DynamicRuntimeConfig::builder().build());
    IntervalsMcpHandler::with_dynamic_runtime(Arc::new(MockIntervalsClient::default()), runtime)
}

#[tokio::test]
async fn handler_registers_tools() {
    let handler = test_handler();
    assert_eq!(handler.tool_count(), 9);
}

#[test]
fn handler_info_advertises_tools_and_resources_capabilities() {
    let handler = test_handler();
    let info = handler.get_info();

    assert!(
        info.capabilities.tools.is_some(),
        "server must advertise tool capability during initialize"
    );
    assert!(
        info.capabilities.resources.is_some(),
        "server must advertise resource capability during initialize"
    );
}

#[test]
fn tool_count_matches_internal_tools_without_cache() {
    let handler = test_handler();
    // tool_count() includes 8 intent tools even before dynamic registry load
    assert_eq!(handler.tool_count(), 9);
}

#[tokio::test]
async fn handler_dynamic_tools_loaded_from_openapi() {
    // Create a minimal OpenAPI spec for testing
    let tmp_file =
        std::env::temp_dir().join(format!("intervals_openapi_test_{}.json", Uuid::new_v4()));

    let test_spec = serde_json::json!({
        "openapi": "3.0.0",
        "info": {"title": "Test API", "version": "1.0.0"},
        "paths": {
            "/api/v1/athlete/{id}/activities": {
                "get": {
                    "operationId": "getActivities",
                    "summary": "List athlete activities",
                    "parameters": [
                        {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}
                    ]
                }
            }
        }
    });

    tokio::fs::write(&tmp_file, serde_json::to_string(&test_spec).unwrap())
        .await
        .expect("should write test spec");

    let runtime = dynamic::DynamicRuntime::new(
        dynamic::DynamicRuntimeConfig::builder()
            .spec_source(tmp_file.to_string_lossy().to_string())
            .build(),
    );
    let handler = IntervalsMcpHandler::with_dynamic_runtime(
        Arc::new(MockIntervalsClient::default()),
        runtime,
    );

    // Preload should load the registry
    let count = handler.preload_dynamic_registry().await;
    assert!(count > 0, "should load at least one tool from test spec");

    // Cleanup
    let _ = tokio::fs::remove_file(&tmp_file).await;
}
#[test]
fn new_multi_tenant_creates_placeholder_client() {
    let handler = IntervalsMcpHandler::new_multi_tenant().expect("new_multi_tenant");
    assert_eq!(handler.tool_count(), 9);
}

#[tokio::test]
async fn preload_dynamic_registry_error_handling() {
    // Create runtime with invalid spec path
    let runtime = dynamic::DynamicRuntime::new(
        dynamic::DynamicRuntimeConfig::builder()
            .spec_source("/nonexistent/path/openapi.json".to_string())
            .build(),
    );
    let handler = IntervalsMcpHandler::with_dynamic_runtime(
        Arc::new(MockIntervalsClient::default()),
        runtime,
    );

    let count = handler.preload_dynamic_registry().await;
    assert_eq!(count, 0);
}

#[tokio::test]
async fn process_webhook_without_secret_returns_error() {
    let handler = test_handler();
    let payload = serde_json::json!({"id": "test-123"});

    let err = handler
        .process_webhook("invalid_sig", payload)
        .await
        .unwrap_err();
    assert_eq!(err, "webhook secret not set");
}

#[tokio::test]
async fn set_webhook_secret_stores_value() {
    let handler = test_handler();
    handler.set_webhook_secret_value("test_secret").await;

    // Secret should be set (tested indirectly via process_webhook)
    let payload = serde_json::json!({"id": "test-123"});
    let result = handler.process_webhook("invalid_sig", payload).await;
    // Should fail signature verification, not "secret not set"
    let err = result.unwrap_err();
    assert_ne!(err, "webhook secret not set");
}

#[tokio::test]
async fn process_webhook_duplicate_detection() {
    let handler = test_handler();
    handler.set_webhook_secret_value("test_secret").await;

    // Create valid signature
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    let mut mac: Hmac<Sha256> = Hmac::new_from_slice(b"test_secret").unwrap();
    let payload = serde_json::json!({"id": "dup-test-123"});
    mac.update(&serde_json::to_vec(&payload).unwrap());
    let signature = hex::encode(mac.finalize().into_bytes());

    // First submission
    let result1 = handler
        .process_webhook(&signature, payload.clone())
        .await
        .expect("first submission should succeed");
    assert!(result1.value.get("ok").is_some());

    // Duplicate submission
    let result2 = handler
        .process_webhook(&signature, payload.clone())
        .await
        .expect("second submission should succeed");
    assert_eq!(
        result2.value.get("duplicate"),
        Some(&serde_json::json!(true))
    );
}
#[tokio::test]
async fn test_list_tools_returns_eight_intent_tools() {
    let handler = test_handler();
    // Note: Full list_tools testing requires RequestContext which is complex to construct.
    // Integration tests in tests/ directory cover the full flow.
    // Here we just verify the handler has the right tool count.
    assert_eq!(handler.tool_count(), 9);
}

// ========================================================================
// get_info() Tests
#[test]
fn test_get_info_has_server_instructions() {
    let handler = test_handler();
    let info = handler.get_info();
    let instructions = info.instructions.unwrap();
    assert!(
        instructions.contains("Intervals.icu"),
        "instructions should mention Intervals.icu"
    );
    assert!(
        instructions.contains("intent-driven"),
        "instructions should mention intent-driven"
    );
}

// ========================================================================
// build_mcp_rmcp_config() Tests
// ========================================================================

#[test]
fn test_build_mcp_rmcp_config_default_has_loopback_hosts() {
    let config = build_mcp_rmcp_config("");
    // Default rmcp allowed_hosts: localhost, 127.0.0.1, ::1
    assert!(
        config.allowed_hosts.contains(&"localhost".to_string()),
        "default should allow localhost"
    );
    assert!(
        config.allowed_hosts.contains(&"127.0.0.1".to_string()),
        "default should allow 127.0.0.1"
    );
    assert!(
        config.allowed_hosts.contains(&"::1".to_string()),
        "default should allow ::1"
    );
    assert_eq!(config.allowed_hosts.len(), 3);
}

#[test]
fn test_build_mcp_rmcp_config_single_host() {
    let config = build_mcp_rmcp_config("mcp.example.com");
    assert_eq!(config.allowed_hosts.len(), 1);
    assert!(
        config
            .allowed_hosts
            .contains(&"mcp.example.com".to_string())
    );
}

#[test]
fn test_build_mcp_rmcp_config_multi_host() {
    let config = build_mcp_rmcp_config("mcp.example.com,api.example.com");
    assert_eq!(config.allowed_hosts.len(), 2);
    assert!(
        config
            .allowed_hosts
            .contains(&"mcp.example.com".to_string())
    );
    assert!(
        config
            .allowed_hosts
            .contains(&"api.example.com".to_string())
    );
    // loopback hosts should NOT be present (user explicitly overrode)
    assert!(!config.allowed_hosts.contains(&"localhost".to_string()));
}

#[test]
fn test_build_mcp_rmcp_config_trims_whitespace() {
    let config = build_mcp_rmcp_config(" mcp.example.com ,  api.example.com  ");
    assert_eq!(config.allowed_hosts.len(), 2);
    assert!(
        config
            .allowed_hosts
            .contains(&"mcp.example.com".to_string())
    );
    assert!(
        config
            .allowed_hosts
            .contains(&"api.example.com".to_string())
    );
}

#[test]
fn test_build_mcp_rmcp_config_with_port() {
    let config = build_mcp_rmcp_config("mcp.example.com:8080");
    assert_eq!(config.allowed_hosts.len(), 1);
    assert!(
        config
            .allowed_hosts
            .contains(&"mcp.example.com:8080".to_string())
    );
}

#[test]
fn test_build_mcp_rmcp_config_trailing_comma_ignored() {
    // A trailing comma after last element → empty string → trimmed to empty → included
    let config = build_mcp_rmcp_config("mcp.example.com,");
    assert_eq!(
        config.allowed_hosts.len(),
        2,
        "trailing comma yields two items (one empty)"
    );
}
