//! Dedicated analysis engine for `analyze_training` intent.
//!
//! Fetches and computes analysis data; intent handlers render the report.

pub(crate) mod compare;
pub(crate) mod period;
pub(crate) mod shared;
pub(crate) mod single;

pub(crate) mod engine;

pub use engine::{analyze_period, analyze_single, compare_periods};

pub use shared::AnalyzeReport;
