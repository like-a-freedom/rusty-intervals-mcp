use axum::{
    Router,
    http::HeaderMap,
    routing::{get, post},
};
use intervals_icu_mcp::auth::{self, AppState};
use intervals_icu_mcp::auth_ui::{self, UiState};
use intervals_icu_mcp::token_registry::TokenRegistry;
use std::sync::Arc;

fn test_ui_state() -> UiState {
    let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
    let jwt_manager = Arc::new(auth::JwtManager::new(secret, [0u8; 32]));
    let app_state = Arc::new(AppState {
        jwt_manager,
        jwt_ttl_seconds: 3600,
        base_url: "https://intervals.icu".to_string(),
        tokens: TokenRegistry::load(None),
    });

    UiState::new(app_state, false, String::new())
}

fn headers_with_cookie(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(axum::http::header::COOKIE, value.parse().unwrap());
    headers
}

fn test_ui_state_with_base(base: &str) -> UiState {
    let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
    let jwt_manager = Arc::new(auth::JwtManager::new(secret, [0u8; 32]));
    let app_state = Arc::new(AppState {
        jwt_manager,
        jwt_ttl_seconds: 3600,
        base_url: "https://intervals.icu".to_string(),
        tokens: TokenRegistry::load(None),
    });

    UiState::new(app_state, false, base.to_string())
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
            !script.contains("replaceState"),
            "{path}: the flash lives in the session, so the script must not \
             need to scrub a query string"
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

/// Every visible form control needs an accessible name (WCAG 4.1.2).
///
/// `field::render` uses `Props.id` only for the label's `for`. The nested input
/// has to be given the same id explicitly or the label resolves to nothing and
/// the control is announced by its placeholder — or by nothing at all.
#[tokio::test]
async fn test_ui_form_controls_have_accessible_names() {
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

    for field in ["athlete_id", "api_key", "ttl_days"] {
        assert!(
            body.contains(&format!("for=\"{field}\"")),
            "{field}: a <label for> must exist"
        );
        assert!(
            body.contains(&format!("id=\"{field}\"")),
            "{field}: the control must carry the id its label points at, or the \
             label is inert and the control has no accessible name"
        );
        assert!(
            !body.contains("id=\"\""),
            "{field}: an empty id attribute must never be emitted"
        );
    }
}

/// Document landmarks and a top-level heading must exist, so assistive tech can
/// navigate the page and its heading outline does not start at <h3>.
#[tokio::test]
async fn test_ui_pages_expose_landmarks_and_h1() {
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

        for landmark in ["<main", "<nav", "<header", "<h1"] {
            assert!(
                body.contains(landmark),
                "{path}: missing {landmark} — landmark/heading structure is required \
                 for screen-reader navigation"
            );
        }
        assert!(
            body.contains(r#"aria-current="page""#),
            "{path}: the current nav item must be marked with aria-current"
        );
    }
}

/// Feedback must be announced, and the two flash states must be distinguishable.
#[tokio::test]
async fn test_ui_flash_banners_are_announced() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui/tokens", get(auth_ui::ui_list_tokens))
        .route("/ui/revoke/{jti}", post(auth_ui::ui_revoke_token))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Drive a real error through the handler, then read it back from the page.
    // Cookie handling is done by hand so the test does not depend on the
    // `cookies` feature of reqwest being enabled.
    let landing = reqwest::get(format!("http://{addr}/ui/tokens"))
        .await
        .unwrap();
    let session_cookie = landing
        .headers()
        .get("set-cookie")
        .expect("session cookie")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let _ = landing.text().await.unwrap();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .post(format!("http://{addr}/ui/revoke/does-not-exist"))
        .header("cookie", &session_cookie)
        .form(&[("_csrf", "stale-token")])
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection());

    let page = reqwest::Client::new()
        .get(format!("http://{addr}/ui/tokens"))
        .header("cookie", &session_cookie)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains(r#"role="alert""#),
        "an error must be a live alert region"
    );
    assert!(
        page.contains("session has expired"),
        "the flash must render on the redirected page: {page}"
    );
}

/// An `<a>` must not wrap a `<button>`: nested interactive elements are invalid
/// and make the keyboard/focus behaviour ambiguous.
#[tokio::test]
async fn test_ui_has_no_interactive_elements_nested_in_links() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui/tokens", get(auth_ui::ui_list_tokens))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let body = reqwest::get(&format!("http://{addr}/ui/tokens"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(
        !body.contains("<a><button") && !body.contains("<a href=\"/ui\"><button"),
        "a <button> must not be nested inside an <a>; style the link as a button"
    );
}

