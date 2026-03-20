//! OpenAPI spec parser for dynamic tool generation.

use crate::dynamic::types::{DynamicOperation, DynamicRegistry, ParamLocation, ParamSpec};
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};

/// Parse an OpenAPI spec into a registry of dynamic operations.
pub fn parse_openapi_spec(
    spec: &Value,
    include_tags: &HashSet<String>,
    exclude_tags: &HashSet<String>,
) -> Result<DynamicRegistry, String> {
    let mut registry = DynamicRegistry::new();
    let Some(paths_obj) = spec.get("paths").and_then(Value::as_object) else {
        return Err("OpenAPI spec has no 'paths' object".to_string());
    };

    for (path, path_item) in paths_obj {
        let Some(path_item_obj) = path_item.as_object() else {
            continue;
        };

        let path_level_params = path_item_obj
            .get("parameters")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for method_name in ["get", "post", "put", "patch", "delete", "head", "options"] {
            let Some(op) = path_item_obj.get(method_name).and_then(Value::as_object) else {
                continue;
            };

            let operation_tags = extract_operation_tags(op);
            if should_filter_operation(&operation_tags, include_tags, exclude_tags) {
                continue;
            }

            if contains_multipart(op) {
                continue;
            }

            let operation_id = op.get("operationId").and_then(Value::as_str);
            let name = operation_id
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| generate_operation_name(method_name, path));

            let method = match method_name.to_ascii_uppercase().parse::<reqwest::Method>() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let description = op
                .get("summary")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    op.get("description")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| generate_human_readable_description(method_name, path));

            let (params, schema_props, mut required) =
                extract_parameters(op, &path_level_params, path);

            let has_json_body = op
                .get("requestBody")
                .and_then(|rb| rb.get("content"))
                .and_then(Value::as_object)
                .is_some_and(|content| content.contains_key("application/json"));

            let mut final_schema_props = schema_props;
            if has_json_body {
                add_body_parameter(&mut final_schema_props, op, &mut required);
            }

            add_response_control_properties(&mut final_schema_props);

            let has_suffix = final_schema_props.get("ext").is_some()
                || final_schema_props.get("format").is_some();

            // Build output schema from OpenAPI responses
            let output_schema = build_output_schema(op);

            let tool = build_tool(
                &name,
                &description,
                final_schema_props,
                &required,
                &method,
                has_suffix,
                output_schema.clone(),
            )?;

            registry.insert(
                name.clone(),
                DynamicOperation {
                    name,
                    method,
                    path_template: path.to_string(),
                    description,
                    params,
                    has_json_body,
                    tool,
                    output_schema: output_schema.map(std::sync::Arc::new),
                },
            );
        }
    }

    Ok(registry)
}

