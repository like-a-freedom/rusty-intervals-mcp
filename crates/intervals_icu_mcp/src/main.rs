use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};
use intervals_icu_mcp::IntervalsMcpHandler;
use intervals_icu_mcp::auth::{
    AppState, HttpBaseUrl, JwtManager, MasterKeyConfig, auth_endpoint, auth_middleware,
};
use intervals_icu_mcp::metrics;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use secrecy::SecretString;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tower_http::timeout::TimeoutLayer;

/// STDIO mode: initialize with credentials from env
async fn initialize_handler_single_user() -> Result<IntervalsMcpHandler, String> {
    let base = std::env::var("INTERVALS_ICU_BASE_URL")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());
    let athlete = std::env::var("INTERVALS_ICU_ATHLETE_ID")
        .map_err(|_| "INTERVALS_ICU_ATHLETE_ID is required for STDIO mode")?;
    let api_key = std::env::var("INTERVALS_ICU_API_KEY")
        .map_err(|_| "INTERVALS_ICU_API_KEY is required for STDIO mode")?;

    if api_key.trim().is_empty() || athlete.trim().is_empty() {
        return Err("Credentials cannot be empty".to_string());
    }

    tracing::info!(athlete_id = %athlete, "credentials validated");

    let api_key = SecretString::new(api_key.into());
    let client =
        intervals_icu_client::http_client::ReqwestIntervalsClient::new(&base, athlete, api_key);
    let handler = IntervalsMcpHandler::new(Arc::new(client));

    // Preload dynamic registry
    let dynamic_tools = match tokio::time::timeout(
        Duration::from_secs(3),
        handler.preload_dynamic_registry(),
    )
    .await
    {
        Ok(count) => count,
        Err(_) => {
            tracing::warn!("timed out preloading dynamic OpenAPI registry");
            0
        }
    };

    tracing::info!("discovered {} dynamic tools", dynamic_tools);
    Ok(handler)
}

/// HTTP mode: multi-tenant with JWT authentication
async fn run_http_server(
    address: SocketAddr,
    max_body_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize Prometheus metrics (HTTP mode only)
    // Ignore error if already initialized (e.g., in tests)
    let _ = metrics::init_prometheus_recorder();

    // JWT_MASTER_KEY is required for HTTP mode (64 bytes = 128 hex chars)
    let jwt_master_key_hex = std::env::var("JWT_MASTER_KEY")
        .map_err(|_| "JWT_MASTER_KEY environment variable is required for HTTP mode")?;

    // Parse master key and derive signing/encryption keys via HKDF
    let master_key_config = MasterKeyConfig::from_hex(&jwt_master_key_hex)
        .map_err(|e| format!("Invalid JWT_MASTER_KEY: {}", e))?;

    let jwt_manager = Arc::new(JwtManager::from_master_key(&master_key_config));

    // JWT TTL: configurable, 3 months (90 days) by default
    let jwt_ttl_seconds = std::env::var("JWT_TTL_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(7776000); // 90 days = 3 months

    // Request timeout: maximum time to process a single request
    let request_timeout_secs = std::env::var("REQUEST_TIMEOUT_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    let request_timeout = Duration::from_secs(request_timeout_secs);

    // Idle timeout: maximum idle connection time
    let idle_timeout_secs = std::env::var("IDLE_TIMEOUT_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(60);
    let idle_timeout = Duration::from_secs(idle_timeout_secs);

    let base_url = std::env::var("INTERVALS_ICU_BASE_URL")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());

    let app_state = Arc::new(AppState {
        jwt_manager: jwt_manager.clone(),
        jwt_ttl_seconds,
        base_url: base_url.clone(),
    });

    // Handler for multi-tenant mode (no credentials, extracted from JWT)
    let handler = IntervalsMcpHandler::new_multi_tenant();

    // Auth endpoint with rate limiting (brute force protection)
    let auth_config = GovernorConfigBuilder::default()
        .per_second(1)
        .burst_size(3)
        .finish()
        .unwrap();
    let auth_route = Router::new()
        .route("/auth", post(auth_endpoint))
        .layer(GovernorLayer::new(auth_config))
        .layer(TimeoutLayer::with_status_code(
            hyper::StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ))
        .with_state(app_state.clone());

    // MCP service with auth middleware and rate limiting
    let session = Arc::new(LocalSessionManager::default());
    let mcp_service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        session,
        StreamableHttpServerConfig::default(),
    );

    let mcp_config = GovernorConfigBuilder::default()
        .per_second(2)
        .burst_size(10)
        .finish()
        .unwrap();
    let mcp_route = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(axum::Extension(HttpBaseUrl(base_url.clone())))
        .layer(axum::middleware::from_fn_with_state(
            jwt_manager.clone(),
            auth_middleware,
        ))
        .layer(GovernorLayer::new(mcp_config))
        .layer(DefaultBodyLimit::max(max_body_size))
        .layer(TimeoutLayer::with_status_code(
            hyper::StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ));

    // Health endpoint (no auth)
    let health_route = Router::new().route("/health", get(|| async { "ok" }));

    // Metrics endpoint (with optional auth)
    let metrics_route = metrics::create_metrics_router();

    let app = Router::new()
        .merge(auth_route)
        .merge(mcp_route)
        .merge(health_route)
        .merge(metrics_route);

    tracing::info!(
        %address,
        request_timeout_secs = request_timeout.as_secs(),
        idle_timeout_secs = idle_timeout.as_secs(),
        "starting HTTP server with JWT authentication"
    );

    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

async fn run_stdio_server(handler: IntervalsMcpHandler) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("starting STDIO MCP server...");

    use rmcp::serve_server;
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let _server = serve_server(handler, transport).await?;

    tracing::info!("STDIO service initialized");
    _server.waiting().await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logging setup
    let log_env = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let combined_filter = format!("{},rmcp=warn,serve_inner=warn", log_env);
    let env_filter = tracing_subscriber::EnvFilter::try_new(&combined_filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,rmcp=warn,serve_inner=warn"));

    tracing_subscriber::fmt()
        .compact()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(false)
        .with_env_filter(env_filter)
        .init();

    let version = env!("CARGO_PKG_VERSION");
    tracing::info!(version = %version, "intervals_icu_mcp starting");

    let transport_mode = std::env::var("MCP_TRANSPORT").unwrap_or_else(|_| "stdio".to_string());

    tracing::info!(%transport_mode, "using transport mode");

    match transport_mode.as_str() {
        "stdio" => {
            let handler = initialize_handler_single_user().await.map_err(|e| {
                tracing::error!("{}", e);
                e
            })?;
            run_stdio_server(handler).await?;
        }
        "http" => {
            let address: SocketAddr = std::env::var("MCP_HTTP_ADDRESS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 3000)));

            let max_body_size = std::env::var("MAX_HTTP_BODY_SIZE")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(4 * 1024 * 1024); // 4MB default

            run_http_server(address, max_body_size).await?;
        }
        other => {
            tracing::error!(mode = %other, "unknown transport mode; must be 'stdio' or 'http'");
            std::process::exit(1);
        }
    }

    Ok(())
}