/// User-facing errors must be plain language.
///
/// The old copy leaked protocol jargon ("Invalid session (CSRF)") and, for
/// token failures, interpolated the raw internal error straight into the query
/// string — which then became page text, browser history, and an outbound
/// Referer header.
#[test]
fn test_ui_error_copy_is_human_readable() {
    let src = include_str!("../src/auth_ui.rs");

    // These are the strings the redirect URLs carried before the copy pass.
    // Assert on the exact encoded forms so the check cannot silently pass.
    for jargon in [
        "Invalid+session+%28CSRF%29",
        "Invalid+CSRF",
        "Invalid+credentials",
        "Client+init+failed",
        "Missing+credentials",
    ] {
        assert!(
            !src.contains(jargon),
            "raw technical copy {jargon:?} must not reach the user"
        );
    }

    assert!(
        !src.contains("Token+generation+failed"),
        "the internal error must not be interpolated into the URL"
    );
    assert!(
        src.contains(r#"tracing::warn!(%e, "token generation failed in UI")"#),
        "the internal error belongs in the log, not the page"
    );

    // Every user-facing error must read as a sentence, not a slug.
    for copy in [
        "Your session has expired. Reload the page and try again.",
        "Enter both your athlete ID and API key.",
        "Intervals.icu rejected your athlete ID or API key.",
        "Token revoked. Clients using it will stop working.",
    ] {
        assert!(src.contains(copy), "expected human-readable copy: {copy:?}");
    }

    // Messages must live in Flash values, never in a redirect URL.
    assert!(
        !src.contains("error=Your") && !src.contains("success=Token"),
        "message text must not be placed in a URL"
    );
}

/// The top-bar nav follows the researched pattern: a real list of links whose
/// active state is driven solely by `aria-current`.
#[tokio::test]
async fn test_ui_nav_uses_aria_current_as_single_source_of_truth() {
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

        assert!(
            body.contains(r#"<ul class="nav-list">"#),
            "{path}: nav must be a real list so screen readers report item count"
        );
        assert!(
            body.contains(r#"aria-current="page""#),
            "{path}: the active nav item must carry aria-current"
        );
        // Count only in body markup: the attribute also appears in the inline
        // stylesheet's attribute selectors, which live in <head>.
        let markup = body.split("</head>").nth(1).unwrap_or(&body);
        assert_eq!(
            markup.matches(r#"aria-current="page""#).count(),
            1,
            "{path}: exactly one item may be marked current"
        );
        // A parallel `.active` class is a second source of truth that can drift
        // from what assistive tech is told. Style must read [aria-current] only.
        assert!(
            !body.contains(r#"class="active"#),
            "{path}: style the active state off [aria-current], not a class"
        );
        // A tablist would misrepresent page navigation as in-page panels.
        assert!(
            !body.contains(r#"role="tablist""#) && !body.contains(r#"role="tab""#),
            "{path}: these are links to pages, not tabs over panels"
        );
    }
}

/// The server-side session lifetime must slide with the cookie.
///
/// The cookie is re-issued with a fresh `Max-Age` on every response. If the
/// server measured the session from its creation instead, an operator who used
/// the UI for over an hour would be logged out mid-session — and their next
/// action would fail with "your session has expired".
#[tokio::test]
async fn test_ui_session_lifetime_slides_with_cookie() {
    let state = test_ui_state();
    let (session_id, csrf_before) = state.session_for_test(&HeaderMap::new()).await;

    let before = state.session_created(&session_id).await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    // Reuse the same session id, exactly as a returning browser would.
    let cookie = format!("mcp_session={session_id}");
    let (session_id_again, csrf_after) =
        state.session_for_test(&headers_with_cookie(&cookie)).await;

    assert_eq!(
        session_id, session_id_again,
        "an existing cookie must be reused, not replaced"
    );
    assert_eq!(
        csrf_before, csrf_after,
        "reusing a session must keep its CSRF token, or the next POST fails"
    );

    let after = state.session_created(&session_id).await;
    assert!(
        after > before,
        "session activity must slide forward on reuse; otherwise an actively-used \
         session is evicted after a fixed hour while the cookie still looks valid"
    );
}

/// Flash messages must not travel in the URL, and must be consumed exactly once.
///
/// Error text in a query string is copy-pasteable, lands in browser history, and
/// is forwarded in any outbound `Referer`. Worse, it replayed on every refresh.
#[tokio::test]
async fn test_ui_flash_is_session_scoped_and_one_shot() {
    let state = test_ui_state();
    let (session_id, _) = state.session_for_test(&HeaderMap::new()).await;

    state
        .set_flash(
            &session_id,
            auth_ui::Flash {
                kind: auth_ui::FlashKind::Success,
                message: "Token revoked.".into(),
            },
        )
        .await;

    let first = state.take_flash(&session_id).await;
    assert_eq!(first.map(|f| f.message), Some("Token revoked.".to_string()));

    let second = state.take_flash(&session_id).await;
    assert!(
        second.is_none(),
        "a flash must be consumed on read so a refresh renders a clean page"
    );
}

/// No handler may redirect to a URL carrying user-facing message text.
#[tokio::test]
async fn test_ui_redirects_never_carry_message_text() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui/token", post(auth_ui::ui_create_token))
        .route("/ui/revoke/{jti}", post(auth_ui::ui_revoke_token))
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

    // Missing CSRF -> error path.
    let resp = client
        .post(format!("http://{addr}/ui/token"))
        .form(&[("athlete_id", "i1"), ("api_key", "k")])
        .send()
        .await
        .unwrap();
    let location = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(location, "/ui");
    assert!(
        !location.contains("error=") && !location.contains("success="),
        "redirect must not carry message text: {location}"
    );

    // Revoke -> success path.
    let resp = client
        .post(format!("http://{addr}/ui/revoke/abc"))
        .form(&[("_csrf", "x")])
        .send()
        .await
        .unwrap();
    let location = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(location, "/ui/tokens");
}

/// Regression guard for a padding bug in the vendored stylesheet.
///
/// `.mui-card__header` was `padding: 1.5rem 1.5rem 0`. Converting it to
/// logical properties dropped the inline axis, so the card title and
/// description sat flush against the card border while the body below kept
/// its padding. Any rule converted from a 3/4-value `padding` must keep both
/// axes.
#[test]
fn test_ui_card_header_keeps_inline_padding() {
    let css = auth_ui::served_css();
    let header = css
        .split(".mui-card__header {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect(".mui-card__header rule must exist");

    assert!(
        header.contains("padding-inline"),
        ".mui-card__header lost its inline padding axis; title/description will \
         touch the card border: {header}"
    );
    assert!(
        header.contains("padding-block"),
        ".mui-card__header lost its block padding axis: {header}"
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
    // The failure message travels in the session, not the URL.
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        "/ui"
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
        "/intervals/ui"
    );
}

/// Regression guard for the vertical gap between the card description and the
/// token field on the post-creation page.
///
/// `.mui-card__header + .mui-card__body` zeroes the body's top padding, so the
/// first element in the body is responsible for its own leading space. When
/// that rule was only applied to `.card-fields`, this page's `.token-display`
/// sat flush against the description with a 0px gap.
#[test]
fn test_token_success_field_has_leading_space() {
    let page_css = auth_ui::inline_style_for_test();

    // Assert on the `.token-display` rule itself. Checking that the value
    // appears *somewhere* in the stylesheet would pass even with the rule
    // reverted, because `.card-fields` carries the same declaration.
    let start = page_css
        .find(".token-display {")
        .expect("a `.token-display` rule must exist");
    let block_end = page_css[start..]
        .find('}')
        .map(|i| start + i)
        .expect("rule must be closed");
    let rule = &page_css[start..block_end];

    assert!(
        rule.contains("margin-block-start: 1.5rem"),
        "the token field needs the same leading space as `.card-fields`, got: {rule}"
    );
}

/// The empty state must not imply an authentication flow that does not exist.
///
/// There is no login, no password, and no identity in this UI: the session
/// cookie only remembers which athlete's tokens this browser created. Saying
/// "Sign in again" and "Session expired" was wrong twice over -- and a
/// first-time visitor with no cookie at all was told their session had expired.
#[tokio::test]
async fn test_empty_state_does_not_claim_authentication() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui/tokens", get(auth_ui::ui_list_tokens))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // No cookie at all: a brand-new visitor, not an expired session.
    let page = reqwest::get(format!("http://{addr}/ui/tokens"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    for fiction in ["Sign in", "sign in", "Session expired", "expired session"] {
        assert!(
            !page.contains(fiction),
            "the UI has no authentication flow, so it must not say {fiction:?}"
        );
    }
    // The registry is the inventory: an empty registry reads as "No tokens yet",
    // never as a lost session.
    assert!(
        page.contains("No tokens yet"),
        "the state should be described accurately"
    );
    assert!(
        !page.contains("No athlete in this session"),
        "the inventory must not depend on the ephemeral session"
    );
    // The call to action should offer the real next step.
    assert!(
        page.contains("Create a Token"),
        "the CTA should be the action a user can actually take"
    );
}

/// The existing tests should also cover the copy fix in the rendered page.
#[tokio::test]
async fn test_empty_state_cta_links_home() {
    let state = test_ui_state();
    let app = Router::new()
        .route("/ui/tokens", get(auth_ui::ui_list_tokens))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let page = reqwest::get(format!("http://{addr}/ui/tokens"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains(r#"href="/ui""#),
        "the CTA must actually navigate somewhere useful"
    );
}

/// The token inventory is the registry, not a session-scoped subset: with no
/// cookie at all, an issued token must still be listed.
#[tokio::test]
async fn test_ui_tokens_lists_whole_registry_without_a_session() {
    let state = test_ui_state();
    state
        .app_state
        .issue_recorded_token("i-registry-visible", "api-key", 3600)
        .await
        .expect("token issued");

    let app = Router::new()
        .route("/ui/tokens", get(auth_ui::ui_list_tokens))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let page = reqwest::get(format!("http://{addr}/ui/tokens"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(
        page.contains("i-registry-visible"),
        "the registry inventory must be visible without a session"
    );
}
