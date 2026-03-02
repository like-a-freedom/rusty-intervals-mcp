//! Error types for Intervals.icu API operations.
//!
//! This module provides a structured error hierarchy following Rust best practices:
//! - Typed errors for different failure modes
//! - Proper `Display` and `Error` trait implementations
//! - Support for error context and chaining via `thiserror`

use thiserror::Error;

/// Configuration-related errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Required environment variable is missing.
    #[error("required environment variable {0} is not set")]
    MissingEnvVar(String),

    /// Invalid configuration value.
    #[error("invalid configuration value for {key}: {message}")]
    InvalidValue { key: String, message: String },

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
    pub fn is_not_found(&self) -> bool {
        self.status == 404
    }

    /// Check if this error represents a 422 Unprocessable Entity response.
    pub fn is_validation_error(&self) -> bool {
        self.status == 422
    }

    /// Check if this error represents an authentication failure (401/403).
    pub fn is_auth_error(&self) -> bool {
        matches!(self.status, 401 | 403)
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
    /// HTTP client or network error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

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
        let body_str = body.into();
        match status {
            404 => Self::NotFound(body_str.clone()),
            401 | 403 => Self::Auth(body_str.clone()),
            422 => Self::Validation(ValidationError::InvalidFormat {
                field: "request".to_string(),
                value: body_str.clone(),
            }),
            _ => Self::Api(ApiError::new(status, body_str.clone(), body_str)),
        }
    }

    /// Check if this error represents a 404 Not Found response.
    pub fn is_not_found(&self) -> bool {
        match self {
            Self::NotFound(_) => true,
            Self::Api(e) => e.is_not_found(),
            _ => false,
        }
    }

    /// Check if this error represents a 422 validation error.
    pub fn is_validation_error(&self) -> bool {
        match self {
            Self::Validation(_) => true,
            Self::Api(e) => e.is_validation_error(),
            _ => false,
        }
    }

    /// Check if this error represents an authentication failure.
    pub fn is_auth_error(&self) -> bool {
        match self {
            Self::Auth(_) => true,
            Self::Api(e) => e.is_auth_error(),
            _ => false,
        }
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
}