fn extract_operation_tags(op: &Map<String, Value>) -> Vec<String> {
    op.get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn should_filter_operation(
    operation_tags: &[String],
    include_tags: &HashSet<String>,
    exclude_tags: &HashSet<String>,
) -> bool {
    if !include_tags.is_empty() {
        return !operation_tags.iter().any(|t| include_tags.contains(t));
    }
    if !exclude_tags.is_empty() {
        return operation_tags.iter().any(|t| exclude_tags.contains(t));
    }
    false
}

fn contains_multipart(op: &Map<String, Value>) -> bool {
    op.get("requestBody")
        .and_then(|rb| rb.get("content"))
        .and_then(Value::as_object)
        .is_some_and(|content| content.keys().any(|k| k.contains("multipart")))
}

fn extract_parameters(
    op: &Map<String, Value>,
    path_level_params: &[Value],
    path: &str,
) -> (Vec<ParamSpec>, Map<String, Value>, Vec<Value>) {
    let mut merged_params = BTreeMap::<(String, String), Value>::new();
    for p in path_level_params {
        if let Some(key) = param_key(p) {
            merged_params.insert(key, p.clone());
        }
    }
    for p in op
        .get("parameters")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        if let Some(key) = param_key(&p) {
            merged_params.insert(key, p);
        }
    }

    let mut params = Vec::new();
    let mut schema_props = Map::new();
    let mut required = Vec::new();

    for p in merged_params.values() {
        let Some(name_s) = p.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(in_s) = p.get("in").and_then(Value::as_str) else {
            continue;
        };
        let required_param = p.get("required").and_then(Value::as_bool).unwrap_or(false);
        let auto_injected = is_auto_injected_athlete_param(name_s, in_s, path);

        let location = match in_s {
            "path" => ParamLocation::Path,
            "query" => ParamLocation::Query,
            _ => continue,
        };

        params.push(ParamSpec {
            name: name_s.to_string(),
            location,
            auto_injected,
        });

        if auto_injected {
            continue;
        }

        let mut schema = parameter_schema(p);
        if (name_s == "ext" || name_s == "format")
            && schema.get("default").is_none()
            && let Some(schema_obj) = schema.as_object_mut()
        {
            schema_obj.insert("default".to_string(), Value::String(String::new()));
            if let Some(Value::String(desc)) = schema_obj.get_mut("description") {
                desc.push_str(" Omit to use default JSON endpoint suffix.");
            }
        }
        schema_props.insert(name_s.to_string(), schema);

        // Add activity_id alias for id path param
        if name_s == "id" && !schema_props.contains_key("activity_id") {
            schema_props.insert("activity_id".to_string(), parameter_schema(p));
        }

        let treat_format_as_optional = name_s == "ext" || name_s == "format";
        if required_param && !treat_format_as_optional {
            required.push(Value::String(name_s.to_string()));
        }
    }

    (params, schema_props, required)
}

fn param_key(value: &Value) -> Option<(String, String)> {
    let obj = value.as_object()?;
    let name = obj.get("name")?.as_str()?.to_string();
    let location = obj.get("in")?.as_str()?.to_string();
    Some((name, location))
}

fn is_auto_injected_athlete_param(name: &str, in_location: &str, path: &str) -> bool {
    let normalized_name = name.to_ascii_lowercase();
    normalized_name == "athlete_id"
        || normalized_name == "athleteid"
        || (normalized_name == "id"
            && in_location == "path"
            && (path.contains("/athlete/{id}") || path.ends_with("/athlete/{id}")))
}

fn parameter_schema(param: &Value) -> Value {
    param
        .get("schema")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "type": "string" }))
}

fn add_body_parameter(
    schema_props: &mut Map<String, Value>,
    op: &Map<String, Value>,
    required: &mut Vec<Value>,
) {
    schema_props.insert(
        "body".to_string(),
        serde_json::json!({
            "type": "object",
            "description": "JSON request body"
        }),
    );

    let body_required = op
        .get("requestBody")
        .and_then(|rb| rb.get("required"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if body_required {
        required.push(Value::String("body".to_string()));
    }
}

fn add_response_control_properties(schema_props: &mut Map<String, Value>) {
    schema_props.insert(
        "compact".to_string(),
        serde_json::json!({
            "type": "boolean",
            "description": "Return compact summary (default: false)"
        }),
    );
    schema_props.insert(
        "fields".to_string(),
        serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
            "description": "Specific fields to include in response"
        }),
    );
    schema_props.insert(
        "body_only".to_string(),
        serde_json::json!({
            "type": "boolean",
            "description": "Return only response body, omit status/content_type (default: true)"
        }),
    );
}

