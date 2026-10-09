//! JWT Authentication and Encryption Module
//!
//! # Token model
//!
//! A JWT is proof of identity, not a secret carrier: its custom claims hold the
//! `athlete_id` and a `jti`, never the upstream credential. The credential and
//! the revocation state live in the server-side [`crate::token_registry`], keyed
//! by `jti`; [`AppState::authenticate`] joins the two. Every issued token — via
//! `POST /auth` or the web UI — is recorded, so every token is revocable and the
//! upstream key can be rotated without re-issuing. See ADR-0011.
//!
//! Legacy tokens (issued before ADR-0011) embedded the encrypted credential in
//! their claims and carried no `jti`; they remain verifiable through the
//! claims-fallback path until they expire.

use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hkdf::Hkdf;
use intervals_icu_client::IntervalsClient;
use jwt_simple::prelude::*;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

use crate::metrics;
use crate::token_registry::{TokenRecord, TokenRegistry};

#[cfg(test)]
use secrecy::ExposeSecret;

/// Generate random bytes using OS CSPRNG
fn fill_random<const N: usize>() -> Result<[u8; N], AuthError> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|_| AuthError::EncryptionError)?;
    Ok(buf)
}

/// Custom claims carried by a token.
///
/// Only what is genuinely custom lives here: a **legacy** token's embedded
/// credential. Identity and the token id use the registered `sub` and `jti`
/// claims (`jti` via `with_jwt_id`), because jwt-simple `#[serde(flatten)]`s
/// custom claims into the same object as the registered ones — a custom `jti`
/// field would collide with the registered `jti` and be dropped on read.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntervalsClaims {
    pub athlete_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_api_key: Option<String>,
}

/// The claims this server relies on after verification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedClaims {
    pub athlete_id: String,
    /// Registered `jti` claim; identifies the registry record.
    pub jti: Option<String>,
    /// Legacy embedded credential (see ADR-0011); `None` for new tokens.
    pub encrypted_api_key: Option<String>,
}

/// Decrypted credentials after JWT verification
#[derive(Clone, Debug)]
pub struct DecryptedCredentials {
    pub athlete_id: String,
    pub api_key: SecretString,
}

/// Base URL injected into HTTP requests so rmcp handlers can build per-request clients.
#[derive(Clone, Debug)]
pub struct HttpBaseUrl(pub String);

/// JWT Manager
pub struct JwtManager {
    signing_key: HS256Key,
    encryption_key: [u8; 32],
    pub issuer: String,
    pub audience: String,
}

/// Application state for HTTP multi-tenant mode
#[derive(Clone)]
pub struct AppState {
    pub jwt_manager: Arc<JwtManager>,
    pub jwt_ttl_seconds: u64,
    pub base_url: String,
    /// Authoritative record of issued tokens, keyed by `jti`. Holds the
    /// revocation state and the encrypted upstream credential that the JWT
    /// deliberately does not carry. Shared with the web UI.
    pub tokens: TokenRegistry,
}

impl AppState {
    /// Resolve a bearer token to its athlete and upstream credential.
    ///
    /// The registry is authoritative: a revoked record rejects the token
    /// unconditionally, and an unknown `jti` is rejected unless the token
    /// carries a legacy embedded credential (the backward-compatibility path).
    pub async fn authenticate(&self, token: &str) -> Result<DecryptedCredentials, AuthError> {
        let claims = self.jwt_manager.verify_claims(token)?;

        let record = match claims.jti.as_deref() {
            Some(jti) => self.tokens.find(jti).await,
            None => None,
        };

        if let Some(record) = &record
            && record.revoked
        {
            tracing::warn!(
                athlete_id = %record.athlete_id,
                jti = %record.jti,
                "Rejected revoked token"
            );
            return Err(AuthError::TokenRevoked);
        }

        // Prefer the stored credential; fall back to one embedded in the claims
        // (legacy tokens, or a registry record written before ADR-0011 whose
        // credential still travels in the token).
        let encrypted = record
            .as_ref()
            .map(|record| record.encrypted_api_key.as_str())
            .filter(|stored| !stored.is_empty())
            .map(str::to_owned)
            .or_else(|| claims.encrypted_api_key.clone())
            .ok_or(AuthError::InvalidToken)?;

        let api_key = self.jwt_manager.decrypt_credential(&encrypted)?;

        Ok(DecryptedCredentials {
            athlete_id: claims.athlete_id,
            api_key,
        })
    }
}

/// Master key configuration with HKDF-derived keys
#[derive(Debug)]
pub struct MasterKeyConfig {
    pub signing_key: [u8; 32],
    pub encryption_key: [u8; 32],
}

