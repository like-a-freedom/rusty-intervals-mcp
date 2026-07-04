//! Fetch-layer error type for the engines crate.
//!
//! Encapsulates all failures that can occur during data fetching
//! *before* reaching the intent layer.  Handlers map these into
//! [`IntentError`](crate::intents::IntentError) at the call site.

use std::fmt;

/// Errors originating in the fetch / analysis-fetch layer.
///
/// This type keeps the engines layer free of dependencies on the
/// intents layer, satisfying the DDD rule that inner layers must
/// not reference outer layers.
#[derive(Debug)]
pub enum FetchError {
    /// The requested date range is invalid.
    InvalidDateRange(String),
    /// An upstream API or I/O error occurred.
    ClientError(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDateRange(msg) => {
                write!(f, "invalid date range: {msg}")
            }
            Self::ClientError(msg) => {
                write!(f, "client error: {msg}")
            }
        }
    }
}

impl std::error::Error for FetchError {}
