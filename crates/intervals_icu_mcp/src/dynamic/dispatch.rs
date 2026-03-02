//! Dispatcher for dynamic OpenAPI operations.

use crate::dynamic::types::DynamicOperation;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, JsonObject};
use serde_json::{Map, Value};

/// Dispatch a dynamic operation with the given arguments.
pub async fn dispatch_operation(
    http_client: &reqwest::Client,
    base_url: &str,
    athlete_id: &str,
    api_key: &str,
    operation: &DynamicOperation,
    arguments: Option<&JsonObject>,
) -> Result<CallToolResult, ErrorData> {
    if api_key.is_empty() {
        return Err(ErrorData::invalid_request(
            "INTERVALS_ICU_API_KEY is required for dynamic OpenAPI tool calls",
            None,
        ));
    }

    let args = arguments.cloned().unwrap_or_default();
    let mut path = operation.path_template.clone();
    let mut query = Vec::<(String, String)>::new();

    for p in &operation.params {
        match p.location {
            crate::dynamic::types::ParamLocation::Path => {
                if p.auto_injected {
                    path = path.replace(&format!("{{{}}}", p.name), athlete_id);
                    continue;
                }

                let replacement = args
                    .get(&p.name)
                    .or_else(|| {
                        if p.name == "id" {
                            args.get("activity_id").or_else(|| args.get("activityId"))
                        } else {
                            None
                        }
                    })
                    .and_then(stringify_argument)
                    .or_else(|| {
                        if p.name == "ext" || p.name == "format" {
                            Some(String::new())
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| {
                        ErrorData::invalid_params(
                            format!("missing required path parameter: {}", p.name),
                            None,
                        )
                    })?;
                path = path.replace(&format!("{{{}}}", p.name), &replacement);
            }
            crate::dynamic::types::ParamLocation::Query => {
                if let Some(value) = args.get(&p.name) {
                    if let Some(arr) = value.as_array() {
                        for v in arr {
                            query.push((p.name.clone(), value_to_query(v)));
                        }
                    } else {
                        query.push((p.name.clone(), value_to_query(value)));
                    }
                }
            }
        }
    }

    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    let mut req = http_client
        .request(operation.method.clone(), &url)
        .basic_auth("API_KEY", Some(api_key.to_string()))
        .query(&query);

    if operation.has_json_body {
        let body = args
            .get("body")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        req = req.json(&body);
    }

    let resp = req.send().await.map_err(|e| {
        ErrorData::internal_error(
            format!("HTTP request failed for {}: {e}", operation.name),
            None,
        )
    })?;

    let status = resp.status();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = resp.bytes().await.map_err(|e| {
        ErrorData::internal_error(format!("failed to read HTTP response body: {e}"), None)
    })?;

    let body_json = parse_response_body(&bytes, &content_type);

    let compact_enabled = args.get("compact").and_then(Value::as_bool).unwrap_or(false);
    let response_fields = args.get("fields").and_then(Value::as_array).map(|a| {
        a.iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    });
    let body_only = args.get("body_only").and_then(Value::as_bool).unwrap_or(true);

    let normalized_body = if compact_enabled {
        match &body_json {
            Value::Object(_) => {
                if let Some(fields) = response_fields.as_ref() {
                    crate::compact::filter_fields(&body_json, fields)
                } else {
                    body_json.clone()
                }
            }
            Value::Array(_) => {
                if let Some(fields) = response_fields.as_ref() {
                    crate::compact::filter_array_fields(&body_json, fields)
                } else {
                    body_json.clone()
                }
            }
            _ => body_json.clone(),
        }
    } else {
        body_json
    };

    if status.is_success() && body_only {
        return Ok(CallToolResult::structured(normalized_body));
    }

    let wrapper = serde_json::json!({
        "status": status.as_u16(),
        "content_type": content_type,
        "body": normalized_body,
    });

    if status.is_success() {
        Ok(CallToolResult::structured(wrapper))
    } else {
        Ok(CallToolResult::structured_error(wrapper))
    }
}

fn stringify_argument(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn value_to_query(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => value.to_string(),
    }
}

fn parse_response_body(bytes: &[u8], content_type: &str) -> Value {
    if bytes.is_empty() {
        return Value::Null;
    }

    if let Ok(json) = serde_json::from_slice::<Value>(bytes) {
        return json;
    }

    match std::str::from_utf8(bytes) {
        Ok(text) => Value::String(text.to_string()),
        Err(_) => serde_json::json!({
            "binary": true,
            "content_type": content_type,
            "byte_length": bytes.len(),
            "message": "Non-UTF8 response body omitted from MCP payload"
        }),
    }
}
