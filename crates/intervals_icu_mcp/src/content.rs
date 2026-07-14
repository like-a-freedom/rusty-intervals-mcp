use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Content block for rich text output — used by both engine reports and MCP transport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Markdown {
        markdown: String,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
    pub fn markdown(markdown: impl Into<String>) -> Self {
        Self::Markdown {
            markdown: markdown.into(),
        }
    }
    pub fn table(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self::Table { headers, rows }
    }
}

/// Error type for intent execution — used by both engine return types and intent handlers.
#[derive(Debug, thiserror::Error)]
pub enum IntentError {
    #[error("Unknown intent: {0}")]
    UnknownIntent(String),
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Idempotency conflict: {0}")]
    IdempotencyConflict(String),
    #[error("API client error: {0}")]
    ApiClientError(String),
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl IntentError {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::ValidationError(msg.into())
    }
    pub fn api(msg: impl Into<String>) -> Self {
        Self::ApiClientError(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::InternalError(msg.into())
    }
}

/// Metadata for output pagination and aggregation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_created: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_modified: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_deleted: Option<u32>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl OutputMetadata {
    pub fn is_empty(&self) -> bool {
        self.has_more.is_none()
            && self.next_offset.is_none()
            && self.total_count.is_none()
            && self.events_created.is_none()
            && self.events_modified.is_none()
            && self.events_deleted.is_none()
            && self.extra.is_empty()
    }
}
