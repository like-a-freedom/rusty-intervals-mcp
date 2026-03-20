pub mod analysis;
/// Engines module - Planning and Analysis engines
///
/// This module provides the business logic engines for:
/// - Shared analytical fetching / auditing for existing coaching intents
/// - Multi-scale planning (Microcycle → Mesocycle → Macrocycle → Annual)
/// - Training analysis (single workout, period, trends, comparisons)
pub mod analysis_audit;
pub mod analysis_fetch;
pub mod coach_guidance;
pub mod coach_metrics;
pub mod planning;

pub use analysis::{AnalysisEngine, WorkoutInsights};
pub use planning::PeriodizationRules;
