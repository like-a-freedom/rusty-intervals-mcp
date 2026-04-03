//! Prometheus metrics for MCP observability in HTTP mode.
//!
//! This module provides metrics collection for:
//! - Tool calls (counter, duration histogram)
//! - Token operations (issued, verified)
//! - HTTP requests (counter, duration, active gauge)
//! - Rate limiting (counter)
//! - Active athletes tracking (gauge, activity histogram)
//!
//! Metrics are only initialized in HTTP mode. STDIO mode is a no-op.

use ::metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// Global Prometheus handle for rendering metrics
static PROMETHEUS_HANDLE: std::sync::OnceLock<PrometheusHandle> = std::sync::OnceLock::new();

/// Active athlete tracking - maps athlete_id to last seen timestamp
struct AthleteTracker {
    athletes: Mutex<HashMap<String, std::time::Instant>>,
    window_duration: Duration,
}

impl AthleteTracker {
    #[must_use]
    fn new(window_secs: u64) -> Self {
        Self {
            athletes: Mutex::new(HashMap::new()),
            window_duration: Duration::from_secs(window_secs),
        }
    }

    /// Record activity for an athlete by ID.
    fn record_activity(&self, athlete_id: &str) {
        let now = std::time::Instant::now();
        let mut athletes = self.athletes.lock().unwrap();

        // Update last seen time for this athlete
        let old_count = athletes.len();
        athletes.insert(athlete_id.to_string(), now);
        let new_count = athletes.len();

        // If new athlete, increment the gauge
        if new_count > old_count {
            gauge!("intervals_icu_mcp_active_athletes").set(new_count as f64);
        }

        // Clean up old entries and update gauge
        self.cleanup_old_entries(&mut athletes);
    }

    /// Remove athletes whose last seen time is outside the window.
    fn cleanup_old_entries(&self, athletes: &mut HashMap<String, std::time::Instant>) {
        let now = std::time::Instant::now();
        let cutoff = now.checked_sub(self.window_duration);

        if let Some(cutoff) = cutoff {
            athletes.retain(|_id, last_seen| *last_seen > cutoff);
        } else {
            // If subtraction failed, clear all (should never happen)
            athletes.clear();
        }

        // Update gauge with current count
        gauge!("intervals_icu_mcp_active_athletes").set(athletes.len() as f64);
    }
}

/// Global athlete tracker with 5-minute window
static ATHLETE_TRACKER: std::sync::OnceLock<AthleteTracker> = std::sync::OnceLock::new();

/// Initialize the Prometheus recorder.
///
/// # Errors
///
/// Returns an error if metrics are not available or initialization fails.
///
/// # Returns
///
/// A handle for rendering metrics.
pub fn init_prometheus_recorder() -> Result<PrometheusHandle, Box<dyn std::error::Error>> {
    // Check if we're in HTTP mode
    let transport_mode = std::env::var("MCP_TRANSPORT").unwrap_or_else(|_| String::from("stdio"));

    if transport_mode != "http" {
        // In STDIO mode, metrics are disabled
        return Err("Metrics are only available in HTTP mode".into());
    }

    // Configure Prometheus with custom buckets for tool duration
    let builder = PrometheusBuilder::new()
        .set_buckets(&[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0])?;

    let handle = builder.install_recorder()?;

    // Initialize the athlete tracker
    ATHLETE_TRACKER
        .set(AthleteTracker::new(300)) // 5 minutes
        .map_err(|_| "Athlete tracker already initialized")?;

    // Store handle globally
    PROMETHEUS_HANDLE
        .set(handle.clone())
        .map_err(|_| "Prometheus recorder already initialized")?;

    tracing::info!("Prometheus metrics initialized");

    Ok(handle)
}

/// Record a tool call with its duration.
pub fn record_tool_call(tool: &str, success: bool, duration_secs: f64) {
    let status = if success { "success" } else { "error" };

    // Increment counter
    counter!(
        "intervals_icu_mcp_tool_calls_total",
        "tool" => tool.to_owned(),
        "status" => status.to_owned()
    )
    .increment(1);

    // Record duration histogram
    histogram!(
        "intervals_icu_mcp_tool_duration_seconds",
        "tool" => tool.to_owned()
    )
    .record(duration_secs);
}

/// Record token issuance.
pub fn record_token_issued() {
    counter!("intervals_icu_mcp_tokens_issued_total").increment(1);
}

