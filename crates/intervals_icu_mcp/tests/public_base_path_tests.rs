use axum::{Router, routing::get};
use intervals_icu_mcp::{apply_public_base_path, normalize_base_path};

fn no_redirect_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

async fn spawn(app: Router) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

#[test]
fn normalize_base_path_cases_integration() {
    assert_eq!(normalize_base_path(""), "");
    assert_eq!(normalize_base_path("/"), "");
    assert_eq!(normalize_base_path("intervals"), "/intervals");
    assert_eq!(normalize_base_path("/intervals/"), "/intervals");
    assert_eq!(normalize_base_path("//intervals//"), "/intervals");
}

#[tokio::test]
async fn root_deployment_unchanged_without_base_path() {
    let app = Router::new().route("/health", get(|| async { "ok" }));
    let app = apply_public_base_path(app, "");
    let addr = spawn(app).await;

    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn prefix_pass_through_serves_nested_routes() {
    // Mirrors production: inner router owns `/` (redirect) and app routes.
    let inner = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(
            "/",
            get(|| async { axum::response::Redirect::to("/intervals/ui") }),
        );
    let app = apply_public_base_path(inner, "/intervals");
    let addr = spawn(app).await;

    let resp = reqwest::get(format!("http://{addr}/intervals/health"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn prefix_root_redirects_to_ui_without_trailing_slash() {
    let inner = Router::new().route(
        "/",
        get(|| async { axum::response::Redirect::to("/intervals/ui") }),
    );
    let app = apply_public_base_path(inner, "/intervals");
    let addr = spawn(app).await;

    let resp = no_redirect_client()
        .get(format!("http://{addr}/intervals"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection(), "status={}", resp.status());
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        "/intervals/ui"
    );
}

#[tokio::test]
async fn prefix_root_redirects_to_ui_with_trailing_slash() {
    // axum nest("/x", …) matches "/x" but NOT "/x/"; the outer fallback must
    // redirect the trailing-slash form (browsers/proxies often hit it).
    let inner = Router::new().route(
        "/",
        get(|| async { axum::response::Redirect::to("/intervals/ui") }),
    );
    let app = apply_public_base_path(inner, "/intervals");
    let addr = spawn(app).await;

    let resp = no_redirect_client()
        .get(format!("http://{addr}/intervals/"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection(), "status={}", resp.status());
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        "/intervals/ui"
    );
}

#[tokio::test]
async fn root_paths_outside_prefix_return_404() {
    let inner = Router::new().route("/health", get(|| async { "ok" }));
    let app = apply_public_base_path(inner, "/intervals");
    let addr = spawn(app).await;

    // Domain root is freed for other services when a prefix is configured.
    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(resp.status(), 404);
    let resp = reqwest::get(format!("http://{addr}/")).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn unknown_paths_under_prefix_return_404() {
    let inner = Router::new().route("/health", get(|| async { "ok" }));
    let app = apply_public_base_path(inner, "/intervals");
    let addr = spawn(app).await;

    let resp = reqwest::get(format!("http://{addr}/intervals/nope"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
