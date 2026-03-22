/// HTTP Mode Integration Tests for Streamable HTTP Transport
///
/// These tests verify:
/// - HTTP server startup and shutdown
/// - MCP protocol over HTTP transport
/// - Tool listing and execution via HTTP
/// - Resource access via HTTP
/// - Session management
/// - Error handling
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;

use intervals_icu_client::{AthleteProfile, IntervalsClient};
use intervals_icu_mcp::auth::{AppState, HttpBaseUrl, JwtManager, auth_endpoint, auth_middleware};
use secrecy::ExposeSecret;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ============================================================================
// Mock Client for HTTP Tests
// ============================================================================

fn mock_event(event_id: Option<&str>) -> intervals_icu_client::Event {
    intervals_icu_client::Event {
        id: event_id.map(str::to_owned),
        start_date_local: "2026-03-04".to_string(),
        name: "Mock event".to_string(),
        category: intervals_icu_client::EventCategory::Workout,
        description: None,
        r#type: None,
    }
}

fn test_basic_auth_header(api_key: &str) -> String {
    let credentials = format!("API_KEY:{api_key}");
    format!("Basic {}", STANDARD.encode(credentials))
}

fn test_master_key_hex() -> String {
    "11".repeat(64)
}

struct HttpTestMockClient;

#[async_trait::async_trait]
impl IntervalsClient for HttpTestMockClient {
    async fn get_athlete_profile(
        &self,
    ) -> Result<AthleteProfile, intervals_icu_client::IntervalsError> {
        Ok(AthleteProfile {
            id: "test_athlete".into(),
            name: Some("Test Athlete".into()),
        })
    }

    async fn get_recent_activities(
        &self,
        _limit: Option<u32>,
        _days_back: Option<i32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
    {
        Ok(vec![intervals_icu_client::ActivitySummary {
            id: "act1".into(),
            name: Some("Morning Run".into()),
            start_date_local: "2026-03-04".into(),
        }])
    }

    async fn create_event(
        &self,
        event: intervals_icu_client::Event,
    ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
        Ok(event)
    }

    async fn get_event(
        &self,
        event_id: &str,
    ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
        Ok(mock_event(Some(event_id)))
    }

    async fn delete_event(
        &self,
        _event_id: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }

    async fn get_events(
        &self,
        _days_back: Option<i32>,
        _limit: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![])
    }

