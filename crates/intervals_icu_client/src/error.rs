//! Error types for Intervals.icu API operations.
//!
//! This module provides a structured error hierarchy following Rust best practices:
//! - Typed errors for different failure modes
//! - Proper `Display` and `Error` trait implementations
//! - Support for error context and chaining via `thiserror`

use thiserror::Error;

/// Transport-level error independent of the HTTP client library.
///
/// `reqwest::Error` and other transport-specific errors are converted to this
/// domain-owned type at the `http_client` boundary, so callers never have to
/// match on a specific HTTP library to handle transport failures.
#[derive(Debug, Clone)]
pub struct TransportError {
    /// Human-readable message for logging/diagnostics.
    pub message: String,
    /// True if the error was caused by a connection timeout.
    pub is_timeout: bool,
    /// True if the error was caused by a connection-refused or DNS failure.
    pub is_connect: bool,
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TransportError {}

/// Configuration-related errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Required environment variable is missing.
    #[error("required environment variable {0} is not set")]
    MissingEnvVar(String),

    /// Invalid configuration value.
    #[error("invalid configuration value for {key}: {message}")]
    InvalidValue { key: String, message: String },

    /// Typed stub for unimplemented trait methods on test-only extension clients.
    ///
    /// Replaces the historical `Other("...is not implemented...")` string so call
    /// sites can match on a genuine variant instead of substring-matching.
    #[error("operation not implemented: {method}")]
    Unsupported { method: &'static str },

    /// General configuration error.
    #[error("configuration error: {0}")]
    Other(String),
}

/// API-level errors from the Intervals.icu service.
#[derive(Debug, Error)]
#[error("API error: status {status}, message: {message}")]
pub struct ApiError {
    /// HTTP status code returned by the API.
    pub status: u16,
    /// Error message from the API response.
    pub message: String,
    /// Raw response body for debugging.
    pub raw_body: String,
}

impl ApiError {
    /// Create a new API error from a response status and body.
    pub fn new(status: u16, message: impl Into<String>, raw_body: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            raw_body: raw_body.into(),
        }
    }

    /// Check if this error represents a 404 Not Found response.
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        self.status == 404
    }

    /// Check if this error represents a 422 Unprocessable Entity response.
    #[must_use]
    pub fn is_validation_error(&self) -> bool {
        self.status == 422
    }

    /// Check if this error represents an authentication failure (401/403).
    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        matches!(self.status, 401 | 403)
    }

    /// Check if this error represents an upstream rate limit response.
    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        self.status == 429
    }
}

/// Input validation errors.
#[derive(Debug, Error)]
pub enum ValidationError {
    /// Required field is missing or empty.
    #[error("required field '{field}' is empty or missing")]
    EmptyField { field: String },

    /// Invalid format for a field.
    #[error("invalid format for {field}: {value}")]
    InvalidFormat { field: String, value: String },

    /// Invalid enum variant.
    #[error("unknown variant for {field}: {value}")]
    UnknownVariant { field: String, value: String },

    /// Missing required parameter.
    #[error("missing required parameter: {0}")]
    MissingParameter(String),

    /// Invalid parameter combination.
    #[error("invalid parameter combination: {0}")]
    InvalidParameterCombination(String),
}

