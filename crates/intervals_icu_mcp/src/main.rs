use std::sync::Arc;
use std::time::Duration;

use intervals_icu_mcp::IntervalsMcpHandler;

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

    tracing::info!("intervals_icu_mcp: log filter: {}", combined_filter);

    // Fail-fast validation: ensure required credentials are set before proceeding
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
        "intervals_icu_mcp: credentials validated"
    );

    let api_key = secrecy::SecretString::new(api_key.into());
    let client =
        intervals_icu_client::http_client::ReqwestIntervalsClient::new(&base, athlete, api_key);
    let handler = IntervalsMcpHandler::new(Arc::new(client));
    let dynamic_tools = match tokio::time::timeout(
        Duration::from_secs(3),
        handler.preload_dynamic_registry(),
    )
    .await
    {
        Ok(count) => count,
        Err(_) => {
            tracing::warn!(
                "intervals_icu_mcp: timed out preloading dynamic OpenAPI registry; continuing startup"
            );
            0
        }
    };

    tracing::info!(
        "intervals_icu_mcp: discovered {} dynamic tools; advertising {} tools and {} prompts",
        dynamic_tools,
        handler.tool_count(),
        handler.prompt_count()
    );

    // Start RMCP server over stdio transport so it's immediately usable with MCP clients
    tracing::info!("intervals_icu_mcp: starting stdio MCP server...");

    use rmcp::serve_server;
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let _server = serve_server(handler, transport).await?;

    tracing::info!("intervals_icu_mcp: service initialized as server");

    _server.waiting().await?;

    Ok(())
}