impl MasterKeyConfig {
    pub fn from_hex(hex_str: &str) -> Result<Self, AuthError> {
        let master_key = hex::decode(hex_str).map_err(|_| AuthError::InvalidKeyFormat)?;

        if master_key.len() != 64 {
            return Err(AuthError::InvalidKeyLength);
        }

        let hk = Hkdf::<Sha256>::new(None, &master_key);
        let mut signing_key = [0u8; 32];
        let mut encryption_key = [0u8; 32];

        hk.expand(b"intervals-mcp-signing", &mut signing_key)
            .map_err(|_| AuthError::KeyDerivationError)?;
        hk.expand(b"intervals-mcp-encryption", &mut encryption_key)
            .map_err(|_| AuthError::KeyDerivationError)?;

        Ok(Self {
            signing_key,
            encryption_key,
        })
    }
}

/// Authentication errors
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Missing credentials")]
    MissingCredentials,
    #[error("Invalid token")]
    InvalidToken,
    #[error("Token expired")]
    TokenExpired,
    #[error("Encryption error")]
    EncryptionError,
    #[error("Server configuration error")]
    ServerConfig,
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("Invalid key format")]
    InvalidKeyFormat,
    #[error("Invalid key length")]
    InvalidKeyLength,
    #[error("Key derivation error")]
    KeyDerivationError,
    #[error("Token revoked")]
    TokenRevoked,
    #[error("JWT error: {0}")]
    JwtError(#[from] jwt_simple::Error),
}

/// Classify a `jwt_simple` verification failure as an expiration by matching
/// the typed `JWTError` in the error chain instead of rendered message text.
fn is_expired_jwt_error(error: &jwt_simple::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<jwt_simple::JWTError>(),
            Some(jwt_simple::JWTError::TokenHasExpired)
        )
    })
}

impl JwtManager {
    pub fn new(jwt_secret: &[u8], encryption_key: [u8; 32]) -> Self {
        Self {
            signing_key: HS256Key::from_bytes(jwt_secret),
            encryption_key,
            issuer: "intervals-icu-mcp".to_string(),
            audience: "intervals-icu-mcp".to_string(),
        }
    }

    pub fn from_master_key(config: &MasterKeyConfig) -> Self {
        Self {
            signing_key: HS256Key::from_bytes(&config.signing_key),
            encryption_key: config.encryption_key,
            issuer: "intervals-icu-mcp".to_string(),
            audience: "intervals-icu-mcp".to_string(),
        }
    }

    /// Issue a bearer token for `athlete_id`.
    ///
    /// The token carries identity only — `athlete_id` and the `jti` that keys
    /// its registry record — plus the standard registered claims. The upstream
    /// credential is *not* embedded; the caller stores it in the
    /// [`TokenRegistry`] under the same `jti`. See ADR-0011.
    pub fn issue_token(
        &self,
        athlete_id: &str,
        ttl_secs: u64,
        jti: &str,
    ) -> Result<String, AuthError> {
        let custom = IntervalsClaims {
            athlete_id: athlete_id.to_string(),
            encrypted_api_key: None,
        };

        let claims = Claims::with_custom_claims(custom, Duration::from_secs(ttl_secs))
            .with_issuer(&self.issuer)
            .with_audience(self.audience.clone())
            .with_subject(athlete_id.to_string())
            .with_jwt_id(jti.to_string());

        self.signing_key
            .authenticate(claims)
            .map_err(|_| AuthError::EncryptionError)
    }

    /// Verify signature, clock, issuer, and audience; return the claims.
    ///
    /// Credential resolution is the caller's job (the registry, keyed by `jti`),
    /// so this stays a pure token-in/claims-out operation.
    pub fn verify_claims(&self, token: &str) -> Result<VerifiedClaims, AuthError> {
        let verification_options = VerificationOptions {
            allowed_issuers: Some(HashSet::from([self.issuer.clone()])),
            allowed_audiences: Some(HashSet::from([self.audience.clone()])),
            required_signature_type: Some("JWT".to_string()),
            ..Default::default()
        };

        self.signing_key
            .verify_token::<IntervalsClaims>(token, Some(verification_options))
            .map(|claims| VerifiedClaims {
                athlete_id: claims.custom.athlete_id,
                jti: claims.jwt_id,
                encrypted_api_key: claims.custom.encrypted_api_key,
            })
            .map_err(|e| {
                if is_expired_jwt_error(&e) {
                    AuthError::TokenExpired
                } else {
                    AuthError::InvalidToken
                }
            })
    }

    /// Encrypt an upstream credential for server-side storage in the registry.
    pub fn encrypt_credential(&self, api_key: &str) -> Result<String, AuthError> {
        self.encrypt_api_key(api_key)
    }

