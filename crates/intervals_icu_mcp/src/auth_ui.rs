use axum::{
    Form,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect, Response},
};
use intervals_icu_client::IntervalsClient;
use maud::{DOCTYPE, Markup, html};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::auth::AppState;

const MAUD_UI_CSS: &str = include_str!("../static/maud-ui.css");
const DEFAULT_TOKEN_TTL_DAYS: u64 = 30;
const MAX_TOKEN_TTL_DAYS: u64 = 3650;
const SECONDS_PER_DAY: u64 = 86_400;
const SESSION_COOKIE_NAME: &str = "mcp_session";
const SESSION_COOKIE_MAX_AGE_SECONDS: u64 = 3600;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenRecord {
    pub jti: String,
    pub athlete_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub revoked: bool,
}

pub type TokenRegistry = Arc<RwLock<Vec<TokenRecord>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SessionContext {
    session_id: String,
    csrf_token: String,
    athlete_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TokenRequestData {
    athlete_id: String,
    api_key: String,
    ttl_days: u64,
}

impl TokenRequestData {
    fn from_form(form: TokenForm) -> Option<Self> {
        let athlete_id = form.athlete_id.or(form.email).unwrap_or_default();
        let api_key = form.api_key.or(form.password).unwrap_or_default();

        if athlete_id.is_empty() || api_key.is_empty() {
            return None;
        }

        Some(Self {
            athlete_id,
            api_key,
            ttl_days: normalize_ttl_days(form.ttl_days),
        })
    }

    fn ttl_seconds(&self) -> u64 {
        self.ttl_days * SECONDS_PER_DAY
    }

    fn ttl_label(&self) -> String {
        format!(
            "{} day{}",
            self.ttl_days,
            if self.ttl_days == 1 { "" } else { "s" }
        )
    }
}

// ── App state for auth_ui ────────────────────────────────────────────────

#[derive(Clone)]
pub struct UiState {
    pub app_state: Arc<AppState>,
    pub sessions: SessionStore,
    pub tokens: TokenRegistry,
    pub registry_path: Option<PathBuf>,
}

impl UiState {
    pub fn new(app_state: Arc<AppState>, registry_path: Option<PathBuf>) -> Self {
        let tokens = Arc::new(RwLock::new(
            registry_path
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
        ));
        Self {
            app_state,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            tokens,
            registry_path,
        }
    }

    async fn session_context(&self, headers: &HeaderMap) -> SessionContext {
        let session_id = self.get_or_create_session_id(headers).await;
        let session = self.session(&session_id).await;

        SessionContext {
            csrf_token: session
                .as_ref()
                .map(|session| session.csrf_token.clone())
                .unwrap_or_default(),
            athlete_id: session.and_then(|session| session.athlete_id),
            session_id,
        }
    }

    async fn get_or_create_session_id(&self, headers: &HeaderMap) -> String {
        if let Some(session_id) = session_id_from_headers(headers)
            && self.sessions.read().await.contains_key(&session_id)
        {
            return session_id;
        }

        let session_id = generate_session_id();
        let csrf = generate_csrf_token();
        self.sessions.write().await.insert(
            session_id.clone(),
            SessionData {
                csrf_token: csrf,
                athlete_id: None,
            },
        );
        session_id
    }

    async fn session(&self, session_id: &str) -> Option<SessionData> {
        self.sessions.read().await.get(session_id).cloned()
    }

    async fn csrf_matches(&self, session_id: &str, submitted_csrf: &str) -> bool {
        self.session(session_id)
            .await
            .map(|session| session.csrf_token == submitted_csrf)
            .unwrap_or(false)
    }

    async fn remember_athlete(&self, session_id: &str, athlete_id: &str) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.athlete_id = Some(athlete_id.to_string());
        }
    }

    async fn tokens_for_athlete(&self, athlete_id: &str) -> Vec<TokenRecord> {
        self.tokens
            .read()
            .await
            .iter()
            .filter(|token| token.athlete_id == athlete_id)
            .cloned()
            .collect()
    }

    async fn record_token(&self, record: TokenRecord) {
        self.tokens.write().await.push(record);
        self.persist_registry().await;
    }

    async fn revoke_token(&self, jti: &str) -> bool {
        let revoked = {
            let mut registry = self.tokens.write().await;
            if let Some(token) = registry.iter_mut().find(|token| token.jti == jti) {
                token.revoked = true;
                true
            } else {
                false
            }
        };

        if revoked {
            self.persist_registry().await;
        }

        revoked
    }

    async fn persist_registry(&self) {
        if let Some(path) = self.registry_path.as_ref() {
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            let tokens = self.tokens.read().await.clone();
            if let Ok(data) = serde_json::to_vec(&tokens) {
                let _ = tokio::fs::write(path, data).await;
            }
        }
    }
}

