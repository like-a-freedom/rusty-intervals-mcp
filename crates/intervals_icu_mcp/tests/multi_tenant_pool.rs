use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use intervals_icu_mcp::{
    auth::{AppState, HttpBaseUrl, JwtManager, MasterKeyConfig, auth_middleware},
    token_registry::TokenRegistry,
};
use std::{
    collections::HashSet,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Seen {
    connections: HashSet<SocketAddr>,
    authorizations: Vec<String>,
}

async fn upstream(
    State(seen): State<Arc<Mutex<Seen>>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut seen = seen.lock().unwrap();
    seen.connections.insert(peer);
    seen.authorizations.push(
        headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned(),
    );
    if uri.path().ends_with("/profile") {
        (
            StatusCode::OK,
            Json(serde_json::json!({"athlete":{"id":"i111111","name":"Test"}})),
        )
    } else {
        (StatusCode::OK, Json(serde_json::json!([])))
    }
}

#[tokio::test]
async fn authenticated_typed_requests_reuse_connections_with_fresh_keys() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let upstream_app = axum::Router::new()
        .fallback(upstream)
        .with_state(seen.clone());
    let upstream_task = tokio::spawn(async move {
        axum::serve(
            listener,
            upstream_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let key = MasterKeyConfig::from_hex(&"11".repeat(64)).unwrap();
    let state = Arc::new(AppState {
        jwt_manager: Arc::new(JwtManager::from_master_key(&key)),
        jwt_ttl_seconds: 3600,
        base_url: base.clone(),
        tokens: TokenRegistry::in_memory(),
    });
    let first = state
        .issue_recorded_token("i111111", "pool-first", 3600)
        .await
        .unwrap();
    let second = state
        .issue_recorded_token("i111111", "pool-second", 3600)
        .await
        .unwrap();
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new_multi_tenant().unwrap();
    let sessions = Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || -> Result<_, std::io::Error> { Ok(handler.clone()) },
        sessions,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );
    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::Extension(HttpBaseUrl(base)))
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    let response = http.post(&url).bearer_auth(&first.token).header("Accept", "application/json, text/event-stream").json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).send().await.unwrap();
    let session = response
        .headers()
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let _ = response.text().await.unwrap();
    let response = http
        .post(&url)
        .bearer_auth(&first.token)
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", &session)
        .json(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    for token in [&first.token, &second.token] {
        let response = http.post(&url).bearer_auth(token).header("Accept", "application/json, text/event-stream").header("mcp-session-id", &session).json(&serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"manage_profile","arguments":{"action":"get","sections":[]}}})).send().await.unwrap();
        let body = response.text().await.unwrap();
        assert!(!body.contains("\"error\":"), "{body}");
    }
    let seen = seen.lock().unwrap();
    let auth_first = format!("Basic {}", STANDARD.encode("API_KEY:pool-first"));
    let auth_second = format!("Basic {}", STANDARD.encode("API_KEY:pool-second"));
    assert_eq!(
        seen.authorizations,
        vec![
            auth_first.clone(),
            auth_first,
            auth_second.clone(),
            auth_second
        ]
    );
    assert_eq!(
        seen.connections.len(),
        1,
        "sequential calls should reuse the upstream TCP connection"
    );
    server.abort();
    upstream_task.abort();
}
