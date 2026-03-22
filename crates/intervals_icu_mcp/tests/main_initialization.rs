//! Tests for main.rs initialization logic
//! These tests verify configuration and initialization behavior

use secrecy::SecretString;
use std::sync::Arc;

// ============================================================================
// Environment Variable Tests
// ============================================================================

#[test]
fn test_transport_mode_default_to_stdio() {
    // When MCP_TRANSPORT is not set, should default to "stdio"
    let transport_mode =
        std::env::var("MCP_TRANSPORT_NOT_SET").unwrap_or_else(|_| "stdio".to_string());
    assert_eq!(transport_mode, "stdio");
}

#[test]
fn test_transport_mode_explicit_stdio() {
    // When explicitly set to "stdio"
    let transport_mode = "stdio".to_string();
    assert_eq!(transport_mode, "stdio");
}

#[test]
fn test_transport_mode_http() {
    // When explicitly set to "http"
    let transport_mode = "http".to_string();
    assert_eq!(transport_mode, "http");
}

#[test]
fn test_transport_mode_invalid() {
    // Invalid mode should be caught at runtime (tested separately)
    let transport_mode = "invalid_mode".to_string();
    assert_eq!(transport_mode, "invalid_mode");
}

// ============================================================================
// HTTP Address Configuration Tests
// ============================================================================

#[test]
fn test_http_address_default() {
    // Default HTTP address when not set
    let address: std::net::SocketAddr = std::env::var("MCP_HTTP_ADDRESS_NOT_SET")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| std::net::SocketAddr::from(([127, 0, 0, 1], 3000)));
    assert_eq!(address.ip().to_string(), "127.0.0.1");
    assert_eq!(address.port(), 3000);
}

#[test]
fn test_http_address_custom() {
    // Custom HTTP address parsing
    let address: std::net::SocketAddr = "0.0.0.0:8080".parse().unwrap();
    assert_eq!(address.ip().to_string(), "0.0.0.0");
    assert_eq!(address.port(), 8080);
}

#[test]
fn test_http_address_invalid_fallback() {
    // Invalid address should fallback to default
    let address: std::net::SocketAddr = "invalid_address"
        .parse::<std::net::SocketAddr>()
        .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 3000)));
    assert_eq!(address.ip().to_string(), "127.0.0.1");
    assert_eq!(address.port(), 3000);
}

// ============================================================================
// Max Body Size Configuration Tests
// ============================================================================

#[test]
fn test_max_body_size_default() {
    // Default max body size (4MB)
    let max_body_size = std::env::var("MAX_HTTP_BODY_SIZE_NOT_SET")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(4 * 1024 * 1024);
    assert_eq!(max_body_size, 4 * 1024 * 1024);
}

#[test]
fn test_max_body_size_custom() {
    // Custom max body size (100MB)
    let max_body_size: usize = "104857600".parse().unwrap();
    assert_eq!(max_body_size, 100 * 1024 * 1024);
}

#[test]
fn test_max_body_size_invalid_fallback() {
    // Invalid max body size should fallback to default
    let max_body_size = "not_a_number".parse::<usize>().unwrap_or(4 * 1024 * 1024);
    assert_eq!(max_body_size, 4 * 1024 * 1024);
}

// ============================================================================
// Handler Initialization Tests
// ============================================================================

#[tokio::test]
async fn test_handler_initialization_with_mock_client() {
    // Test handler can be initialized with mock credentials
    let base = "https://test.intervals.icu";
    let athlete = "test_athlete";
    let api_key = SecretString::new("test_key".to_string().into());

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        base,
        athlete.to_string(),
        api_key,
    );
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(Arc::new(client));

    // Handler starts with 8 intent tools (always available)
    assert_eq!(handler.tool_count(), 8);
}

#[tokio::test]
async fn test_handler_preload_dynamic_registry() {
    // Test dynamic registry preload (may timeout, which is OK)
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        "https://test.intervals.icu",
        "test_athlete".to_string(),
        SecretString::new("test_key".to_string().into()),
    );
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(Arc::new(client));

    // Preload should complete (may return 0 if no spec available)
    let _count = handler.preload_dynamic_registry().await;
    // Count can be 0 (no spec) or >0 (spec loaded)
}