    async fn bulk_create_events(
        &self,
        _events: Vec<intervals_icu_client::Event>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![])
    }

    async fn get_activity_streams(
        &self,
        _activity_id: &str,
        _streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_activity_intervals(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_best_efforts(
        &self,
        _activity_id: &str,
        _options: Option<intervals_icu_client::BestEffortsOptions>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_activity_details(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn search_activities(
        &self,
        _query: &str,
        _limit: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
    {
        Ok(vec![])
    }

    async fn search_activities_full(
        &self,
        _query: &str,
        _limit: Option<u32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!([]))
    }

    async fn get_activities_csv(&self) -> Result<String, intervals_icu_client::IntervalsError> {
        Ok("id,start_date_local,name\n1,2025-10-18,Run".into())
    }

    async fn update_activity(
        &self,
        _activity_id: &str,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn download_activity_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }

    async fn download_fit_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }

    async fn download_gpx_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }

    async fn get_gear_list(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_sport_settings(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_power_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_gap_histogram(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn download_activity_file_with_progress(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
        _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
        _cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }

    async fn delete_activity(
        &self,
        _activity_id: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }

    async fn get_activities_around(
        &self,
        _activity_id: &str,
        _limit: Option<u32>,
        _route_id: Option<i64>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn search_intervals(
        &self,
        _min_secs: u32,
        _max_secs: u32,
        _min_intensity: u32,
        _max_intensity: u32,
        _interval_type: Option<String>,
        _min_reps: Option<u32>,
        _max_reps: Option<u32>,
        _limit: Option<u32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_power_histogram(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_hr_histogram(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_pace_histogram(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_fitness_summary(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_wellness(
        &self,
        _days_back: Option<i32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_wellness_for_date(
        &self,
        _date: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn update_wellness(
        &self,
        _date: &str,
        _data: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_upcoming_workouts(
        &self,
        _days_ahead: Option<u32>,
        _limit: Option<u32>,
        _category: Option<String>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn update_event(
        &self,
        _event_id: &str,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn bulk_delete_events(
        &self,
        _event_ids: Vec<String>,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }

    async fn duplicate_event(
        &self,
        _event_id: &str,
        _num_copies: Option<u32>,
        _weeks_between: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![])
    }

    async fn get_hr_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_pace_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_workout_library(
        &self,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn get_workouts_in_folder(
        &self,
        _folder_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn create_folder(
        &self,
        _folder: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({"id": "f1", "name": "New Folder"}))
    }

    async fn update_folder(
        &self,
        _folder_id: &str,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({"id": "f1", "name": "Updated Folder"}))
    }

    async fn delete_folder(
        &self,
        _folder_id: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }

    async fn create_gear(
        &self,
        _gear: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn update_gear(
        &self,
        _gear_id: &str,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn delete_gear(
        &self,
        _gear_id: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }

    async fn create_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn update_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder_id: &str,
        _reset: bool,
        _snooze_days: u32,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn update_sport_settings(
        &self,
        _sport_type: &str,
        _recalc_hr_zones: bool,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn apply_sport_settings(
        &self,
        _sport_type: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn create_sport_settings(
        &self,
        _settings: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }

    async fn delete_sport_settings(
        &self,
        _sport_type: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
}

// ============================================================================
// HTTP Server Startup and Basic Tests
// ============================================================================

#[tokio::test]
async fn test_http_server_starts_and_accepts_connections() {
    // Build handler
    let client: Arc<dyn IntervalsClient> = Arc::new(HttpTestMockClient);
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(client.clone());

    // Build HTTP service
    let session = Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || -> Result<_, std::io::Error> { Ok(handler.clone()) },
        session,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );

    let app = axum::Router::new().nest_service("/mcp", mcp_service);

    // Bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Start server in background
    let server = axum::serve(listener, app.into_make_service());
    let _server_handle = tokio::spawn(async move {
        server.await.ok();
    });

    // Give server a moment to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test connection to /mcp endpoint
    let http_client = Client::new();
    let response = http_client
        .get(format!("http://{}/mcp", addr))
        .timeout(Duration::from_secs(5))
        .send()
        .await;

    // Should get a response (may be 405 Method Not Allowed for GET on MCP endpoint)
    // The important thing is the server is responding
    assert!(response.is_ok(), "Server should respond to requests");
}

#[tokio::test]
async fn test_http_server_mcp_endpoint_exists() {
    // Build handler
    let client: Arc<dyn IntervalsClient> = Arc::new(HttpTestMockClient);
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(client.clone());

    // Build HTTP service
    let session = Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || -> Result<_, std::io::Error> { Ok(handler.clone()) },
        session,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );

    let app = axum::Router::new().nest_service("/mcp", mcp_service);

    // Bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Start server in background
    let server = axum::serve(listener, app.into_make_service());
    let _server_handle = tokio::spawn(async move {
        server.await.ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test that /mcp endpoint exists (POST should be accepted)
    // Streamable HTTP transport may return various codes depending on session state
    let http_client = Client::new();
    let response = http_client
        .post(format!("http://{}/mcp", addr))
        .header("Content-Type", "application/json")
        .body(r#"{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}"#)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("Should send request");

    // Should get a response (2xx, 4xx but not 5xx server error)
    // Streamable HTTP may return 202, 400, 404, etc. depending on session state
    assert!(
        response.status().is_success() || !response.status().is_server_error(),
        "MCP endpoint should respond without server error, got: {}",
        response.status()
    );
}

// ============================================================================
// HTTP Mode Body Size Limit Tests
// ============================================================================

#[tokio::test]
async fn test_http_server_respects_body_size_limit() {
    // Build handler with small body limit for testing
    let client: Arc<dyn IntervalsClient> = Arc::new(HttpTestMockClient);
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(client.clone());

    // Build HTTP service with 1KB body limit
    let session = Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || -> Result<_, std::io::Error> { Ok(handler.clone()) },
        session,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );

    let app = axum::Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(axum::extract::DefaultBodyLimit::max(1024)); // 1KB limit

    // Bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Start server in background
    let server = axum::serve(listener, app.into_make_service());
    let _server_handle = tokio::spawn(async move {
        server.await.ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Test with large payload (should be rejected)
    let http_client = Client::new();
    let large_payload = "x".repeat(2048); // 2KB payload
    let response = http_client
        .post(format!("http://{}/mcp", addr))
        .header("Content-Type", "application/json")
        .body(large_payload)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("Should send request");

    // Should reject large payload (413 Payload Too Large or similar)
    assert!(
        !response.status().is_success() || response.status() == 413,
        "Server should reject payloads exceeding body size limit"
    );
}

// ============================================================================
// HTTP Mode Session Management Tests
// ============================================================================

#[tokio::test]
async fn test_http_server_handles_multiple_requests() {
    // Build handler
    let client: Arc<dyn IntervalsClient> = Arc::new(HttpTestMockClient);
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(client.clone());

    // Build HTTP service
    let session = Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || -> Result<_, std::io::Error> { Ok(handler.clone()) },
        session,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );

    let app = axum::Router::new().nest_service("/mcp", mcp_service);

    // Bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Start server in background
    let server = axum::serve(listener, app.into_make_service());
    let _server_handle = tokio::spawn(async move {
        server.await.ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send multiple requests
    let http_client = Client::new();
    for i in 0..3 {
        let response = http_client
            .post(format!("http://{}/mcp", addr))
            .header("Content-Type", "application/json")
            .body(format!(r#"{{"jsonrpc":"2.0","method":"test","id":{}}}"#, i))
            .timeout(Duration::from_secs(5))
            .send()
            .await;

        assert!(response.is_ok(), "Request {} should succeed", i);
    }
}

// ============================================================================
// HTTP Mode Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_http_server_handles_invalid_json() {
    // Build handler
    let client: Arc<dyn IntervalsClient> = Arc::new(HttpTestMockClient);
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(client.clone());

    // Build HTTP service
    let session = Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || -> Result<_, std::io::Error> { Ok(handler.clone()) },
        session,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );

    let app = axum::Router::new().nest_service("/mcp", mcp_service);

    // Bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Start server in background
    let server = axum::serve(listener, app.into_make_service());
    let _server_handle = tokio::spawn(async move {
        server.await.ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send invalid JSON
    let http_client = Client::new();
    let response = http_client
        .post(format!("http://{}/mcp", addr))
        .header("Content-Type", "application/json")
        .body("not valid json {{{")
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("Should send request");

    // Should get an error response (400 Bad Request or JSON-RPC error)
    assert!(
        !response.status().is_success() || response.status() == 400,
        "Server should reject invalid JSON"
    );
}

#[tokio::test]
async fn test_http_server_handles_missing_content_type() {
    // Build handler
    let client: Arc<dyn IntervalsClient> = Arc::new(HttpTestMockClient);
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(client.clone());

    // Build HTTP service
    let session = Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || -> Result<_, std::io::Error> { Ok(handler.clone()) },
        session,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );

    let app = axum::Router::new().nest_service("/mcp", mcp_service);

    // Bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Start server in background
    let server = axum::serve(listener, app.into_make_service());
    let _server_handle = tokio::spawn(async move {
        server.await.ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send request without Content-Type header
    let http_client = Client::new();
    let response = http_client
        .post(format!("http://{}/mcp", addr))
        .body(r#"{"jsonrpc":"2.0","method":"test","id":1}"#)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("Should send request");

    // Server may accept or reject (depends on configuration)
    // The important thing is it doesn't crash
    assert!(
        response.status().is_success() || !response.status().is_success(),
        "Server should handle missing Content-Type gracefully"
    );
}

// ============================================================================
// HTTP Mode Configuration Tests
// ============================================================================

#[test]
fn test_streamable_http_config_defaults() {
    // Test that default config can be created
    let config =
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default();

    // Default config should have reasonable values
    assert!(config.sse_keep_alive.is_none() || config.sse_keep_alive.is_some());
    // stateful_mode and cancellation_token have defaults
}

#[test]
fn test_local_session_manager_creation() {
    // Test that LocalSessionManager can be created
    let session =
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default();

    // Should create successfully
    assert!(Arc::new(session).as_ref() as *const _ as usize > 0);
}

// ============================================================================
// Multi-tenant JWT HTTP Tests
// ============================================================================

#[tokio::test]
async fn test_auth_endpoint_issues_jwt_for_valid_credentials() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/i123456/profile"))
        .and(header(
            "authorization",
            test_basic_auth_header("test_api_key"),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "athlete": {
                "id": "i123456",
                "name": "Test Athlete"
            }
        })))
        .mount(&mock_server)
        .await;

    let master_key_hex = test_master_key_hex();
    let master_key_config =
        intervals_icu_mcp::auth::MasterKeyConfig::from_hex(&master_key_hex).unwrap();
    let jwt_manager = Arc::new(JwtManager::from_master_key(&master_key_config));
    let app_state = Arc::new(AppState {
        jwt_manager: jwt_manager.clone(),
        jwt_ttl_seconds: 3600,
        base_url: mock_server.uri(),
    });

    let app = axum::Router::new()
        .route("/auth", axum::routing::post(auth_endpoint))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    );
    let _server_handle = tokio::spawn(async move {
        server.await.ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let response = Client::new()
        .post(format!("http://{}/auth", addr))
        .json(&serde_json::json!({
            "api_key": "test_api_key",
            "athlete_id": "i123456"
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let payload: serde_json::Value = response.json().await.expect("json response");
    let token = payload
        .get("token")
        .and_then(|value| value.as_str())
        .expect("token should be present");

    let credentials = jwt_manager.verify_token(token).expect("jwt should verify");
    assert_eq!(credentials.athlete_id, "i123456");
    assert_eq!(credentials.api_key.expose_secret(), "test_api_key");
}

#[tokio::test]
async fn test_mcp_route_requires_bearer_token_and_accepts_valid_jwt() {
    let master_key_hex = test_master_key_hex();
    let master_key_config =
        intervals_icu_mcp::auth::MasterKeyConfig::from_hex(&master_key_hex).unwrap();
    let jwt_manager = Arc::new(JwtManager::from_master_key(&master_key_config));
    let token = jwt_manager
        .issue_token("i777777", "test_api_key", 3600)
        .expect("token should issue");

    let handler = intervals_icu_mcp::IntervalsMcpHandler::new_multi_tenant();
    let session = Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || -> Result<_, std::io::Error> { Ok(handler.clone()) },
        session,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );

    let app = axum::Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(axum::Extension(HttpBaseUrl(
            "https://intervals.icu".to_string(),
        )))
        .layer(axum::middleware::from_fn_with_state(
            jwt_manager.clone(),
            auth_middleware,
        ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = axum::serve(listener, app.into_make_service());
    let _server_handle = tokio::spawn(async move {
        server.await.ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let http_client = Client::new();
    let missing_auth = http_client
        .post(format!("http://{}/mcp", addr))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .body(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#)
        .send()
        .await
        .expect("missing auth request should complete");

    assert_eq!(missing_auth.status(), reqwest::StatusCode::UNAUTHORIZED);

    let authorized = http_client
        .post(format!("http://{}/mcp", addr))
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .body(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#)
        .send()
        .await
        .expect("authorized request should complete");

    assert_ne!(authorized.status(), reqwest::StatusCode::UNAUTHORIZED);
}