    /// Decrypt a credential produced by [`Self::encrypt_credential`] — or
    /// embedded in a legacy token's claims.
    pub fn decrypt_credential(&self, encrypted: &str) -> Result<SecretString, AuthError> {
        self.decrypt_api_key(encrypted)
            .map(|api_key| SecretString::new(api_key.into()))
    }

    /// Sign a legacy-shaped token that embeds its credential in the claims.
    ///
    /// Test-only: production issuance always records the credential in the
    /// registry. This fixture exercises the backward-compatibility path.
    #[cfg(test)]
    fn issue_legacy_token(
        &self,
        athlete_id: &str,
        api_key: &str,
        ttl_secs: u64,
    ) -> Result<String, AuthError> {
        let custom = IntervalsClaims {
            athlete_id: athlete_id.to_string(),
            encrypted_api_key: Some(self.encrypt_api_key(api_key)?),
        };
        let claims = Claims::with_custom_claims(custom, Duration::from_secs(ttl_secs))
            .with_issuer(&self.issuer)
            .with_audience(self.audience.clone())
            .with_subject(athlete_id.to_string());
        self.signing_key
            .authenticate(claims)
            .map_err(|_| AuthError::EncryptionError)
    }

    /// Best-effort recovery of the athlete identity behind a rejected token.
    ///
    /// The HMAC signature is still verified — so only tokens minted by this
    /// manager can be attributed — while clock checks (`exp`/`iat`/`nbf`) and
    /// `iss`/`aud` matching are relaxed. The result is safe to log for
    /// rejected requests (expired, revoked, ...). It must never be used to
    /// authorise a request. Tokens with an invalid signature (or expired for
    /// more than ~10 years) cannot be attributed and return `None`.
    pub fn identify_rejected_token(&self, token: &str) -> Option<String> {
        let verification_options = VerificationOptions {
            required_signature_type: Some("JWT".to_string()),
            // Neutralize every clock-based rejection so an expired-but-
            // authentic token can still be attributed in rejection logs.
            // `iss`/`aud` are intentionally not checked: the HMAC signature
            // already proves the token was minted by this manager, and
            // attribution must survive issuer/audience shape changes.
            accept_future: true,
            time_tolerance: Some(Duration::from_secs(10 * 365 * 24 * 60 * 60)),
            ..Default::default()
        };

        self.signing_key
            .verify_token::<IntervalsClaims>(token, Some(verification_options))
            .ok()
            .map(|claims| claims.custom.athlete_id)
    }

    fn encrypt_api_key(&self, api_key: &str) -> Result<String, AuthError> {
        let cipher = Aes256Gcm::new((&self.encryption_key).into());
        let nonce_bytes = fill_random::<12>()?;
        let nonce: &aes_gcm::Nonce<aes_gcm::aead::consts::U12> = nonce_bytes
            .as_slice()
            .try_into()
            .map_err(|_| AuthError::EncryptionError)?;

        let ciphertext = cipher
            .encrypt(nonce, api_key.as_bytes())
            .map_err(|_| AuthError::EncryptionError)?;

        let mut result = nonce_bytes.to_vec();
        result.extend(ciphertext);
        Ok(STANDARD.encode(&result))
    }

    fn decrypt_api_key(&self, encrypted: &str) -> Result<String, AuthError> {
        let data = STANDARD
            .decode(encrypted)
            .map_err(|_| AuthError::EncryptionError)?;

        if data.len() < 12 {
            return Err(AuthError::EncryptionError);
        }

        let nonce: &aes_gcm::Nonce<aes_gcm::aead::consts::U12> = data[..12]
            .try_into()
            .map_err(|_| AuthError::EncryptionError)?;
        let ciphertext = &data[12..];

        let cipher = Aes256Gcm::new((&self.encryption_key).into());
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| AuthError::EncryptionError)?;

        String::from_utf8(plaintext).map_err(|_| AuthError::EncryptionError)
    }
}

// ============================================================================
// HTTP Axum Handlers
// ============================================================================

use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

#[derive(Deserialize)]
pub struct AuthRequest {
    pub api_key: String,
    pub athlete_id: String,
}

#[derive(Serialize, Debug)]
pub struct AuthResponse {
    pub token: String,
    pub expires_in: u64,
    pub athlete_id: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message): (StatusCode, &str) = match self {
            AuthError::MissingCredentials => (StatusCode::UNAUTHORIZED, "Missing credentials"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid token"),
            AuthError::TokenExpired => (StatusCode::UNAUTHORIZED, "Token expired"),
            AuthError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "Invalid credentials"),
            AuthError::TokenRevoked => (StatusCode::UNAUTHORIZED, "Token revoked"),
            // Internal errors: return a generic message instead of leaking details.
            AuthError::EncryptionError
            | AuthError::ServerConfig
            | AuthError::InvalidKeyFormat
            | AuthError::InvalidKeyLength
            | AuthError::KeyDerivationError
            | AuthError::JwtError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth configuration error",
            ),
        };

        (status, message).into_response()
    }
}

