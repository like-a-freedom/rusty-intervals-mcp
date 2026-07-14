//! Dedicated analysis engine for `analyze_training` intent.
//!
//! Owns the full fetch→compute→render pipeline for single-workout and period
//! analysis.  The intent handler validates input and dispatches here.

pub(crate) mod render;

pub(crate) mod compare;
pub(crate) mod period;
pub(crate) mod shared;
pub(crate) mod single;

pub(crate) mod engine;

pub use engine::{analyze_period, analyze_single, compare_periods};
