//! Domain modules for business logic encapsulation.
//!
//! This module follows the **Information Expert** GRASP principle by:
//! - Encapsulating domain-specific logic in dedicated modules
//! - Keeping validation, transformation, and compaction logic close to the data
//! - Providing clear boundaries between different business concerns
//!
//! # Modules
//!
//! - [`activity_analysis`]: Activity data transformations and analysis
//! - [`events`]: Event/calendar validation and compaction
//! - [`fitness`]: Fitness metrics and CTL/ATL/TSB analysis
//! - [`gear`]: Gear management and compaction
//! - [`resources`]: MCP resource handling
//! - [`sport_settings`]: Sport settings filtering and compaction
//! - [`wellness`]: Wellness data summarization
//! - [`workouts`]: Workout library operations

pub mod activity_analysis;
pub mod events;
pub mod fitness;
pub mod gear;
pub mod resources;
pub mod sport_settings;
pub mod wellness;
pub mod workouts;

// Re-export commonly used constants for convenience
pub use gear::DEFAULT_FIELDS as GEAR_DEFAULT_FIELDS;
pub use sport_settings::DEFAULT_FIELDS as SPORT_SETTINGS_DEFAULT_FIELDS;
pub use wellness::DEFAULT_FIELDS as WELLNESS_DEFAULT_FIELDS;
