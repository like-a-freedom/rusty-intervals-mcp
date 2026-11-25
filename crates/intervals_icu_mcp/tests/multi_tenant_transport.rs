use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use intervals_icu_mcp::auth::{
    AppState, HttpBaseUrl, JwtManager, MasterKeyConfig, auth_middleware,
};
use intervals_icu_mcp::token_registry::TokenRegistry;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

fn basic(key: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("API_KEY:{key}")))
}

#[tokio::test]
async fn authenticated_dynamic_uses_current_tenant_and_rotated_key() {
    let upstream = MockServer::start().await;
    let wrong_upstream = MockServer::start().await;
    let spec = serde_json::json!({"openapi":"3.0.0","info":{"title":"test","version":"1"},"paths":{
        "/api/v1/athlete/{id}":{"get":{"operationId":"getAthlete","parameters":[{"name":"id","in":"path","required":true,"schema":{"type":"string"}}],"responses":{"200":{"description":"ok"}}}}
    }});
    Mock::given(method("GET"))
        .and(path("/api/v1/docs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(spec))
        .expect(1)
        .mount(&upstream)
        .await;
    for (athlete, key) in [
        ("i111111", "key-one"),
        ("i222222", "key-two"),
        ("i111111", "key-rotated"),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/athlete/{athlete}")))
            .and(header("authorization", basic(key)))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id":athlete,"name":athlete})),
            )
            .expect(1)
            .mount(&upstream)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/athlete/{athlete}/sport-settings")))
            .and(header("authorization", basic(key)))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .expect(1)
            .mount(&upstream)
            .await;
    }
    // This integration test runs in its own process and has a single test.
    unsafe {
        std::env::set_var("INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED", "1");
        std::env::set_var("INTERVALS_ICU_BASE_URL", wrong_upstream.uri());
        std::env::set_var("INTERVALS_ICU_ATHLETE_ID", "wrong-athlete");
        std::env::set_var("INTERVALS_ICU_API_KEY", "wrong-key");
    }
    let key = MasterKeyConfig::from_hex(&"11".repeat(64)).unwrap();
    let state = Arc::new(AppState {
        jwt_manager: Arc::new(JwtManager::from_master_key(&key)),
        jwt_ttl_seconds: 3600,
        base_url: upstream.uri(),
        tokens: TokenRegistry::load(None),
    });
    let first = state
        .issue_recorded_token("i111111", "key-one", 3600)
        .await
        .unwrap();
    let second = state
        .issue_recorded_token("i222222", "key-two", 3600)
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
        .layer(axum::Extension(HttpBaseUrl(upstream.uri())))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    let mut sessions = Vec::new();
    for token in [&first.token, &second.token] {
        let response = http.post(&url).bearer_auth(token).header("Accept", "application/json, text/event-stream")
            .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).send().await.unwrap();
        assert!(response.status().is_success());
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
            .bearer_auth(token)
            .header("Accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session)
            .json(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        sessions.push(session);
    }
    for (index, token) in [(0, &first.token), (1, &second.token), (0, &first.token)] {
        if index == 0
            && upstream
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|r| r.url.path() == "/api/v1/athlete/i111111")
        {
            let mut record = state.tokens.find(&first.jti).await.unwrap();
            record.encrypted_api_key =
                Some(state.jwt_manager.encrypt_credential("key-rotated").unwrap());
            state.tokens.record(record).await;
        }
        let response = http.post(&url).bearer_auth(token).header("Accept", "application/json, text/event-stream").header("mcp-session-id", &sessions[index])
            .json(&serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"manage_profile","arguments":{"action":"get","sections":[]}}})).send().await.unwrap();
        assert!(response.status().is_success());
        let body = response.text().await.unwrap();
        let paths: Vec<_> = upstream
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| request.url.path().to_owned())
            .collect();
        assert!(
            !body.contains("\"isError\":true") && !body.contains("\"error\":"),
            "dynamic authenticated call failed: {body}; upstream paths: {paths:?}"
        );
    }
    assert!(wrong_upstream.received_requests().await.unwrap().is_empty());
    server.abort();
}
