use axum::{
    Form,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect, Response},
};
use intervals_icu_client::IntervalsClient;
use maud::{DOCTYPE, Markup, html};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::auth::AppState;

const MAUD_UI_CSS: &str = include_str!("../static/maud-ui.css");

// ── Session types ────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct SessionData {
    pub csrf_token: String,
    pub athlete_id: Option<String>,
}

pub type SessionStore = Arc<RwLock<HashMap<String, SessionData>>>;

fn generate_session_id() -> String {
    Uuid::new_v4().to_string()
}

fn generate_csrf_token() -> String {
    Uuid::new_v4().to_string()
}

// ── Token registry ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct TokenRecord {
    pub jti: String,
    pub athlete_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub revoked: bool,
}

pub type TokenRegistry = Arc<RwLock<Vec<TokenRecord>>>;

// ── App state for auth_ui ────────────────────────────────────────────────

#[derive(Clone)]
pub struct UiState {
    pub app_state: Arc<AppState>,
    pub sessions: SessionStore,
    pub tokens: TokenRegistry,
}

// ── CSS route ────────────────────────────────────────────────────────────

pub async fn serve_css() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "text/css; charset=utf-8".parse().unwrap(),
    );
    headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=3600".parse().unwrap(),
    );
    (headers, MAUD_UI_CSS)
}

// ── HTML shell ───────────────────────────────────────────────────────────

fn page_shell(title: &str, _csrf_token: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — Intervals.icu MCP" }
                link rel="stylesheet" href="/ui/static/css";
                style {
                    ".mui-card { max-width: 28rem; margin: 4rem auto; }"
                    ".mui-card__body { padding: 1.5rem; }"
                    ".token-display { "
                    "  font-family: 'SF Mono', 'Fira Code', monospace;"
                    "  font-size: 0.75rem;"
                    "  word-break: break-all;"
                    "  background: var(--mui-bg-muted, #f5f5f5);"
                    "  padding: 1rem;"
                    "  border-radius: 0.5rem;"
                    "  position: relative;"
                    "}"
                    ".copy-btn { "
                    "  position: absolute; top: 0.5rem; right: 0.5rem;"
                    "}"
                    ".error-msg { color: var(--mui-danger, #e11d48); margin-bottom: 1rem; }"
                    ".success-msg { color: var(--mui-success, #16a34a); margin-bottom: 1rem; }"
                    "input[type=hidden] { display: none; }"
                    ".nav-bar { display: flex; align-items: center; gap: 1rem; padding: 0.75rem 1.5rem; "
                    "  border-bottom: 1px solid var(--mui-border, #e5e7eb); }"
                    ".nav-bar a { color: var(--mui-text-muted, #6b7280); text-decoration: none; font-size: 0.875rem; }"
                    ".nav-bar a:hover { color: var(--mui-text, #111827); }"
                    ".nav-bar .brand { font-weight: 600; font-size: 1rem; color: var(--mui-text, #111827); }"
                }
            }
            body {
                .nav-bar {
                    .brand { "Intervals.icu MCP" }
                    a href="/ui" { "Home" }
                    a href="/ui/tokens" { "Tokens" }
                }
                .mui-container {
                    (body)
                }
            }
        }
    }
}

// ── Landing page (GET /ui) ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UiQuery {
    pub error: Option<String>,
    pub success: Option<String>,
}

pub async fn ui_home(
    State(ui): State<UiState>,
    Query(query): Query<UiQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let session_id = get_or_create_session(&ui.sessions, &headers).await;
    let session = ui.sessions.read().await.get(&session_id).cloned();
    let csrf = session
        .as_ref()
        .map(|s| s.csrf_token.clone())
        .unwrap_or_default();

    use maud_ui::primitives::{button, card, field, input};

    let body = card::render(card::Props {
        title: Some("MCP Server Token".into()),
        description: Some("Enter your Intervals.icu credentials to generate an API token".into()),
        children: html! {
            @if let Some(ref error) = query.error {
                div class="error-msg" { (error) }
            }
            @if let Some(ref success) = query.success {
                div class="success-msg" { (success) }
            }
            form method="POST" action="/ui/token" {
                input type="hidden" name="_csrf" value=(csrf);
                (field::render(field::Props {
                    label: "Athlete ID".into(),
                    id: "athlete_id".into(),
                    required: true,
                    children: html! {
                        (input::render(input::Props {
                            name: "athlete_id".into(),
                            placeholder: "e.g. i123456".into(),
                            required: true,
                            ..Default::default()
                        }))
                    },
                    ..Default::default()
                }))
                (field::render(field::Props {
                    label: "API Key".into(),
                    id: "api_key".into(),
                    required: true,
                    children: html! {
                        (input::render(input::Props {
                            name: "api_key".into(),
                            input_type: input::InputType::Password,
                            placeholder: "Your Intervals.icu API key".into(),
                            required: true,
                            ..Default::default()
                        }))
                    },
                    ..Default::default()
                }))
                br;
                (button::render(button::Props {
                    label: "Generate Token".into(),
                    button_type: "submit",
                    variant: button::Variant::Default,
                    ..Default::default()
                }))
            }
        },
        ..Default::default()
    });

    let html = page_shell("Token Setup", &csrf, body);
    let mut resp = html.into_response();
    set_session_cookie(&mut resp, &session_id);
    resp
}

