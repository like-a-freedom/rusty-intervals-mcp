use std::sync::Arc;
use std::time::Duration;

use axum::{body::Body, handler::Handler, http::Request};
use intervals_icu_mcp::metrics::metrics_middleware;
use metrics_exporter_prometheus::PrometheusBuilder;
use tokio::sync::Notify;

fn active_requests(output: &str) -> f64 {
    output
        .lines()
        .find_map(|line| line.strip_prefix("intervals_icu_mcp_active_requests "))
        .expect("active requests gauge missing")
        .parse()
        .unwrap()
}

#[tokio::test]
async fn cancelled_request_decrements_active_gauge() {
    let handle = PrometheusBuilder::new().install_recorder().unwrap();
    let entered = Arc::new(Notify::new());
    let handler = {
        let entered = Arc::clone(&entered);
        move || async move {
            entered.notify_one();
            std::future::pending::<()>().await;
            "ok"
        }
    };
    let handler = handler.layer(axum::middleware::from_fn(metrics_middleware));
    let request = Request::builder()
        .uri("/blocked")
        .body(Body::empty())
        .unwrap();
    let task = tokio::spawn(handler.call(request, ()));

    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .expect("handler not entered");
    assert_eq!(active_requests(&handle.render()), 1.0);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(active_requests(&handle.render()), 0.0);

    let handler = (|| async { "ok" }).layer(axum::middleware::from_fn(metrics_middleware));
    let request = Request::builder()
        .uri("/complete")
        .body(Body::empty())
        .unwrap();
    let response = handler.call(request, ()).await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let output = handle.render();
    assert_eq!(active_requests(&output), 0.0);
    assert!(output.contains("intervals_icu_mcp_http_requests_total"));
    assert!(output.contains("intervals_icu_mcp_http_request_duration_seconds"));
}
