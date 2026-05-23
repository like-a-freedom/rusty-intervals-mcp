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

mod activity;
mod athlete;
mod event;
mod fitness;
mod gear;
mod route;
mod sport_settings;
mod weather;
mod wellness;
mod workout;

pub use activity::ActivityService;
pub use athlete::AthleteService;
pub use event::EventService;
pub use fitness::FitnessService;
pub use gear::GearService;
pub use route::RouteService;
pub use sport_settings::SportSettingsService;
pub use weather::WeatherService;
pub use wellness::WellnessService;
pub use workout::WorkoutService;

// Re-export the main IntervalsClient trait from the parent module
// The blanket implementation was removed to avoid conflicts with existing code.
// Users can either:
// 1. Use the main IntervalsClient trait (monolithic, backward compatible)
// 2. Use individual service traits for better modularity
