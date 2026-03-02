//! Modular service traits for Intervals.icu API operations.
//!
//! This module splits the monolithic `IntervalsClient` trait into cohesive,
//! domain-specific service traits following the **Single Responsibility Principle**.
//!
//! # Benefits
//! - **Testability**: Mock only the services you need
//! - **Modularity**: Clear separation of concerns
//! - **Composability**: Mix and match service implementations
//! - **Maintainability**: Easier to understand and modify

mod athlete;
mod activity;
mod event;
mod fitness;
mod gear;
mod wellness;
mod workout;
mod sport_settings;

pub use athlete::AthleteService;
pub use activity::ActivityService;
pub use event::EventService;
pub use fitness::FitnessService;
pub use gear::GearService;
pub use wellness::WellnessService;
pub use workout::WorkoutService;
pub use sport_settings::SportSettingsService;

// Re-export the main IntervalsClient trait from the parent module
// The blanket implementation was removed to avoid conflicts with existing code.
// Users can either:
// 1. Use the main IntervalsClient trait (monolithic, backward compatible)
// 2. Use individual service traits for better modularity
