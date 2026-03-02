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

            // Use enhanced description if available, otherwise fallback to OpenAPI summary/description
            let description = get_operation_description(&name)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    op.get("summary")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
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

/// Get enhanced description for known operation IDs.
/// Returns None if no enhanced description is available.
fn get_operation_description(operation_id: &str) -> Option<&'static str> {
    Some(match operation_id {
        // === Activities ===
        "listActivities" => {
            "Primary activity discovery tool. Start here for most activity-related requests. Returns recent activity summaries (id, name, date, distance, moving_time) and supports date-range browsing."
        }
        "getActivity" => {
            "Get full details for a known activity ID. Use after listActivities/searchForActivities when you already selected a specific activity."
        }
        "searchForActivities" => {
            "Search activities by text query. Use for finding activities by name, location, notes, or other text criteria."
        }
        "searchForActivitiesFull" => {
            "Full-text search across all activity fields. More comprehensive than searchForActivities but may be slower."
        }
        "downloadActivitiesAsCSV" => {
            "Download all activities as CSV format. Useful for bulk export and external analysis."
        }
        "listActivitiesAround" => {
            "Get activities before and after a specific activity. Returns context activities for comparison."
        }
        "updateActivity" => {
            "Update activity metadata (name, description, gear, etc.). Requires activity ID and fields to update."
        }
        "deleteActivity" => "Permanently delete an activity. This action cannot be undone.",
        "downloadActivityFile" => {
            "Download original activity file (FIT, TCX, GPX). Returns file content or saves to disk."
        }
        "downloadActivityFitFile" => {
            "Download activity as FIT file format. Standard format for cycling computers."
        }
        "downloadActivityGpxFile" => {
            "Download activity as GPX file format. Standard format for GPS data exchange."
        }

        // === Activity Analysis — критично! Не для listing activities ===
        "getActivityStreams" => {
            "Get time-series data streams from activity (power, heartrate, cadence, speed, etc.). Returns arrays of data points over time. For large activities, use compact=true or max_points to reduce response size."
        }
        "getIntervals" => {
            "Get structured workout intervals from activity. Returns interval analysis with power/HR/duration data for each detected interval."
        }
        "findBestEfforts" => {
            "Find best efforts from activity for standard durations (5min, 20min, 1hr, etc.). Returns peak power/pace efforts."
        }
        "searchForIntervals" => {
            "Search intervals across all activities by duration and intensity. Returns matching intervals from multiple activities."
        }
        "getPowerHistogram" => {
            "Get power distribution histogram for activity. Returns time/power bins showing power zone distribution."
        }
        "getHRHistogram" => {
            "Get heart rate distribution histogram for activity. Returns time/HR bins showing HR zone distribution."
        }
        "getPaceHistogram" => {
            "Get pace distribution histogram for activity. Returns time/pace bins for running activities."
        }
        "getGapHistogram" => {
            "Get graded adjusted pace (GAP) histogram for activity. Shows effort-adjusted pace distribution for trail/road running."
        }

        // === Power/HR/Pace Curves — критично! Не для listing activities ===
        "listAthletePowerCurves" => {
            "Specialized analytics tool for power-duration curves only (critical power modeling). REQUIRED: type='Run'|'Ride'|'Swim'. OPTIONAL: days_back (positive integer, default 365). Returns curve points and activityReferences (not an activities list). Do NOT use this to browse or search recent activities; use listActivities instead."
        }
        "listAthleteHRCurves" => {
            "Specialized analytics tool for heart-rate curve modeling only. REQUIRED: type='Run'|'Ride'|'Swim'. OPTIONAL: days_back. Returns HR curves and activityReferences (not an activities list). For activity browsing, use listActivities."
        }
        "listAthletePaceCurves" => {
            "Specialized analytics tool for pace-curve modeling only. REQUIRED: type='Run'|'Ride'|'Swim'. OPTIONAL: days_back. Returns pace curves and activityReferences (not an activities list). For activity browsing, use listActivities."
        }

        // === Athlete Profile ===
        "getAthlete" => {
            "Get athlete profile information. Returns name, athlete ID, and basic profile data."
        }
        "getAthleteProfile" => {
            "Get complete athlete profile including fitness metrics (CTL/ATL/TSB), fatigue, form, and sport settings."
        }

        // === Events/Calendar ===
        "listEvents" => {
            "List calendar events for athlete. Returns planned workouts, races, notes, holidays, and other calendar items."
        }
        "getEvent" => {
            "Get specific calendar event by ID. Returns event details including workout data, date, category. Note: expects event ID, not activity ID."
        }
        "createEvent" => {
            "Create new calendar event (workout, race, note, etc.). Requires event object with start_date_local, name, and category."
        }
        "updateEvent" => "Update existing calendar event. Requires event ID and fields to update.",
        "deleteEvent" => "Delete calendar event. Requires event ID.",
        "createMultipleEvents" => {
            "Create multiple calendar events in bulk. Useful for importing training plans."
        }
        "deleteEventsBulk" => {
            "Delete multiple calendar events in bulk. Requires array of event IDs or external IDs."
        }
        "duplicateEvents" => {
            "Duplicate calendar events (e.g., recurring workouts). Requires event IDs and copy parameters."
        }

        // === Wellness ===
        "listWellnessRecords" => {
            "List wellness records (HRV, resting HR, sleep, weight). Returns wellness data for date range."
        }
        "getRecord" => {
            "Get wellness record for specific date. Returns HRV, resting HR, sleep data, and subjective metrics for that date."
        }
        "updateWellness" => {
            "Update or create wellness record for a date. Requires date and wellness data (HRV, sleep, etc.)."
        }

        // === Gear ===
        "listGear" => {
            "List athlete's gear (bikes, shoes, etc.). Returns gear list with distance, dates, and usage statistics."
        }
        "createGear" => {
            "Create new gear entry. Requires gear name, type, and optional initial distance."
        }
        "updateGear" => "Update gear information. Requires gear ID and fields to update.",
        "deleteGear" => "Delete gear entry. Requires gear ID.",
        "createReminder" => {
            "Create gear maintenance reminder. Requires gear ID and reminder parameters."
        }
        "updateReminder" => {
            "Update gear reminder. Requires gear ID, reminder ID, and update parameters."
        }

        // === Sport Settings ===
        "listSettings" => {
            "Get athlete's sport settings (FTP, FTHR, zones, etc.). Returns settings for all configured sports."
        }
        "updateSettings" => "Update sport settings. Requires sport type and settings object.",
        "applyToActivities" => {
            "Apply sport settings to existing activities. Recalculates power/HR zones for affected activities."
        }
        "createSettings" => {
            "Create new sport settings. Requires settings object with sport type and thresholds."
        }
        "deleteSettings" => "Delete sport settings. Requires sport type.",

        // === Workout Library ===
        "listFolders" => {
            "List workout library folders and training plans. Returns folder structure with workouts and plans."
        }
        "listWorkouts" => {
            "List workouts in athlete's library. Returns workout definitions, structures, and metadata."
        }

        // === Other ===
        "getFitnessSummary" => {
            "Get aggregated fitness summary (CTL/ATL/TSB trends). Returns fitness metrics over time period."
        }

        _ => return None,
    })
}
