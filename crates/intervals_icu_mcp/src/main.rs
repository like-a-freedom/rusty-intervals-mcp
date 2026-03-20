use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use intervals_icu_mcp::IntervalsMcpHandler;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use secrecy::SecretString;

/// Initialize the MCP handler with common setup logic.
///
/// This function:
/// - Validates credentials from environment variables
/// - Creates the IntervalsClient
/// - Creates the IntervalsMcpHandler
/// - Preloads the dynamic OpenAPI registry (with 3s timeout)
/// - Logs the number of discovered tools
///
/// Returns the initialized handler on success, or exits the process on error.
async fn initialize_handler() -> IntervalsMcpHandler {
    let base = std::env::var("INTERVALS_ICU_BASE_URL")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());
    let athlete = std::env::var("INTERVALS_ICU_ATHLETE_ID").unwrap_or_else(|_| "".to_string());
    let api_key = std::env::var("INTERVALS_ICU_API_KEY").unwrap_or_else(|_| "".to_string());

    if api_key.trim().is_empty() {
        tracing::error!(
            "INTERVALS_ICU_API_KEY is not set. \
             Please set this environment variable to your Intervals.icu API key. \
             See https://intervals.icu/settings -> Developer section."
        );
        std::process::exit(1);
    }

    if athlete.trim().is_empty() {
        tracing::error!(
            "INTERVALS_ICU_ATHLETE_ID is not set. \
             Please set this environment variable to your athlete ID (format: i123456). \
             You can find your athlete ID in your Intervals.icu profile URL."
        );
        std::process::exit(1);
    }

    tracing::info!(
        athlete_id = %athlete,
        base_url = %base,
        "credentials validated"
    );

    let api_key = SecretString::new(api_key.into());
    let client =
        intervals_icu_client::http_client::ReqwestIntervalsClient::new(&base, athlete, api_key);
    let handler = IntervalsMcpHandler::new(Arc::new(client));

    // Preload dynamic registry with timeout
    let dynamic_tools = match tokio::time::timeout(
        Duration::from_secs(3),
        handler.preload_dynamic_registry(),
    )
    .await
    {
        Ok(count) => count,
        Err(_) => {
            tracing::warn!(
                "timed out preloading dynamic OpenAPI registry; continuing startup"
            );
            0
        }
    };

    tracing::info!(
        "discovered {} dynamic tools; advertising {} tools (8 intents + dynamic)",
        dynamic_tools,
        handler.tool_count()
    );

    handler
}

/// Run the MCP server over STDIO transport.
///
/// This mode is designed for local MCP clients such as VS Code Copilot,
/// Claude Desktop, or other IDE-based MCP hosts that communicate via
/// standard input/output streams.
async fn run_stdio_server(handler: IntervalsMcpHandler) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("starting STDIO MCP server...");

    use rmcp::serve_server;
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let _server = serve_server(handler, transport).await?;

    tracing::info!("STDIO service initialized");
    _server.waiting().await?;

    Ok(())
}

/// Run the MCP server over Streamable HTTP transport.
///
/// This mode is designed for remote MCP clients and supports multiple
/// concurrent connections. The server listens on the specified address
/// and serves the MCP protocol at the `/mcp` endpoint.
///
/// # Arguments
///
/// * `handler` - The MCP handler instance
/// * `address` - The socket address to bind to (e.g., "127.0.0.1:3000")
/// * `max_body_size` - Maximum allowed request body size in bytes
async fn run_http_server(
    handler: IntervalsMcpHandler,
    address: SocketAddr,
    max_body_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(%address, max_body_bytes = max_body_size, "starting HTTP server");

    // Create the StreamableHttpService
    let factory = move || -> Result<_, std::io::Error> { Ok(handler.clone()) };
    let session = Arc::new(LocalSessionManager::default());
    let config = StreamableHttpServerConfig::default();
    let mcp_service = StreamableHttpService::new(factory, session, config);

    // Build the Axum router with the MCP service mounted at /mcp
    let app = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(DefaultBodyLimit::max(max_body_size));

    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure logging from standard `RUST_LOG` environment variable.
    // See https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html
    let log_env = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

    // Append per-target overrides to keep rmcp internals quiet by default
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
    tracing::info!(
        version = %version,
        log_filter = %combined_filter,
        "intervals_icu_mcp starting"
    );
    tracing::debug!(version = %version, "debug logging enabled");
    tracing::trace!(version = %version, "trace logging initialized");
    tracing::warn!(version = %version, "server starting (warnings may appear during operation)");

    // Determine transport mode from environment variable.
    // Default to "stdio" for backward compatibility with local MCP clients.
    let transport_mode =
        std::env::var("MCP_TRANSPORT").unwrap_or_else(|_| "stdio".to_string());

    tracing::info!(%transport_mode, "using transport mode");

    // Initialize the common MCP handler
    let handler = initialize_handler().await;

    // Select and run the appropriate transport
    match transport_mode.as_str() {
        "stdio" => {
            run_stdio_server(handler).await?;
        }
        "http" => {
            // Read HTTP-specific configuration
            let address: SocketAddr = std::env::var("MCP_HTTP_ADDRESS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 3000)));

            let max_body_size = std::env::var("MAX_HTTP_BODY_SIZE")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(50 * 1024 * 1024);

            run_http_server(handler, address, max_body_size).await?;
        }
        other => {
            tracing::error!(
                mode = %other,
                "unknown transport mode; must be 'stdio' or 'http'"
            );
            std::process::exit(1);
        }
    }

    Ok(())
}
