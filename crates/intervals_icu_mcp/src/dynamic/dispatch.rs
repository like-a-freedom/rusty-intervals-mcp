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

    // Validate parameters before sending to API
    if let Err(e) = validate_curve_parameters(&operation.name, &args) {
        let mut message = format!("Invalid parameters for {}: {}", operation.name, e);
        if let Some(hint) = curve_recovery_hint(&operation.name) {
            message.push(' ');
            message.push_str(hint);
        }
        return Err(ErrorData::invalid_params(message, None));
    }
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

    // Transform confusing field names to prevent model confusion
    // listAthletePowerCurves returns {"activities": {...}, "list": [...]}
    // Model confuses "activities" with activities list, so rename to "activityReferences"
    let transformed_body = transform_confusing_fields(&body_json, &operation.name);
    let transformed_body = augment_curve_error_response(&transformed_body, &operation.name, status);

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
        match &transformed_body {
            Value::Object(_) => {
                if let Some(fields) = response_fields.as_ref() {
                    crate::compact::filter_fields(&transformed_body, fields)
                } else {
                    transformed_body.clone()
                }
            }
            Value::Array(_) => {
                if let Some(fields) = response_fields.as_ref() {
                    crate::compact::filter_array_fields(&transformed_body, fields)
                } else {
                    transformed_body.clone()
                }
            }
            _ => transformed_body.clone(),
        }
    } else {
        transformed_body
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

fn is_curve_operation(operation_name: &str) -> bool {
    operation_name == "listAthletePowerCurves"
        || operation_name == "listAthleteHRCurves"
        || operation_name == "listAthletePaceCurves"
}

fn curve_recovery_hint(operation_name: &str) -> Option<&'static str> {
    if !is_curve_operation(operation_name) {
        return None;
    }

    Some(
        "If you need activity browsing/search, use listActivities or searchForActivities instead. \
         Curve tools require type='Run'|'Ride'|'Swim' and optional days_back only.",
    )
}

/// Transform confusing field names in API responses to prevent model confusion.
///
/// Specifically:
/// - listAthletePowerCurves, listAthleteHRCurves, listAthletePaceCurves return
///   {"activities": {...}, "list": [...]} where "activities" is a map of activity
///   references, NOT an activities list. Rename to "activityReferences" to avoid
///   confusion with listActivities response.
fn transform_confusing_fields(body: &Value, operation_name: &str) -> Value {
    if !is_curve_operation(operation_name) {
        return body.clone();
    }

    // Only transform objects
    let Some(obj) = body.as_object() else {
        return body.clone();
    };

    // Clone the object and rename "activities" to "activityReferences"
    let mut transformed = obj.clone();
    if let Some(activities) = transformed.remove("activities") {
        transformed.insert("activityReferences".to_string(), activities);
    }

    Value::Object(transformed)
}

fn augment_curve_error_response(
    body: &Value,
    operation_name: &str,
    status: reqwest::StatusCode,
) -> Value {
    let should_augment = is_curve_operation(operation_name)
        && (status == reqwest::StatusCode::UNPROCESSABLE_ENTITY
            || status == reqwest::StatusCode::BAD_REQUEST);

    if !should_augment {
        return body.clone();
    }

    let mut obj = match body {
        Value::Object(map) => map.clone(),
        _ => {
            let mut map = Map::new();
            map.insert("upstream_error".to_string(), body.clone());
            map
        }
    };

    obj.entry("errorCategory".to_string())
        .or_insert_with(|| Value::String("likely_wrong_tool_or_params".to_string()));
    obj.entry("recovery".to_string()).or_insert_with(|| {
        serde_json::json!({
            "message": "Curve tools are specialized. Use type='Run'|'Ride'|'Swim' and optional days_back. Do not send empty strings for optional params.",
            "recommended_tools": ["listActivities", "searchForActivities", "getActivity"],
            "do_not_retry_same_arguments": true
        })
    });

    Value::Object(obj)
}

/// Validate parameters for curve-related operations before sending to API.
///
/// This prevents common model mistakes:
/// - Empty string parameters (now: "", newest: "")
/// - Missing required 'type' parameter
/// - Invalid parameter formats
/// - Unknown/unsupported parameters
fn validate_curve_parameters(operation_name: &str, args: &JsonObject) -> Result<(), String> {
    if !is_curve_operation(operation_name) {
        return Ok(());
    }

    // REQUIRED: type parameter must be non-empty string
    let type_param = args
        .get("type")
        .ok_or("Missing required parameter 'type' (sport type like 'Run', 'Ride', 'Swim')")?;

    if let Some(type_str) = type_param.as_str() {
        if type_str.trim().is_empty() {
            return Err(
                "Parameter 'type' must be a non-empty string (e.g., 'Run', 'Ride', 'Swim'). \
                 Empty string is not valid."
                    .to_string(),
            );
        }
    } else {
        return Err("Parameter 'type' must be a string".to_string());
    }

    // OPTIONAL: days_back must be positive integer if provided
    if let Some(days_back) = args.get("days_back") {
        if let Some(days) = days_back.as_i64() {
            if days <= 0 {
                return Err(
                    "Parameter 'days_back' must be a positive integer (e.g., 30, 90, 365)"
                        .to_string(),
                );
            }
        } else {
            return Err("Parameter 'days_back' must be an integer".to_string());
        }
    }

    // REJECT: unsupported query aliases that commonly trigger 422 loops upstream
    for key in ["now", "newest"] {
        if args.contains_key(key) {
            return Err(format!(
                "Parameter '{}' is not supported for curve tools. Use 'days_back' and 'type' instead.",
                key
            ));
        }
    }

    // REJECT: Empty string parameters (now, newest, etc.)
    // These are common model mistakes - passing empty strings instead of omitting
    for (key, value) in args {
        if let Some(s) = value.as_str()
            && s.trim().is_empty()
            && !["ext", "format"].contains(&key.as_str())
            && key != "type"
        {
            return Err(format!(
                "Parameter '{}' cannot be an empty string. \
                 Omit optional parameters or provide valid values. \
                 Common mistake: passing empty string for 'now', 'newest', etc.",
                key
            ));
        }
    }

    // WARN about unknown parameters (don't reject, just for awareness)
    let allowed_params = [
        "type",
        "days_back",
        "compact",
        "fields",
        "body_only",
        "ext",
        "format",
        // Legacy/optional params that API accepts but we don't promote
        "subMaxEfforts",
        "pmType",
    ];
    for key in args.keys() {
        if !allowed_params.contains(&key.as_str()) {
            // Log warning but don't fail - API may accept additional params
            tracing::warn!(
                "Unknown parameter '{}' for {}. Allowed: {:?}",
                key,
                operation_name,
                allowed_params
            );
        }
    }

    Ok(())
}
