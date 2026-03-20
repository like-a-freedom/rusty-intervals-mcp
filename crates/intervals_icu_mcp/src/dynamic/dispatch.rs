//! Dispatcher for dynamic OpenAPI operations.

use crate::dynamic::types::DynamicOperation;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, JsonObject};
use serde_json::Value;

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

    for p in &operation.params {
        match p.location {
            crate::dynamic::types::ParamLocation::Path => {
                if p.auto_injected {
                    path = path.replace(&format!("{{{}}}", p.name), athlete_id);
                    continue;
                }

                let replacement = resolve_path_argument(&args, p).ok_or_else(|| {
                    ErrorData::invalid_params(
                        format!("missing required path parameter: {}", p.name),
                        None,
                    )
                })?;
                path = path.replace(&format!("{{{}}}", p.name), &replacement);
            }
            crate::dynamic::types::ParamLocation::Query => {}
        }
    }

    let query = collect_query_pairs(&args, &operation.params);

    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    let mut req = http_client
        .request(operation.method.clone(), &url)
        .basic_auth("API_KEY", Some(api_key.to_string()))
        .query(&query);

    if operation.has_json_body {
        let body = args
            .get("body")
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
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

    let compact_enabled = args
        .get("compact")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let response_fields = args.get("fields").and_then(Value::as_array).map(|a| {
        a.iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    });
    let body_only = args
        .get("body_only")
        .and_then(Value::as_bool)
        .unwrap_or(true);

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

fn resolve_path_argument(
    args: &JsonObject,
    param: &crate::dynamic::types::ParamSpec,
) -> Option<String> {
    args.get(&param.name)
        .or_else(|| {
            if param.name == "id" {
                args.get("activity_id").or_else(|| args.get("activityId"))
            } else {
                None
            }
        })
        .and_then(stringify_argument)
        .or_else(|| {
            if param.name == "ext" || param.name == "format" {
                Some(String::new())
            } else {
                None
            }
        })
}

fn collect_query_pairs(
    args: &JsonObject,
    params: &[crate::dynamic::types::ParamSpec],
) -> Vec<(String, String)> {
    let mut query = Vec::new();

    for param in params {
        if param.location != crate::dynamic::types::ParamLocation::Query {
            continue;
        }

        if let Some(value) = args.get(&param.name) {
            if let Some(arr) = value.as_array() {
                for item in arr {
                    query.push((param.name.clone(), value_to_query(item)));
                }
            } else {
                query.push((param.name.clone(), value_to_query(value)));
            }
        }
    }

    query
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ========================================================================
    // stringify_argument() Tests
    // ========================================================================

    #[test]
    fn test_stringify_argument_string() {
        let value = json!("test_value");
        let result = stringify_argument(&value);
        assert_eq!(result, Some("test_value".to_string()));
    }

    #[test]
    fn test_stringify_argument_number() {
        let value = json!(42);
        let result = stringify_argument(&value);
        assert_eq!(result, Some("42".to_string()));

        let value = json!(std::f64::consts::PI);
        let result = stringify_argument(&value);
        assert!(result.is_some_and(|v| v.contains("3.14")));
    }

    #[test]
    fn test_stringify_argument_bool() {
        let value = json!(true);
        let result = stringify_argument(&value);
        assert_eq!(result, Some("true".to_string()));

        let value = json!(false);
        let result = stringify_argument(&value);
        assert_eq!(result, Some("false".to_string()));
    }

    #[test]
    fn test_stringify_argument_null() {
        let value = json!(null);
        let result = stringify_argument(&value);
        assert_eq!(result, None);
    }

    #[test]
    fn test_stringify_argument_array() {
        let value = json!([1, 2, 3]);
        let result = stringify_argument(&value);
        assert_eq!(result, None);
    }

    #[test]
    fn test_stringify_argument_object() {
        let value = json!({"key": "value"});
        let result = stringify_argument(&value);
        assert_eq!(result, None);
    }

    // ========================================================================
    // value_to_query() Tests
    // ========================================================================

    #[test]
    fn test_value_to_query_string() {
        let value = json!("test");
        let result = value_to_query(&value);
        assert_eq!(result, "test");
    }

    #[test]
    fn test_value_to_query_number() {
        let value = json!(123);
        let result = value_to_query(&value);
        assert_eq!(result, "123");

        let value = json!(std::f64::consts::PI);
        let result = value_to_query(&value);
        assert!(result.contains("3.14"));
    }

    #[test]
    fn test_value_to_query_bool() {
        let value = json!(true);
        let result = value_to_query(&value);
        assert_eq!(result, "true");

        let value = json!(false);
        let result = value_to_query(&value);
        assert_eq!(result, "false");
    }

    #[test]
    fn test_value_to_query_array() {
        let value = json!([1, 2, 3]);
        let result = value_to_query(&value);
        assert_eq!(result, "[1,2,3]");
    }

    #[test]
    fn test_value_to_query_object() {
        let value = json!({"key": "value"});
        let result = value_to_query(&value);
        assert!(result.contains("key"));
    }

    // ========================================================================
    // parse_response_body() Tests
    // ========================================================================

    #[test]
    fn test_parse_response_body_empty() {
        let result = parse_response_body(&[], "application/json");
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn test_parse_response_body_valid_json() {
        let json_bytes = b"{\"key\": \"value\"}";
        let result = parse_response_body(json_bytes, "application/json");
        assert_eq!(result.get("key").and_then(|v| v.as_str()), Some("value"));
    }

    #[test]
    fn test_parse_response_body_json_array() {
        let json_bytes = b"[1, 2, 3]";
        let result = parse_response_body(json_bytes, "application/json");
        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_parse_response_body_plain_text() {
        let text_bytes = b"Hello, World!";
        let result = parse_response_body(text_bytes, "text/plain");
        assert_eq!(result, Value::String("Hello, World!".to_string()));
    }

    #[test]
    fn test_parse_response_body_invalid_utf8() {
        // Invalid UTF-8 bytes
        let invalid_bytes = &[0xFF, 0xFE, 0xFD];
        let result = parse_response_body(invalid_bytes, "application/octet-stream");

        assert!(result.is_object());
        assert_eq!(result.get("binary").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(result.get("byte_length").and_then(|v| v.as_u64()), Some(3));
    }

    #[test]
    fn test_parse_response_body_json_number() {
        let json_bytes = b"42";
        let result = parse_response_body(json_bytes, "application/json");
        assert_eq!(result.as_i64(), Some(42));
    }

    #[test]
    fn test_parse_response_body_json_float() {
        let json_bytes = b"3.14";
        let result = parse_response_body(json_bytes, "application/json");
        assert!(
            result
                .as_f64()
                .is_some_and(|v| (v - std::f64::consts::PI).abs() < 0.01)
        );
    }

    #[test]
    fn test_parse_response_body_json_boolean() {
        let json_bytes = b"true";
        let result = parse_response_body(json_bytes, "application/json");
        assert_eq!(result.as_bool(), Some(true));
    }

    #[test]
    fn test_parse_response_body_json_null() {
        let json_bytes = b"null";
        let result = parse_response_body(json_bytes, "application/json");
        assert!(result.is_null());
    }

    #[test]
    fn test_parse_response_body_unicode() {
        let unicode_bytes = "Hello, 世界!".as_bytes();
        let result = parse_response_body(unicode_bytes, "text/plain");
        assert_eq!(result, Value::String("Hello, 世界!".to_string()));
    }

    // ========================================================================
    // Integration-style Tests
    // ========================================================================

    #[test]
    fn test_dispatch_helper_functions_together() {
        // Test that helper functions work together correctly
        let path_value = json!("123");
        let query_value = json!([1, 2, 3]);

        let path_str = stringify_argument(&path_value);
        let query_str = value_to_query(&query_value);

        assert_eq!(path_str, Some("123".to_string()));
        assert_eq!(query_str, "[1,2,3]");
    }

    #[test]
    fn test_parse_body_roundtrip() {
        let original = json!({"status": "ok", "count": 42});
        let bytes = serde_json::to_vec(&original).unwrap();
        let result = parse_response_body(&bytes, "application/json");

        assert_eq!(result.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(result.get("count").and_then(|v| v.as_i64()), Some(42));
    }

    #[test]
    fn test_resolve_path_argument_uses_activity_id_alias_for_id() {
        let args = serde_json::from_value::<JsonObject>(json!({
            "activity_id": "abc123"
        }))
        .unwrap();

        let param = crate::dynamic::types::ParamSpec {
            name: "id".to_string(),
            location: crate::dynamic::types::ParamLocation::Path,
            auto_injected: false,
        };

        let value = resolve_path_argument(&args, &param).expect("id alias should resolve");
        assert_eq!(value, "abc123");
    }

    #[test]
    fn test_resolve_path_argument_defaults_optional_suffix_params_to_empty() {
        let args = JsonObject::new();

        let param = crate::dynamic::types::ParamSpec {
            name: "format".to_string(),
            location: crate::dynamic::types::ParamLocation::Path,
            auto_injected: false,
        };

        let value = resolve_path_argument(&args, &param).expect("format should default");
        assert_eq!(value, "");
    }

    #[test]
    fn test_collect_query_pairs_expands_arrays() {
        let args = serde_json::from_value::<JsonObject>(json!({
            "id": "ignore-me",
            "types": ["run", "ride"],
            "oldest": 30,
            "include_all": true
        }))
        .unwrap();

        let params = vec![
            crate::dynamic::types::ParamSpec {
                name: "types".to_string(),
                location: crate::dynamic::types::ParamLocation::Query,
                auto_injected: false,
            },
            crate::dynamic::types::ParamSpec {
                name: "oldest".to_string(),
                location: crate::dynamic::types::ParamLocation::Query,
                auto_injected: false,
            },
            crate::dynamic::types::ParamSpec {
                name: "include_all".to_string(),
                location: crate::dynamic::types::ParamLocation::Query,
                auto_injected: false,
            },
        ];

        let pairs = collect_query_pairs(&args, &params);
        assert_eq!(
            pairs,
            vec![
                ("types".to_string(), "run".to_string()),
                ("types".to_string(), "ride".to_string()),
                ("oldest".to_string(), "30".to_string()),
                ("include_all".to_string(), "true".to_string()),
            ]
        );
    }
}