fn build_tool(
    name: &str,
    description: &str,
    schema_props: Map<String, Value>,
    required: &[Value],
    method: &reqwest::Method,
    has_suffix: bool,
    output_schema: Option<Map<String, Value>>,
) -> Result<Tool, String> {
    let mut input_schema = Map::new();
    input_schema.insert("type".to_string(), Value::String("object".to_string()));
    input_schema.insert("properties".to_string(), Value::Object(schema_props));
    if !required.is_empty() {
        input_schema.insert("required".to_string(), Value::Array(required.to_vec()));
    }

    let schema_obj: rmcp::model::JsonObject =
        match serde_json::from_value(Value::Object(input_schema)) {
            Ok(s) => s,
            Err(e) => {
                return Err(format!("failed to build schema for {name}: {e}"));
            }
        };

    let mut full_desc = description.to_string();
    full_desc.push_str(" (supports compact, fields, and body_only response options)");
    if has_suffix {
        full_desc.push_str(" Path suffix parameter is optional and defaults to empty.");
    }

    let mut tool = Tool::new(name.to_string(), full_desc, std::sync::Arc::new(schema_obj));
    tool.annotations = Some(method_to_annotations(method));

    // Set output schema if available
    if let Some(output_schema_obj) = output_schema
        && let Ok(output_schema_arc) =
            serde_json::from_value::<rmcp::model::JsonObject>(Value::Object(output_schema_obj))
    {
        tool.output_schema = Some(std::sync::Arc::new(output_schema_arc));
    }

    Ok(tool)
}

fn method_to_annotations(method: &reqwest::Method) -> ToolAnnotations {
    let is_read_only = method == reqwest::Method::GET || method == reqwest::Method::HEAD;
    let is_idempotent = method == reqwest::Method::GET
        || method == reqwest::Method::HEAD
        || method == reqwest::Method::PUT
        || method == reqwest::Method::DELETE;

    ToolAnnotations::new()
        .read_only(is_read_only)
        .destructive(method == reqwest::Method::DELETE)
        .idempotent(is_idempotent)
}

fn generate_operation_name(method: &str, path: &str) -> String {
    let method_prefix = method.to_ascii_lowercase();
    let path_parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .collect();

    let path_suffix = path_parts.join("_");
    if path_suffix.is_empty() {
        method_prefix
    } else {
        format!("{method_prefix}_{path_suffix}")
    }
}

fn generate_human_readable_description(method: &str, path: &str) -> String {
    let action = match method {
        "get" => "Retrieve",
        "post" => "Create",
        "put" => "Update",
        "patch" => "Patch",
        "delete" => "Delete",
        _ => "Execute",
    };

    let path_clean = path.trim_matches('/').replace(['{', '}'], "");
    format!("{action} {path_clean}")
}

fn preferred_success_response(responses: &Map<String, Value>) -> Option<&Value> {
    responses
        .get("200")
        .or_else(|| responses.get("201"))
        .or_else(|| responses.get("204"))
        .or_else(|| {
            responses
                .iter()
                .find(|(k, _)| k.starts_with('2'))
                .map(|(_, v)| v)
        })
}

fn preferred_json_media_type(content: &Map<String, Value>) -> Option<&Value> {
    content.get("application/json").or_else(|| {
        content
            .iter()
            .find(|(k, _)| k.contains("json"))
            .map(|(_, v)| v)
    })
}

fn normalize_output_schema(schema: &Map<String, Value>) -> Map<String, Value> {
    if let Some(ref_val) = schema.get("$ref").and_then(Value::as_str) {
        let mut ref_schema = Map::new();
        ref_schema.insert("type".to_string(), Value::String("object".to_string()));
        ref_schema.insert(
            "description".to_string(),
            Value::String(format!("Response schema (ref: {})", ref_val)),
        );
        ref_schema
    } else {
        schema.clone()
    }
}

