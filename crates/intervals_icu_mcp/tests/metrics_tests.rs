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

struct EnvironmentSnapshot(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl EnvironmentSnapshot {
    fn capture(keys: &'static [&'static str]) -> Self {
        Self(
            keys.iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        )
    }

    fn set(&self, key: &str, value: &str) {
        unsafe { std::env::set_var(key, value) };
    }

    fn remove(&self, key: &str) {
        unsafe { std::env::remove_var(key) };
    }
}

impl Drop for EnvironmentSnapshot {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

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

#[tokio::test]
#[serial]
async fn mutation_journal_metrics_expose_only_low_cardinality_signals() {
    ensure_recorder_initialized();

    intervals_icu_mcp::metrics::record_operation_journal_error();
    intervals_icu_mcp::metrics::record_operation_journal_quota_rejected("unknown_per_tenant");
    intervals_icu_mcp::metrics::set_operation_journal_unknown_snapshot(3, 42);

    let output = intervals_icu_mcp::metrics::get_prometheus_handle()
        .expect("Prometheus handle should be available after init")
        .render();

    assert!(output.contains("intervals_icu_mcp_operation_journal_errors_total"));
    assert!(output.contains("intervals_icu_mcp_operation_journal_quota_rejections_total"));
    assert!(output.contains("quota=\"unknown_per_tenant\""));
    assert!(output.contains("intervals_icu_mcp_operation_journal_unknown_operations 3"));
    assert!(output.contains("intervals_icu_mcp_operation_journal_oldest_unknown_age_seconds 42"));
}

#[tokio::test]
#[serial]
async fn oldest_unknown_age_advances_between_scrapes_without_a_refresh() {
    let environment = EnvironmentSnapshot::capture(&["PROMETHEUS_METRICS_TOKEN"]);
    environment.remove("PROMETHEUS_METRICS_TOKEN");
    ensure_recorder_initialized();
    intervals_icu_mcp::metrics::set_operation_journal_unknown_snapshot(3, 42);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/metrics", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            intervals_icu_mcp::metrics::create_metrics_router(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::new();
    let scrape_age = || async {
        let text = client
            .get(&url)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        text.lines()
            .find_map(|line| {
                line.strip_prefix("intervals_icu_mcp_operation_journal_oldest_unknown_age_seconds ")
            })
            .unwrap()
            .parse::<u64>()
            .unwrap()
    };
    let before = scrape_age().await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let after = scrape_age().await;
    server.abort();
    assert!(
        after > before,
        "age should advance between scrapes: {before} -> {after}"
    );
}

/// Verify HTTP startup replaces stale in-process metrics with the SQLite snapshot.
#[tokio::test]
#[serial]
async fn http_startup_refreshes_unknown_operation_metrics_from_sqlite() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("state.sqlite3");
    let database_path = database_path.to_str().unwrap();
    let environment = EnvironmentSnapshot::capture(&[
        "MCP_TRANSPORT",
        "JWT_MASTER_KEY",
        "MCP_STATE_DB_PATH",
        "MCP_STATE_ENCRYPTION_KEY",
        "MCP_TOKEN_REGISTRY_PATH",
        "PROMETHEUS_METRICS_TOKEN",
    ]);
    environment.set("MCP_TRANSPORT", "http");
    environment.set("JWT_MASTER_KEY", &"11".repeat(64));
    environment.set("MCP_STATE_DB_PATH", database_path);
    environment.set("MCP_STATE_ENCRYPTION_KEY", &"22".repeat(32));
    environment.remove("MCP_TOKEN_REGISTRY_PATH");
    environment.remove("PROMETHEUS_METRICS_TOKEN");
    ensure_recorder_initialized();

    intervals_icu_mcp::metrics::set_operation_journal_unknown_snapshot(2, 99);

    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);

    let server = intervals_icu_mcp::run_http_server(address, 1024);
    tokio::pin!(server);
    let response = tokio::select! {
        result = &mut server => panic!("HTTP server stopped before metrics could be scraped: {result:?}"),
        response = async {
            for _ in 0..100 {
                if let Ok(mut stream) = tokio::net::TcpStream::connect(address).await {
                    stream
                        .write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                        .await
                        .unwrap();
                    let mut bytes = Vec::new();
                    stream.read_to_end(&mut bytes).await.unwrap();
                    return Some(String::from_utf8_lossy(&bytes).into_owned());
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            None
        } => response.expect("HTTP server should expose /metrics"),
    };
    assert!(
        response.contains("intervals_icu_mcp_operation_journal_unknown_operations 0"),
        "startup should replace stale unknown count with the SQLite snapshot: {response}"
    );
    assert!(
        response.contains("intervals_icu_mcp_operation_journal_oldest_unknown_age_seconds 0"),
        "startup should replace stale unknown age with the SQLite snapshot: {response}"
    );
}

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
        output.contains("intervals_icu_client_upstream_requests_total"),
        "Should contain upstream requests counter: {}",
        output
    );
    assert!(
        output.contains("intervals_icu_client_upstream_request_duration_seconds"),
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
        output.contains("intervals_icu_client_upstream_errors_total"),
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