fn normalize_ttl_days(ttl_days: Option<u64>) -> u64 {
    ttl_days
        .unwrap_or(DEFAULT_TOKEN_TTL_DAYS)
        .clamp(1, MAX_TOKEN_TTL_DAYS)
}

fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| {
            cookie.split(';').map(str::trim).find_map(|pair| {
                let (name, value) = pair.split_once('=')?;
                (name == SESSION_COOKIE_NAME).then(|| value.to_string())
            })
        })
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

fn page_shell(
    title: &str,
    page: &str,
    body: Markup,
    flash_success: Option<String>,
    flash_error: Option<String>,
) -> Markup {
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
                script {
                    r#"document.addEventListener("DOMContentLoaded",()=>{const a=document.querySelector(".error-alert,.success-alert");a&&(setTimeout(()=>{a.style.transition="opacity .4s";a.style.opacity="0";setTimeout(()=>a.remove(),400)},5e3),window.history.replaceState&&((n=new URL(window.location)).searchParams.delete("error"),n.searchParams.delete("success"),window.history.replaceState({},"",n)));document.querySelector("[data-clipboard]")?.addEventListener("click",function(){navigator.clipboard.writeText(this.dataset.clipboard).then(()=>{let t=this.querySelector(".copy-toast");t||(t=document.createElement("span"),t.className="copy-toast",t.textContent="Copied!",this.appendChild(t),requestAnimationFrame(()=>t.classList.add("show"))),setTimeout(()=>{t.classList.remove("show"),setTimeout(()=>t.remove(),250)},1200)}).catch(()=>{})})})"#
                }
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
                      padding-top: 2rem;
                      border-radius: 0.5rem;
                      border: 1px solid #1e293b;
                      position: relative;
                    }
                    .copy-btn { position: absolute; top: 0.5rem; right: 0.5rem; background: none; border: none; color: #64748b; cursor: pointer; padding: 0.25rem; line-height: 1; display: inline-flex; align-items: center; justify-content: center; border-radius: 0.25rem; transition: color .15s, background .15s; }
                    .copy-btn:hover { color: #e2e8f0; background: rgba(226,232,240,0.08); }
                    .copy-btn:active svg { transform: scale(0.85); }
                    .copy-btn svg { display: block; transition: transform .15s; }
                    .copy-toast { position: absolute; top: -1.5rem; right: 0; font-size: 0.65rem; color: #22c55e; white-space: nowrap; opacity: 0; transition: opacity .25s, transform .25s; pointer-events: none; transform: translateY(4px); }
                    .copy-toast.show { opacity: 1; transform: translateY(0); }
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
                    .flash-banner { margin: 1rem auto; max-width: 48rem; animation: flashIn .35s ease-out; }
                    @keyframes flashIn { from { opacity: 0; transform: translateY(-8px); } to { opacity: 1; transform: translateY(0); } }
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
                @if let Some(ref msg) = flash_error {
                    .flash-banner.error-alert { (msg) }
                } @else if let Some(ref msg) = flash_success {
                    .flash-banner.success-alert { (msg) }
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

fn render_home_body(csrf: &str) -> Markup {
    use maud_ui::primitives::{button, card, field, input};

    html! {
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
                },
                ..Default::default()
            }))
        }
    }
}

