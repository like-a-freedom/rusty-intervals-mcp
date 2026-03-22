//! JWT Authentication and Encryption Module

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
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

use crate::metrics;

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

    pub fn from_master_key(config: &MasterKeyConfig) -> Self {
        Self {
            signing_key: HS256Key::from_bytes(&config.signing_key),
            encryption_key: config.encryption_key,
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

#[derive(Serialize, Debug)]
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
            AuthError::InvalidKeyFormat => StatusCode::INTERNAL_SERVER_ERROR,
            AuthError::InvalidKeyLength => StatusCode::INTERNAL_SERVER_ERROR,
            AuthError::KeyDerivationError => StatusCode::INTERNAL_SERVER_ERROR,
            AuthError::JwtError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status, self.to_string()).into_response()
    }
}

/// POST /auth - get JWT token
pub async fn auth_endpoint(
    State(state): State<Arc<AppState>>,
    axum::extract::ConnectInfo(client_addr): axum::extract::ConnectInfo<SocketAddr>,
    Json(req): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, AuthError> {
    let client_ip = client_addr.to_string();

    // Validate credentials against intervals.icu API
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &state.base_url,
        req.athlete_id.clone(),
        SecretString::new(req.api_key.clone().into()),
    );

    // Simple validation call - get_athlete_profile
    client.get_athlete_profile().await.map_err(|e| {
        tracing::warn!(
            client_ip = %client_ip,
            athlete_id = %req.athlete_id,
            error = %e,
            "Invalid credentials at /auth"
        );
        AuthError::InvalidCredentials
    })?;

    // Validation successful - issue JWT
    let token =
        state
            .jwt_manager
            .issue_token(&req.athlete_id, &req.api_key, state.jwt_ttl_seconds)?;

    // Record token issuance metric
    metrics::record_token_issued();

    tracing::info!(
        client_ip = %client_ip,
        athlete_id = %req.athlete_id,
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
    State(jwt_manager): State<Arc<JwtManager>>,
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

    // Get client IP for logging
    let client_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0)
        .map(|addr| addr.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let credentials = match auth_header {
        Some(token) => match jwt_manager.verify_token(token) {
            Ok(creds) => {
                // Record successful verification
                metrics::record_token_verification("valid");
                creds
            }
            Err(e) => {
                tracing::warn!(
                    client_ip = %client_ip,
                    error = %e,
                    "Failed JWT verification"
                );
                // Record failed verification with status
                let status = match &e {
                    AuthError::TokenExpired => "expired",
                    _ => "invalid",
                };
                metrics::record_token_verification(status);
                metrics::record_auth_failure("invalid_token");
                return Err(StatusCode::UNAUTHORIZED);
            }
        },
        None => {
            tracing::warn!(
                client_ip = %client_ip,
                "Missing Authorization header"
            );
            // Record failed verification (missing token)
            metrics::record_token_verification("invalid");
            metrics::record_auth_failure("missing_token");
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

    #[test]
    fn test_http_base_url_clone() {
        let base_url = HttpBaseUrl("https://intervals.icu".to_string());
        let cloned = base_url.clone();
        assert_eq!(cloned.0, "https://intervals.icu");
    }

    #[tokio::test]
    async fn test_auth_endpoint_invalid_credentials() {
        let secret = b"test_secret_key_for_jwt_signing_12345678901234567890123456789012";
        let jwt_manager = Arc::new(JwtManager::new(secret, [0u8; 32]));
        let state = Arc::new(AppState {
            jwt_manager,
            jwt_ttl_seconds: 3600,
            base_url: "https://intervals.icu".to_string(),
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
            Json(req),
        )
        .await;

        // Should fail with InvalidCredentials (API call will fail)
        assert!(result.is_err());
    }

    // ========================================
    // TDD Tests for HKDF-based JWT Manager
    // ========================================

    // RED: Test that MasterKeyConfig can be created from hex string
    #[test]
    fn test_master_key_config_from_hex() {
        // Valid 64-byte (128 hex chars) master key
        let master_key_hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

        let result = MasterKeyConfig::from_hex(master_key_hex);

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
        let short_key_hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

        let result = MasterKeyConfig::from_hex(short_key_hex);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AuthError::InvalidKeyLength));
    }

    // RED: Test that HKDF produces different signing and encryption keys
    #[test]
    fn test_hkdf_produces_different_keys() {
        let master_key_hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

        let config = MasterKeyConfig::from_hex(master_key_hex).unwrap();

        // Signing and encryption keys should be different
        assert_ne!(config.signing_key, config.encryption_key);

        // Keys should not be all zeros
        assert_ne!(config.signing_key, [0u8; 32]);
        assert_ne!(config.encryption_key, [0u8; 32]);
    }

    // RED: Test JwtManager can be created from MasterKeyConfig
    #[test]
    fn test_jwt_manager_from_master_key() {
        let master_key_hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

        let config = MasterKeyConfig::from_hex(master_key_hex).unwrap();
        let manager = JwtManager::from_master_key(&config);

        assert_eq!(manager.issuer, "intervals-icu-mcp");
        assert_eq!(manager.audience, "intervals-icu-mcp");
    }

    // RED: Test JWT token issuance and verification with HKDF
    #[test]
    fn test_jwt_issue_and_verify_with_hkdf() {
        let master_key_hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

        let config = MasterKeyConfig::from_hex(master_key_hex).unwrap();
        let manager = JwtManager::from_master_key(&config);

        let athlete_id = "i123456";
        let api_key = "test_api_key_123";

        // Issue token
        let token = manager.issue_token(athlete_id, api_key, 3600).unwrap();

        // Verify token
        let credentials = manager.verify_token(&token).unwrap();

        assert_eq!(credentials.athlete_id, athlete_id);
        assert_eq!(credentials.api_key.expose_secret(), api_key);
    }

    // RED: Test that different master keys produce different tokens
    #[test]
    fn test_different_master_keys_produce_different_tokens() {
        let master_key_hex_1 = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let master_key_hex_2 = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";

        let config1 = MasterKeyConfig::from_hex(master_key_hex_1).unwrap();
        let config2 = MasterKeyConfig::from_hex(master_key_hex_2).unwrap();

        let manager1 = JwtManager::from_master_key(&config1);
        let manager2 = JwtManager::from_master_key(&config2);

        let athlete_id = "i123456";
        let api_key = "test_api_key_123";

        let token1 = manager1.issue_token(athlete_id, api_key, 3600).unwrap();
        let token2 = manager2.issue_token(athlete_id, api_key, 3600).unwrap();

        // Tokens should be different
        assert_ne!(token1, token2);

        // Token from manager1 should NOT be verifiable by manager2
        assert!(manager2.verify_token(&token1).is_err());
    }
}
