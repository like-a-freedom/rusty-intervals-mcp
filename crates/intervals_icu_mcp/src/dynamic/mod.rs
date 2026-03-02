//! Dynamic OpenAPI tool generation module.
//!
//! This module provides runtime generation of MCP tools from the Intervals.icu OpenAPI spec.
//! It supports:
//! - Automatic tool generation from OpenAPI spec
//! - Tag-based filtering (include/exclude)
//! - Caching with configurable refresh intervals
//! - Compact response mode for token efficiency

mod dispatch;
mod parser;
mod runtime;
mod types;

pub use dispatch::dispatch_operation;
pub use parser::parse_openapi_spec;
pub use runtime::{DynamicRuntime, DynamicRuntimeConfig, DynamicRuntimeConfigBuilder};
pub use types::{DynamicOperation, DynamicRegistry, ParamLocation, ParamSpec};