pub async fn ui_home(
    State(ui): State<UiState>,
    Query(query): Query<UiQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let session = ui.session_context(&headers).await;
    let html = page_shell(
        "Token Setup",
        "/ui",
        render_home_body(&session.csrf_token),
        query.success,
        query.error,
    );
    let mut resp = html.into_response();
    set_session_cookie(&mut resp, &session.session_id);
    resp
}

fn set_session_cookie(resp: &mut axum::response::Response, session_id: &str) {
    use axum::http::header::SET_COOKIE;
    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Strict; Path=/ui; Max-Age={}",
        SESSION_COOKIE_NAME, session_id, SESSION_COOKIE_MAX_AGE_SECONDS
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
    let session = ui.session_context(&headers).await;
    let submitted_csrf = form._csrf.clone().unwrap_or_default();
    if !ui.csrf_matches(&session.session_id, &submitted_csrf).await {
        return redirect_with_session("/ui?error=Invalid+session+%28CSRF%29", &session.session_id);
    }

    let Some(token_request) = TokenRequestData::from_form(form) else {
        return redirect_with_session("/ui?error=Missing+credentials", &session.session_id);
    };

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &ui.app_state.base_url,
        token_request.athlete_id.clone(),
        secrecy::SecretString::new(token_request.api_key.clone().into()),
    );

    let ttl_seconds = token_request.ttl_seconds();

    match client.get_athlete_profile().await {
        Ok(_) => match ui.app_state.jwt_manager.issue_token(
            &token_request.athlete_id,
            &token_request.api_key,
            ttl_seconds,
        ) {
            Ok(token) => {
                let now = chrono::Utc::now();
                let expires = now + chrono::TimeDelta::seconds(ttl_seconds as i64);
                let expires_at = expires.to_rfc3339();
                let expiry_formatted = format_datetime(&expires_at);

                ui.record_token(TokenRecord {
                    jti: Uuid::new_v4().to_string(),
                    athlete_id: token_request.athlete_id.clone(),
                    issued_at: now.to_rfc3339(),
                    expires_at,
                    revoked: false,
                })
                .await;

                ui.remember_athlete(&session.session_id, &token_request.athlete_id)
                    .await;

                crate::metrics::record_token_issued_with_source("ui");
                crate::metrics::record_ui_action("token_created");
                tracing::info!(
                    athlete_id = %token_request.athlete_id,
                    ui_action = "token_created",
                    "Token created via web UI"
                );

                let body = render_token_success(
                    &token,
                    &token_request.athlete_id,
                    &expiry_formatted,
                    &token_request.ttl_label(),
                );
                let html = page_shell("Token Generated", "/ui", body, None, None);
                let mut resp = html.into_response();
                set_session_cookie(&mut resp, &session.session_id);
                resp
            }
            Err(e) => redirect_with_session(
                &format!("/ui?error=Token+generation+failed%3A+{e}"),
                &session.session_id,
            ),
        },
        Err(_) => redirect_with_session("/ui?error=Invalid+credentials", &session.session_id),
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
                button.copy-btn data-clipboard=(token) {
                    svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" {
                        rect x="8" y="2" width="8" height="4" rx="1" ry="1" {}
                        path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" {}
                    }
                }
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

