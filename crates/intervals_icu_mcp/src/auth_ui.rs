use axum::{
    Form,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect, Response},
};
use intervals_icu_client::IntervalsClient;
use maud::{DOCTYPE, Markup, PreEscaped, html};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::auth::{AppState, TokenRevocationSet};

const MAUD_UI_CSS: &str = include_str!("../static/maud-ui.css");

/// Page-local JavaScript.
///
/// Rendered via `PreEscaped`: browsers do not decode HTML entities inside
/// `<script>`, so an escaped payload is a syntax error and never runs.
const INLINE_SCRIPT: &str = r#"
document.addEventListener("DOMContentLoaded", () => {
  // The message lives in the session, not the URL, so there is nothing to
  // scrub here. It auto-dismisses; a refresh renders a clean page.
  const banner = document.querySelector(".flash-banner");
  if (banner) {
    setTimeout(() => {
      banner.classList.add("dismissing");
      setTimeout(() => banner.remove(), 400);
    }, 8000);
  }

  // Irreversible actions confirm before the request leaves the browser.
  // `data-confirmed` guards against a second submit re-prompting.
  document.querySelectorAll("form[data-confirm]").forEach((form) => {
    form.addEventListener("submit", (event) => {
      if (form.dataset.confirmed === "1") return;
      event.preventDefault();
      if (window.confirm(form.dataset.confirm)) {
        form.dataset.confirmed = "1";
        form.submit();
      }
    });
  });

  document.querySelectorAll("[data-clipboard]").forEach((btn) => {
    const announce = (msg) => {
      const live = document.getElementById("copy-status");
      if (live) live.textContent = msg;
    };
    btn.addEventListener("click", async () => {
      const value = btn.dataset.clipboard;
      // `navigator.clipboard` is undefined outside a secure context, and
      // accessing it there throws synchronously — which a trailing .catch()
      // cannot intercept. Guard the access itself.
      try {
        if (navigator.clipboard && window.isSecureContext) {
          await navigator.clipboard.writeText(value);
        } else {
          const field = document.getElementById("generated-token");
          if (!field) throw new Error("no fallback field");
          field.focus();
          field.select();
          field.setSelectionRange(0, value.length);
        }
        announce(btn.dataset.copied || "Copied");
        btn.classList.add("copied");
      } catch (_) {
        announce("Copy failed — select the token and copy manually");
      }
    });
  });
});
"#;