/// Main error type for Intervals.icu client operations.
///
/// This enum provides a unified error type that covers all failure modes
/// while maintaining type safety and clear error categorization.
#[derive(Debug, Error)]
pub enum IntervalsError {
    /// Transport-level error (network, timeout, connection failure).
    ///
    /// Carries a `TransportError` (domain-owned) instead of a `reqwest::Error`
    /// directly so the public API does not depend on the HTTP client library.
    /// See ADR-0003.
    #[error("transport error: {0}")]
    Transport(TransportError),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    /// API returned an error response.
    #[error("API error: {0}")]
    Api(#[from] ApiError),

    /// Failed to parse JSON response.
    #[error("JSON decode error: {0}")]
    JsonDecode(#[from] serde_json::Error),

    /// Input validation failed.
    #[error("validation error: {0}")]
    Validation(#[from] ValidationError),

    /// Resource not found.
    #[error("resource not found: {0}")]
    NotFound(String),

    /// Authentication or authorization failed.
    #[error("authentication error: {0}")]
    Auth(String),

    /// I/O error during a file or stream operation (download to disk,
    /// file sync, etc.). Distinguished from `Http` because the underlying
    /// transport succeeded — the failure is on the persistence side.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Operation cancelled by an external signal (download watch channel,
    /// shutdown, etc.). String payload so callers can log human-readable
    /// reasons without inventing a sub-enum per cancellation source.
    #[error("operation cancelled: {reason}")]
    Cancelled { reason: String },

    /// Response body could not be decoded into the target domain type even
    /// though it was valid JSON. Distinct from `JsonDecode` (parse failure)
    /// — this means the payload exists but doesn't match the expected schema.
    /// `snippet` carries a bounded preview of the body for diagnostics.
    #[error("decode error: {message} — body: {snippet}")]
    Decode { message: String, snippet: String },
}

impl IntervalsError {
    /// Create an error from an API response status and body.
    ///
    /// This method maps HTTP status codes to appropriate error variants:
    /// - 404 -> `NotFound`
    /// - 401/403 -> `Auth`
    /// - 422 -> `Validation`
    /// - Other -> `Api`
    pub fn from_status(status: u16, body: impl Into<String>) -> Self {
        match status {
            404 => Self::NotFound(body.into()),
            401 | 403 => Self::Auth(body.into()),
            422 => Self::Validation(ValidationError::InvalidFormat {
                field: "request".to_string(),
                value: body.into(),
            }),
            _ => {
                let body = body.into();
                Self::Api(ApiError::new(status, body.clone(), body))
            }
        }
    }

    /// Check if this error represents a 404 Not Found response.
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        match self {
            Self::NotFound(_) => true,
            Self::Api(e) => e.is_not_found(),
            _ => false,
        }
    }

    /// Check if this error represents a 422 validation error.
    #[must_use]
    pub fn is_validation_error(&self) -> bool {
        match self {
            Self::Validation(_) => true,
            Self::Api(e) => e.is_validation_error(),
            _ => false,
        }
    }

    /// Check if this error represents an authentication failure.
    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        match self {
            Self::Auth(_) => true,
            Self::Api(e) => e.is_auth_error(),
            _ => false,
        }
    }

    /// Check if this error represents an upstream rate limit response.
    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        match self {
            Self::Api(e) => e.is_rate_limited(),
            _ => false,
        }
    }

    /// Check if this error represents a cancellation signal received mid-flight.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    /// Check if this error originated from local I/O (file or stream) rather
    /// than the network layer.
    #[must_use]
    pub fn is_io(&self) -> bool {
        matches!(self, Self::Io(_))
    }

    /// Check if this error represents a transport-level timeout.
    /// Returns `false` for non-transport errors.
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Transport(t) if t.is_timeout)
    }

    /// Check if this error represents a transport-level connection failure
    /// (refused, DNS failure, etc.). Returns `false` for non-transport errors.
    #[must_use]
    pub fn is_connect(&self) -> bool {
        matches!(self, Self::Transport(t) if t.is_connect)
    }
}

impl From<TransportError> for IntervalsError {
    fn from(e: TransportError) -> Self {
        Self::Transport(e)
    }
}

