//! JWT Authentication and Encryption Module

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use intervals_icu_client::IntervalsClient;
use jwt_simple::prelude::*;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use thiserror::Error;

#[cfg(test)]
use secrecy::ExposeSecret;

/// Generate random bytes using OS CSPRNG
fn fill_random(buf: &mut [u8]) {
    getrandom::fill(buf).expect("Failed to generate random bytes");
}

/// Custom claims for JWT tokens
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntervalsClaims {
    pub athlete_id: String,
    pub encrypted_api_key: String,
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
    #[error("JWT error: {0}")]
    JwtError(#[from] jwt_simple::Error),
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

    pub fn issue_token(
        &self,
        athlete_id: &str,
        api_key: &str,
        ttl_secs: u64,
    ) -> Result<String, AuthError> {
        let encrypted = self.encrypt_api_key(api_key)?;

        let custom = IntervalsClaims {
            athlete_id: athlete_id.to_string(),
            encrypted_api_key: encrypted,
        };

        let claims = Claims::with_custom_claims(custom, Duration::from_secs(ttl_secs))
            .with_issuer(&self.issuer)
            .with_audience(self.audience.clone())
            .with_subject(athlete_id.to_string());

        self.signing_key
            .authenticate(claims)
            .map_err(|_| AuthError::EncryptionError)
    }

    pub fn verify_token(&self, token: &str) -> Result<DecryptedCredentials, AuthError> {
        let verification_options = VerificationOptions {
            allowed_issuers: Some(HashSet::from([self.issuer.clone()])),
            allowed_audiences: Some(HashSet::from([self.audience.clone()])),
            required_signature_type: Some("JWT".to_string()),
            ..Default::default()
        };

        let claims: JWTClaims<IntervalsClaims> = self
            .signing_key
            .verify_token::<IntervalsClaims>(token, Some(verification_options))
            .map_err(|e| {
                if e.to_string().contains("expired") {
                    AuthError::TokenExpired
                } else {
                    AuthError::InvalidToken
                }
            })?;

        let api_key = self.decrypt_api_key(&claims.custom.encrypted_api_key)?;

        Ok(DecryptedCredentials {
            athlete_id: claims.custom.athlete_id.clone(),
            api_key: SecretString::new(api_key.into()),
        })
    }

    fn encrypt_api_key(&self, api_key: &str) -> Result<String, AuthError> {
        let cipher = Aes256Gcm::new((&self.encryption_key).into());
        let mut nonce = [0u8; 12];
        fill_random(&mut nonce);

        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), api_key.as_bytes())
            .map_err(|_| AuthError::EncryptionError)?;

        let mut result = nonce.to_vec();
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

        let nonce = &data[..12];
        let ciphertext = &data[12..];

        let cipher = Aes256Gcm::new((&self.encryption_key).into());
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
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

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub expires_in: u64,
    pub athlete_id: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = match self {
            AuthError::MissingCredentials => StatusCode::UNAUTHORIZED,
            AuthError::InvalidToken => StatusCode::UNAUTHORIZED,
            AuthError::TokenExpired => StatusCode::UNAUTHORIZED,
            AuthError::InvalidCredentials => StatusCode::UNAUTHORIZED,
            AuthError::EncryptionError => StatusCode::INTERNAL_SERVER_ERROR,
            AuthError::ServerConfig => StatusCode::INTERNAL_SERVER_ERROR,
            AuthError::JwtError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status, self.to_string()).into_response()
    }
}

/// POST /auth - получить JWT токен
pub async fn auth_endpoint(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, AuthError> {
    // Валидация credentials против intervals.icu API
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &state.base_url,
        req.athlete_id.clone(),
        SecretString::new(req.api_key.clone().into()),
    );

    // Простой validation call - get_athlete_profile
    client
        .get_athlete_profile()
        .await
        .map_err(|_| AuthError::InvalidCredentials)?;

    // Валидация прошла успешно - выдаём JWT
    let token =
        state
            .jwt_manager
            .issue_token(&req.athlete_id, &req.api_key, state.jwt_ttl_seconds)?;

    tracing::info!(athlete_id = %req.athlete_id, "Issued JWT token");

    Ok(Json(AuthResponse {
        token,
        expires_in: state.jwt_ttl_seconds,
        athlete_id: req.athlete_id,
    }))
}

