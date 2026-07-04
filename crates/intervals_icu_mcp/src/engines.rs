pub mod adaptation;
pub mod ade;
pub mod analysis;
pub mod analysis_audit;
pub mod analysis_fetch;
pub mod changepoint;
pub mod coach_guidance;
pub mod coach_metrics;
pub mod coach_metrics_constants;
pub mod cp_regression;
pub mod fetch_error;
pub mod forecast;
pub mod progress_tracking;
pub mod race_readiness;
pub mod shared;
pub mod trail_execution;

pub use analysis::{AnalysisEngine, WorkoutInsights};
