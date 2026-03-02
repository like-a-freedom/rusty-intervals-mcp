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

    /// List all tools sorted by intent-aware priority and then by name.
    pub fn list_tools(&self) -> Vec<Tool> {
        let mut tools: Vec<Tool> = self.operations.values().map(|o| o.tool.clone()).collect();
        tools.sort_by(|a, b| {
            tool_priority(&a.name)
                .cmp(&tool_priority(&b.name))
                .then_with(|| a.name.cmp(&b.name))
        });
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

fn tool_priority(tool_name: &str) -> u8 {
    match tool_name {
        // General activity discovery and lookup tools should appear first.
        "listActivities"
        | "searchForActivities"
        | "searchForActivitiesFull"
        | "getActivity"
        | "listActivitiesAround" => 0,

        // High-signal profile/context tools come next.
        "getAthlete"
        | "getAthleteProfile"
        | "getFitnessSummary"
        | "listEvents"
        | "listWellnessRecords" => 1,

        // Curves are intentionally listed later to reduce accidental anchoring.
        "listAthletePowerCurves" | "listAthleteHRCurves" | "listAthletePaceCurves" => 3,

        // Everything else remains in a stable alphabetical order within this group.
        _ => 2,
    }
}

impl Default for DynamicRegistry {
    fn default() -> Self {
        Self::new()
    }
}