/// Page-local CSS, layered on top of `maud-ui.css`.
///
/// Rendered via `PreEscaped` for the same reason as `INLINE_SCRIPT`.
const INLINE_STYLE: &str = r#"                    .mui-card { margin-block: var(--mui-space-section); margin-inline: auto; }
                    .mui-card__body { padding: var(--mui-space-card); }
                    .mui-card__title { margin-bottom: 0.5rem; }
                    .mui-card__description { margin-top: 0.5rem; }
                    /* Vertical rhythm lives here, not in markup <br> tags.
                       The header collapses its own bottom padding, so the first
                       group needs its own leading space or it touches the
                       description. */
                    .card-fields {
                      display: flex;
                      flex-direction: column;
                      gap: 1.25rem;
                      margin-block-start: 1.5rem;
                    }
                    .card-fields > * { margin-block-start: 0; }
                    .mui-container { padding-inline: var(--mui-space-gutter); }
                    .ui-narrow { max-inline-size: 28rem; margin-inline: auto; }
                    .ui-measure { max-inline-size: 56rem; margin-inline: auto; }
                    .form-actions { margin-block-start: 0.5rem; }
                    .form-actions .mui-btn { inline-size: 100%; }
                    /* Same rhythm reason as `.card-fields`: the card header
                       collapses its own bottom padding, so the first element in
                       the body needs its own leading space or it touches the
                       description. */
                    .token-display {
                      margin-block-start: 1.5rem;
                      position: relative;
                      background: var(--mui-bg-input);
                      border: 1px solid var(--mui-border);
                      border-radius: var(--mui-radius-md);
                    }
                    .token-display__value {
                      display: block;
                      inline-size: 100%;
                      /* Room for the copy button; the field stays fully readable. */
                      padding-block: 2.75rem 1rem;
                      padding-inline: 1rem;
                      border: none;
                      background: transparent;
                      color: var(--mui-text);
                      font-family: var(--mui-font-mono);
                      font-size: 0.75rem;
                      line-height: 1.6;
                      overflow-wrap: anywhere;
                      overflow-y: auto;
                      resize: none;
                      /* Field-sizing lets it fit the whole credential instead of
                         clipping it behind a fixed rows="3". */
                      field-sizing: content;
                      min-block-size: 6lh;
                      max-block-size: 14lh;
                    }
                    .token-display__value:focus-visible {
                      outline: 2px solid var(--mui-border-focus);
                      outline-offset: -2px;
                      border-radius: var(--mui-radius-md);
                    }
                    .copy-btn {
                      position: absolute;
                      inset-block-start: 0.375rem;
                      inset-inline-end: 0.375rem;
                      display: inline-flex;
                      align-items: center;
                      justify-content: center;
                      padding: 0;
                      background: transparent;
                      border: none;
                      border-radius: var(--mui-radius-sm);
                      color: var(--mui-text-muted);
                      cursor: pointer;
                      inline-size: 1.75rem;
                      block-size: 1.75rem;
                    }
                    /* Hit area larger than the visual (WCAG 2.5.8). */
                    .copy-btn::after {
                      content: "";
                      position: absolute;
                      inset: min(0px, (100% - 44px) / 2);
                    }
                    .copy-btn svg { display: block; inline-size: 1rem; block-size: 1rem; }
                    @media (hover: hover) and (pointer: fine) {
                      .copy-btn:hover { color: var(--mui-text); background: var(--mui-bg-card); }
                    }
                    .copy-status { display: block; margin-top: 0.5rem; min-height: 1.25rem; font-size: var(--mui-text-xs); color: var(--mui-success-text, #4ade80); }
                    .copy-btn.copied { color: var(--mui-success-text, #4ade80); }
                    .error-alert,
                    .success-alert {
                      padding: 0.75rem 1rem;
                      border-radius: var(--mui-radius-md);
                      border: 1px solid;
                      font-size: 0.875rem;
                    }
                    .error-alert {
                      background: color-mix(in oklch, var(--mui-danger) 12%, transparent);
                      border-color: color-mix(in oklch, var(--mui-danger) 30%, transparent);
                      color: var(--mui-danger-text);
                    }
                    .success-alert {
                      background: color-mix(in oklch, var(--mui-success) 12%, transparent);
                      border-color: color-mix(in oklch, var(--mui-success) 30%, transparent);
                      color: var(--mui-success-text);
                    }
                    .flash-banner {
                      margin: 0 auto var(--mui-space-card);
                      /* Share the content measure so the banner and the card
                         share one left edge instead of floating wider. */
                      max-inline-size: min(28rem, 100%);
                      display: flex;
                      align-items: flex-start;
                      gap: 0.5rem;
                    }
                    @keyframes flashIn { from { opacity: 0; translate: 0 -0.5rem; } to { opacity: 1; translate: 0; } }
                    @media (prefers-reduced-motion: no-preference) {
                      .flash-banner { animation: flashIn 350ms var(--mui-ease-out); }
                      .flash-banner.dismissing { opacity: 0; transition: opacity 400ms var(--mui-ease-out); }
                      .copy-btn:active svg { scale: 0.85; }
                      .copy-btn svg { transition: scale 150ms var(--mui-ease-out); }
                    }
                    /* Top-bar nav: 2 items, brand left, links right.
                       Active state = 2px underline + weight + colour, all driven
                       off [aria-current="page"] so there is one source of truth.
                       44px min target satisfies WCAG 2.2 SC 2.5.5 outright. */
                    .nav-bar {
                      display: flex;
                      flex-wrap: wrap;
                      align-items: center;
                      gap: 0.25rem 1.5rem;
                      min-block-size: 3.5rem;
                      padding-inline: var(--mui-space-gutter);
                      border-block-end: 1px solid var(--mui-border);
                      background: var(--mui-bg);
                    }
                    .nav-bar .brand {
                      font-size: 1.0625rem;
                      font-weight: 600;
                      line-height: 1;
                      color: var(--mui-text);
                      text-decoration: none;
                    }
                    .nav-bar nav { margin-inline-start: auto; }
                    .nav-list {
                      display: flex;
                      flex-wrap: wrap;
                      align-items: center;
                      gap: 0.125rem;
                      margin: 0;
                      padding: 0;
                      list-style: none;
                    }
                    .nav-link {
                      position: relative;
                      display: inline-flex;
                      align-items: center;
                      min-block-size: 2.75rem;
                      padding-inline: 0.75rem;
                      border-radius: var(--mui-radius-sm);
                      font-size: 0.875rem;
                      line-height: 1;
                      color: var(--mui-text-muted);
                      text-decoration: none;
                      transition: color 120ms var(--mui-ease-out);
                    }
                    /* Indicator: absolutely positioned so it takes no layout
                       space, transparent by default, only opacity animates. */
                    .nav-link::after {
                      content: "";
                      position: absolute;
                      inset-inline: 0.5rem;
                      inset-block-end: 0.375rem;
                      block-size: 2px;
                      border-radius: 2px;
                      background: var(--mui-accent);
                      opacity: 0;
                      transition: opacity 120ms var(--mui-ease-out);
                    }
                    .nav-link[aria-current="page"] {
                      font-weight: 600;
                      color: var(--mui-text);
                    }
                    .nav-link[aria-current="page"]::after { opacity: 1; }
                    /* Pull the ring inward so it never fights the underline. */
                    .nav-link:focus-visible {
                      outline: 2px solid var(--mui-accent);
                      outline-offset: -2px;
                    }
                    @media (hover: hover) and (pointer: fine) {
                      .nav-link:hover { color: var(--mui-text); }
                    }
                    @media (prefers-reduced-motion: reduce) {
                      .nav-link,
                      .nav-link::after { transition: none; }
                    }
                    /* Two links never need a hamburger; just tighten up. */
                    @media (max-width: 30rem) {
                      .nav-bar { gap: 0.125rem 0.75rem; padding-inline: 0.75rem; }
                      .nav-link { padding-inline: 0.5rem; }
                    }
                    .mui-table-wrapper { width: 100%; overflow-x: auto; }
                    .mui-table { width: 100%; }
                    .mui-table .sort-link {
                      color: inherit;
                      text-decoration: none;
                      display: inline-flex;
                      align-items: center;
                      gap: 0.25rem;
                      padding-block: 0.25rem;
                    }
                    @media (hover: hover) and (pointer: fine) {
                      .mui-table .sort-link:hover { color: var(--mui-text); }
                    }
                    .sort-arrow { font-size: var(--mui-text-xs); line-height: 1; color: var(--mui-text-subtle); }
                    /* Status reads as a quiet tinted chip, not a saturated block.
                       Uses the *-text tokens, which exist for text on tint. */
                    .mui-badge--success {
                      background: color-mix(in oklch, var(--mui-success) 14%, transparent);
                      border-color: color-mix(in oklch, var(--mui-success) 32%, transparent);
                      color: var(--mui-success-text);
                    }
                    .mui-badge--danger {
                      background: color-mix(in oklch, var(--mui-danger) 14%, transparent);
                      border-color: color-mix(in oklch, var(--mui-danger) 32%, transparent);
                      color: var(--mui-danger-text);
                    }
                    /* Destructive button: a solid saturated block shouted at
                       every row. Keep the fill for affordance but calm it and let
                       the text carry more of the contrast load. */
                    .mui-btn--danger {
                      background: color-mix(in oklch, var(--mui-danger) 82%, var(--mui-bg-card));
                      border-color: color-mix(in oklch, var(--mui-danger) 90%, black);
                      color: var(--mui-accent-fg);
                    }
                    /* 8. A real empty state, reused for both zero-token cases. */
                    .empty-state {
                      display: grid;
                      justify-items: center;
                      gap: 0.5rem;
                      padding-block: 2.5rem 1.5rem;
                      text-align: center;
                    }
                    .empty-state__icon {
                      display: grid;
                      place-items: center;
                      inline-size: 2.5rem;
                      block-size: 2.5rem;
                      border-radius: var(--mui-radius-full);
                      background: var(--mui-bg-input);
                      color: var(--mui-text-muted);
                      margin-block-end: 0.25rem;
                    }
                    .empty-state__title { color: var(--mui-text); font-weight: 600; }
                    .empty-state__body {
                      color: var(--mui-text-muted);
                      font-size: 0.875rem;
                      line-height: 1.5;
                      max-inline-size: 28rem;
                      margin: 0;
                    }
                    .empty-state__action { margin-block-start: 0.75rem; }
                    .token-empty { text-align: center; padding-block: 2rem 0.5rem; }
                    .token-empty__title { color: var(--mui-text); font-weight: 600; margin: 0 0 0.375rem; }
                    .token-empty__body { color: var(--mui-text-muted); font-size: 0.875rem; margin: 0 0 1.25rem; }
                    .token-empty__link { color: var(--mui-accent-text); font-size: 0.875rem; }
                    /* Action links styled as buttons: <a> must not wrap <button>. */
                    .btn-reset,
                    .btn-primary {
                      display: inline-flex;
                      align-items: center;
                      justify-content: center;
                      block-size: 2.25rem;
                      padding-inline: 1rem;
                      border-radius: var(--mui-radius-md);
                      font-family: var(--mui-font-sans);
                      font-size: 0.875rem;
                      font-weight: 500;
                      text-decoration: none;
                    }
                    .btn-reset { background: var(--mui-bg-input); border: 1px solid var(--mui-border); color: var(--mui-text); }
                    .btn-primary { background: var(--mui-accent); border: 1px solid var(--mui-accent); color: var(--mui-accent-fg); }
                    @media (hover: hover) and (pointer: fine) {
                      .btn-reset:hover { background: var(--mui-bg-card); border-color: var(--mui-border-hover); }
                      .btn-primary:hover { background: var(--mui-accent-hover); border-color: var(--mui-accent-hover); }
                    }
                    .ttl-select {
                      inline-size: 100%;
                      padding-block: 0.5rem;
                      padding-inline: 0.75rem;
                      border: 1px solid var(--mui-border);
                      border-radius: var(--mui-radius-md);
                      background: var(--mui-bg);
                      color: var(--mui-text);
                      font-size: 0.875rem;
                    }
                    /* Safari zooms the viewport when a focused field is under
                       16px. Only touch devices need the bump. */
                    @media (pointer: coarse) {
                      .mui-input, .ttl-select { font-size: max(16px, 0.875rem); }
                    }
                    .field-hint { font-weight: 400; color: var(--mui-text-muted); font-size: var(--mui-text-xs); margin-inline-start: 0.5rem; }
                    .visually-hidden {
                      position: absolute;
                      inline-size: 1px;
                      block-size: 1px;
                      padding: 0;
                      margin: -1px;
                      overflow: clip;
                      clip-path: inset(50%);
                      white-space: nowrap;
                      border: 0;
                    }
                    .token-actions {
                      display: flex;
                      flex-wrap: wrap;
                      gap: 0.5rem;
                      margin-block-start: 1.25rem;
                    }
                    .token-actions > * { flex: 1 1 auto; }
                    .token-actions .mui-btn { inline-size: 100%; }
                    .token-info-grid { display: grid; grid-template-columns: auto 1fr; gap: 0.5rem 1rem; margin-top: 1rem; font-size: 0.875rem; }
                    .token-info-grid dt { color: var(--mui-text-muted); text-transform: uppercase; letter-spacing: 0.05em; font-size: 0.75rem; }
                    .token-info-grid dd { margin: 0; }"#;
const DEFAULT_TOKEN_TTL_DAYS: u64 = 30;
const MAX_TOKEN_TTL_DAYS: u64 = 3650;
const SECONDS_PER_DAY: u64 = 86_400;
const SESSION_COOKIE_NAME: &str = "mcp_session";
const SESSION_COOKIE_MAX_AGE_SECONDS: u64 = 3600;
const MAX_SESSIONS: usize = 1000;

// ── Session types ────────────────────────────────────────────────────────

/// A one-shot message carried across a POST→redirect→GET.
///
/// Stored server-side in the session, so the URL stays clean and the text
/// never lands in browser history, a `Referer` header, or a proxy log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flash {
    pub kind: FlashKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashKind {
    Success,
    Error,
}

#[derive(Clone, Debug)]
pub struct SessionData {
    pub csrf_token: String,
    pub athlete_id: Option<String>,
    /// Last time this session was used. Refreshed on every request so the
    /// server-side lifetime slides in step with the cookie's `Max-Age`.
    created_at: Instant,
    pub flash: Option<Flash>,
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
    /// Shared revocation set — same instance as in AppState.
    pub revoked_jtis: TokenRevocationSet,
    /// Whether to set the `Secure` flag on session cookies.
    /// `false` when running behind a TLS-terminating reverse proxy or on plain HTTP.
    pub cookie_secure: bool,
    /// Public reverse-proxy path prefix, already normalized via
    /// `crate::normalize_base_path`: `""` (root) or `"/intervals"`.
    /// Used to prefix UI links, redirects, and the session cookie `Path`.
    pub base_path: String,
}

impl UiState {
    pub fn new(
        app_state: Arc<AppState>,
        revoked_jtis: TokenRevocationSet,
        registry_path: Option<PathBuf>,
        cookie_secure: bool,
        base_path: String,
    ) -> Self {
        // Defense in depth: re-normalize so callers cannot pass a raw env
        // value ("/intervals/", "intervals", "//x") into URL/cookie emission.
        let base_path = crate::normalize_base_path(&base_path);
        let tokens = Arc::new(RwLock::new(
            registry_path
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
        ));
        // Pre-populate revocation set from existing registry
        if let Some(path) = registry_path.as_ref()
            && let Ok(content) = std::fs::read_to_string(path)
            && let Ok(records) = serde_json::from_str::<Vec<TokenRecord>>(&content)
        {
            let mut guard = revoked_jtis.blocking_write();
            for record in &records {
                if record.revoked {
                    guard.insert(record.jti.clone());
                }
            }
        }
        Self {
            app_state,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            tokens,
            registry_path,
            revoked_jtis,
            cookie_secure,
            base_path,
        }
    }

    /// Prefix a logical app path (e.g. `"/ui/tokens"`) with the public base path.
    pub fn ui_url(&self, path: &str) -> String {
        format!("{}{}", self.base_path, path)
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
            // The cookie is re-issued with a fresh Max-Age on every response, so
            // the server must slide the expiry too. Without this the server
            // evicts an actively-used session while the browser still believes
            // it is valid, and the next POST fails its CSRF check.
            if let Some(session) = self.sessions.write().await.get_mut(&session_id) {
                session.created_at = Instant::now();
            }
            return session_id;
        }

        self.evict_stale_sessions().await;

        let session_id = generate_session_id();
        let csrf = generate_csrf_token();
        self.sessions.write().await.insert(
            session_id.clone(),
            SessionData {
                csrf_token: csrf,
                athlete_id: None,
                created_at: Instant::now(),
                flash: None,
            },
        );
        session_id
    }

    /// Remove sessions unused for longer than the cookie lifetime.
    ///
    /// The window is measured from `created_at`, which slides forward on every
    /// request. That matters: the cookie is re-issued with a fresh `Max-Age` on
    /// every response, so a server measuring from creation would silently expire
    /// an actively-used session while the browser still considered it live — and
    /// the operator's next POST would fail its CSRF check for no visible reason.
    async fn evict_stale_sessions(&self) {
        let mut sessions = self.sessions.write().await;
        // Remove sessions older than the cookie max age.
        let cutoff =
            Instant::now() - std::time::Duration::from_secs(SESSION_COOKIE_MAX_AGE_SECONDS);
        sessions.retain(|_, s| s.created_at > cutoff);

        // If still over capacity, remove the oldest entries.
        if sessions.len() > MAX_SESSIONS {
            let mut entries: Vec<(String, Instant)> = sessions
                .iter()
                .map(|(k, v)| (k.clone(), v.created_at))
                .collect();
            entries.sort_by_key(|(_, t)| *t);
            let to_remove: usize = sessions.len() - MAX_SESSIONS;
            for (key, _) in entries.iter().take(to_remove) {
                sessions.remove(key);
            }
        }
    }

    async fn session(&self, session_id: &str) -> Option<SessionData> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Queue a message for the next render of this session.
    pub async fn set_flash(&self, session_id: &str, flash: Flash) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.flash = Some(flash);
        }
    }

    /// Read and clear the pending message. One-shot: a refresh must not replay it.
    pub async fn take_flash(&self, session_id: &str) -> Option<Flash> {
        self.sessions
            .write()
            .await
            .get_mut(session_id)
            .and_then(|session| session.flash.take())
    }

    /// Test seam: resolve the session a set of headers implies,
    /// returning `(session_id, csrf_token)`.
    #[doc(hidden)]
    pub async fn session_for_test(&self, headers: &HeaderMap) -> (String, String) {
        let ctx = self.session_context(headers).await;
        (ctx.session_id, ctx.csrf_token)
    }

    /// Test seam: when the session was created.
    #[doc(hidden)]
    pub async fn session_created(&self, session_id: &str) -> Instant {
        self.sessions
            .read()
            .await
            .get(session_id)
            .map(|s| s.created_at)
            .unwrap_or_else(Instant::now)
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
            // Also add to the shared revocation set so middleware can reject it.
            self.revoked_jtis.write().await.insert(jti.to_string());
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

/// The served stylesheet.
///
/// Exposed so tests can assert on it without re-reading the file.
pub fn served_css() -> &'static str {
    MAUD_UI_CSS
}

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

fn page_shell(title: &str, page: &str, body: Markup, flash: Option<Flash>, base: &str) -> Markup {
    fn nav_link(label: &str, path: &str, current: &str, base: &str) -> Markup {
        let href = format!("{base}{path}");
        // `aria-current` is the single source of truth for the active state, so
        // the visual highlight and the announced state can never disagree.
        if href == current {
            html! {
                li {
                    a.nav-link href=(href) aria-current="page" { (label) }
                }
            }
        } else {
            html! {
                li {
                    a.nav-link href=(href) { (label) }
                }
            }
        }
    }
    let current = format!("{base}{page}");
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                // Tell the UA to paint the dark surface before CSS loads, and
                // let the OS light/dark preference reach the theme tokens.
                meta name="color-scheme" content="dark light";
                link rel="icon" href="data:,";
                title { (title) " — Intervals.icu MCP" }
                link rel="stylesheet" href=(format!("{base}/ui/static/css"));
                // `PreEscaped`: browsers do not decode HTML entities inside
                // <script>/<style>, so escaping here would ship a syntax error
                // and the flash banner's query string would survive a refresh.
                script { (PreEscaped(INLINE_SCRIPT)) }
                style { (PreEscaped(INLINE_STYLE)) }
            }
            body {
                header.nav-bar {
                    a.brand href=(format!("{base}/ui")) { "Intervals.icu MCP" }
                    nav aria-label="Primary" {
                        ul.nav-list {
                            (nav_link("Home", "/ui", &current, base))
                            (nav_link("Tokens", "/ui/tokens", &current, base))
                        }
                    }
                }
                main #main {
                    h1.visually-hidden { (title) }
                    @if let Some(ref msg) = flash {
                        @match msg.kind {
                            FlashKind::Error => .flash-banner.error-alert role="alert" { (msg.message.clone()) },
                            FlashKind::Success => .flash-banner.success-alert role="status" aria-live="polite" { (msg.message.clone()) },
                        }
                    }
                    .mui-container {
                        (body)
                    }
                }
            }
        }
    }
}

