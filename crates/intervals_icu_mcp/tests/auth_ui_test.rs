use axum::{
    Router,
    routing::{get, post},
};
use intervals_icu_mcp::auth::{self, AppState};
use intervals_icu_mcp::auth_ui::{self, UiState};
use std::sync::Arc;

fn test_ui_state() -> UiState {
    use std::collections::HashSet;
    let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
    let jwt_manager = Arc::new(auth::JwtManager::new(secret, [0u8; 32]));
    let revoked_jtis = Arc::new(tokio::sync::RwLock::new(HashSet::new()));
    let app_state = Arc::new(AppState {
        jwt_manager,
        jwt_ttl_seconds: 3600,
        base_url: "https://intervals.icu".to_string(),
        revoked_jtis: revoked_jtis.clone(),
    });

    UiState::new(app_state, revoked_jtis, None, false, String::new())
}

fn test_ui_state_with_base(base: &str) -> UiState {
    use std::collections::HashSet;
    let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
    let jwt_manager = Arc::new(auth::JwtManager::new(secret, [0u8; 32]));
    let revoked_jtis = Arc::new(tokio::sync::RwLock::new(HashSet::new()));
    let app_state = Arc::new(AppState {
        jwt_manager,
        jwt_ttl_seconds: 3600,
        base_url: "https://intervals.icu".to_string(),
        revoked_jtis: revoked_jtis.clone(),
    });

    UiState::new(app_state, revoked_jtis, None, false, base.to_string())
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
        body.contains("Generate Token"),
        "page should contain 'Generate Token', got: {}..",
        &body[..200]
    );
    assert!(
        body.contains("Home"),
        "page should contain 'Home', got: {}..",
        &body[..200]
    );
    assert!(
        body.contains(r#"href="/ui/static/css""#),
        "root deployment must keep absolute root CSS link, got: {}..",
        &body[..300]
    );
    assert!(
        body.contains(r#"action="/ui/token""#),
        "root deployment must keep root form action"
    );
}

/// Extract the inline `<script>` body from a rendered page.
///
/// Returns `None` when the page has no inline script.
fn inline_script(body: &str) -> Option<&str> {
    let start = body.find("<script>")? + "<script>".len();
    let end = body[start..].find("</script>")? + start;
    Some(&body[start..end])
}

/// The flash banner's cleanup script strips `?success=` / `?error=` from the URL
/// with `history.replaceState`. If the browser cannot parse that script, the
/// query string survives a refresh and the "Token revoked" banner reappears on
/// every reload.
///
/// maud HTML-escapes interpolated text, which turns the script's quotes into
/// `&quot;`. Browsers do not decode entities inside `<script>`, so the escaped
/// form is a syntax error and the whole block never runs.
#[tokio::test]
async fn test_ui_inline_script_is_not_html_escaped() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui", get(auth_ui::ui_home))
        .route("/ui/tokens", get(auth_ui::ui_list_tokens))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    for path in ["/ui", "/ui/tokens"] {
        let body = reqwest::get(&format!("http://{addr}{path}"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        let script = inline_script(&body)
            .unwrap_or_else(|| panic!("{path}: page must ship an inline script"));

        assert!(
            !script.contains("&quot;"),
            "{path}: inline script was HTML-escaped; the browser cannot parse it, \
             so the flash banner query string survives a refresh"
        );
        assert!(
            !script.contains("&amp;") && !script.contains("&lt;") && !script.contains("&gt;"),
            "{path}: inline script contains escaped entities and will not parse"
        );
        assert!(
            script.contains("replaceState"),
            "{path}: script must strip the flash query string on load"
        );
    }
}

/// The same escaping hazard applies to the inline `<style>` block: a stray `<`
/// or `&` in the page-local CSS would break the whole block.
#[tokio::test]
async fn test_ui_inline_style_is_not_html_escaped() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui", get(auth_ui::ui_home))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let body = reqwest::get(&format!("http://{addr}/ui"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let start = body.find("<style>").expect("page must ship inline styles") + 7;
    let end = body[start..].find("</style>").unwrap() + start;
    let style = &body[start..end];

    assert!(
        !style.contains("&quot;") && !style.contains("&amp;"),
        "inline <style> was HTML-escaped and will not parse"
    );
    assert!(
        style.contains(".flash-banner"),
        "inline <style> must keep the flash banner rules"
    );
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
    assert_eq!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/css; charset=utf-8"
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
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        "/ui?error=Invalid+session+%28CSRF%29"
    );
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

#[tokio::test]
async fn test_ui_home_links_carry_base_path() {
    let state = test_ui_state_with_base("/intervals");
    let inner = Router::new()
        .route("/ui", get(auth_ui::ui_home))
        .route("/ui/static/css", get(auth_ui::serve_css))
        .with_state(state);
    let app = axum::Router::new().nest("/intervals", inner);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let resp = reqwest::get(&format!("http://{addr}/intervals/ui"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .expect("ui_home sets a session cookie")
        .to_str()
        .unwrap()
        .to_string();
    let body = resp.text().await.unwrap();
    assert!(
        body.contains(r#"href="/intervals/ui/static/css""#),
        "css link: {}",
        &body[..400]
    );
    assert!(
        body.contains(r#"action="/intervals/ui/token""#),
        "form action: {}",
        &body[..400]
    );
    assert!(
        body.contains(r#"href="/intervals/ui/tokens""#),
        "nav link: {}",
        &body[..400]
    );

    assert!(
        set_cookie.contains("Path=/intervals/ui"),
        "cookie={set_cookie}"
    );
}

#[tokio::test]
async fn test_ui_create_token_redirect_carries_base_path() {
    let state = test_ui_state_with_base("/intervals");
    let inner = Router::new()
        .route("/ui/token", post(auth_ui::ui_create_token))
        .with_state(state);
    let app = axum::Router::new().nest("/intervals", inner);

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
        .post(format!("http://{addr}/intervals/ui/token"))
        .form(&[("athlete_id", "i123456"), ("api_key", "secret")])
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection());
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        "/intervals/ui?error=Invalid+session+%28CSRF%29"
    );
}
