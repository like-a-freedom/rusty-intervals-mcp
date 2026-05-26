use axum::{
    Router,
    routing::{get, post},
};
use intervals_icu_mcp::auth::{self, AppState};
use intervals_icu_mcp::auth_ui::{self, UiState};
use std::sync::Arc;
use tokio::sync::RwLock;

fn test_ui_state() -> UiState {
    let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
    let jwt_manager = Arc::new(auth::JwtManager::new(secret, [0u8; 32]));
    let app_state = Arc::new(AppState {
        jwt_manager,
        jwt_ttl_seconds: 3600,
        base_url: "https://intervals.icu".to_string(),
    });

    UiState {
        app_state,
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        tokens: Arc::new(RwLock::new(Vec::new())),
    }
}

#[tokio::test]
async fn test_ui_home_returns_html() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui", get(auth_ui::ui_home))
        .route("/ui/static/css", get(auth_ui::serve_css))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let resp = reqwest::get(&format!("http://{}/ui", addr)).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(
        body.contains("Generate Token") || body.contains("Home"),
        "page should contain expected headings, got: {}..",
        &body[..200]
    );
    assert!(body.contains("stylesheet"), "page should link a stylesheet");
}

#[tokio::test]
async fn test_ui_css_served() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui/static/css", get(auth_ui::serve_css))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let resp = reqwest::get(&format!("http://{}/ui/static/css", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("text/css")
    );
}

#[tokio::test]
async fn test_ui_create_token_no_csrf_redirects() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui/token", post(auth_ui::ui_create_token))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .post(format!("http://{}/ui/token", addr))
        .form(&[("athlete_id", "i123456"), ("api_key", "secret")])
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection());
}

#[tokio::test]
async fn test_ui_tokens_page_renders() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui/tokens", get(auth_ui::ui_list_tokens))
        .route("/ui/static/css", get(auth_ui::serve_css))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let resp = reqwest::get(&format!("http://{}/ui/tokens", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert!(
        body.contains("Active Tokens") || body.contains("No tokens"),
        "page should contain expected content"
    );
}
