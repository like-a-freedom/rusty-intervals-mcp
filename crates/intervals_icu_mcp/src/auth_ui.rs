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

fn page_shell(title: &str, _csrf_token: &str, page: &str, body: Markup) -> Markup {
    fn nav_link(label: &str, href: &str, current: &str) -> Markup {
        if href == current {
            html! { a.active href=(href) { (label) } }
        } else {
            html! { a href=(href) { (label) } }
        }
    }
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — Intervals.icu MCP" }
                link rel="stylesheet" href="/ui/static/css";
                style {
                    r#"                    .mui-card { margin: 4rem auto; }
                    .mui-card--narrow { max-width: 28rem; }
                    .mui-card__body { padding: 1.5rem; }
                    .mui-card__title { margin-bottom: 0.5rem; }
                    .mui-card__description { margin-top: 0.5rem; }
                    .mui-field + .mui-field { margin-top: 1.25rem; }
                    .tokens-page-card .mui-card { max-width: none; width: 100%; }
                    .token-display {
                      font-family: 'SF Mono', 'Fira Code', monospace;
                      font-size: 0.75rem;
                      word-break: break-all;
                      background: #0f172a;
                      color: #e2e8f0;
                      padding: 1rem;
                      border-radius: 0.5rem;
                      border: 1px solid #1e293b;
                    }
                    .copy-btn { position: absolute; top: 0.5rem; right: 0.5rem; }
                    .error-alert {
                      background: rgba(225, 29, 72, 0.1);
                      border: 1px solid var(--mui-danger, #e11d48);
                      color: var(--mui-danger, #e11d48);
                      padding: 0.75rem 1rem;
                      border-radius: 0.5rem;
                      font-size: 0.875rem;
                    }
                    .success-alert {
                      background: rgba(22, 163, 74, 0.1);
                      border: 1px solid var(--mui-success, #16a34a);
                      color: var(--mui-success, #16a34a);
                      padding: 0.75rem 1rem;
                      border-radius: 0.5rem;
                      font-size: 0.875rem;
                    }
                    input[type=hidden] { display: none; }
                    .nav-bar { display: flex; align-items: center; gap: 1rem; padding: 0.75rem 1.5rem;
                      border-bottom: 1px solid var(--mui-border, #e5e7eb); }
                    .nav-bar a { color: var(--mui-text-muted, #6b7280); text-decoration: none; font-size: 0.875rem; }
                    .nav-bar a:hover { color: var(--mui-text, #111827); }
                    .nav-bar a.active { color: var(--mui-text, #111827); font-weight: 500; }
                    .nav-bar .brand { font-weight: 600; font-size: 1rem; color: var(--mui-text, #111827); }
                    .mui-table-wrapper { width: 100%; overflow-x: auto; }
                    .mui-table { width: 100%; }
                    .mui-table th a { color: inherit; text-decoration: none; display: inline-flex; align-items: center; gap: 0.25rem; }
                    .mui-table th a:hover { opacity: 0.75; }
                    .sort-arrow { display: inline-block; font-size: 0.625rem; line-height: 1; }
                    .ttl-select { width: 100%; padding: 0.5rem; border: 1px solid var(--mui-border, #e5e7eb); border-radius: 0.375rem; background: var(--mui-bg, #fff); color: var(--mui-text, #111827); font-size: 0.875rem; }
                    .token-info-grid { display: grid; grid-template-columns: auto 1fr; gap: 0.5rem 1rem; margin-top: 1rem; font-size: 0.875rem; }
                    .token-info-grid dt { color: var(--mui-muted-foreground, #6b7280); text-transform: uppercase; letter-spacing: 0.05em; font-size: 0.75rem; }
                    .token-info-grid dd { margin: 0; }"#
                }
            }
            body {
                .nav-bar {
                    .brand { "Intervals.icu MCP" }
                    (nav_link("Home", "/ui", page))
                    (nav_link("Tokens", "/ui/tokens", page))
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

    let body = html! {
        div style="max-width: 28rem; margin: 0 auto;" {
            (card::render(card::Props {
                title: Some("MCP Server Token".into()),
                description: Some("Enter your Intervals.icu credentials to generate an API token".into()),
        children: html! {
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
                (field::render(field::Props {
                    label: "Token TTL".into(),
                    id: "ttl_days".into(),
                    description: Some("How long the token will be valid.".into()),
                    children: html! {
                        select.ttl-select name="ttl_days" id="ttl_days" {
                            option value="1" { "1 day" }
                            option value="7" { "7 days" }
                            option value="30" selected { "30 days (default)" }
                            option value="90" { "90 days" }
                            option value="180" { "180 days" }
                            option value="365" { "1 year" }
                            option value="1825" { "5 years" }
                            option value="3650" { "10 years" }
                        }
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
            @if let Some(ref error) = query.error {
                br;
                div.error-alert { (error) }
            }
            @if let Some(ref success) = query.success {
                br;
                div.success-alert { (success) }
            }
        },
        ..Default::default()
    }))
    }
    };

    let html = page_shell("Token Setup", &csrf, "/ui", body);
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
    pub ttl_days: Option<u64>,
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

    let ttl_days = form.ttl_days.unwrap_or(30).clamp(1, 3650);
    let ttl_seconds = ttl_days * 86400;

    match client.get_athlete_profile().await {
        Ok(_) => match ui
            .app_state
            .jwt_manager
            .issue_token(&athlete_id, &api_key, ttl_seconds)
        {
            Ok(token) => {
                let jti = Uuid::new_v4().to_string();
                let now = chrono::Utc::now();
                let expires = now + chrono::TimeDelta::seconds(ttl_seconds as i64);
                let expiry_formatted = format_datetime(&expires.to_rfc3339());
                let ttl_label = format!("{} day{}", ttl_days, if ttl_days == 1 { "" } else { "s" });

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
                let body = render_token_success(&token, &athlete_id, &expiry_formatted, &ttl_label);
                let html = page_shell("Token Generated", &csrf, "/ui", body);
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

fn format_datetime(rfc3339: &str) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rfc3339) {
        dt.format("%d %b %Y %H:%M").to_string()
    } else {
        rfc3339.to_string()
    }
}

fn render_token_success(
    token: &str,
    athlete_id: &str,
    expiry_formatted: &str,
    ttl_label: &str,
) -> Markup {
    use maud_ui::primitives::{button, card};

    let card_content = card::render(card::Props {
        title: Some("Token Generated".into()),
        description: Some("Copy this token now — you won't see it again.".into()),
        children: html! {
            div class="token-display" {
                (token)
            }
            br;
            dl.token-info-grid {
                dt { "Athlete ID" }
                dd { (athlete_id) }
                dt { "TTL" }
                dd { (ttl_label) }
                dt { "Expires" }
                dd { (expiry_formatted) }
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
    });
    html! {
        div style="max-width: 28rem; margin: 0 auto;" {
            (card_content)
        }
    }
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

#[derive(Deserialize, Default)]
pub struct TokenListQuery {
    pub sort: Option<String>,
    pub order: Option<String>,
    pub success: Option<String>,
}

enum SortField {
    IssuedAt,
    ExpiresAt,
    AthleteId,
    Status,
}

fn parse_sort_field(s: &str) -> SortField {
    match s {
        "issued" => SortField::IssuedAt,
        "expires" => SortField::ExpiresAt,
        "athlete" => SortField::AthleteId,
        "status" => SortField::Status,
        _ => SortField::IssuedAt,
    }
}

fn sort_tokens(mut tokens: Vec<TokenRecord>, sort: SortField, ascending: bool) -> Vec<TokenRecord> {
    tokens.sort_by(|a, b| {
        let cmp = match sort {
            SortField::IssuedAt => a.issued_at.cmp(&b.issued_at),
            SortField::ExpiresAt => a.expires_at.cmp(&b.expires_at),
            SortField::AthleteId => a.athlete_id.cmp(&b.athlete_id),
            SortField::Status => a.revoked.cmp(&b.revoked),
        };
        if ascending { cmp } else { cmp.reverse() }
    });
    tokens
}

pub async fn ui_list_tokens(
    State(ui): State<UiState>,
    headers: HeaderMap,
    Query(query): Query<TokenListQuery>,
) -> Response {
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

    let sort = parse_sort_field(query.sort.as_deref().unwrap_or("issued"));
    let ascending = query.order.as_deref() != Some("desc");
    let sorted = sort_tokens(filtered, sort, ascending);

    let body = render_token_list(&sorted, &csrf, &query.sort, &query.order, &query.success);
    let html = page_shell("Token Management", &csrf, "/ui/tokens", body);
    let mut resp = html.into_response();
    set_session_cookie(&mut resp, &session_id);
    resp
}

fn sort_href(
    sort_field: &str,
    current_sort: &Option<String>,
    current_order: &Option<String>,
) -> String {
    let is_current = current_sort.as_deref() == Some(sort_field);
    let new_order = if is_current && current_order.as_deref() != Some("desc") {
        "desc"
    } else {
        "asc"
    };
    format!("/ui/tokens?sort={sort_field}&order={new_order}")
}

fn sort_arrow(
    sort_field: &str,
    current_sort: &Option<String>,
    current_order: &Option<String>,
) -> &'static str {
    if current_sort.as_deref() == Some(sort_field) {
        if current_order.as_deref() == Some("desc") {
            " ▼"
        } else {
            " ▲"
        }
    } else {
        ""
    }
}

fn render_token_list(
    tokens: &[TokenRecord],
    csrf: &str,
    current_sort: &Option<String>,
    current_order: &Option<String>,
    success: &Option<String>,
) -> Markup {
    use maud_ui::primitives::{badge, button, card};

    fn th_link(
        label: &str,
        field: &str,
        current_sort: &Option<String>,
        current_order: &Option<String>,
    ) -> Markup {
        let arrow = sort_arrow(field, current_sort, current_order);
        html! {
            th.mui-table__th {
                a href=(sort_href(field, current_sort, current_order)) style="color:inherit;text-decoration:none;display:inline-flex;align-items:center;gap:0.125rem;" {
                    (label)
                    @if !arrow.is_empty() {
                        span.sort-arrow { (arrow) }
                    }
                }
            }
        }
    }

    card::render(card::Props {
        title: Some("Active Tokens".into()),
        description: Some("Manage tokens issued through this interface.".into()),
        children: html! {
            @if let Some(msg) = success {
                div.success-alert { (msg) }
                br;
            }
            @if tokens.is_empty() {
                p { "No tokens have been issued yet." }
                a href="/ui" {
                    (button::render(button::Props {
                        label: "Generate a Token".into(),
                        ..Default::default()
                    }))
                }
            } @else {
                div.mui-table-wrapper {
                    table.mui-table {
                        thead {
                            tr {
                                (th_link("Athlete ID", "athlete", current_sort, current_order))
                                (th_link("Issued", "issued", current_sort, current_order))
                                (th_link("Expires", "expires", current_sort, current_order))
                                (th_link("Status", "status", current_sort, current_order))
                                th.mui-table__th { "Action" }
                            }
                        }
                        tbody {
                            @for t in tokens {
                                tr.mui-table__row {
                                    @let issued_fmt = format_datetime(&t.issued_at);
                                    @let expires_fmt = format_datetime(&t.expires_at);
                                    td.mui-table__td { (t.athlete_id) }
                                    td.mui-table__td { (issued_fmt) }
                                    td.mui-table__td { (expires_fmt) }
                                    td.mui-table__td {
                                        @if t.revoked {
                                            (badge::render(badge::Props { label: "Revoked".into(), variant: badge::Variant::Danger, ..Default::default() }))
                                        } @else {
                                            (badge::render(badge::Props { label: "Active".into(), variant: badge::Variant::Success, ..Default::default() }))
                                        }
                                    }
                                    td.mui-table__td {
                                        @if !t.revoked {
                                            form method="POST" action={"/ui/revoke/"(t.jti)} {
                                                input type="hidden" name="_csrf" value=(csrf);
                                                (button::render(button::Props {
                                                    label: "Revoke".into(),
                                                    variant: button::Variant::Danger,
                                                    size: button::Size::Sm,
                                                    button_type: "submit",
                                                    ..Default::default()
                                                }))
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
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

    redirect_with_session("/ui/tokens?success=Token+revoked", &session_id)
}
