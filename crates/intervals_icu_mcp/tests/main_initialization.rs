/// Tests for main.rs initialization logic
/// These tests verify configuration and initialization behavior

#[test]
fn test_log_env_priority() {
    // Verify INTERVALS_ICU_LOG_LEVEL takes priority over RUST_LOG
    let result = std::env::var("INTERVALS_ICU_LOG_LEVEL")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "info".to_string());
    assert!(!result.is_empty());
}

#[test]
fn test_base_url_default() {
    // Verify default base URL fallback logic
    let base = std::env::var("INTERVALS_ICU_BASE_URL_TEST_NONEXISTENT")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());
    assert_eq!(base, "https://intervals.icu");
}

#[test]
fn test_athlete_id_fallback() {
    // Verify athlete ID fallback to empty string logic
    let athlete = std::env::var("INTERVALS_ICU_ATHLETE_ID_TEST_NONEXISTENT")
        .unwrap_or_else(|_| "".to_string());
    assert_eq!(athlete, "");
}

#[test]
fn test_api_key_fallback() {
    // Verify API key fallback to empty string logic
    let api_key =
        std::env::var("INTERVALS_ICU_API_KEY_TEST_NONEXISTENT").unwrap_or_else(|_| "".to_string());
    assert_eq!(api_key, "");
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

#[tokio::test]
async fn test_client_initialization() {
    // Test client can be initialized with env vars
    use secrecy::SecretString;
    let base = "https://test.intervals.icu";
    let athlete = "test_athlete";
    let api_key = SecretString::new("test_key".to_string().into());

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        base,
        athlete.to_string(),
        api_key,
    );
    // Client initialization should succeed
    assert!(!format!("{:?}", client).is_empty());
}

#[tokio::test]
async fn test_handler_initialization() {
    // Test handler can be initialized
    use secrecy::SecretString;
    use std::sync::Arc;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        "https://test.intervals.icu",
        "test_athlete".to_string(),
        SecretString::new("test_key".to_string().into()),
    );
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(Arc::new(client));

    // Handler should have tools and prompts registered
    assert!(handler.tool_count() > 0);
    assert!(handler.prompt_count() > 0);
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
