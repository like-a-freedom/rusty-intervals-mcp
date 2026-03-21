use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};
use intervals_icu_mcp::IntervalsMcpHandler;
use intervals_icu_mcp::auth::{AppState, HttpBaseUrl, JwtManager, auth_endpoint, auth_middleware};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use secrecy::SecretString;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};

/// STDIO mode: инициализация с credentials из env
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

/// HTTP mode: multi-tenant с JWT authentication
async fn run_http_server(
    address: SocketAddr,
    max_body_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // JWT_SECRET обязателен для HTTP mode
    let jwt_secret = std::env::var("JWT_SECRET")
        .map_err(|_| "JWT_SECRET environment variable is required for HTTP mode")?;

    // JWT_ENCRYPTION_KEY для шифрования api_key (32 байта = 64 hex chars)
    let encryption_key_hex = std::env::var("JWT_ENCRYPTION_KEY")
        .map_err(|_| "JWT_ENCRYPTION_KEY environment variable is required for HTTP mode")?;
    let encryption_key = hex::decode(&encryption_key_hex)
        .map_err(|_| "JWT_ENCRYPTION_KEY must be a valid hex string")?;

    if encryption_key.len() != 32 {
        return Err("JWT_ENCRYPTION_KEY must be exactly 32 bytes (64 hex chars)".into());
    }

    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&encryption_key);

    let jwt_manager = Arc::new(JwtManager::new(jwt_secret.as_bytes(), key_array));

    // JWT TTL: настраиваемый, максимум 7 дней
    let jwt_ttl_seconds = std::env::var("JWT_TTL_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(86400)
        .min(7 * 24 * 3600);

    let base_url = std::env::var("INTERVALS_ICU_BASE_URL")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());

    let app_state = Arc::new(AppState {
        jwt_manager: jwt_manager.clone(),
        jwt_ttl_seconds,
        base_url: base_url.clone(),
    });

    // Handler для multi-tenant mode (без credentials, они извлекаются из JWT)
    let handler = IntervalsMcpHandler::new_multi_tenant();

    // Auth endpoint с rate limiting (защита от brute force)
    let auth_config = GovernorConfigBuilder::default()
        .per_second(1)
        .burst_size(3)
        .finish()
        .unwrap();
    let auth_route = Router::new()
        .route("/auth", post(auth_endpoint))
        .layer(GovernorLayer::new(auth_config))
        .with_state(app_state.clone());

    // MCP service с auth middleware и rate limiting
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
        .layer(DefaultBodyLimit::max(max_body_size));

    // Health endpoint (без auth)
    let health_route = Router::new().route("/health", get(|| async { "ok" }));

    let app = Router::new()
        .merge(auth_route)
        .merge(mcp_route)
        .merge(health_route);

    tracing::info!(%address, "starting HTTP server with JWT authentication");

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
