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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ========================================================================
    // ParamLocation Tests
    // ========================================================================

    #[test]
    fn test_param_location_variants() {
        let path_loc = ParamLocation::Path;
        let query_loc = ParamLocation::Query;

        assert_ne!(path_loc, query_loc);
    }

    #[test]
    fn test_param_location_equality() {
        let path1 = ParamLocation::Path;
        let path2 = ParamLocation::Path;
        assert_eq!(path1, path2);

        let query1 = ParamLocation::Query;
        let query2 = ParamLocation::Query;
        assert_eq!(query1, query2);
    }

    // ========================================================================
    // ParamSpec Tests
    // ========================================================================

    #[test]
    fn test_param_spec_construction() {
        let spec = ParamSpec {
            name: "id".to_string(),
            location: ParamLocation::Path,
            auto_injected: false,
        };

        assert_eq!(spec.name, "id");
        assert_eq!(spec.location, ParamLocation::Path);
        assert!(!spec.auto_injected);
    }

    #[test]
    fn test_param_spec_auto_injected() {
        let spec = ParamSpec {
            name: "athlete_id".to_string(),
            location: ParamLocation::Query,
            auto_injected: true,
        };

        assert_eq!(spec.name, "athlete_id");
        assert!(spec.auto_injected);
    }

    // ========================================================================
    // DynamicOperation Tests
    // ========================================================================

    #[test]
    fn test_dynamic_operation_construction() {
        use rmcp::model::Tool;

        let tool = Tool::new(
            "test_tool".to_string(),
            "Test description".to_string(),
            std::sync::Arc::new(serde_json::Map::new()),
        );

        let op = DynamicOperation {
            name: "get_activity".to_string(),
            method: reqwest::Method::GET,
            path_template: "/api/activity/{id}".to_string(),
            description: "Get activity details".to_string(),
            params: vec![],
            has_json_body: false,
            tool,
            output_schema: None,
        };

        assert_eq!(op.name, "get_activity");
        assert_eq!(op.method, reqwest::Method::GET);
        assert_eq!(op.path_template, "/api/activity/{id}");
        assert!(!op.has_json_body);
        assert!(op.output_schema.is_none());
    }

    #[test]
    fn test_dynamic_operation_with_output_schema() {
        use rmcp::model::Tool;

        let tool = Tool::new(
            "test_tool".to_string(),
            "Test".to_string(),
            std::sync::Arc::new(serde_json::Map::new()),
        );

        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));

        let op = DynamicOperation {
            name: "test".to_string(),
            method: reqwest::Method::GET,
            path_template: "/test".to_string(),
            description: "Test".to_string(),
            params: vec![],
            has_json_body: false,
            tool,
            output_schema: Some(std::sync::Arc::new(schema)),
        };

        assert!(op.output_schema.is_some());
    }

    // ========================================================================
    // DynamicRegistry Tests
    // ========================================================================

    #[test]
    fn test_registry_new() {
        let registry = DynamicRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_default() {
        let registry = DynamicRegistry::default();
        assert!(registry.is_empty());
    }

    #[test]
    fn test_registry_insert_and_get() {
        use rmcp::model::Tool;

        let mut registry = DynamicRegistry::new();

        let tool = Tool::new(
            "test_tool".to_string(),
            "Test".to_string(),
            std::sync::Arc::new(serde_json::Map::new()),
        );

        let op = DynamicOperation {
            name: "test_op".to_string(),
            method: reqwest::Method::GET,
            path_template: "/test".to_string(),
            description: "Test operation".to_string(),
            params: vec![],
            has_json_body: false,
            tool,
            output_schema: None,
        };

        registry.insert("test_op".to_string(), op.clone());

        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());

        let retrieved = registry.operation("test_op");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test_op");
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let registry = DynamicRegistry::new();
        let result = registry.operation("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_registry_list_tools() {
        use rmcp::model::Tool;

        let mut registry = DynamicRegistry::new();

        // Add tools in non-alphabetical order
        for name in ["zebra", "alpha", "middle"] {
            let tool = Tool::new(
                name.to_string(),
                "Test".to_string(),
                std::sync::Arc::new(serde_json::Map::new()),
            );

            let op = DynamicOperation {
                name: name.to_string(),
                method: reqwest::Method::GET,
                path_template: "/test".to_string(),
                description: "Test".to_string(),
                params: vec![],
                has_json_body: false,
                tool,
                output_schema: None,
            };

            registry.insert(name.to_string(), op);
        }

        let tools = registry.list_tools();
        assert_eq!(tools.len(), 3);

        // Verify sorted order
        assert_eq!(tools[0].name, "alpha");
        assert_eq!(tools[1].name, "middle");
        assert_eq!(tools[2].name, "zebra");
    }

    #[test]
    fn test_registry_len() {
        use rmcp::model::Tool;

        let mut registry = DynamicRegistry::new();
        assert_eq!(registry.len(), 0);

        let tool = Tool::new(
            "test".to_string(),
            "Test".to_string(),
            std::sync::Arc::new(serde_json::Map::new()),
        );

        registry.insert(
            "op1".to_string(),
            DynamicOperation {
                name: "op1".to_string(),
                method: reqwest::Method::GET,
                path_template: "/test".to_string(),
                description: "Test".to_string(),
                params: vec![],
                has_json_body: false,
                tool: tool.clone(),
                output_schema: None,
            },
        );

        assert_eq!(registry.len(), 1);

        registry.insert(
            "op2".to_string(),
            DynamicOperation {
                name: "op2".to_string(),
                method: reqwest::Method::GET,
                path_template: "/test".to_string(),
                description: "Test".to_string(),
                params: vec![],
                has_json_body: false,
                tool,
                output_schema: None,
            },
        );

        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_registry_overwrite() {
        use rmcp::model::Tool;

        let mut registry = DynamicRegistry::new();

        let tool1 = Tool::new(
            "test".to_string(),
            "First".to_string(),
            std::sync::Arc::new(serde_json::Map::new()),
        );

        registry.insert(
            "test".to_string(),
            DynamicOperation {
                name: "test".to_string(),
                method: reqwest::Method::GET,
                path_template: "/test1".to_string(),
                description: "First".to_string(),
                params: vec![],
                has_json_body: false,
                tool: tool1,
                output_schema: None,
            },
        );

        let tool2 = Tool::new(
            "test".to_string(),
            "Second".to_string(),
            std::sync::Arc::new(serde_json::Map::new()),
        );

        registry.insert(
            "test".to_string(),
            DynamicOperation {
                name: "test".to_string(),
                method: reqwest::Method::POST,
                path_template: "/test2".to_string(),
                description: "Second".to_string(),
                params: vec![],
                has_json_body: false,
                tool: tool2,
                output_schema: None,
            },
        );

        // Should have only one entry (overwritten)
        assert_eq!(registry.len(), 1);
        let op = registry.operation("test").unwrap();
        assert_eq!(op.method, reqwest::Method::POST);
        assert_eq!(op.path_template, "/test2");
    }

    #[test]
    fn test_registry_multiple_operations() {
        use rmcp::model::Tool;

        let mut registry = DynamicRegistry::new();

        for i in 1..=5 {
            let tool = Tool::new(
                format!("op{}", i),
                format!("Operation {}", i),
                std::sync::Arc::new(serde_json::Map::new()),
            );

            registry.insert(
                format!("op{}", i),
                DynamicOperation {
                    name: format!("op{}", i),
                    method: reqwest::Method::GET,
                    path_template: format!("/test/{}", i),
                    description: format!("Operation {}", i),
                    params: vec![],
                    has_json_body: false,
                    tool,
                    output_schema: None,
                },
            );
        }

        assert_eq!(registry.len(), 5);

        // Verify all operations are retrievable
        for i in 1..=5 {
            let op = registry.operation(&format!("op{}", i));
            assert!(op.is_some());
            assert_eq!(op.unwrap().path_template, format!("/test/{}", i));
        }
    }

    #[test]
    fn test_param_spec_clone() {
        let spec = ParamSpec {
            name: "test".to_string(),
            location: ParamLocation::Query,
            auto_injected: true,
        };

        let cloned = spec.clone();
        assert_eq!(cloned.name, spec.name);
        assert_eq!(cloned.location, spec.location);
        assert_eq!(cloned.auto_injected, spec.auto_injected);
    }

    #[test]
    fn test_dynamic_operation_clone() {
        use rmcp::model::Tool;

        let tool = Tool::new(
            "test".to_string(),
            "Test".to_string(),
            std::sync::Arc::new(serde_json::Map::new()),
        );

        let op = DynamicOperation {
            name: "test_op".to_string(),
            method: reqwest::Method::GET,
            path_template: "/test".to_string(),
            description: "Test operation".to_string(),
            params: vec![ParamSpec {
                name: "id".to_string(),
                location: ParamLocation::Path,
                auto_injected: false,
            }],
            has_json_body: false,
            tool,
            output_schema: None,
        };

        let cloned = op.clone();
        assert_eq!(cloned.name, op.name);
        assert_eq!(cloned.method, op.method);
        assert_eq!(cloned.params.len(), op.params.len());
    }
}