/// Build output schema from OpenAPI operation responses.
/// Extracts schema from responses.200.content.application/json.schema
fn build_output_schema(op: &Map<String, Value>) -> Option<Map<String, Value>> {
    op.get("responses")
        .and_then(Value::as_object)
        .and_then(preferred_success_response)
        .and_then(Value::as_object)
        .and_then(|response| response.get("content"))
        .and_then(Value::as_object)
        .and_then(preferred_json_media_type)
        .and_then(Value::as_object)
        .and_then(|media_type| media_type.get("schema"))
        .and_then(Value::as_object)
        .map(normalize_output_schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ========================================================================
    // parse_openapi_spec() Tests
    // ========================================================================

    #[test]
    fn test_parse_empty_spec() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {}
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_spec_no_paths() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"}
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("paths"));
    }

    #[test]
    fn test_parse_simple_path() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/activities": {
                    "get": {
                        "operationId": "get_activities",
                        "summary": "Get all activities",
                        "parameters": []
                    }
                }
            }
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        let registry = result.unwrap();
        assert_eq!(registry.len(), 1);

        let op = registry.operation("get_activities").unwrap();
        assert_eq!(op.name, "get_activities");
        assert_eq!(op.method, reqwest::Method::GET);
        assert_eq!(op.path_template, "/activities");
    }

    #[test]
    fn test_parse_path_with_parameter() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/activities/{id}": {
                    "get": {
                        "operationId": "get_activity",
                        "summary": "Get activity by ID",
                        "parameters": [{
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"}
                        }]
                    }
                }
            }
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        let registry = result.unwrap();
        assert_eq!(registry.len(), 1);

        let op = registry.operation("get_activity").unwrap();
        assert_eq!(op.params.len(), 1);
        assert_eq!(op.params[0].name, "id");
    }

    #[test]
    fn test_parse_multiple_methods_same_path() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/activities": {
                    "get": {
                        "operationId": "list_activities",
                        "summary": "List activities",
                        "parameters": []
                    },
                    "post": {
                        "operationId": "create_activity",
                        "summary": "Create activity",
                        "parameters": [],
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {"type": "object"}
                                }
                            }
                        }
                    }
                }
            }
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        let registry = result.unwrap();
        assert_eq!(registry.len(), 2);

        let get_op = registry.operation("list_activities").unwrap();
        assert_eq!(get_op.method, reqwest::Method::GET);
        assert!(!get_op.has_json_body);

        let post_op = registry.operation("create_activity").unwrap();
        assert_eq!(post_op.method, reqwest::Method::POST);
        assert!(post_op.has_json_body);
    }

    #[test]
    fn test_parse_with_tags() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/activities": {
                    "get": {
                        "operationId": "get_activities",
                        "summary": "Get activities",
                        "tags": ["activities"],
                        "parameters": []
                    },
                    "post": {
                        "operationId": "create_activity",
                        "summary": "Create activity",
                        "tags": ["admin"],
                        "parameters": [],
                        "requestBody": {
                            "content": {"application/json": {"schema": {"type": "object"}}}
                        }
                    }
                }
            }
        });

        // Include only "activities" tag
        let mut include_tags = HashSet::new();
        include_tags.insert("activities".to_string());

        let result = parse_openapi_spec(&spec, &include_tags, &HashSet::new());
        assert!(result.is_ok());
        let registry = result.unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry.operation("get_activities").is_some());
        assert!(registry.operation("create_activity").is_none());
    }

    #[test]
    fn test_parse_exclude_tags() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/activities": {
                    "get": {
                        "operationId": "get_activities",
                        "summary": "Get activities",
                        "tags": ["activities"],
                        "parameters": []
                    },
                    "delete": {
                        "operationId": "delete_activity",
                        "summary": "Delete activity",
                        "tags": ["admin"],
                        "parameters": []
                    }
                }
            }
        });

        // Exclude "admin" tag
        let mut exclude_tags = HashSet::new();
        exclude_tags.insert("admin".to_string());

        let result = parse_openapi_spec(&spec, &HashSet::new(), &exclude_tags);
        assert!(result.is_ok());
        let registry = result.unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry.operation("get_activities").is_some());
        assert!(registry.operation("delete_activity").is_none());
    }

    #[test]
    fn test_parse_skips_multipart() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/upload": {
                    "post": {
                        "operationId": "upload_file",
                        "summary": "Upload file",
                        "requestBody": {
                            "content": {
                                "multipart/form-data": {
                                    "schema": {"type": "object"}
                                }
                            }
                        }
                    }
                }
            }
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_generates_operation_id() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/activities": {
                    "get": {
                        "summary": "Get activities",
                        "parameters": []
                    }
                }
            }
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        let registry = result.unwrap();
        assert_eq!(registry.len(), 1);

        // Should generate ID from method and path
        let op = registry.operation("get_activities").unwrap();
        assert_eq!(op.name, "get_activities");
    }

    #[test]
    fn test_parse_generates_description() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/activities": {
                    "get": {
                        "operationId": "get_activities",
                        "parameters": []
                    }
                }
            }
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        let registry = result.unwrap();
        let op = registry.operation("get_activities").unwrap();
        assert!(op.description.contains("Retrieve"));
        assert!(op.description.contains("activities"));
    }

    #[test]
    fn test_parse_uses_summary_over_description() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/activities": {
                    "get": {
                        "operationId": "get_activities",
                        "summary": "Custom Summary",
                        "description": "Custom Description",
                        "parameters": []
                    }
                }
            }
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        let registry = result.unwrap();
        let op = registry.operation("get_activities").unwrap();
        assert!(op.description.contains("Custom Summary"));
    }

    #[test]
    fn test_parse_path_level_params() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/athlete/{athlete_id}/activities": {
                    "parameters": [{
                        "name": "athlete_id",
                        "in": "path",
                        "required": true,
                        "schema": {"type": "string"}
                    }],
                    "get": {
                        "operationId": "get_athlete_activities",
                        "summary": "Get athlete activities",
                        "parameters": [{
                            "name": "limit",
                            "in": "query",
                            "schema": {"type": "integer"}
                        }]
                    }
                }
            }
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        let registry = result.unwrap();
        let op = registry.operation("get_athlete_activities").unwrap();

        // Should have both path-level and operation-level params
        assert_eq!(op.params.len(), 2);
    }

    #[test]
    fn test_parse_adds_response_control_params() {
        let spec = json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/activities": {
                    "get": {
                        "operationId": "get_activities",
                        "summary": "Get activities",
                        "parameters": []
                    }
                }
            }
        });

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok());
        let registry = result.unwrap();
        let op = registry.operation("get_activities").unwrap();

        // Verify tool was created with proper structure
        assert_eq!(op.tool.name, "get_activities");
        let desc = op
            .tool
            .description
            .as_ref()
            .map(|s| s.as_ref())
            .unwrap_or("");
        assert!(desc.contains("compact"));
        assert!(desc.contains("fields"));
        assert!(desc.contains("body_only"));
    }

    // ========================================================================
    // Helper Function Tests
    // ========================================================================

    #[test]
    fn test_generate_operation_name() {
        assert_eq!(
            generate_operation_name("get", "/activities"),
            "get_activities"
        );
        assert_eq!(
            generate_operation_name("post", "/activities"),
            "post_activities"
        );
        assert_eq!(
            generate_operation_name("get", "/athlete/{id}/activities"),
            "get_athlete_activities"
        );
    }

    #[test]
    fn test_generate_human_readable_description() {
        assert!(generate_human_readable_description("get", "/activities").contains("Retrieve"));
        assert!(generate_human_readable_description("post", "/activities").contains("Create"));
        assert!(generate_human_readable_description("put", "/activities").contains("Update"));
        assert!(generate_human_readable_description("delete", "/activities").contains("Delete"));
    }

    #[test]
    fn test_extract_operation_tags() {
        use serde_json::Map;

        let mut op = Map::new();
        op.insert("tags".to_string(), json!(["tag1", "tag2"]));

        let tags = extract_operation_tags(&op);
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"tag1".to_string()));
        assert!(tags.contains(&"tag2".to_string()));
    }

    #[test]
    fn test_extract_operation_tags_empty() {
        use serde_json::Map;

        let op = Map::new();
        let tags = extract_operation_tags(&op);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_should_filter_operation_include() {
        let mut include_tags = HashSet::new();
        include_tags.insert("required".to_string());

        // Has required tag - should NOT filter
        assert!(!should_filter_operation(
            &["required".to_string()],
            &include_tags,
            &HashSet::new()
        ));

        // Missing required tag - should filter
        assert!(should_filter_operation(
            &["other".to_string()],
            &include_tags,
            &HashSet::new()
        ));
    }

    #[test]
    fn test_should_filter_operation_exclude() {
        let mut exclude_tags = HashSet::new();
        exclude_tags.insert("admin".to_string());

        // Has excluded tag - should filter
        assert!(should_filter_operation(
            &["admin".to_string()],
            &HashSet::new(),
            &exclude_tags
        ));

        // No excluded tag - should NOT filter
        assert!(!should_filter_operation(
            &["user".to_string()],
            &HashSet::new(),
            &exclude_tags
        ));
    }

    #[test]
    fn test_is_auto_injected_athlete_param() {
        // athlete_id should be auto-injected
        assert!(is_auto_injected_athlete_param(
            "athlete_id",
            "query",
            "/test"
        ));

        // athleteId from current live spec should also be auto-injected
        assert!(is_auto_injected_athlete_param(
            "athleteId",
            "path",
            "/api/v1/athlete/{athleteId}/sport-settings/{id}/apply"
        ));

        // id in path for athlete endpoint should be auto-injected
        assert!(is_auto_injected_athlete_param(
            "id",
            "path",
            "/athlete/{id}"
        ));

        // nested resource ids must NOT be auto-injected
        assert!(!is_auto_injected_athlete_param(
            "id",
            "path",
            "/api/v1/athlete/{athleteId}/sport-settings/{id}/apply"
        ));

        // Regular params should NOT be auto-injected
        assert!(!is_auto_injected_athlete_param("limit", "query", "/test"));
        assert!(!is_auto_injected_athlete_param("id", "query", "/test"));
    }

    #[test]
    fn test_param_key_extraction() {
        let param = json!({
            "name": "id",
            "in": "path"
        });

        let key = param_key(&param);
        assert!(key.is_some());
        let (name, location) = key.unwrap();
        assert_eq!(name, "id");
        assert_eq!(location, "path");
    }

    #[test]
    fn test_param_key_missing_fields() {
        // Missing name
        let param = json!({"in": "path"});
        assert!(param_key(&param).is_none());

        // Missing in
        let param = json!({"name": "id"});
        assert!(param_key(&param).is_none());
    }

    #[test]
    fn test_parameter_schema_default() {
        let param = json!({"name": "test"});
        let schema = parameter_schema(&param);
        assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("string"));
    }

    #[test]
    fn test_parameter_schema_custom() {
        let param = json!({
            "name": "limit",
            "schema": {"type": "integer", "format": "int32"}
        });
        let schema = parameter_schema(&param);
        assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("integer"));
    }

    #[test]
    fn test_build_output_schema_200() {
        use serde_json::Map;

        let mut op = Map::new();
        let mut responses = Map::new();
        let mut response_200 = Map::new();
        let mut content = Map::new();
        let mut json_media = Map::new();
        json_media.insert("schema".to_string(), json!({"type": "object"}));
        content.insert("application/json".to_string(), Value::Object(json_media));
        response_200.insert("content".to_string(), Value::Object(content));
        responses.insert("200".to_string(), Value::Object(response_200));
        op.insert("responses".to_string(), Value::Object(responses));

        let schema = build_output_schema(&op);
        assert!(schema.is_some());
        let schema = schema.unwrap();
        assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("object"));
    }

    #[test]
    fn test_build_output_schema_201() {
        use serde_json::Map;

        let mut op = Map::new();
        let mut responses = Map::new();
        let mut response_201 = Map::new();
        let mut content = Map::new();
        let mut json_media = Map::new();
        json_media.insert("schema".to_string(), json!({"type": "object"}));
        content.insert("application/json".to_string(), Value::Object(json_media));
        response_201.insert("content".to_string(), Value::Object(content));
        responses.insert("201".to_string(), Value::Object(response_201));
        op.insert("responses".to_string(), Value::Object(responses));

        let schema = build_output_schema(&op);
        assert!(schema.is_some());
    }

    #[test]
    fn test_build_output_schema_204_no_content() {
        use serde_json::Map;

        let mut op = Map::new();
        let mut responses = Map::new();
        let response_204 = Map::new();
        responses.insert("204".to_string(), Value::Object(response_204));
        op.insert("responses".to_string(), Value::Object(responses));

        let schema = build_output_schema(&op);
        // 204 has no content, so schema should be None
        assert!(schema.is_none());
    }

    #[test]
    fn test_build_output_schema_no_responses() {
        use serde_json::Map;
        let op = Map::new();
        let schema = build_output_schema(&op);
        assert!(schema.is_none());
    }

    #[test]
    fn test_build_output_schema_ref_handling() {
        use serde_json::Map;

        let mut op = Map::new();
        let mut responses = Map::new();
        let mut response_200 = Map::new();
        let mut content = Map::new();
        let mut json_media = Map::new();
        json_media.insert(
            "schema".to_string(),
            json!({"$ref": "#/components/schemas/Activity"}),
        );
        content.insert("application/json".to_string(), Value::Object(json_media));
        response_200.insert("content".to_string(), Value::Object(content));
        responses.insert("200".to_string(), Value::Object(response_200));
        op.insert("responses".to_string(), Value::Object(responses));

        let schema = build_output_schema(&op);
        assert!(schema.is_some());
        let schema = schema.unwrap();
        // Should convert $ref to object type with description
        assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("object"));
        assert!(schema.get("description").is_some());
    }

    #[test]
    fn test_preferred_success_response_prioritizes_200_then_201_then_any_2xx() {
        use serde_json::Map;

        let mut responses = Map::new();
        responses.insert("204".to_string(), json!({"description": "no content"}));
        responses.insert("201".to_string(), json!({"description": "created"}));
        responses.insert("200".to_string(), json!({"description": "ok"}));

        let selected = preferred_success_response(&responses).expect("response should exist");
        assert_eq!(
            selected.get("description").and_then(Value::as_str),
            Some("ok")
        );

        responses.remove("200");
        let selected = preferred_success_response(&responses).expect("response should exist");
        assert_eq!(
            selected.get("description").and_then(Value::as_str),
            Some("created")
        );

        responses.remove("201");
        responses.insert(
            "299".to_string(),
            json!({"description": "wildcard success"}),
        );
        let selected = preferred_success_response(&responses).expect("response should exist");
        assert_eq!(
            selected.get("description").and_then(Value::as_str),
            Some("no content")
        );
    }

    #[test]
    fn test_preferred_json_media_type_accepts_json_like_content_types() {
        use serde_json::Map;

        let mut content = Map::new();
        content.insert(
            "text/plain".to_string(),
            json!({"schema": {"type": "string"}}),
        );
        content.insert(
            "application/problem+json".to_string(),
            json!({"schema": {"type": "object", "title": "Problem"}}),
        );

        let media = preferred_json_media_type(&content).expect("json-like media type expected");
        assert_eq!(
            media
                .get("schema")
                .and_then(Value::as_object)
                .and_then(|schema| schema.get("title"))
                .and_then(Value::as_str),
            Some("Problem")
        );
    }

    #[test]
    fn test_method_to_annotations_read_only() {
        let annotations_get = method_to_annotations(&reqwest::Method::GET);
        assert_eq!(annotations_get.read_only_hint, Some(true));
        assert_eq!(annotations_get.destructive_hint, Some(false));

        let annotations_head = method_to_annotations(&reqwest::Method::HEAD);
        assert_eq!(annotations_head.read_only_hint, Some(true));
    }

    #[test]
    fn test_method_to_annotations_destructive() {
        let annotations_delete = method_to_annotations(&reqwest::Method::DELETE);
        assert_eq!(annotations_delete.read_only_hint, Some(false));
        assert_eq!(annotations_delete.destructive_hint, Some(true));
    }

    #[test]
    fn test_method_to_annotations_idempotent() {
        let annotations_put = method_to_annotations(&reqwest::Method::PUT);
        assert_eq!(annotations_put.idempotent_hint, Some(true));

        let annotations_delete = method_to_annotations(&reqwest::Method::DELETE);
        assert_eq!(annotations_delete.idempotent_hint, Some(true));

        let annotations_post = method_to_annotations(&reqwest::Method::POST);
        assert_eq!(annotations_post.idempotent_hint, Some(false));
    }

    // ========================================================================
    // Integration Tests (Live API)
    // ========================================================================
    // These tests verify the parser works against the live Intervals.icu API.
    // They are marked with #[ignore] to skip by default.
    // Run with: cargo test -- --ignored

    #[tokio::test]
    #[ignore = "Requires network access to intervals.icu API"]
    async fn test_parse_live_intervals_api_spec() {
        let client = reqwest::Client::new();
        let spec = client
            .get("https://intervals.icu/api/v1/docs")
            .send()
            .await
            .expect("Failed to fetch OpenAPI spec")
            .json::<Value>()
            .await
            .expect("Failed to parse OpenAPI spec JSON");

        let result = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok(), "Failed to parse live OpenAPI spec");

        let registry = result.unwrap();

        // Verify we got a reasonable number of operations
        // (The API has 100+ endpoints, so this is a sanity check)
        assert!(
            registry.len() > 50,
            "Expected >50 operations, got {}. API may have changed.",
            registry.len()
        );

        // Verify some expected operations exist
        assert!(
            registry.operation("get_activities").is_some()
                || registry.operation("list_activities").is_some(),
            "Expected to find activities endpoint"
        );
    }

    #[tokio::test]
    #[ignore = "Requires network access to intervals.icu API"]
    async fn test_live_api_tool_schemas_valid() {
        let client = reqwest::Client::new();
        let spec = client
            .get("https://intervals.icu/api/v1/docs")
            .send()
            .await
            .expect("Failed to fetch OpenAPI spec")
            .json::<Value>()
            .await
            .expect("Failed to parse OpenAPI spec JSON");

        let registry = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new())
            .expect("Failed to parse spec");

        // Verify all tools have required fields
        for tool in registry.list_tools() {
            assert!(!tool.name.is_empty(), "Tool name should not be empty");
            assert!(
                tool.description.is_some(),
                "Tool {} should have description",
                tool.name
            );
            // Check that input_schema has content (it's an Arc<JsonObject>)
            assert!(
                !tool.input_schema.as_ref().is_empty(),
                "Tool {} should have input schema properties",
                tool.name
            );
        }
    }

    #[tokio::test]
    #[ignore = "Requires network access to intervals.icu API"]
    async fn test_live_api_has_expected_endpoints() {
        let client = reqwest::Client::new();
        let spec = client
            .get("https://intervals.icu/api/v1/docs")
            .send()
            .await
            .expect("Failed to fetch OpenAPI spec")
            .json::<Value>()
            .await
            .expect("Failed to parse OpenAPI spec JSON");

        let registry = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new())
            .expect("Failed to parse spec");

        // Check for key endpoint categories
        let tools = registry.list_tools();
        let tool_names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

        // Should have athlete endpoints
        let has_athlete = tool_names.iter().any(|n| n.contains("athlete"));
        assert!(has_athlete, "Should have athlete-related endpoints");

        // Should have activity endpoints
        let has_activity = tool_names
            .iter()
            .any(|n| n.contains("activity") || n.contains("activities"));
        assert!(has_activity, "Should have activity-related endpoints");

        // Should have workout/event endpoints
        let has_workout_or_event = tool_names
            .iter()
            .any(|n| n.contains("workout") || n.contains("event"));
        assert!(has_workout_or_event, "Should have workout/event endpoints");
    }
}
