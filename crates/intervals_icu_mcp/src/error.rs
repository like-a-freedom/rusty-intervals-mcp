//! Custom error types for the MCP server.

use thiserror::Error;

/// MCP server errors.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("API error: {0}")]
    Api(#[from] intervals_icu_client::IntervalsError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Download error: {0}")]
    Download(String),

    #[error("Webhook error: {0}")]
    Webhook(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<String> for McpError {
    fn from(err: String) -> Self {
        McpError::Internal(err)
    }
}

impl From<McpError> for String {
    fn from(err: McpError) -> Self {
        err.to_string()
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for McpError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        McpError::Internal(err.to_string())
    }
}

/// Result type alias for MCP operations.
pub type McpResult<T> = Result<T, McpError>;
