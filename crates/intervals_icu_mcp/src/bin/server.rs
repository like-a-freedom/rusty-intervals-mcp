use axum::debug_handler;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tracing::info;

use intervals_icu_client::{Event, IntervalsClient, http_client::ReqwestIntervalsClient};
use serde::Serialize;

#[derive(Serialize)]
struct ProfileDto {
    id: String,
    name: Option<String>,
}

#[derive(Serialize)]
struct ActivitySummaryDto {
    id: String,
    name: Option<String>,
}
use intervals_icu_mcp::IntervalsMcpHandler;
use secrecy::SecretString;

struct AppState {
    client: Arc<dyn IntervalsClient>,
    metrics: PrometheusHandle,
    handler: crate::IntervalsMcpHandler,
}

#[debug_handler]
async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

#[debug_handler]
async fn metrics_endpoint(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let body = state.metrics.render();
    ([("content-type", "text/plain; version=0.0.4")], body)
}

#[debug_handler]
async fn get_profile(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ProfileDto>, (StatusCode, String)> {
    let p = state.client.get_athlete_profile().await.map_err(map_err)?;
    Ok(Json(ProfileDto {
        id: p.id,
        name: p.name,
    }))
}

#[debug_handler]
async fn get_activities(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<ActivitySummaryDto>>, (StatusCode, String)> {
    let limit = params.get("limit").and_then(|s| s.parse::<u32>().ok());
    let days_back = params.get("days_back").and_then(|s| s.parse::<u32>().ok());
    let acts = state
        .client
        .get_recent_activities(limit, days_back)
        .await
        .map_err(map_err)?;
    let dto = acts
        .into_iter()
        .map(|a| ActivitySummaryDto {
            id: a.id,
            name: a.name,
        })
        .collect();
    Ok(Json(dto))
}

#[debug_handler]
async fn create_event(
    State(state): State<Arc<AppState>>,
    Json(ev): Json<Event>,
) -> Result<Json<Event>, (StatusCode, String)> {
    state
        .client
        .create_event(ev)
        .await
        .map(Json)
        .map_err(map_err)
}

#[debug_handler]
async fn get_event(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Event>, (StatusCode, String)> {
    state.client.get_event(&id).await.map(Json).map_err(map_err)
}

#[debug_handler]
async fn webhook(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> Result<StatusCode, (StatusCode, String)> {
    let sig = headers
        .get("x-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "missing signature".to_string()))?;

    // forward directly to the handler; no intermediate `obj` variable required
    match state.handler.process_webhook(sig, payload).await {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

#[debug_handler]
async fn delete_event(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .client
        .delete_event(&id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

fn map_err(e: intervals_icu_client::IntervalsError) -> (StatusCode, String) {
    match e {
        intervals_icu_client::IntervalsError::Http(_) => (StatusCode::BAD_GATEWAY, e.to_string()),
        intervals_icu_client::IntervalsError::Config(_) => (StatusCode::BAD_REQUEST, e.to_string()),
    }
}

/// Validate that required credentials are present in the provided values.
/// Returns Ok((api_key, athlete)) when both are non-empty, otherwise an Err
/// with a human-friendly message.
fn validate_credentials_from(api_key: &str, athlete: &str) -> Result<(String, String), String> {
    if api_key.trim().is_empty() || athlete.trim().is_empty() {
        Err("INTERVALS_ICU_API_KEY and INTERVALS_ICU_ATHLETE_ID must be set".to_string())
    } else {
        Ok((api_key.to_owned(), athlete.to_owned()))
    }
}

/// Read credentials from the environment and validate them.
fn validate_credentials() -> Result<(String, String), String> {
    let api_key = std::env::var("INTERVALS_ICU_API_KEY").unwrap_or_default();
    let athlete = std::env::var("INTERVALS_ICU_ATHLETE_ID").unwrap_or_default();
    validate_credentials_from(&api_key, &athlete)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn validate_credentials_fails_when_missing() {
        // Validate using explicit inputs to avoid mutating global env in tests
        assert!(validate_credentials_from("", "").is_err());
    }

    #[test]
    fn validate_credentials_succeeds_when_present() {
        let res = validate_credentials_from("tok", "i123");
        assert!(res.is_ok());
        let (key, athlete) = res.unwrap();
        assert_eq!(key, "tok");
        assert_eq!(athlete, "i123");
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Configure logging from env var `INTERVALS_ICU_LOG_LEVEL` (or fallback to `RUST_LOG`, default `info`).
    let log_env = std::env::var("INTERVALS_ICU_LOG_LEVEL")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "info".to_string());
    // Default to compact, human-friendly output and silence very verbose
    // RMCP-internal debug fields by default (they can be enabled via env).
    let env_filter = tracing_subscriber::EnvFilter::try_new(log_env.clone())
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,rmcp=warn"));
    tracing_subscriber::fmt()
        .compact()
        .with_ansi(false)
        .with_target(false)
        .with_env_filter(env_filter)
        .init();
    tracing::info!(%log_env, "intervals_icu_mcp:http: log filter");

    let builder = PrometheusBuilder::new();
    let handle = builder.install_recorder()?;

    let base_url = std::env::var("INTERVALS_ICU_BASE_URL")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());

    let (api_key_raw, athlete) = match validate_credentials() {
        Ok((k, a)) => (k, a),
        Err(msg) => {
            tracing::info!(%msg, "missing credentials; aborting startup");
            std::process::exit(1);
        }
    };

    let api_key = SecretString::new(api_key_raw.into_boxed_str());
    let client: Arc<dyn IntervalsClient> = Arc::new(ReqwestIntervalsClient::new(
        &base_url,
        athlete.clone(),
        api_key,
    ));
    let handler = crate::IntervalsMcpHandler::new(client.clone());
    let state = Arc::new(AppState {
        client: client.clone(),
        metrics: handle.clone(),
        handler: handler.clone(),
    });

    let max_body_size = std::env::var("MAX_HTTP_BODY_SIZE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50 * 1024 * 1024);

    // Build rmcp StreamableHttpService mounted at /mcp
    let handler = crate::IntervalsMcpHandler::new(state.client.clone());
    let factory = move || -> Result<_, std::io::Error> { Ok(handler.clone()) };
    let session = std::sync::Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        factory,
        session,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_endpoint))
        .route("/athlete/profile", get(get_profile))
        .route("/activities", get(get_activities))
        .route("/events", post(create_event))
        .route("/events/{id}", get(get_event).delete(delete_event))
        .route("/webhook", post(webhook))
        .nest_service("/mcp", mcp_service)
        .layer(axum::extract::DefaultBodyLimit::max(max_body_size))
        .with_state(state.clone());

    let addr: SocketAddr = std::env::var("ADDRESS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 3000)));
    info!(%addr, max_body_bytes = max_body_size, "starting HTTP server");

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind to address {addr}: {e}");
            std::process::exit(1);
        }
    };

    let server = axum::serve(listener, app.into_make_service());
    if let Err(e) = server
        .with_graceful_shutdown(async {
            signal::ctrl_c()
                .await
                .expect("failed to install ctrl+c handler");
        })
        .await
    {
        tracing::error!("Server error: {e}");
        std::process::exit(1);
    }

    Ok(())
}
