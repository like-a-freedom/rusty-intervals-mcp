//! Type definitions for dynamic OpenAPI tool generation.

use rmcp::model::Tool;
use std::collections::HashMap;

/// Location of a parameter in an HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamLocation {
    Path,
    Query,
}

/// Specification for a single parameter.
#[derive(Debug, Clone)]
pub struct ParamSpec {
    pub name: String,
    pub location: ParamLocation,
    pub auto_injected: bool,
}

/// A dynamic operation generated from OpenAPI spec.
#[derive(Debug, Clone)]
pub struct DynamicOperation {
    pub name: String,
    pub method: reqwest::Method,
    pub path_template: String,
    pub description: String,
    pub params: Vec<ParamSpec>,
    pub has_json_body: bool,
    pub tool: Tool,
    pub output_schema: Option<std::sync::Arc<rmcp::model::JsonObject>>,
}

/// Registry of dynamically generated operations.
#[derive(Debug, Clone)]
pub struct DynamicRegistry {
    operations: HashMap<String, DynamicOperation>,
}

impl DynamicRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            operations: HashMap::new(),
        }
    }

    /// Get an operation by name.
    pub fn operation(&self, name: &str) -> Option<&DynamicOperation> {
        self.operations.get(name)
    }

    /// List all tools sorted by name.
    pub fn list_tools(&self) -> Vec<Tool> {
        let mut tools: Vec<Tool> = self.operations.values().map(|o| o.tool.clone()).collect();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    /// Get the number of operations.
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Insert an operation into the registry.
    pub fn insert(&mut self, name: String, operation: DynamicOperation) {
        self.operations.insert(name, operation);
    }
}

impl Default for DynamicRegistry {
    fn default() -> Self {
        Self::new()
    }
}
