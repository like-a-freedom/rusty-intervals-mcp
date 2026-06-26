//! Domain types for structured API responses.
//!
//! These types replace `serde_json::Value` returns in the most-used endpoints,
//! providing compile-time safety and documentation of expected shapes.

pub mod workout;