// ── Landing page (GET /ui) ───────────────────────────────────────────────

/// The home page takes no query parameters any more.
///
/// Flash messages live in the session and are consumed once by `page_shell`,
/// so a reload renders a clean page instead of replaying the old message.
#[derive(Deserialize, Default)]
pub struct UiQuery {}

fn render_home_body(csrf: &str, base: &str) -> Markup {
    use maud_ui::primitives::{button, card, field, input};

    html! {
        div.ui-narrow {
            (card::render(card::Props {
                title: Some("MCP Server Token".into()),
                description: Some("Enter your Intervals.icu credentials to generate an API token".into()),
                children: html! {
                    form method="POST" action=(format!("{base}/ui/token")) {
                        input type="hidden" name="_csrf" value=(csrf);
                        div.card-fields {
                        (field::render(field::Props {
                            label: "Athlete ID".into(),
                            id: "athlete_id".into(),
                            required: true,
                            children: html! {
                                // `id` must match the label's `for`; without it the
                                // control has no accessible name (WCAG 4.1.2).
                                (input::render(input::Props {
                                    id: "athlete_id".into(),
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
                                // `id` must match the label's `for`; without it the
                                // control has no accessible name (WCAG 4.1.2).
                                (input::render(input::Props {
                                    id: "api_key".into(),
                                    name: "api_key".into(),
                                    input_type: input::InputType::Password,
                                    placeholder: "Your Intervals.icu API key".into(),
                                    required: true,
                                    ..Default::default()
                                }))
                            },
                            ..Default::default()
                        }))
                        div.mui-field {
                            label.mui-field__label for="ttl_days" {
                                "Token TTL"
                                span.field-hint { "How long the token will be valid." }
                            }
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
                        }
                        div.form-actions {
                            (button::render(button::Props {
                                label: "Generate Token".into(),
                                button_type: "submit",
                                variant: button::Variant::Primary,
                                ..Default::default()
                            }))
                        }
                        }
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
    let _ = &query; // reserved for future non-flash query params
    let html = page_shell(
        "Token Setup",
        "/ui",
        render_home_body(&session.csrf_token, &ui.base_path),
        ui.take_flash(&session.session_id).await,
        &ui.base_path,
    );
    let mut resp = html.into_response();
    set_session_cookie(
        &mut resp,
        &session.session_id,
        ui.cookie_secure,
        &ui.base_path,
    );
    resp
}

fn set_session_cookie(
    resp: &mut axum::response::Response,
    session_id: &str,
    cookie_secure: bool,
    base_path: &str,
) {
    use axum::http::header::SET_COOKIE;
    let secure = if cookie_secure { "; Secure" } else { "" };
    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Strict; Path={}/ui; Max-Age={}{secure}",
        SESSION_COOKIE_NAME, session_id, base_path, SESSION_COOKIE_MAX_AGE_SECONDS
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

/// Redirect to `path` carrying a one-shot message in the session.
///
/// The message never appears in the URL: no copy-pasteable error text, nothing
/// in browser history, and nothing leaked into an outbound `Referer` header.
async fn redirect_with_flash(ui: &UiState, session_id: &str, path: &str, flash: Flash) -> Response {
    ui.set_flash(session_id, flash).await;
    redirect_with_session(
        &ui.ui_url(path),
        session_id,
        ui.cookie_secure,
        &ui.base_path,
    )
}

fn redirect_with_session(
    url: &str,
    session_id: &str,
    cookie_secure: bool,
    base_path: &str,
) -> Response {
    let mut resp = Redirect::to(url).into_response();
    set_session_cookie(&mut resp, session_id, cookie_secure, base_path);
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
        return redirect_with_flash(
            &ui,
            &session.session_id,
            "/ui",
            Flash {
                kind: FlashKind::Error,
                message: "Your session has expired. Reload the page and try again.".into(),
            },
        )
        .await;
    }

    let Some(token_request) = TokenRequestData::from_form(form) else {
        return redirect_with_flash(
            &ui,
            &session.session_id,
            "/ui",
            Flash {
                kind: FlashKind::Error,
                message: "Enter both your athlete ID and API key.".into(),
            },
        )
        .await;
    };

    let client = match intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &ui.app_state.base_url,
        token_request.athlete_id.clone(),
        secrecy::SecretString::new(token_request.api_key.clone().into()),
    ) {
        Ok(c) => c,
        Err(_e) => {
            return redirect_with_flash(
                &ui,
                &session.session_id,
                "/ui",
                Flash {
                    kind: FlashKind::Error,
                    message: "Could not reach Intervals.icu. Check your connection and try again."
                        .into(),
                },
            )
            .await;
        }
    };

    let ttl_seconds = token_request.ttl_seconds();

    match client.get_athlete_profile().await {
        Ok(_) => {
            let jti = Uuid::new_v4().to_string();
            match ui.app_state.jwt_manager.issue_token(
                &token_request.athlete_id,
                &token_request.api_key,
                ttl_seconds,
                Some(&jti),
            ) {
                Ok(token) => {
                    let now = chrono::Utc::now();
                    let expires = now + chrono::TimeDelta::seconds(ttl_seconds as i64);
                    let expires_at = expires.to_rfc3339();
                    let expiry_formatted = format_datetime(&expires_at);

                    ui.record_token(TokenRecord {
                        jti: jti.clone(),
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
                        jti = %jti,
                        ui_action = "token_created",
                        "Token created via web UI"
                    );

                    let body = render_token_success(
                        &token,
                        &token_request.athlete_id,
                        &expiry_formatted,
                        &token_request.ttl_label(),
                        &ui.base_path,
                    );
                    let html = page_shell("Token Generated", "/ui", body, None, &ui.base_path);
                    let mut resp = html.into_response();
                    set_session_cookie(
                        &mut resp,
                        &session.session_id,
                        ui.cookie_secure,
                        &ui.base_path,
                    );
                    resp
                }
                Err(e) => {
                    // Keep the internal error out of the URL: it becomes page
                    // text, browser history, and an outbound Referer header.
                    tracing::warn!(%e, "token generation failed in UI");
                    redirect_with_flash(
                        &ui,
                        &session.session_id,
                        "/ui",
                        Flash {
                            kind: FlashKind::Error,
                            message: "Could not create the token. Please try again.".into(),
                        },
                    )
                    .await
                }
            }
        }
        Err(_) => redirect_with_flash(
            &ui,
            &session.session_id,
            "/ui",
            Flash {
                kind: FlashKind::Error,
                message:
                    "Intervals.icu rejected your athlete ID or API key. Check both and try again."
                        .into(),
            },
        )
        .await,
    }
}

fn format_datetime(rfc3339: &str) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rfc3339) {
        dt.format("%d %b %Y %H:%M").to_string()
    } else {
        rfc3339.to_string()
    }
}

/// Test seam: the page-local stylesheet, so visual tests measure real CSS
/// instead of a hand-copied copy that can silently drift.
#[doc(hidden)]
pub fn inline_style_for_test() -> &'static str {
    INLINE_STYLE
}

/// Test seam: render the post-creation page without needing network access.
#[doc(hidden)]
pub fn render_token_success_for_test(
    token: &str,
    athlete_id: &str,
    expiry_formatted: &str,
    ttl_label: &str,
    base: &str,
) -> String {
    render_token_success(token, athlete_id, expiry_formatted, ttl_label, base).into_string()
}

fn render_token_success(
    token: &str,
    athlete_id: &str,
    expiry_formatted: &str,
    ttl_label: &str,
    base: &str,
) -> Markup {
    use maud_ui::primitives::card;

    let card_content = card::render(card::Props {
        title: Some("Token Generated".into()),
        description: Some("Copy this token now — you won't see it again.".into()),
        children: html! {
            // `readonly` textarea: the token stays selectable and gets native
            // select-all + long-press-to-copy, which is the only copy path that
            // works outside a secure context (plain-HTTP self-hosting).
            div class="token-display" {
                textarea #generated-token class="token-display__value" readonly spellcheck="false" {
                    (token)
                }
                button.copy-btn type="button" data-clipboard=(token) aria-label="Copy token to clipboard" {
                    svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false" {
                        rect x="8" y="2" width="8" height="4" rx="1" ry="1" {}
                        path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" {}
                    }
                    span.visually-hidden { "Copy token" }
                }
            }
            span #copy-status.copy-status role="status" aria-live="polite" {}
            dl.token-info-grid {
                dt { "Athlete ID" }
                dd { (athlete_id) }
                dt { "TTL" }
                dd { (ttl_label) }
                dt { "Expires" }
                dd { (expiry_formatted) }
            }
            div.token-actions {
                a.btn-reset href=(format!("{base}/ui")) { "Back to Home" }
                a.btn-primary href=(format!("{base}/ui/tokens")) { "View All Tokens" }
            }
        },
        ..Default::default()
    });
    html! {
        div.ui-narrow {
            (card_content)
        }
    }
}

// ── Token listing (GET /ui/tokens) ───────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct TokenListQuery {
    pub sort: Option<String>,
    pub order: Option<String>,
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

    // The handler already defaults to issued/asc. Surface that default in the
    // header, otherwise the rendered order contradicts the visible affordance.
    let effective_sort = query.sort.clone().unwrap_or_else(|| "issued".to_string());
    let effective_order = query.order.clone().unwrap_or_else(|| "asc".to_string());

    let body = render_token_list(
        &sorted,
        &session.csrf_token,
        &Some(effective_sort),
        &Some(effective_order),
        &ui.base_path,
        !current_athlete.is_empty(),
    );
    let html = page_shell(
        "Token Management",
        "/ui/tokens",
        body,
        ui.take_flash(&session.session_id).await,
        &ui.base_path,
    );
    let mut resp = html.into_response();
    set_session_cookie(
        &mut resp,
        &session.session_id,
        ui.cookie_secure,
        &ui.base_path,
    );
    resp
}

fn sort_href(
    sort_field: &str,
    current_sort: &Option<String>,
    current_order: &Option<String>,
    base: &str,
) -> String {
    let is_current = current_sort.as_deref() == Some(sort_field);
    let new_order = if is_current && current_order.as_deref() != Some("desc") {
        "desc"
    } else {
        "asc"
    };
    format!("{base}/ui/tokens?sort={sort_field}&order={new_order}")
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
    base: &str,
    session_known: bool,
) -> Markup {
    use maud_ui::primitives::{badge, button, card};

    fn th_link(
        label: &str,
        field: &str,
        current_sort: &Option<String>,
        current_order: &Option<String>,
        base: &str,
    ) -> Markup {
        let arrow = sort_arrow(field, current_sort, current_order);
        let active = current_sort.as_deref() == Some(field);
        let dir = match current_order.as_deref() {
            Some("desc") if active => "descending",
            Some(_) if active => "ascending",
            _ => "none",
        };
        html! {
            th.mui-table__th scope="col" aria-sort=(dir) {
                a.sort-link href=(sort_href(field, current_sort, current_order, base)) {
                    (label)
                    span.sort-arrow aria-hidden="true" { (arrow) }
                    span.visually-hidden {
                        @if active {
                            " (sorted)"
                        } @else {
                            " (sort)"
                        }
                    }
                }
            }
        }
    }

    let table_card = card::render(card::Props {
        title: Some("Active Tokens".into()),
        description: Some("Manage tokens issued through this interface.".into()),
        children: html! {
            @if tokens.is_empty() {
                // Two genuinely different situations. Saying "No tokens yet" when
                // we simply lost the session is a false statement about the
                // operator's security posture, and its only call to action pushes
                // them to mint a duplicate credential.
                div.empty-state {
                    span.empty-state__icon aria-hidden="true" {
                        svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"
                            viewBox="0 0 24 24" fill="none" stroke="currentColor"
                            stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" {
                            @if session_known {
                                path d="M20 13a8 8 0 1 1-3.6-6.7" {}
                                polyline points="9 11 12 14 22 4" {}
                            } @else {
                                circle cx="12" cy="12" r="9" {}
                                polyline points="12 8 12 12 12 16" {}
                            }
                        }
                    }
                    @if session_known {
                        p.empty-state__title { "No tokens yet" }
                        p.empty-state__body {
                            "You have not created an API token through this interface yet."
                        }
                        div.empty-state__action {
                            (button::render(button::Props {
                                label: "Create a Token".into(),
                                variant: button::Variant::Primary,
                                ..Default::default()
                            }))
                        }
                    } @else {
                        p.empty-state__title { "No athlete in this session" }
                        p.empty-state__body {
                            "This browser session isn't linked to an athlete, so we \
                             can't tell which tokens belong to it. Tokens you \
                             created earlier are still valid."
                        }
                        div.empty-state__action {
                            a class="btn-primary" href=(format!("{base}/ui")) {
                                "Create a Token"
                            }
                        }
                    }
                }
            } @else {
                div.mui-table-wrapper {
                    table.mui-table {
                        caption.visually-hidden { "Tokens issued through this interface." }
                        thead {
                            tr {
                                (th_link("Athlete ID", "athlete", current_sort, current_order, base))
                                (th_link("Issued", "issued", current_sort, current_order, base))
                                (th_link("Expires", "expires", current_sort, current_order, base))
                                (th_link("Status", "status", current_sort, current_order, base))
                                th.mui-table__th scope="col" { "Action" }
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
                                            // Irreversible: revoking breaks every MCP
                                            // client using this token. Confirm before
                                            // acting rather than after.
                                            form method="POST"
                                                action=(format!("{base}/ui/revoke/{}", t.jti))
                                                data-confirm="Revoke this token?\n\nAny MCP client using it will stop working immediately. This cannot be undone."
                                            {
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
    });
    html! {
        div.ui-measure { (table_card) }
    }
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
        return redirect_with_flash(
            &ui,
            &session.session_id,
            "/ui/tokens",
            Flash {
                kind: FlashKind::Error,
                message: "Your session has expired. Reload the page and try again.".into(),
            },
        )
        .await;
    }

    ui.revoke_token(&jti).await;

    crate::metrics::record_ui_action("token_revoked");
    tracing::info!(
        jti = %jti,
        ui_action = "token_revoked",
        "Token revoked via web UI"
    );

    redirect_with_flash(
        &ui,
        &session.session_id,
        "/ui/tokens",
        Flash {
            kind: FlashKind::Success,
            message: "Token revoked. Clients using it will stop working.".into(),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn test_ui_state() -> UiState {
        use std::collections::HashSet;
        let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
        let jwt_manager = Arc::new(crate::auth::JwtManager::new(secret, [0u8; 32]));
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
        let jwt_manager = Arc::new(crate::auth::JwtManager::new(secret, [0u8; 32]));
        let revoked_jtis = Arc::new(tokio::sync::RwLock::new(HashSet::new()));
        let app_state = Arc::new(AppState {
            jwt_manager,
            jwt_ttl_seconds: 3600,
            base_url: "https://intervals.icu".to_string(),
            revoked_jtis: revoked_jtis.clone(),
        });

        UiState::new(app_state, revoked_jtis, None, false, base.to_string())
    }

    #[test]
    fn auth_ui_ui_url_prefixes_paths() {
        let ui = test_ui_state_with_base("/intervals");
        assert_eq!(ui.ui_url("/ui"), "/intervals/ui");
        assert_eq!(
            ui.ui_url("/ui/tokens?sort=issued&order=asc"),
            "/intervals/ui/tokens?sort=issued&order=asc"
        );
    }

    #[test]
    fn auth_ui_ui_url_root_is_identity() {
        let ui = test_ui_state();
        assert_eq!(ui.ui_url("/ui"), "/ui");
    }

    #[test]
    fn auth_ui_session_cookie_path_scoped_under_base() {
        let mut resp = axum::response::Response::new(axum::body::Body::empty());
        set_session_cookie(&mut resp, "sid", false, "/intervals");
        let cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cookie.contains("Path=/intervals/ui"), "cookie={cookie}");
    }

    #[test]
    fn auth_ui_session_cookie_path_root_unchanged() {
        let mut resp = axum::response::Response::new(axum::body::Body::empty());
        set_session_cookie(&mut resp, "sid", false, "");
        let cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        // Trailing ';' matters: a bare `contains("Path=/ui")` would also match
        // a wrongly-scoped `Path=/intervals/ui`.
        assert!(cookie.contains("Path=/ui;"), "cookie={cookie}");
    }

    #[test]
    fn auth_ui_new_normalizes_base_path() {
        let ui = test_ui_state_with_base("/intervals/");
        assert_eq!(ui.base_path, "/intervals");
        assert_eq!(ui.ui_url("/ui"), "/intervals/ui");
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