/// POST /auth - get JWT token
pub async fn auth_endpoint(
    State(state): State<Arc<AppState>>,
    axum::extract::ConnectInfo(client_addr): axum::extract::ConnectInfo<SocketAddr>,
    request_id: Option<axum::Extension<crate::RequestId>>,
    client_ip_ext: Option<axum::Extension<crate::ClientIp>>,
    Json(req): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, AuthError> {
    let client_ip = client_ip_ext
        .map(|axum::Extension(ip)| ip.0)
        .unwrap_or_else(|| client_addr.to_string());
    let request_id = request_id
        .map(|axum::Extension(id)| id.0)
        .unwrap_or_else(|| "unknown".to_string());

    // Validate credentials against intervals.icu API
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &state.base_url,
        req.athlete_id.clone(),
        SecretString::new(req.api_key.clone().into()),
    )
    .map_err(|e| {
        tracing::warn!(
            client_ip = %client_ip,
            athlete_id = %req.athlete_id,
            error = %e,
            "Failed to create client at /auth"
        );
        AuthError::ServerConfig
    })?;

    // Simple validation call - get_athlete_profile
    client.get_athlete_profile().await.map_err(|e| {
        tracing::warn!(
            client_ip = %client_ip,
            request_id = %request_id,
            athlete_id = %req.athlete_id,
            error = %e,
            "Invalid credentials at /auth"
        );
        AuthError::InvalidCredentials
    })?;

    // Validation successful — issue a revocable token recorded server-side.
    let jti = Uuid::new_v4().to_string();
    let encrypted_api_key = state.jwt_manager.encrypt_credential(&req.api_key)?;
    let token = state
        .jwt_manager
        .issue_token(&req.athlete_id, state.jwt_ttl_seconds, &jti)?;

    let now = chrono::Utc::now();
    state
        .tokens
        .record(TokenRecord {
            jti: jti.clone(),
            athlete_id: req.athlete_id.clone(),
            issued_at: now.to_rfc3339(),
            expires_at: (now + chrono::TimeDelta::seconds(state.jwt_ttl_seconds as i64))
                .to_rfc3339(),
            revoked: false,
            encrypted_api_key,
        })
        .await;

    // Record token issuance metric
    metrics::record_token_issued();

    tracing::info!(
        client_ip = %client_ip,
        request_id = %request_id,
        athlete_id = %req.athlete_id,
        jti = %jti,
        "Issued JWT token"
    );

    Ok(Json(AuthResponse {
        token,
        expires_in: state.jwt_ttl_seconds,
        athlete_id: req.athlete_id,
    }))
}