/// Result type alias for Intervals.icu operations.
pub type Result<T> = std::result::Result<T, IntervalsError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_is_not_found() {
        let err = ApiError::new(404, "not found", "body");
        assert!(err.is_not_found());
        assert!(!err.is_validation_error());
        assert!(!err.is_auth_error());
    }

    #[test]
    fn api_error_is_auth() {
        let err = ApiError::new(401, "unauthorized", "body");
        assert!(err.is_auth_error());
        assert!(!err.is_not_found());
    }

    #[test]
    fn api_error_is_validation() {
        let err = ApiError::new(422, "invalid", "body");
        assert!(err.is_validation_error());
        assert!(!err.is_not_found());
    }

    #[test]
    fn api_error_is_rate_limited() {
        let err = ApiError::new(429, "rate limit", "body");
        assert!(err.is_rate_limited());
    }

    #[test]
    fn intervals_error_from_status_404() {
        let err = IntervalsError::from_status(404, "not found");
        assert!(err.is_not_found());
    }

    #[test]
    fn intervals_error_from_status_401() {
        let err = IntervalsError::from_status(401, "unauthorized");
        assert!(err.is_auth_error());
    }

    #[test]
    fn intervals_error_from_status_422() {
        let err = IntervalsError::from_status(422, "invalid input");
        assert!(err.is_validation_error());
    }

    #[test]
    fn intervals_error_from_status_other() {
        let err = IntervalsError::from_status(500, "server error");
        assert!(!err.is_not_found());
        assert!(!err.is_auth_error());
        assert!(!err.is_validation_error());
    }

    #[test]
    fn intervals_error_from_status_429() {
        let err = IntervalsError::from_status(429, "rate limited");
        assert!(err.is_rate_limited());
    }

    #[test]
    fn validation_error_display() {
        let err = ValidationError::EmptyField {
            field: "name".to_string(),
        };
        assert_eq!(err.to_string(), "required field 'name' is empty or missing");
    }

    #[test]
    fn config_error_display() {
        let err = ConfigError::MissingEnvVar("API_KEY".to_string());
        assert_eq!(
            err.to_string(),
            "required environment variable API_KEY is not set"
        );
    }

    // ------------------------------------------------------------------
    // New variants (Phase 5b.1, ADR-0004): Io, Cancelled, Decode, Unsupported
    // ------------------------------------------------------------------

    #[test]
    fn intervals_error_io_from_std_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: IntervalsError = io_err.into();
        assert!(matches!(err, IntervalsError::Io(_)));
        assert!(err.to_string().contains("I/O error"));
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn intervals_error_io_vido_question_mark() {
        fn returns_io() -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "denied",
            ))
        }
        fn prop() -> Result<()> {
            returns_io()?;
            Ok(())
        }
        let err = prop().unwrap_err();
        assert!(matches!(err, IntervalsError::Io(_)));
    }

    #[test]
    fn intervals_error_cancelled_has_reason() {
        let err = IntervalsError::Cancelled {
            reason: "download cancelled".to_string(),
        };
        assert!(matches!(err, IntervalsError::Cancelled { .. }));
        assert_eq!(err.to_string(), "operation cancelled: download cancelled");
    }

    #[test]
    fn intervals_error_decode_carries_message_and_snippet() {
        let err = IntervalsError::Decode {
            message: "missing field id".to_string(),
            snippet: "{\"foo\":1}".to_string(),
        };
        assert!(matches!(err, IntervalsError::Decode { .. }));
        let s = err.to_string();
        assert!(s.contains("decode error"));
        assert!(s.contains("missing field id"));
        assert!(s.contains("{\"foo\":1}"));
    }

    #[test]
    fn intervals_error_decode_is_distinct_from_json_decode() {
        // The point of separation: Decode = valid JSON, wrong schema.
        // JsonDecode = parse failure. Two distinct variants by intent.
        let decode = IntervalsError::Decode {
            message: "schema".into(),
            snippet: "[]".into(),
        };
        let json_decode: IntervalsError =
            serde_json::from_str::<i32>("not_json").unwrap_err().into();
        assert!(matches!(decode, IntervalsError::Decode { .. }));
        assert!(matches!(json_decode, IntervalsError::JsonDecode(_)));
    }

    #[test]
    fn config_error_unsupported_carries_method_name() {
        let err = ConfigError::Unsupported {
            method: "list_routes",
        };
        assert_eq!(err.to_string(), "operation not implemented: list_routes");
    }

    #[test]
    fn config_error_other_keeps_legacy_message_passthrough() {
        let err = ConfigError::Other("legacy string".to_string());
        assert_eq!(err.to_string(), "configuration error: legacy string");
    }

    // ------------------------------------------------------------------
    // ADR-0003: TransportError abstraction
    // ------------------------------------------------------------------

    /// `TransportError` carries the original error's message unchanged so log
    /// lines and diagnostic surfaces remain stable across the transport swap.
    #[test]
    fn transport_error_display_uses_inner_message() {
        let err = crate::error::TransportError {
            message: "connection refused".to_string(),
            is_timeout: false,
            is_connect: true,
        };
        assert_eq!(err.to_string(), "connection refused");
    }

    /// `IntervalsError::Transport` is the new public surface replacing the
    /// leaky `IntervalsError::Http(#[from] reqwest::Error)`.
    #[test]
    fn intervals_error_transport_matches_new_variant() {
        let err = IntervalsError::Transport(crate::error::TransportError {
            message: "dns lookup failed".to_string(),
            is_timeout: false,
            is_connect: true,
        });
        assert!(matches!(err, IntervalsError::Transport(_)));
    }

    /// `is_timeout` is a first-class query on `IntervalsError` so call sites
    /// can branch on transport semantics without knowing the HTTP library.
    #[test]
    fn intervals_error_is_timeout_queries_transport() {
        let timeout = IntervalsError::Transport(crate::error::TransportError {
            message: "request timed out".to_string(),
            is_timeout: true,
            is_connect: false,
        });
        let non_timeout = IntervalsError::Transport(crate::error::TransportError {
            message: "connection refused".to_string(),
            is_timeout: false,
            is_connect: true,
        });
        assert!(timeout.is_timeout());
        assert!(!non_timeout.is_timeout());
    }

    /// `is_connect` mirrors `is_timeout` for connection-level failures.
    #[test]
    fn intervals_error_is_connect_queries_transport() {
        let connect = IntervalsError::Transport(crate::error::TransportError {
            message: "dns failure".to_string(),
            is_timeout: false,
            is_connect: true,
        });
        let timeout = IntervalsError::Transport(crate::error::TransportError {
            message: "read timeout".to_string(),
            is_timeout: true,
            is_connect: false,
        });
        assert!(connect.is_connect());
        assert!(!timeout.is_connect());
    }
}
