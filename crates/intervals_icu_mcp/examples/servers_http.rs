use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::net::SocketAddr;
use tokio::signal;

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn metrics(handle: PrometheusHandle) -> impl IntoResponse {
    let body = handle.render();
    ([("content-type", "text/plain; version=0.0.4")], body)
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Configure logging from standard `RUST_LOG` environment variable.
    let log_env = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let env_filter = tracing_subscriber::EnvFilter::try_new(&log_env)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
    tracing::info!("intervals_icu_mcp:example:http: log filter = {}", log_env);

    // Install prometheus recorder
    let builder = PrometheusBuilder::new();
    let handle = builder.install_recorder()?;

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(move || metrics(handle.clone())));

    let addr: SocketAddr = std::env::var("ADDRESS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "127.0.0.1:3000".parse().unwrap());
    tracing::info!(%addr, "starting HTTP example server");
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to address {addr}: {e}");
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
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
    Ok(())
}
