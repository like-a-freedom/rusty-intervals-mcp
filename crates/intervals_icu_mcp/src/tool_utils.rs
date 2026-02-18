//! Common utility functions for tool implementations.
//!
//! These utilities are intended for use in ongoing refactoring to reduce
//! code duplication across tool implementations.

#![allow(dead_code, clippy::extra_unused_type_parameters)]

use crate::{McpResult, ObjectResult};
use rmcp::Json;

/// Execute a tool operation with compact response handling.
///
/// This is a helper for the common pattern:
/// 1. Call client method
/// 2. Apply compact/filter transformation
/// 3. Return ObjectResult
pub async fn execute_compact_tool<F, Fut, T, C>(
    client_fut: Fut,
    compact: bool,
    fields: Option<&[String]>,
    compact_fn: C,
) -> McpResult<Json<ObjectResult>>
where
    Fut: std::future::Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
    C: FnOnce(&T, Option<&[String]>) -> serde_json::Value,
    T: serde::Serialize,
{
    let value = client_fut.await.map_err(|e| e.to_string())?;

    let result = if compact {
        compact_fn(&value, fields)
    } else if let Some(fields) = fields {
        crate::compact::filter_fields(&serde_json::to_value(&value)?, fields)
    } else {
        serde_json::to_value(&value)?
    };

    Ok(Json(ObjectResult { value: result }))
}

/// Execute a tool operation with array compact response handling.
pub async fn execute_array_compact_tool<F, Fut, T, C>(
    client_fut: Fut,
    compact: bool,
    fields: Option<&[String]>,
    compact_fn: C,
) -> McpResult<Json<ObjectResult>>
where
    Fut: std::future::Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
    C: FnOnce(&T, Option<&[String]>) -> serde_json::Value,
    T: serde::Serialize,
{
    let value = client_fut.await.map_err(|e| e.to_string())?;

    let result = if compact {
        compact_fn(&value, fields)
    } else if let Some(fields) = fields {
        crate::compact::filter_array_fields(&serde_json::to_value(&value)?, fields)
    } else {
        serde_json::to_value(&value)?
    };

    Ok(Json(ObjectResult { value: result }))
}

/// Normalize date string to YYYY-MM-DD format.
/// Accepts YYYY-MM-DD, RFC3339, or naive YYYY-MM-DDTHH:MM:SS.
pub fn normalize_date_str(s: &str) -> Result<String, String> {
    crate::domains::events::normalize_date_str(s).map_err(|_| format!("invalid date: {}", s))
}

/// Normalize event start date to ISO datetime format.
pub fn normalize_event_start(s: &str) -> Result<String, String> {
    crate::domains::events::normalize_event_start(s)
        .map_err(|_| format!("invalid start_date_local: {}", s))
}

/// Validate event and apply defaults.
pub fn validate_event(
    event: intervals_icu_client::Event,
) -> Result<intervals_icu_client::Event, String> {
    crate::domains::events::validate_and_prepare_event(event).map_err(|e| match e {
        crate::domains::events::EventValidationError::EmptyName => {
            "invalid event: name is empty".to_string()
        }
        crate::domains::events::EventValidationError::InvalidStartDate(s) => {
            format!("invalid start_date_local: {}", s)
        }
        crate::domains::events::EventValidationError::UnknownCategory => {
            "invalid category: unknown".to_string()
        }
    })
}
