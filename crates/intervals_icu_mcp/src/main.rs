use std::sync::Arc;

use intervals_icu_mcp::IntervalsMcpHandler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure logging from env var `INTERVALS_ICU_LOG_LEVEL` (or fallback to `RUST_LOG`, default `info`).
    let log_env = std::env::var("INTERVALS_ICU_LOG_LEVEL")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "info".to_string());

    // Append per-target overrides to keep rmcp internals quiet by default
    let combined_filter = format!("{},rmcp=warn,serve_inner=warn", log_env);
    let env_filter = tracing_subscriber::EnvFilter::try_new(combined_filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,rmcp=warn,serve_inner=warn"));
    tracing_subscriber::fmt()
        .compact()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(false)
        .with_env_filter(env_filter)
        .init();
    tracing::info!("intervals_icu_mcp: log filter: {}", log_env);

    let base = std::env::var("INTERVALS_ICU_BASE_URL")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());
    let athlete = std::env::var("INTERVALS_ICU_ATHLETE_ID").unwrap_or_else(|_| "".to_string());
    let api_key = std::env::var("INTERVALS_ICU_API_KEY").unwrap_or_else(|_| "".to_string());

    let api_key = secrecy::SecretString::new(api_key.into());
    let client =
        intervals_icu_client::http_client::ReqwestIntervalsClient::new(&base, athlete, api_key);
    let _handler = IntervalsMcpHandler::new(Arc::new(client));

    tracing::info!(
        "intervals_icu_mcp: registered {} tools and {} prompts",
        _handler.tool_count(),
        _handler.prompt_count()
    );

    // Start RMCP server over stdio transport so it's immediately usable with MCP clients
    tracing::info!("intervals_icu_mcp: starting stdio MCP server...");

    use rmcp::serve_server;
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let _server = serve_server(_handler, transport).await?;

    tracing::info!("intervals_icu_mcp: service initialized as server");

    _server.waiting().await?;

    Ok(())
}