/// Axum middleware for extracting JWT from Authorization header
pub async fn auth_middleware(
    State(app_state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    use axum::extract::ConnectInfo;
    use axum::http::header::AUTHORIZATION;
    use std::net::SocketAddr;

    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    // Real client IP resolved by the outer middleware; the socket peer is
    // only reachable when the middleware is bypassed (e.g. direct calls).
    let client_ip = request
        .extensions()
        .get::<crate::ClientIp>()
        .map(|ip| ip.0.clone())
        .or_else(|| {
            request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());
    // Correlation id assigned/upstream by the outer request-id middleware.
    let request_id = request
        .extensions()
        .get::<crate::RequestId>()
        .map(|id| id.0.clone())
        .unwrap_or_else(|| "unknown".to_string());

    let credentials = if let Some(token) = auth_header {
        match app_state.authenticate(token).await {
            Ok(creds) => {
                // Record successful verification
                metrics::record_token_verification("valid");
                creds
            }
            Err(e) => {
                // Attribute the rejection when the token is authentic but
                // expired/revoked, so operators can tell whose token it was.
                let athlete_id = app_state.jwt_manager.identify_rejected_token(token);
                let status = match &e {
                    AuthError::TokenExpired => "expired",
                    AuthError::TokenRevoked => "revoked",
                    _ => "invalid",
                };
                tracing::warn!(
                    client_ip = %client_ip,
                    request_id = %request_id,
                    athlete_id = athlete_id.as_deref().unwrap_or("unknown"),
                    error_kind = status,
                    error = %e,
                    "Failed JWT verification"
                );
                metrics::record_token_verification(status);
                metrics::record_auth_failure("invalid_token");
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
    } else {
        tracing::warn!(
            client_ip = %client_ip,
            request_id = %request_id,
            "Missing Authorization header"
        );
        // Record failed verification (missing token)
        metrics::record_token_verification("invalid");
        metrics::record_auth_failure("missing_token");
        return Err(StatusCode::UNAUTHORIZED);
    };

    // Per-request routine already covered by the access log; keep it quiet.
    tracing::debug!(athlete_id = %credentials.athlete_id, "Authenticated request");

    request.extensions_mut().insert(credentials);

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeated_hex(byte: &str, repeat_count: usize) -> String {
        byte.repeat(repeat_count)
    }

    fn create_test_manager() -> JwtManager {
        let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
        let encryption_key = [0u8; 32];
        JwtManager::new(secret, encryption_key)
    }

    #[test]
    fn test_jwt_manager_creation() {
        let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
        let encryption_key = [0u8; 32];
        let manager = JwtManager::new(secret, encryption_key);
        assert_eq!(manager.issuer, "intervals-icu-mcp");
        assert_eq!(manager.audience, "intervals-icu-mcp");
    }

    #[test]
    fn test_encrypt_decrypt_round_trip() {
        let manager = create_test_manager();
        let api_key = "test_api_key_12345";
        let encrypted = manager.encrypt_api_key(api_key).unwrap();
        let decrypted = manager.decrypt_api_key(&encrypted).unwrap();
        assert_eq!(api_key, decrypted);
    }

    #[test]
    fn test_encryption_produces_different_output() {
        let manager = create_test_manager();
        let api_key = "test_api_key";
        let encrypted1 = manager.encrypt_api_key(api_key).unwrap();
        let encrypted2 = manager.encrypt_api_key(api_key).unwrap();
        assert_ne!(encrypted1, encrypted2);
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails() {
        let manager = create_test_manager();
        let api_key = "test_api_key";
        let encrypted = manager.encrypt_api_key(api_key).unwrap();

        let wrong_manager = JwtManager::new(
            b"wrong_key_________________________________________",
            [1u8; 32],
        );
        let result = wrong_manager.decrypt_api_key(&encrypted);
        assert!(matches!(result, Err(AuthError::EncryptionError)));
    }

    fn app_state() -> AppState {
        AppState {
            jwt_manager: Arc::new(create_test_manager()),
            jwt_ttl_seconds: 3600,
            base_url: "https://intervals.icu".to_string(),
            tokens: TokenRegistry::load(None),
        }
    }

    async fn record_token(state: &AppState, jti: &str, athlete_id: &str, api_key: &str) {
        state
            .tokens
            .record(TokenRecord {
                jti: jti.to_string(),
                athlete_id: athlete_id.to_string(),
                issued_at: chrono::Utc::now().to_rfc3339(),
                expires_at: chrono::Utc::now().to_rfc3339(),
                revoked: false,
                encrypted_api_key: state.jwt_manager.encrypt_credential(api_key).unwrap(),
            })
            .await;
    }

    #[test]
    fn test_issue_token_success() {
        let manager = create_test_manager();
        let token = manager.issue_token("i123456", 3600, "jti-1").unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);

        // The JWT carries identity only — no embedded credential.
        let claims = manager.verify_claims(&token).unwrap();
        assert_eq!(claims.athlete_id, "i123456");
        assert_eq!(claims.jti.as_deref(), Some("jti-1"));
        assert!(claims.encrypted_api_key.is_none());
    }

    #[tokio::test]
    async fn test_authenticate_resolves_credential_from_registry() {
        let state = app_state();
        record_token(&state, "jti-1", "i123456", "test_api_key").await;
        let token = state
            .jwt_manager
            .issue_token("i123456", 3600, "jti-1")
            .unwrap();

        let credentials = state.authenticate(&token).await.unwrap();
        assert_eq!(credentials.athlete_id, "i123456");
        assert_eq!(credentials.api_key.expose_secret(), "test_api_key");
    }

    #[tokio::test]
    async fn test_authenticate_rejects_invalid_token() {
        let state = app_state();
        let result = state.authenticate("invalid.token.here").await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn test_authenticate_rejects_unknown_jti_without_credential() {
        // A token whose signature is valid but whose jti has no registry record
        // and carries no legacy credential must fail closed.
        let state = app_state();
        let token = state
            .jwt_manager
            .issue_token("i123456", 3600, "ghost")
            .unwrap();
        let result = state.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn test_authenticate_accepts_legacy_token_with_embedded_credential() {
        let state = app_state();
        let token = state
            .jwt_manager
            .issue_legacy_token("i123456", "legacy_key", 3600)
            .unwrap();

        let credentials = state.authenticate(&token).await.unwrap();
        assert_eq!(credentials.athlete_id, "i123456");
        assert_eq!(credentials.api_key.expose_secret(), "legacy_key");
    }

    #[tokio::test]
    async fn test_authenticate_falls_back_to_claims_when_record_lacks_credential() {
        // Registry files written before ADR-0011 keep the credential in the
        // token's claims; a record with an empty credential must fall back.
        let state = app_state();
        record_token(&state, "jti-1", "i123456", "stale").await;
        state
            .tokens
            .record(TokenRecord {
                encrypted_api_key: String::new(),
                ..state.tokens.find("jti-1").await.expect("record")
            })
            .await;

        let manager = &state.jwt_manager;
        let custom = IntervalsClaims {
            athlete_id: "i123456".to_string(),
            encrypted_api_key: Some(manager.encrypt_credential("from_claims").unwrap()),
        };
        let claims = Claims::with_custom_claims(custom, Duration::from_secs(3600))
            .with_issuer(&manager.issuer)
            .with_audience(manager.audience.clone())
            .with_subject("i123456".to_string())
            .with_jwt_id("jti-1");
        let token = manager.signing_key.authenticate(claims).unwrap();

        let credentials = state.authenticate(&token).await.unwrap();
        assert_eq!(credentials.api_key.expose_secret(), "from_claims");
    }

    #[tokio::test]
    async fn test_authenticate_rejects_revoked_token() {
        let state = app_state();
        record_token(&state, "jti-1", "i123456", "test_api_key").await;
        let token = state
            .jwt_manager
            .issue_token("i123456", 3600, "jti-1")
            .unwrap();
        assert!(state.tokens.revoke("jti-1").await);

        let result = state.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::TokenRevoked)));
    }

    /// Sign authentic claims whose `exp` lies one day in the past, producing
    /// a token that must fail verification as expired. Issuer, audience, and
    /// subject mirror `issue_token` so the fixture matches production tokens.
    fn create_expired_token(manager: &JwtManager, athlete_id: &str) -> String {
        let custom = IntervalsClaims {
            athlete_id: athlete_id.to_string(),
            encrypted_api_key: None,
        };
        let mut claims = Claims::with_custom_claims(custom, Duration::from_secs(0))
            .with_issuer(&manager.issuer)
            .with_audience(manager.audience.clone())
            .with_subject(athlete_id.to_string())
            .with_jwt_id("expired-jti");
        claims.expires_at = Some(Clock::now_since_epoch() - Duration::from_secs(86_400));
        manager
            .signing_key
            .authenticate(claims)
            .expect("signing expired claims should work")
    }

    #[test]
    fn test_verify_expired_token_maps_to_token_expired() {
        let manager = create_test_manager();
        let token = create_expired_token(&manager, "i424242");
        let result = manager.verify_claims(&token);
        assert!(matches!(result, Err(AuthError::TokenExpired)));
    }

    #[test]
    fn test_identify_rejected_token_recovers_athlete_for_expired_token() {
        let manager = create_test_manager();
        let token = create_expired_token(&manager, "i424242");
        assert_eq!(
            manager.identify_rejected_token(&token).as_deref(),
            Some("i424242")
        );
    }

    #[test]
    fn test_identify_rejected_token_returns_athlete_for_valid_token() {
        let manager = create_test_manager();
        let token = manager
            .issue_token("i111111", 3600, "jti-1")
            .expect("token should issue");
        assert_eq!(
            manager.identify_rejected_token(&token).as_deref(),
            Some("i111111")
        );
    }

    #[test]
    fn test_identify_rejected_token_garbage_returns_none() {
        let manager = create_test_manager();
        assert_eq!(manager.identify_rejected_token("not.a.token"), None);
    }

    #[test]
    fn test_identify_rejected_token_tampered_returns_none() {
        let manager = create_test_manager();
        let token = manager
            .issue_token("i222222", 3600, "jti-1")
            .expect("token should issue");
        let mut parts: Vec<String> = token.split('.').map(str::to_string).collect();
        assert_eq!(parts.len(), 3, "JWT should have three segments");
        // Flip the payload so the signature no longer matches.
        parts[1] = format!("{}A", parts[1].trim_end_matches('A'));
        let tampered = parts.join(".");
        assert_eq!(manager.identify_rejected_token(&tampered), None);
    }

    #[test]
    fn test_identify_rejected_token_foreign_key_returns_none() {
        let manager = create_test_manager();
        let other = JwtManager::new(b"a completely different signing secret!!", [1u8; 32]);
        let foreign_token = other
            .issue_token("i333333", 3600, "jti-1")
            .expect("token should issue");
        assert_eq!(manager.identify_rejected_token(&foreign_token), None);
    }

    #[test]
    fn test_verify_tampered_token_fails() {
        let manager = create_test_manager();
        let token = manager.issue_token("i123456", 3600, "jti-1").unwrap();
        let mut tampered = token.chars().collect::<Vec<_>>();
        tampered[10] = 'X';
        let tampered_token: String = tampered.into_iter().collect();
        let result = manager.verify_claims(&tampered_token);
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn test_issue_and_verify_round_trip() {
        let state = app_state();
        let athlete_id = "i999999";
        let api_key = "round_trip_test_key";
        record_token(&state, "jti-1", athlete_id, api_key).await;
        let token = state
            .jwt_manager
            .issue_token(athlete_id, 3600, "jti-1")
            .unwrap();
        let credentials = state.authenticate(&token).await.unwrap();
        assert_eq!(credentials.athlete_id, athlete_id);
        assert_eq!(credentials.api_key.expose_secret(), api_key);
    }

    #[test]
    fn test_verify_token_rejects_wrong_issuer() {
        let manager = create_test_manager();
        let claims = Claims::with_custom_claims(
            IntervalsClaims {
                athlete_id: "i123456".to_string(),
                encrypted_api_key: None,
            },
            Duration::from_secs(3600),
        )
        .with_issuer("different-issuer")
        .with_audience(manager.audience.clone())
        .with_subject("i123456".to_string());

        let token = manager.signing_key.authenticate(claims).unwrap();
        let result = manager.verify_claims(&token);

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_verify_token_rejects_wrong_audience() {
        let manager = create_test_manager();
        let claims = Claims::with_custom_claims(
            IntervalsClaims {
                athlete_id: "i123456".to_string(),
                encrypted_api_key: None,
            },
            Duration::from_secs(3600),
        )
        .with_issuer(&manager.issuer)
        .with_audience("different-audience")
        .with_subject("i123456".to_string());

        let token = manager.signing_key.authenticate(claims).unwrap();
        let result = manager.verify_claims(&token);

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn test_multiple_tokens_independent() {
        let state = app_state();
        record_token(&state, "jti-1", "athlete1", "key1").await;
        record_token(&state, "jti-2", "athlete2", "key2").await;
        let token1 = state
            .jwt_manager
            .issue_token("athlete1", 3600, "jti-1")
            .unwrap();
        let token2 = state
            .jwt_manager
            .issue_token("athlete2", 3600, "jti-2")
            .unwrap();
        let creds1 = state.authenticate(&token1).await.unwrap();
        let creds2 = state.authenticate(&token2).await.unwrap();
        assert_ne!(creds1.athlete_id, creds2.athlete_id);
        assert_ne!(
            creds1.api_key.expose_secret(),
            creds2.api_key.expose_secret()
        );
    }

    #[tokio::test]
    async fn test_special_characters_in_api_key() {
        let state = app_state();
        let api_key = "special!@#$%^&*()_+-=[]{}|;':\",./<>?";
        record_token(&state, "jti-1", "i123456", api_key).await;
        let token = state
            .jwt_manager
            .issue_token("i123456", 3600, "jti-1")
            .unwrap();
        let credentials = state.authenticate(&token).await.unwrap();
        assert_eq!(credentials.api_key.expose_secret(), api_key);
    }

    #[test]
    fn test_auth_error_into_response_missing_credentials() {
        let response = AuthError::MissingCredentials.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_auth_error_into_response_invalid_token() {
        let response = AuthError::InvalidToken.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_auth_error_into_response_token_expired() {
        let response = AuthError::TokenExpired.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_auth_error_into_response_invalid_credentials() {
        let response = AuthError::InvalidCredentials.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_auth_error_into_response_encryption_error() {
        let response = AuthError::EncryptionError.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_auth_error_into_response_server_config() {
        let response = AuthError::ServerConfig.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_auth_error_into_response_jwt_error() {
        // jwt_simple::Error is anyhow-based, use anyhow::anyhow! to create one
        let response = AuthError::JwtError(anyhow::anyhow!("test error")).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_app_state_clone() {
        let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
        let jwt_manager = Arc::new(JwtManager::new(secret, [0u8; 32]));
        let state = AppState {
            jwt_manager: jwt_manager.clone(),
            jwt_ttl_seconds: 3600,
            base_url: "https://intervals.icu".to_string(),
            tokens: TokenRegistry::load(None),
        };

        let cloned = state.clone();
        assert_eq!(cloned.jwt_ttl_seconds, 3600);
        assert_eq!(cloned.base_url, "https://intervals.icu");
        assert!(Arc::ptr_eq(&cloned.jwt_manager, &jwt_manager));
    }

    #[test]
    fn test_decrypted_credentials_clone() {
        let creds = DecryptedCredentials {
            athlete_id: "i123456".to_string(),
            api_key: SecretString::new("test_key".to_string().into()),
        };

        let cloned = creds.clone();
        assert_eq!(cloned.athlete_id, "i123456");
        assert_eq!(cloned.api_key.expose_secret(), "test_key");
    }

    #[tokio::test]
    async fn test_auth_endpoint_invalid_credentials() {
        let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
        let jwt_manager = Arc::new(JwtManager::new(secret, [0u8; 32]));
        let state = Arc::new(AppState {
            jwt_manager,
            jwt_ttl_seconds: 3600,
            base_url: "https://intervals.icu".to_string(),
            tokens: TokenRegistry::load(None),
        });

        let req = AuthRequest {
            athlete_id: "i123456".to_string(),
            api_key: "invalid_key".to_string(),
        };

        // Use wiremock or mock to test this properly
        // For now, test that it returns the right error type
        let result = auth_endpoint(
            State(state),
            axum::extract::ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 8080))),
            None,
            None,
            Json(req),
        )
        .await;

        assert!(matches!(result, Err(AuthError::InvalidCredentials)));
    }

    // ========================================
    // TDD Tests for HKDF-based JWT Manager
    // ========================================

    // RED: Test that MasterKeyConfig can be created from hex string
    #[test]
    fn test_master_key_config_from_hex() {
        // Valid 64-byte (128 hex chars) master key
        let master_key_hex = repeated_hex("11", 64);

        let result = MasterKeyConfig::from_hex(&master_key_hex);

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.signing_key.len(), 32);
        assert_eq!(config.encryption_key.len(), 32);
    }

    // RED: Test that invalid hex string fails
    #[test]
    fn test_master_key_config_invalid_hex() {
        let invalid_hex = "not_valid_hex_chars!";

        let result = MasterKeyConfig::from_hex(invalid_hex);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AuthError::InvalidKeyFormat));
    }

    // RED: Test that wrong length key fails
    #[test]
    fn test_master_key_config_wrong_length() {
        // Only 32 bytes (64 hex chars) instead of 64 bytes
        let short_key_hex = repeated_hex("11", 32);

        let result = MasterKeyConfig::from_hex(&short_key_hex);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AuthError::InvalidKeyLength));
    }

    // RED: Test that HKDF produces different signing and encryption keys
    #[test]
    fn test_hkdf_produces_different_keys() {
        let master_key_hex = repeated_hex("11", 64);

        let config = MasterKeyConfig::from_hex(&master_key_hex).unwrap();

        // Signing and encryption keys should be different
        assert_ne!(config.signing_key, config.encryption_key);

        // Keys should not be all zeros
        assert_ne!(config.signing_key, [0u8; 32]);
        assert_ne!(config.encryption_key, [0u8; 32]);
    }

    // RED: Test JwtManager can be created from MasterKeyConfig
    #[test]
    fn test_jwt_manager_from_master_key() {
        let master_key_hex = repeated_hex("11", 64);

        let config = MasterKeyConfig::from_hex(&master_key_hex).unwrap();
        let manager = JwtManager::from_master_key(&config);

        assert_eq!(manager.issuer, "intervals-icu-mcp");
        assert_eq!(manager.audience, "intervals-icu-mcp");
    }

    // RED: Test JWT token issuance and verification with HKDF
    #[tokio::test]
    async fn test_jwt_issue_and_verify_with_hkdf() {
        let master_key_hex = repeated_hex("11", 64);

        let config = MasterKeyConfig::from_hex(&master_key_hex).unwrap();
        let manager = Arc::new(JwtManager::from_master_key(&config));
        let state = AppState {
            jwt_manager: manager,
            jwt_ttl_seconds: 3600,
            base_url: "https://intervals.icu".to_string(),
            tokens: TokenRegistry::load(None),
        };

        let athlete_id = "i123456";
        let api_key = "test_api_key_123";

        record_token(&state, "jti-1", athlete_id, api_key).await;
        let token = state
            .jwt_manager
            .issue_token(athlete_id, 3600, "jti-1")
            .unwrap();

        let credentials = state.authenticate(&token).await.unwrap();
        assert_eq!(credentials.athlete_id, athlete_id);
        assert_eq!(credentials.api_key.expose_secret(), api_key);
    }

    // RED: Test that different master keys produce different tokens
    #[test]
    fn test_different_master_keys_produce_different_tokens() {
        let master_key_hex_1 = repeated_hex("11", 64);
        let master_key_hex_2 = repeated_hex("22", 64);

        let config1 = MasterKeyConfig::from_hex(&master_key_hex_1).unwrap();
        let config2 = MasterKeyConfig::from_hex(&master_key_hex_2).unwrap();

        let manager1 = JwtManager::from_master_key(&config1);
        let manager2 = JwtManager::from_master_key(&config2);

        let athlete_id = "i123456";

        let token1 = manager1.issue_token(athlete_id, 3600, "jti-1").unwrap();
        let token2 = manager2.issue_token(athlete_id, 3600, "jti-1").unwrap();

        // Tokens should be different
        assert_ne!(token1, token2);

        // Token from manager1 should NOT be verifiable by manager2
        assert!(manager2.verify_claims(&token1).is_err());
    }
}
