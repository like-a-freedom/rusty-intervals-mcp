use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn production_router_records_http_metrics() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_intervals_icu_mcp"))
            .env("MCP_TRANSPORT", "http")
            .env("MCP_HTTP_ADDRESS", address.to_string())
            .env("JWT_MASTER_KEY", "00".repeat(64))
            .env("MCP_PUBLIC_BASE_PATH", "/test")
            .env_remove("PROMETHEUS_METRICS_TOKEN")
            .env_remove("MCP_TOKEN_REGISTRY_PATH")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::new();
    let base = format!("http://{address}/test");
    let mut ready = false;
    for _ in 0..100 {
        if let Ok(response) = client.get(format!("{base}/health")).send().await {
            assert!(response.status().is_success());
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ready, "production server did not start");

    let output = client
        .get(format!("{base}/metrics"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    let health_counter = output.lines().find(|line| {
        line.starts_with("intervals_icu_mcp_http_requests_total{")
            && line.contains("method=\"GET\"")
            && line.contains("path=\"/test/health\"")
            && line.contains("status=\"200\"")
    });
    assert!(
        health_counter.is_some(),
        "missing health request counter: {output}"
    );
    assert!(output.contains("intervals_icu_mcp_http_request_duration_seconds"));
    assert!(output.contains("intervals_icu_mcp_active_requests"));
}