// ── Token listing (GET /ui/tokens) ───────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct TokenListQuery {
    pub sort: Option<String>,
    pub order: Option<String>,
    pub success: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    let session = ui.session_context(&headers).await;
    let current_athlete = session.athlete_id.unwrap_or_default();
    let filtered = if current_athlete.is_empty() {
        vec![]
    } else {
        ui.tokens_for_athlete(&current_athlete).await
    };

    let sort = parse_sort_field(query.sort.as_deref().unwrap_or("issued"));
    let ascending = query.order.as_deref() != Some("desc");
    let sorted = sort_tokens(filtered, sort, ascending);

    let body = render_token_list(&sorted, &session.csrf_token, &query.sort, &query.order);
    let html = page_shell(
        "Token Management",
        "/ui/tokens",
        body,
        query.success,
        query.error,
    );
    let mut resp = html.into_response();
    set_session_cookie(&mut resp, &session.session_id);
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
            @if tokens.is_empty() {
                div style="text-align:center;padding:2rem 0;" {
                    p style="color:var(--mui-muted-foreground,#6b7280);margin-bottom:1rem;" {
                        "No tokens yet. Create one to get started."
                    }
                    a href="/ui" {
                        (button::render(button::Props {
                            label: "Create a Token".into(),
                            ..Default::default()
                        }))
                    }
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
    let session = ui.session_context(&headers).await;
    let submitted_csrf = form.get("_csrf").map(|value| value.as_str()).unwrap_or("");
    if !ui.csrf_matches(&session.session_id, submitted_csrf).await {
        return redirect_with_session("/ui/tokens?error=Invalid+CSRF", &session.session_id);
    }

    ui.revoke_token(&jti).await;

    crate::metrics::record_ui_action("token_revoked");
    tracing::info!(
        jti = %jti,
        ui_action = "token_revoked",
        "Token revoked via web UI"
    );

    redirect_with_session("/ui/tokens?success=Token+revoked", &session.session_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn test_ui_state() -> UiState {
        let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
        let jwt_manager = Arc::new(crate::auth::JwtManager::new(secret, [0u8; 32]));
        let app_state = Arc::new(AppState {
            jwt_manager,
            jwt_ttl_seconds: 3600,
            base_url: "https://intervals.icu".to_string(),
        });

        UiState::new(app_state, None)
    }

    fn headers_with_cookie(cookie: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
        headers
    }

    #[test]
    fn auth_ui_token_request_uses_primary_fields() {
        let request = TokenRequestData::from_form(TokenForm {
            _csrf: None,
            athlete_id: Some("i123456".to_string()),
            api_key: Some("secret".to_string()),
            email: Some("ignored@example.com".to_string()),
            password: Some("ignored".to_string()),
            ttl_days: Some(7),
        })
        .expect("request should be normalized");

        assert_eq!(request.athlete_id, "i123456");
        assert_eq!(request.api_key, "secret");
        assert_eq!(request.ttl_days, 7);
    }

    #[test]
    fn auth_ui_token_request_uses_fallback_fields() {
        let request = TokenRequestData::from_form(TokenForm {
            _csrf: None,
            athlete_id: None,
            api_key: None,
            email: Some("i654321".to_string()),
            password: Some("fallback-secret".to_string()),
            ttl_days: Some(1),
        })
        .expect("fallback fields should work");

        assert_eq!(request.athlete_id, "i654321");
        assert_eq!(request.api_key, "fallback-secret");
        assert_eq!(request.ttl_seconds(), SECONDS_PER_DAY);
        assert_eq!(request.ttl_label(), "1 day");
    }

    #[test]
    fn auth_ui_token_request_rejects_missing_credentials() {
        let request = TokenRequestData::from_form(TokenForm {
            _csrf: None,
            athlete_id: Some(String::new()),
            api_key: Some(String::new()),
            email: None,
            password: None,
            ttl_days: None,
        });

        assert!(request.is_none());
    }

    #[test]
    fn auth_ui_normalize_ttl_days_applies_defaults_and_limits() {
        assert_eq!(normalize_ttl_days(None), DEFAULT_TOKEN_TTL_DAYS);
        assert_eq!(normalize_ttl_days(Some(0)), 1);
        assert_eq!(normalize_ttl_days(Some(9_999)), MAX_TOKEN_TTL_DAYS);
    }

    #[test]
    fn auth_ui_session_id_from_headers_handles_multiple_cookies() {
        let headers = headers_with_cookie("theme=dark; mcp_session=session-123; mode=compact");

        assert_eq!(
            session_id_from_headers(&headers),
            Some("session-123".to_string())
        );
    }

    #[tokio::test]
    async fn auth_ui_session_context_reuses_existing_session() {
        let ui = test_ui_state();
        let first_session = ui.session_context(&HeaderMap::new()).await;
        let headers = headers_with_cookie(&format!(
            "theme=dark; {}={}",
            SESSION_COOKIE_NAME, first_session.session_id
        ));

        let reused_session = ui.session_context(&headers).await;

        assert_eq!(reused_session.session_id, first_session.session_id);
        assert_eq!(reused_session.csrf_token, first_session.csrf_token);
    }

    #[tokio::test]
    async fn auth_ui_session_helpers_track_athlete_and_csrf() {
        let ui = test_ui_state();
        let session = ui.session_context(&HeaderMap::new()).await;

        assert!(
            ui.csrf_matches(&session.session_id, &session.csrf_token)
                .await
        );
        assert!(!ui.csrf_matches(&session.session_id, "invalid").await);

        ui.remember_athlete(&session.session_id, "i123456").await;

        let headers =
            headers_with_cookie(&format!("{}={}", SESSION_COOKIE_NAME, session.session_id));
        let updated_session = ui.session_context(&headers).await;

        assert_eq!(updated_session.athlete_id.as_deref(), Some("i123456"));
    }

    #[tokio::test]
    async fn auth_ui_token_registry_filters_and_revokes() {
        let ui = test_ui_state();

        ui.record_token(TokenRecord {
            jti: "jti-1".to_string(),
            athlete_id: "athlete-a".to_string(),
            issued_at: "2026-05-26T10:00:00Z".to_string(),
            expires_at: "2026-05-27T10:00:00Z".to_string(),
            revoked: false,
        })
        .await;
        ui.record_token(TokenRecord {
            jti: "jti-2".to_string(),
            athlete_id: "athlete-b".to_string(),
            issued_at: "2026-05-26T11:00:00Z".to_string(),
            expires_at: "2026-05-27T11:00:00Z".to_string(),
            revoked: false,
        })
        .await;

        let athlete_tokens = ui.tokens_for_athlete("athlete-a").await;
        assert_eq!(athlete_tokens.len(), 1);
        assert_eq!(athlete_tokens[0].jti, "jti-1");
        assert!(!athlete_tokens[0].revoked);

        assert!(ui.revoke_token("jti-1").await);
        assert!(!ui.revoke_token("missing-jti").await);

        let athlete_tokens = ui.tokens_for_athlete("athlete-a").await;
        assert!(athlete_tokens[0].revoked);
    }

    #[test]
    fn auth_ui_sort_tokens_orders_by_requested_field() {
        let tokens = vec![
            TokenRecord {
                jti: "jti-1".to_string(),
                athlete_id: "b-athlete".to_string(),
                issued_at: "2026-05-26T11:00:00Z".to_string(),
                expires_at: "2026-05-28T11:00:00Z".to_string(),
                revoked: false,
            },
            TokenRecord {
                jti: "jti-2".to_string(),
                athlete_id: "a-athlete".to_string(),
                issued_at: "2026-05-26T10:00:00Z".to_string(),
                expires_at: "2026-05-27T10:00:00Z".to_string(),
                revoked: true,
            },
        ];

        let sorted = sort_tokens(tokens, SortField::AthleteId, true);
        assert_eq!(sorted[0].athlete_id, "a-athlete");

        let sorted = sort_tokens(sorted, SortField::Status, true);
        assert!(!sorted[0].revoked);
    }
}
