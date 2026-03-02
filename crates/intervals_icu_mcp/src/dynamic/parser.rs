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
    name == "athlete_id" || (name == "id" && in_location == "path" && path.contains("/athlete/"))
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

    ToolAnnotations {
        title: None,
        read_only_hint: Some(is_read_only),
        destructive_hint: Some(method == reqwest::Method::DELETE),
        idempotent_hint: Some(is_idempotent),
        open_world_hint: None,
    }
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

/// Build output schema from OpenAPI operation responses.
/// Extracts schema from responses.200.content.application/json.schema
fn build_output_schema(op: &Map<String, Value>) -> Option<Map<String, Value>> {
    op.get("responses")
        .and_then(Value::as_object)
        .and_then(|responses| {
            // Try 200, then 201, then 204, then any 2xx
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
        })
        .and_then(Value::as_object)
        .and_then(|response| response.get("content"))
        .and_then(Value::as_object)
        .and_then(|content| {
            // Try application/json first, then any JSON-like content type
            content.get("application/json").or_else(|| {
                content
                    .iter()
                    .find(|(k, _)| k.contains("json"))
                    .map(|(_, v)| v)
            })
        })
        .and_then(Value::as_object)
        .and_then(|media_type| media_type.get("schema"))
        .and_then(Value::as_object)
        .map(|schema| {
            // Convert $ref to inline type if present
            let schema = schema.clone();
            if let Some(ref_val) = schema.get("$ref").and_then(Value::as_str) {
                // For $ref, create a generic object schema
                let mut ref_schema = Map::new();
                ref_schema.insert("type".to_string(), Value::String("object".to_string()));
                ref_schema.insert(
                    "description".to_string(),
                    Value::String(format!("Response schema (ref: {})", ref_val)),
                );
                ref_schema
            } else {
                schema
            }
        })
}
