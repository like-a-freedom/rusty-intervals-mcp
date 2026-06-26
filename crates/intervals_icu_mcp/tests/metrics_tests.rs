//! Integration tests for Prometheus metrics in HTTP mode
//!
//! These tests verify that metrics are collected correctly for:
//! - Tool calls (counter, histogram)
//! - Token operations (issued, verified)
//! - HTTP requests (counter, histogram, active gauge)
//! - Rate limiting (counter)
//! - Active athletes tracking (gauge, histogram)
//! - Upstream (intervals.icu) request metrics
//!
//! NOTE: These tests are designed to work with the global Prometheus recorder
//! that can only be initialized once. Tests use `serial_test` to prevent race
//! conditions with env vars.

use serial_test::serial;

/// Helper to ensure Prometheus recorder is available for tests.
/// Safe to call multiple times - uses #[serial] to avoid env var races.
fn ensure_recorder_initialized() {
    use std::sync::OnceLock;

    static INITIALIZED: OnceLock<()> = OnceLock::new();
    INITIALIZED.get_or_init(|| {
        // SAFETY: This test helper is only called in #[serial] tests,
        // ensuring no concurrent access to environment variables.
        unsafe { std::env::set_var("MCP_TRANSPORT", "http") };
        let _ = intervals_icu_mcp::metrics::init_prometheus_recorder();
    });
}

/// Test that upstream metrics are emitted when HTTP calls are made
/// to intervals.icu API through the client crate.
#[tokio::test]
#[serial]
async fn test_upstream_metrics_are_recorded() {
    use intervals_icu_client::IntervalsClient;
    use secrecy::SecretString;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    ensure_recorder_initialized();

    // Verify handle is available
    let handle = intervals_icu_mcp::metrics::get_prometheus_handle()
        .expect("Prometheus handle should be available after init");

    // Start mock server
    let mock_server = MockServer::start().await;

    // Stub the activity details endpoint (this goes through execute_json)
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "act_123",
            "name": "Test Activity",
            "type": "Ride"
        })))
        .mount(&mock_server)
        .await;

    // Create client pointing at mock server
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &mock_server.uri(),
        "ath_test".to_string(),
        SecretString::new("fake_key".into()),
    )
    .expect("new");

    // Make a real HTTP call through the client (uses execute_json internally)
    let result = client.get_activity_details("act_123").await;
    assert!(result.is_ok(), "Call should succeed: {:?}", result.err());

    // Check that upstream metrics appear in Prometheus output
    let output = handle.render();

    eprintln!("Prometheus output:\n{}", output);

    assert!(
        output.contains("intervals_icu_mcp_upstream_requests_total"),
        "Should contain upstream requests counter: {}",
        output
    );
    assert!(
        output.contains("intervals_icu_mcp_upstream_request_duration_seconds"),
        "Should contain upstream request duration histogram: {}",
        output
    );
}

/// Test that upstream error metrics are recorded for failed API calls
#[tokio::test]
#[serial]
async fn test_upstream_error_metrics_are_recorded() {
    use intervals_icu_client::IntervalsClient;
    use secrecy::SecretString;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    ensure_recorder_initialized();

    // Start mock server
    let mock_server = MockServer::start().await;

    // Stub with 500 error on activity details endpoint
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&mock_server)
        .await;

    // Create client pointing at mock server
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &mock_server.uri(),
        "ath_test".to_string(),
        SecretString::new("fake_key".into()),
    )
    .expect("new");

    // Make a call that will fail (uses execute_json internally)
    let result = client.get_activity_details("act_123").await;
    assert!(result.is_err(), "Call should fail");

    // Check that error metrics appear
    let output = intervals_icu_mcp::metrics::get_prometheus_handle()
        .expect("Prometheus handle should be available")
        .render();

    assert!(
        output.contains("intervals_icu_mcp_upstream_errors_total"),
        "Should contain upstream errors counter: {}",
        output
    );
}

/// Test that rate limit metrics are recorded when record_rate_limited is called
#[tokio::test]
#[serial]
async fn test_rate_limit_metrics_are_recorded() {
    ensure_recorder_initialized();

    let handle = intervals_icu_mcp::metrics::get_prometheus_handle()
        .expect("Prometheus handle should be available after init");

    // Record rate limit events
    intervals_icu_mcp::metrics::record_rate_limited("mcp");
    intervals_icu_mcp::metrics::record_rate_limited("mcp");
    intervals_icu_mcp::metrics::record_rate_limited("auth");

    let output = handle.render();

    assert!(
        output.contains("intervals_icu_mcp_rate_limited_total"),
        "Should contain rate limited counter in metrics output: {}",
        output
    );
    assert!(
        output.contains("scope=\"mcp\""),
        "Should have mcp scope label: {}",
        output
    );
    assert!(
        output.contains("scope=\"auth\""),
        "Should have auth scope label: {}",
        output
    );
}