async fn get_or_create_session(store: &SessionStore, headers: &HeaderMap) -> String {
    if let Some(cookie) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for pair in cookie.split("; ") {
            if let Some(sid) = pair.strip_prefix("mcp_session=")
                && store.read().await.contains_key(sid)
            {
                return sid.to_string();
            }
        }
    }
    let sid = generate_session_id();
    let csrf = generate_csrf_token();
    store.write().await.insert(
        sid.clone(),
        SessionData {
            csrf_token: csrf,
            athlete_id: None,
        },
    );
    sid
}

fn set_session_cookie(resp: &mut axum::response::Response, session_id: &str) {
    use axum::http::header::SET_COOKIE;
    let cookie = format!(
        "mcp_session={}; HttpOnly; SameSite=Strict; Path=/ui; Max-Age=3600",
        session_id
    );
    resp.headers_mut()
        .insert(SET_COOKIE, cookie.parse().unwrap());
}

// ── Token creation (POST /ui/token) ──────────────────────────────────────

#[derive(Deserialize)]
pub struct TokenForm {
    pub _csrf: Option<String>,
    pub athlete_id: Option<String>,
    pub api_key: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
}

fn redirect_with_session(url: &str, session_id: &str) -> Response {
    let mut resp = Redirect::to(url).into_response();
    set_session_cookie(&mut resp, session_id);
    resp
}

pub async fn ui_create_token(
    State(ui): State<UiState>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> Response {
    let session_id = get_or_create_session(&ui.sessions, &headers).await;
    let submitted_csrf = form._csrf.as_deref().unwrap_or("");
    let valid_csrf = {
        let session = ui.sessions.read().await.get(&session_id).cloned();
        session
            .map(|s| s.csrf_token == submitted_csrf)
            .unwrap_or(false)
    };
    if !valid_csrf {
        return redirect_with_session("/ui?error=Invalid+session+%28CSRF%29", &session_id);
    }

    let athlete_id = form.athlete_id.or(form.email).unwrap_or_default();
    let api_key = form.api_key.or(form.password).unwrap_or_default();

    if athlete_id.is_empty() || api_key.is_empty() {
        return redirect_with_session("/ui?error=Missing+credentials", &session_id);
    }

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &ui.app_state.base_url,
        athlete_id.clone(),
        secrecy::SecretString::new(api_key.clone().into()),
    );

    match client.get_athlete_profile().await {
        Ok(_) => match ui.app_state.jwt_manager.issue_token(
            &athlete_id,
            &api_key,
            ui.app_state.jwt_ttl_seconds,
        ) {
            Ok(token) => {
                let jti = Uuid::new_v4().to_string();
                let now = chrono::Utc::now();
                let expires = now + chrono::TimeDelta::seconds(ui.app_state.jwt_ttl_seconds as i64);

                let mut registry = ui.tokens.write().await;
                registry.push(TokenRecord {
                    jti: jti.clone(),
                    athlete_id: athlete_id.clone(),
                    issued_at: now.to_rfc3339(),
                    expires_at: expires.to_rfc3339(),
                    revoked: false,
                });
                drop(registry);

                let mut session = ui.sessions.write().await;
                if let Some(s) = session.get_mut(&session_id) {
                    s.athlete_id = Some(athlete_id.clone());
                }
                drop(session);

                crate::metrics::record_token_issued_with_source("ui");
                crate::metrics::record_ui_action("token_created");
                tracing::info!(
                    athlete_id = %athlete_id,
                    ui_action = "token_created",
                    "Token created via web UI"
                );

                let csrf = get_csrf(&ui.sessions, &session_id).await;
                let body = render_token_success(&token, &athlete_id, ui.app_state.jwt_ttl_seconds);
                let html = page_shell("Token Generated", &csrf, body);
                let mut resp = html.into_response();
                set_session_cookie(&mut resp, &session_id);
                resp
            }
            Err(e) => redirect_with_session(
                &format!("/ui?error=Token+generation+failed%3A+{e}"),
                &session_id,
            ),
        },
        Err(_) => redirect_with_session("/ui?error=Invalid+credentials", &session_id),
    }
}