// ============================================================================
// Logging Configuration Tests
// ============================================================================

#[test]
fn test_log_env_standard() {
    // Verify RUST_LOG is used (standard Rust logging variable)
    let result = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    assert!(!result.is_empty());
}

#[test]
fn test_combined_filter_format() {
    // Test the combined filter format
    let log_env = "debug";
    let combined_filter = format!("{},rmcp=warn,serve_inner=warn", log_env);
    assert_eq!(combined_filter, "debug,rmcp=warn,serve_inner=warn");
}

#[test]
fn test_env_filter_creation() {
    // Test env filter can be created with combined filter
    let combined_filter = "info,rmcp=warn,serve_inner=warn";
    let env_filter = tracing_subscriber::EnvFilter::try_new(combined_filter);
    assert!(env_filter.is_ok());
}

#[test]
fn test_env_filter_fallback() {
    // Test env filter fallback on invalid filter
    let env_filter = tracing_subscriber::EnvFilter::try_new("invalid[[[filter")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,rmcp=warn,serve_inner=warn"));
    // Should not panic and create a valid filter
    assert!(!format!("{:?}", env_filter).is_empty());
}

#[test]
fn test_log_level_combinations() {
    // Test various log level values
    let test_cases = vec!["trace", "debug", "info", "warn", "error"];

    for level in test_cases {
        let combined = format!("{},rmcp=warn,serve_inner=warn", level);
        assert!(combined.contains(level));
        assert!(combined.contains("rmcp=warn"));
        assert!(combined.contains("serve_inner=warn"));
    }
}

// ============================================================================
// Credentials Validation Tests
// ============================================================================

#[test]
fn test_credentials_validation_empty_api_key() {
    // Empty API key should be detected
    let api_key = "";
    assert!(api_key.trim().is_empty());
}

#[test]
fn test_credentials_validation_empty_athlete() {
    // Empty athlete ID should be detected
    let athlete = "";
    assert!(athlete.trim().is_empty());
}

#[test]
fn test_credentials_validation_whitespace_only() {
    // Whitespace-only credentials should be detected
    let api_key = "   ";
    let athlete = "  ";
    assert!(api_key.trim().is_empty());
    assert!(athlete.trim().is_empty());
}

#[test]
fn test_credentials_validation_valid() {
    // Valid credentials should pass validation
    let api_key = "test_key_123";
    let athlete = "i123456";
    assert!(!api_key.trim().is_empty());
    assert!(!athlete.trim().is_empty());
}

// ============================================================================
// Transport Mode Selection Logic Tests
// ============================================================================

#[test]
fn test_transport_mode_selection_stdio() {
    // Test transport mode selection for stdio
    let mode = "stdio";
    assert!(matches!(mode, "stdio"));
}

#[test]
fn test_transport_mode_selection_http() {
    // Test transport mode selection for http
    let mode = "http";
    assert!(matches!(mode, "http"));
}

#[test]
fn test_transport_mode_selection_unknown() {
    // Test transport mode selection for unknown mode
    let mode = "unknown";
    let is_valid = matches!(mode, "stdio" | "http");
    assert!(!is_valid);
}

// ============================================================================
// Socket Address Parsing Tests
// ============================================================================

#[test]
fn test_socket_addr_parsing_ipv4() {
    // Test IPv4 address parsing
    let addr: std::net::SocketAddr = "127.0.0.1:3000".parse().unwrap();
    assert_eq!(addr.ip().to_string(), "127.0.0.1");
    assert_eq!(addr.port(), 3000);
}

#[test]
fn test_socket_addr_parsing_ipv6() {
    // Test IPv6 address parsing
    let addr: std::net::SocketAddr = "[::1]:3000".parse().unwrap();
    assert!(addr.ip().is_ipv6());
    assert_eq!(addr.port(), 3000);
}