/// Axum middleware для извлечения JWT из Authorization header
pub async fn auth_middleware(
    State(jwt_manager): State<Arc<JwtManager>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    use axum::http::header::AUTHORIZATION;

    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let credentials = match auth_header {
        Some(token) => jwt_manager
            .verify_token(token)
            .map_err(|_| StatusCode::UNAUTHORIZED)?,
        None => {
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    tracing::info!(athlete_id = %credentials.athlete_id, "Authenticated request");

    request.extensions_mut().insert(credentials);

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Decryption should fail with wrong key
        assert!(result.is_err());
    }

    #[test]
    fn test_issue_token_success() {
        let manager = create_test_manager();
        let token = manager
            .issue_token("i123456", "test_api_key", 3600)
            .unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn test_verify_token_success() {
        let manager = create_test_manager();
        let token = manager
            .issue_token("i123456", "test_api_key", 3600)
            .unwrap();
        let credentials = manager.verify_token(&token).unwrap();
        assert_eq!(credentials.athlete_id, "i123456");
        assert_eq!(credentials.api_key.expose_secret(), "test_api_key");
    }

    #[test]
    fn test_verify_invalid_token_fails() {
        let manager = create_test_manager();
        let result = manager.verify_token("invalid.token.here");
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_verify_tampered_token_fails() {
        let manager = create_test_manager();
        let token = manager
            .issue_token("i123456", "test_api_key", 3600)
            .unwrap();
        let mut tampered = token.chars().collect::<Vec<_>>();
        tampered[10] = 'X';
        let tampered_token: String = tampered.into_iter().collect();
        let result = manager.verify_token(&tampered_token);
        assert!(result.is_err());
    }

    #[test]
    fn test_issue_and_verify_round_trip() {
        let manager = create_test_manager();
        let athlete_id = "i999999";
        let api_key = "round_trip_test_key";
        let token = manager.issue_token(athlete_id, api_key, 3600).unwrap();
        let credentials = manager.verify_token(&token).unwrap();
        assert_eq!(credentials.athlete_id, athlete_id);
        assert_eq!(credentials.api_key.expose_secret(), api_key);
    }

    #[test]
    fn test_verify_token_rejects_wrong_issuer() {
        let manager = create_test_manager();
        let claims = Claims::with_custom_claims(
            IntervalsClaims {
                athlete_id: "i123456".to_string(),
                encrypted_api_key: manager.encrypt_api_key("test_api_key").unwrap(),
            },
            Duration::from_secs(3600),
        )
        .with_issuer("different-issuer")
        .with_audience(manager.audience.clone())
        .with_subject("i123456".to_string());

        let token = manager.signing_key.authenticate(claims).unwrap();
        let result = manager.verify_token(&token);

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_verify_token_rejects_wrong_audience() {
        let manager = create_test_manager();
        let claims = Claims::with_custom_claims(
            IntervalsClaims {
                athlete_id: "i123456".to_string(),
                encrypted_api_key: manager.encrypt_api_key("test_api_key").unwrap(),
            },
            Duration::from_secs(3600),
        )
        .with_issuer(&manager.issuer)
        .with_audience("different-audience")
        .with_subject("i123456".to_string());

        let token = manager.signing_key.authenticate(claims).unwrap();
        let result = manager.verify_token(&token);

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_multiple_tokens_independent() {
        let manager = create_test_manager();
        let token1 = manager.issue_token("athlete1", "key1", 3600).unwrap();
        let token2 = manager.issue_token("athlete2", "key2", 3600).unwrap();
        let creds1 = manager.verify_token(&token1).unwrap();
        let creds2 = manager.verify_token(&token2).unwrap();
        assert_ne!(creds1.athlete_id, creds2.athlete_id);
        assert_ne!(
            creds1.api_key.expose_secret(),
            creds2.api_key.expose_secret()
        );
    }

    #[test]
    fn test_special_characters_in_api_key() {
        let manager = create_test_manager();
        let api_key = "special!@#$%^&*()_+-=[]{}|;':\",./<>?";
        let token = manager.issue_token("i123456", api_key, 3600).unwrap();
        let credentials = manager.verify_token(&token).unwrap();
        assert_eq!(credentials.api_key.expose_secret(), api_key);
    }
}
