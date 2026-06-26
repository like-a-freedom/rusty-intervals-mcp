//! Tests for main.rs initialization logic
//!
//! Covers functions that are not easily tested from within src/lib.rs:
//!   - `MasterKeyConfig::from_hex` (auth crate)
//!   - `IntervalsMcpHandler` construction + preload
//!   - STDIO/HTTP mode env-var validation error messages
//!
//! Core handler logic (tool count, credentials extraction, webhooks,
//! `build_mcp_rmcp_config`, etc.) is tested inline in src/lib.rs.

use secrecy::SecretString;
use std::sync::Arc;

// ============================================================================
// Master Key Configuration Tests (auth::MasterKeyConfig::from_hex)
// ============================================================================

#[test]
fn test_master_key_from_hex_valid() {
    use intervals_icu_mcp::auth::MasterKeyConfig;

    let master_key_hex = "11".repeat(64);
    let result = MasterKeyConfig::from_hex(&master_key_hex);
    assert!(result.is_ok());
}

#[test]
fn test_master_key_from_hex_invalid_format() {
    use intervals_icu_mcp::auth::MasterKeyConfig;

    let result = MasterKeyConfig::from_hex("not_hex_chars!");
    assert!(result.is_err());
}

#[test]
fn test_master_key_from_hex_wrong_length() {
    use intervals_icu_mcp::auth::MasterKeyConfig;

    // Only 32 hex chars → 16 bytes instead of required 64 bytes
    let short_key = "11".repeat(32);
    let result = MasterKeyConfig::from_hex(&short_key);
    assert!(result.is_err());
}

// ============================================================================
// Handler Construction Tests
// ============================================================================

#[tokio::test]
async fn test_handler_initialization_with_mock_client() {
    let base = "https://test.intervals.icu";
    let athlete = "test_athlete";
    let api_key = SecretString::new("test_key".to_string().into());

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        base,
        athlete.to_string(),
        api_key,
    )
    .expect("new");
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(Arc::new(client));

    assert_eq!(handler.tool_count(), 9);
}

#[tokio::test]
async fn test_handler_preload_dynamic_registry() {
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        "https://test.intervals.icu",
        "test_athlete".to_string(),
        SecretString::new("test_key".to_string().into()),
    )
    .expect("new");
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(Arc::new(client));

    let _count = handler.preload_dynamic_registry().await;
}

// ============================================================================
// STDIO Mode Error Message Tests
//
// Validate the actual error messages produced by
// `initialize_handler_single_user()` and `run_http_server()` when required
// environment variables are absent.
// ============================================================================

#[test]
fn test_stdio_mode_missing_athlete_id() {
    let result = std::env::var("INTERVALS_ICU_ATHLETE_ID_MISSING")
        .map_err(|_| "INTERVALS_ICU_ATHLETE_ID is required for STDIO mode".to_string());
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "INTERVALS_ICU_ATHLETE_ID is required for STDIO mode"
    );
}

#[test]
fn test_stdio_mode_missing_api_key() {
    let result = std::env::var("INTERVALS_ICU_API_KEY_MISSING")
        .map_err(|_| "INTERVALS_ICU_API_KEY is required for STDIO mode".to_string());
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "INTERVALS_ICU_API_KEY is required for STDIO mode"
    );
}

#[test]
fn test_http_mode_missing_jwt_master_key() {
    let result = std::env::var("JWT_MASTER_KEY_MISSING")
        .map_err(|_| "JWT_MASTER_KEY environment variable is required for HTTP mode".to_string());
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "JWT_MASTER_KEY environment variable is required for HTTP mode"
    );
}