/// Record token verification.
pub fn record_token_verification(status: &str) {
    counter!(
        "intervals_icu_mcp_token_verifications_total",
        "status" => status.to_owned()
    )
    .increment(1);
}

/// Record rate-limited request.
pub fn record_rate_limited(endpoint: &str) {
    counter!(
        "intervals_icu_mcp_rate_limited_total",
        "endpoint" => endpoint.to_owned()
    )
    .increment(1);
}

/// Record authentication failure with reason.
pub fn record_auth_failure(reason: &str) {
    counter!(
        "intervals_icu_mcp_auth_failures_total",
        "reason" => reason.to_owned()
    )
    .increment(1);
}

/// Record MCP session status.
pub fn record_mcp_session(status: &str) {
    counter!(
        "intervals_icu_mcp_mcp_sessions_total",
        "status" => status.to_owned()
    )
    .increment(1);
}

/// Record MCP method call.
pub fn record_mcp_method_call(method: &str) {
    counter!(
        "intervals_icu_mcp_mcp_method_calls_total",
        "method" => method.to_owned()
    )
    .increment(1);
}

/// Record idempotency cache operation.
pub fn record_idempotency(result: &str) {
    counter!(
        "intervals_icu_mcp_idempotency_total",
        "result" => result.to_owned()
    )
    .increment(1);
}

/// Record HTTP request with duration.
pub fn record_http_request(method: &str, path: &str, status: u16, duration_secs: f64) {
    let status_str = format!("{status}");

    // Increment counter
    counter!(
        "intervals_icu_mcp_http_requests_total",
        "method" => method.to_owned(),
        "path" => path.to_owned(),
        "status" => status_str
    )
    .increment(1);

    // Record duration histogram
    histogram!(
        "intervals_icu_mcp_http_request_duration_seconds",
        "method" => method.to_owned(),
        "path" => path.to_owned()
    )
    .record(duration_secs);
}

/// Increment active requests gauge.
pub fn increment_active_requests() {
    gauge!("intervals_icu_mcp_active_requests").increment(1.0);
}

/// Decrement active requests gauge.
pub fn decrement_active_requests() {
    gauge!("intervals_icu_mcp_active_requests").decrement(1.0);
}

/// Record athlete activity.
pub fn record_athlete_activity(athlete_id: &str) {
    if let Some(tracker) = ATHLETE_TRACKER.get() {
        tracker.record_activity(athlete_id);

        // Record activity in histogram (bucket by request count per athlete)
        // Simplified: just record 1.0 as placeholder
        histogram!("intervals_icu_mcp_athlete_activity_bucket").record(1.0);
    }
}

/// Get the Prometheus handle for rendering metrics.
#[must_use]
pub fn get_prometheus_handle() -> Option<&'static PrometheusHandle> {
    PROMETHEUS_HANDLE.get()
}

/// Create an Axum router for the metrics endpoint.
///
/// The endpoint will require authentication if `PROMETHEUS_METRICS_TOKEN` is set.
pub fn create_metrics_router() -> axum::Router {
    use axum::{
        Router,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::get,
    };

    async fn metrics_handler(headers: HeaderMap) -> Response {
        // Check for optional token authentication
        if let Ok(token) = std::env::var("PROMETHEUS_METRICS_TOKEN") {
            let auth_header = headers
                .get("authorization")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("");

            let provided_token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);

            if provided_token != token {
                return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
            }
        }

        // Get metrics output
        match get_prometheus_handle() {
            Some(handle) => {
                let body = handle.render();
                ([("content-type", "text/plain; version=0.0.4")], body).into_response()
            }
            None => (StatusCode::SERVICE_UNAVAILABLE, "Metrics not initialized").into_response(),
        }
    }

    Router::new().route("/metrics", get(metrics_handler))
}

/// HTTP middleware for tracking request metrics.
///
/// This middleware should be applied to all HTTP routes to collect:
/// - Request count
/// - Request duration
/// - Active requests gauge
pub async fn metrics_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::extract::MatchedPath;

    let start = std::time::Instant::now();
    let method = request.method().to_string();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| String::from("unknown"));

    // Increment active requests
    increment_active_requests();

    // Process request
    let response = next.run(request).await;

    // Decrement active requests
    decrement_active_requests();

    // Record metrics
    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16();
    record_http_request(&method, &path, status, duration);

    response
}

// Tests are in the separate `tests/metrics_tests.rs` file to avoid
// global state conflicts with the Prometheus recorder