fn render_token_success(token: &str, athlete_id: &str, expires_in: u64) -> Markup {
    use maud_ui::primitives::{button, card};

    let expiry_human = if expires_in >= 86400 {
        format!("{} days", expires_in / 86400)
    } else {
        format!("{} seconds", expires_in)
    };

    card::render(card::Props {
        title: Some("Token Generated".into()),
        description: Some("Copy this token now — you won't see it again.".into()),
        children: html! {
            div class="token-display" {
                (token)
            }
            br;
            p {
                strong { "Athlete ID: " } (athlete_id) br;
                strong { "Expires: " } (expiry_human)
            }
            br;
            a href="/ui" {
                (button::render(button::Props {
                    label: "Back to Home".into(),
                    variant: button::Variant::Outline,
                    ..Default::default()
                }))
            }
            " "
            a href="/ui/tokens" {
                (button::render(button::Props {
                    label: "View All Tokens".into(),
                    variant: button::Variant::Primary,
                    ..Default::default()
                }))
            }
        },
        ..Default::default()
    })
}

async fn get_csrf(store: &SessionStore, session_id: &str) -> String {
    store
        .read()
        .await
        .get(session_id)
        .map(|s| s.csrf_token.clone())
        .unwrap_or_default()
}

// ── Token listing (GET /ui/tokens) ───────────────────────────────────────

pub async fn ui_list_tokens(State(ui): State<UiState>, headers: HeaderMap) -> Response {
    let session_id = get_or_create_session(&ui.sessions, &headers).await;
    let csrf = get_csrf(&ui.sessions, &session_id).await;

    let session = ui.sessions.read().await.get(&session_id).cloned();
    let current_athlete = session.and_then(|s| s.athlete_id).unwrap_or_default();

    let all_tokens = ui.tokens.read().await.clone();
    let filtered: Vec<TokenRecord> = if current_athlete.is_empty() {
        vec![]
    } else {
        all_tokens
            .into_iter()
            .filter(|t| t.athlete_id == current_athlete)
            .collect()
    };

    let body = render_token_list(&filtered, &csrf);
    let html = page_shell("Token Management", &csrf, body);
    let mut resp = html.into_response();
    set_session_cookie(&mut resp, &session_id);
    resp
}

fn render_token_list(tokens: &[TokenRecord], csrf: &str) -> Markup {
    use maud_ui::primitives::{badge, button, card, table};

    card::render(card::Props {
        title: Some("Active Tokens".into()),
        description: Some("Manage tokens issued through this interface.".into()),
        children: html! {
            @if tokens.is_empty() {
                p { "No tokens have been issued yet." }
                a href="/ui" {
                    (button::render(button::Props {
                        label: "Generate a Token".into(),
                        ..Default::default()
                    }))
                }
            } @else {
                (table::render(table::Props {
                    headers: vec![
                        "Athlete ID".into(),
                        "Issued".into(),
                        "Expires".into(),
                        "Status".into(),
                        "Action".into(),
                    ],
                    rich_rows: tokens
                        .iter()
                        .map(|t| {
                            let status_badge = if t.revoked {
                                badge::render(badge::Props { label: "Revoked".into(), variant: badge::Variant::Danger, ..Default::default() })
                            } else {
                                badge::render(badge::Props { label: "Active".into(), variant: badge::Variant::Success, ..Default::default() })
                            };
                            let action = if !t.revoked {
                                html! {
                                    form method="POST" action={"/ui/revoke/"(t.jti)} {
                                        input type="hidden" name="_csrf" value=(csrf);
                                        (button::render(button::Props {
                                            label: "Revoke".into(),
                                            variant: button::Variant::Danger,
                                            size: button::Size::Sm,
                                            ..Default::default()
                                        }))
                                    }
                                }
                            } else {
                                html! {}
                            };
                            vec![
                                table::CellMarkup::markup(html! { (t.athlete_id) }, false),
                                table::CellMarkup::markup(html! { (t.issued_at) }, false),
                                table::CellMarkup::markup(html! { (t.expires_at) }, false),
                                table::CellMarkup::markup(status_badge, false),
                                table::CellMarkup::markup(action, false),
                            ]
                        })
                        .collect(),
                    ..Default::default()
                }))
            }
        },
        ..Default::default()
    })
}

// ── Token revocation (POST /ui/revoke/:jti) ──────────────────────────────

pub async fn ui_revoke_token(
    State(ui): State<UiState>,
    headers: HeaderMap,
    axum::extract::Path(jti): axum::extract::Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let session_id = get_or_create_session(&ui.sessions, &headers).await;
    let valid_csrf = {
        let session = ui.sessions.read().await.get(&session_id).cloned();
        let submitted = form.get("_csrf").map(|s| s.as_str()).unwrap_or("");
        session.map(|s| s.csrf_token == submitted).unwrap_or(false)
    };
    if !valid_csrf {
        return redirect_with_session("/ui/tokens?error=Invalid+CSRF", &session_id);
    }

    {
        let mut registry = ui.tokens.write().await;
        if let Some(token) = registry.iter_mut().find(|t| t.jti == jti) {
            token.revoked = true;
        }
    }

    crate::metrics::record_ui_action("token_revoked");
    tracing::info!(
        jti = %jti,
        ui_action = "token_revoked",
        "Token revoked via web UI"
    );

    redirect_with_session("/ui/tokens", &session_id)
}
