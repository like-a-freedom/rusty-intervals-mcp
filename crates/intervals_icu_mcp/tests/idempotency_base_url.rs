use std::sync::Arc;

use intervals_icu_mcp::auth::{
    AppState, HttpBaseUrl, JwtManager, MasterKeyConfig, auth_middleware,
};
use intervals_icu_mcp::token_registry::TokenRegistry;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn replay_is_scoped_to_upstream() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    for upstream in [&first, &second] {
        Mock::given(method("POST"))
            .and(path("/api/v1/athlete/i123/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "1", "name": "Run", "start_date_local": "2026-01-01T00:00:00", "category": "WORKOUT"
            })))
            .mount(upstream).await;
    }
    let key = MasterKeyConfig::from_hex(&"11".repeat(64)).unwrap();
    let state = Arc::new(AppState {
        jwt_manager: Arc::new(JwtManager::from_master_key(&key)),
        jwt_ttl_seconds: 3600,
        base_url: first.uri(),
        tokens: TokenRegistry::in_memory(),
    });
    let token = state
        .issue_recorded_token("i123", "first-key", 3600)
        .await
        .unwrap();
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new_multi_tenant().unwrap();
    let mut urls = Vec::new();
    let mut servers = Vec::new();
    for upstream in [&first, &second] {
        let handler = handler.clone();
        let service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
            move || -> Result<_, std::io::Error> { Ok(handler.clone()) },
            Arc::new(rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default()),
            rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
        );
        let app = axum::Router::new()
            .nest_service("/mcp", service)
            .layer(axum::Extension(HttpBaseUrl(upstream.uri())))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        urls.push(format!("http://{}/mcp", listener.local_addr().unwrap()));
        servers.push(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap()
        }));
    }
    let http = reqwest::Client::new();
    let mut sessions = Vec::new();
    for url in &urls {
        let response = http.post(url).bearer_auth(&token.token).header("Accept", "application/json, text/event-stream")
            .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).send().await.unwrap();
        let session = response
            .headers()
            .get("mcp-session-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let _ = response.text().await.unwrap();
        http.post(url)
            .bearer_auth(&token.token)
            .header("Accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session)
            .json(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await
            .unwrap();
        sessions.push(session);
    }
    for index in [0, 1, 0] {
        let response = http.post(&urls[index]).bearer_auth(&token.token).header("Accept", "application/json, text/event-stream")
            .header("mcp-session-id", &sessions[index])
            .json(&serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"modify_training","arguments":{"action":"create","new_date":"2026-01-01","new_name":"Run","idempotency_token":"same-token"}}})).send().await.unwrap();
        let body = response.text().await.unwrap();
        assert!(
            !body.contains("\"isError\":true") && !body.contains("\"error\":"),
            "{body}"
        );
    }
    assert_eq!(first.received_requests().await.unwrap().len(), 1);
    assert_eq!(second.received_requests().await.unwrap().len(), 1);
    for server in servers {
        server.abort();
    }
}