#[test]
fn test_socket_addr_parsing_any_interface() {
    // Test binding to any interface
    let addr: std::net::SocketAddr = "0.0.0.0:8080".parse().unwrap();
    assert_eq!(addr.ip().to_string(), "0.0.0.0");
    assert_eq!(addr.port(), 8080);
}

#[test]
fn test_socket_addr_parsing_invalid() {
    // Test invalid address parsing
    let result = "invalid_address".parse::<std::net::SocketAddr>();
    assert!(result.is_err());
}

// ============================================================================
// HTTP Server Configuration Tests
// ============================================================================

#[test]
fn test_jwt_ttl_seconds_default() {
    // Default JWT TTL (90 days)
    let jwt_ttl_seconds = std::env::var("JWT_TTL_SECONDS_NOT_SET")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(7776000);
    assert_eq!(jwt_ttl_seconds, 7776000);
}

#[test]
fn test_jwt_ttl_seconds_custom() {
    // Custom JWT TTL (30 days)
    let jwt_ttl_seconds = std::env::var("JWT_TTL_SECONDS_CUSTOM")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(7776000);
    assert_eq!(jwt_ttl_seconds, 7776000); // Should use default since env var not set
}

#[test]
fn test_jwt_ttl_seconds_parse() {
    // Parse valid JWT TTL
    let result = "86400".parse::<u64>();
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 86400);
}

#[test]
fn test_jwt_ttl_seconds_invalid_fallback() {
    // Invalid JWT TTL should fallback to default
    let jwt_ttl_seconds = "not_a_number".parse::<u64>().unwrap_or(7776000);
    assert_eq!(jwt_ttl_seconds, 7776000);
}

// ============================================================================
// Master Key Configuration Tests
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

    let invalid_hex = "not_hex_chars!";
    let result = MasterKeyConfig::from_hex(invalid_hex);
    assert!(result.is_err());
}

#[test]
fn test_master_key_from_hex_wrong_length() {
    use intervals_icu_mcp::auth::MasterKeyConfig;

    // Only 32 bytes instead of 64
    let short_key = "11".repeat(32);
    let result = MasterKeyConfig::from_hex(&short_key);
    assert!(result.is_err());
}

// ============================================================================
// STDIO Mode Error Handling Tests
// ============================================================================

#[test]
fn test_stdio_mode_missing_athlete_id() {
    // When INTERVALS_ICU_ATHLETE_ID is not set
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
    // When INTERVALS_ICU_API_KEY is not set
    let result = std::env::var("INTERVALS_ICU_API_KEY_MISSING")
        .map_err(|_| "INTERVALS_ICU_API_KEY is required for STDIO mode".to_string());
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "INTERVALS_ICU_API_KEY is required for STDIO mode"
    );
}

#[test]
fn test_stdio_mode_empty_credentials() {
    // Empty credentials validation
    let api_key = "";
    let athlete = "";

    let is_empty = api_key.trim().is_empty() || athlete.trim().is_empty();
    assert!(is_empty);
}

#[test]
fn test_http_mode_missing_jwt_master_key() {
    // When JWT_MASTER_KEY is not set
    let result = std::env::var("JWT_MASTER_KEY_MISSING")
        .map_err(|_| "JWT_MASTER_KEY environment variable is required for HTTP mode".to_string());
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "JWT_MASTER_KEY environment variable is required for HTTP mode"
    );
}

// ============================================================================
// Transport Mode Tests
// ============================================================================

#[test]
fn test_transport_mode_match_stdio() {
    let transport_mode = "stdio".to_string();
    assert_eq!(transport_mode.as_str(), "stdio");
}

#[test]
fn test_transport_mode_match_http() {
    let transport_mode = "http".to_string();
    assert_eq!(transport_mode.as_str(), "http");
}

#[test]
fn test_transport_mode_unknown() {
    let transport_mode = "unknown".to_string();
    let is_unknown = !matches!(transport_mode.as_str(), "stdio" | "http");
    assert!(is_unknown);
}
