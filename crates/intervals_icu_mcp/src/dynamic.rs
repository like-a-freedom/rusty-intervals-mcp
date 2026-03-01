use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rmcp::ErrorData;
use rmcp::model::{CallToolResult, JsonObject, ListToolsResult, Tool, ToolAnnotations};
use serde_json::{Map, Value};

const OPENAPI_DEFAULT_PATH: &str = "/api/v1/docs";
const OPENAPI_FETCH_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParamLocation {
    Path,
    Query,
}

#[derive(Debug, Clone)]
struct ParamSpec {
    name: String,
    location: ParamLocation,
    auto_injected: bool,
}

#[derive(Debug, Clone)]
pub struct DynamicOperation {
    pub name: String,
    pub method: reqwest::Method,
    pub path_template: String,
    pub description: String,
    params: Vec<ParamSpec>,
    has_json_body: bool,
    pub tool: Tool,
}

#[derive(Debug, Clone)]
pub struct DynamicRegistry {
    operations: HashMap<String, DynamicOperation>,
}

impl DynamicRegistry {
    pub fn operation(&self, name: &str) -> Option<&DynamicOperation> {
        self.operations.get(name)
    }

    pub fn list_tools(&self) -> Vec<Tool> {
        let mut tools: Vec<Tool> = self.operations.values().map(|o| o.tool.clone()).collect();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    pub fn len(&self) -> usize {
        self.operations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    pub fn from_openapi(spec: &Value) -> Result<Self, String> {
        Self::from_openapi_with_tags(spec, &HashSet::new(), &HashSet::new())
    }

    pub fn from_openapi_with_tags(
        spec: &Value,
        include_tags: &HashSet<String>,
        exclude_tags: &HashSet<String>,
    ) -> Result<Self, String> {
        let mut operations = HashMap::new();
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

                let name = op
                    .get("operationId")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| generate_operation_name(method_name, path));

                let method = match method_name.to_ascii_uppercase().parse::<reqwest::Method>() {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let description = op
                    .get("summary")
                    .and_then(Value::as_str)
                    .or_else(|| op.get("description").and_then(Value::as_str))
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("{} {}", method_name.to_uppercase(), path));

                let mut merged_params = BTreeMap::<(String, String), Value>::new();
                for p in &path_level_params {
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
                    let required_param =
                        p.get("required").and_then(Value::as_bool).unwrap_or(false);
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

                    schema_props.insert(name_s.to_string(), parameter_schema(p));

                    let treat_format_as_optional = name_s == "ext" || name_s == "format";
                    if required_param && !treat_format_as_optional {
                        required.push(Value::String(name_s.to_string()));
                    }
                }

                let has_json_body = op
                    .get("requestBody")
                    .and_then(|rb| rb.get("content"))
                    .and_then(Value::as_object)
                    .is_some_and(|content| content.contains_key("application/json"));

                if has_json_body {
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

                add_response_control_properties(&mut schema_props);

                let mut input_schema = Map::new();
                input_schema.insert("type".to_string(), Value::String("object".to_string()));
                input_schema.insert("properties".to_string(), Value::Object(schema_props));
                if !required.is_empty() {
                    input_schema.insert("required".to_string(), Value::Array(required));
                }

                let schema_obj: JsonObject =
                    match serde_json::from_value(Value::Object(input_schema)) {
                        Ok(s) => s,
                        Err(e) => {
                            return Err(format!("failed to build schema for {name}: {e}"));
                        }
                    };

                let mut tool = Tool::new(name.clone(), description.clone(), Arc::new(schema_obj));
                tool.annotations = Some(method_to_annotations(&method));

                operations.insert(
                    name.clone(),
                    DynamicOperation {
                        name,
                        method,
                        path_template: path.to_string(),
                        description,
                        params,
                        has_json_body,
                        tool,
                    },
                );
            }
        }

        Ok(Self { operations })
    }
}

#[derive(Clone)]
pub struct DynamicRuntime {
    http_client: reqwest::Client,
    pub base_url: String,
    pub athlete_id: String,
    pub api_key: String,
    pub spec_source: Option<String>,
    include_tags: Arc<HashSet<String>>,
    exclude_tags: Arc<HashSet<String>>,
    refresh_interval: Duration,
    last_refresh_attempt: Arc<tokio::sync::Mutex<Option<Instant>>>,
    registry: Arc<tokio::sync::RwLock<Option<Arc<DynamicRegistry>>>>,
    cached_tool_count: Arc<AtomicUsize>,
}

impl DynamicRuntime {
    pub fn new(
        base_url: String,
        athlete_id: String,
        api_key: String,
        spec_source: Option<String>,
        include_tags_raw: Option<String>,
        exclude_tags_raw: Option<String>,
        refresh_interval: Duration,
    ) -> Self {
        let include_tags = parse_tag_set(include_tags_raw.as_deref());
        let exclude_tags = parse_tag_set(exclude_tags_raw.as_deref());

        if !include_tags.is_empty() && !exclude_tags.is_empty() {
            tracing::warn!(
                "Both INTERVALS_INCLUDE_TAGS and INTERVALS_EXCLUDE_TAGS are set; INTERVALS_EXCLUDE_TAGS will be ignored"
            );
        }

        if include_tags.is_empty() && exclude_tags.is_empty() {
            tracing::debug!("Tool scope mode: all OpenAPI tags enabled (default)");
        } else if !include_tags.is_empty() {
            tracing::debug!(
                include_tags = ?include_tags,
                "Tool scope mode: include-only tag filter"
            );
        } else {
            tracing::debug!(
                exclude_tags = ?exclude_tags,
                "Tool scope mode: exclude tag filter"
            );
        }

        Self {
            http_client: reqwest::Client::new(),
            base_url,
            athlete_id,
            api_key,
            spec_source,
            include_tags: Arc::new(include_tags),
            exclude_tags: Arc::new(exclude_tags),
            refresh_interval,
            last_refresh_attempt: Arc::new(tokio::sync::Mutex::new(None)),
            registry: Arc::new(tokio::sync::RwLock::new(None)),
            cached_tool_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn from_env() -> Self {
        let base_url = std::env::var("INTERVALS_ICU_BASE_URL")
            .unwrap_or_else(|_| "https://intervals.icu".to_string());
        let athlete_id = std::env::var("INTERVALS_ICU_ATHLETE_ID").unwrap_or_default();
        let api_key = std::env::var("INTERVALS_ICU_API_KEY").unwrap_or_default();
        let spec_source = std::env::var("INTERVALS_ICU_OPENAPI_SPEC").ok();
        let include_tags_raw = std::env::var("INTERVALS_INCLUDE_TAGS").ok();
        let exclude_tags_raw = std::env::var("INTERVALS_EXCLUDE_TAGS").ok();
        let refresh_secs = std::env::var("INTERVALS_ICU_SPEC_REFRESH_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300);

        Self::new(
            base_url,
            athlete_id,
            api_key,
            spec_source,
            include_tags_raw,
            exclude_tags_raw,
            Duration::from_secs(refresh_secs),
        )
    }

    pub fn cached_tool_count(&self) -> usize {
        self.cached_tool_count.load(Ordering::Relaxed)
    }

    pub async fn ensure_registry(&self) -> Result<Arc<DynamicRegistry>, ErrorData> {
        if let Some(existing) = self.registry.read().await.clone() {
            if !self.should_attempt_refresh().await {
                tracing::debug!(
                    refresh_interval_secs = self.refresh_interval.as_secs(),
                    "OpenAPI registry cache hit; refresh skipped"
                );
                return Ok(existing);
            }

            tracing::debug!("OpenAPI registry cache hit; attempting refresh");
            match self.try_build_registry().await {
                Ok(parsed) => {
                    self.cached_tool_count
                        .store(parsed.len(), Ordering::Relaxed);
                    let mut write = self.registry.write().await;
                    *write = Some(parsed.clone());
                    tracing::debug!(
                        tool_count = parsed.len(),
                        "OpenAPI registry refresh succeeded"
                    );
                    return Ok(parsed);
                }
                Err(err) => {
                    tracing::debug!(
                        error = %err,
                        "OpenAPI registry refresh failed; using cached registry"
                    );
                    return Ok(existing);
                }
            }
        }

        tracing::debug!("OpenAPI registry cache miss; loading spec");
        let parsed = self.try_build_registry().await.map_err(|e| {
            ErrorData::internal_error(format!("failed to build OpenAPI registry: {e}"), None)
        })?;
        self.cached_tool_count
            .store(parsed.len(), Ordering::Relaxed);

        let mut write = self.registry.write().await;
        *write = Some(parsed.clone());

        let mut last_refresh = self.last_refresh_attempt.lock().await;
        *last_refresh = Some(Instant::now());

        Ok(parsed)
    }

    async fn should_attempt_refresh(&self) -> bool {
        if self.refresh_interval.is_zero() {
            return true;
        }

        let mut guard = self.last_refresh_attempt.lock().await;
        let now = Instant::now();

        match *guard {
            Some(last) if now.duration_since(last) < self.refresh_interval => false,
            _ => {
                *guard = Some(now);
                true
            }
        }
    }

    async fn try_build_registry(&self) -> Result<Arc<DynamicRegistry>, String> {
        let spec = self.load_spec().await?;
        let use_exclude = self.include_tags.is_empty();
        let empty_exclude = HashSet::new();
        let parsed = DynamicRegistry::from_openapi_with_tags(
            &spec,
            &self.include_tags,
            if use_exclude {
                &self.exclude_tags
            } else {
                &empty_exclude
            },
        )?;
        Ok(Arc::new(parsed))
    }

    async fn load_spec(&self) -> Result<Value, String> {
        if let Some(source) = &self.spec_source {
            return load_spec_from_source(&self.http_client, source).await;
        }

        let remote = format!(
            "{}{}",
            self.base_url.trim_end_matches('/'),
            OPENAPI_DEFAULT_PATH
        );
        match load_spec_from_source(&self.http_client, &remote).await {
            Ok(v) => Ok(v),
            Err(remote_err) => {
                let fallback = default_local_spec_path();
                match tokio::fs::read_to_string(&fallback).await {
                    Ok(content) => serde_json::from_str(&content).map_err(|e| {
                        format!("remote error: {remote_err}; fallback parse error: {e}")
                    }),
                    Err(local_err) => Err(format!(
                        "remote error: {remote_err}; local fallback error ({}): {local_err}",
                        fallback.display()
                    )),
                }
            }
        }
    }

    pub async fn dispatch_openapi(
        &self,
        operation: &DynamicOperation,
        arguments: Option<&JsonObject>,
    ) -> Result<CallToolResult, ErrorData> {
        if self.api_key.is_empty() {
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
                ParamLocation::Path => {
                    if p.auto_injected {
                        path = path.replace(&format!("{{{}}}", p.name), &self.athlete_id);
                        continue;
                    }

                    let replacement = args
                        .get(&p.name)
                        .and_then(stringify_argument)
                        .unwrap_or_default();
                    path = path.replace(&format!("{{{}}}", p.name), &replacement);
                }
                ParamLocation::Query => {
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

        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let mut req = self
            .http_client
            .request(operation.method.clone(), &url)
            .basic_auth("API_KEY", Some(self.api_key.clone()))
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

fn default_local_spec_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .map(ToOwned::to_owned)
        .unwrap_or(manifest);
    workspace_root.join("docs").join("intervals_icu_api.json")
}

async fn load_spec_from_source(http: &reqwest::Client, source: &str) -> Result<Value, String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let resp = http
            .get(source)
            .timeout(Duration::from_secs(OPENAPI_FETCH_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| format!("request error: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("unexpected status {status} from {source}"));
        }
        let value = resp
            .json::<Value>()
            .await
            .map_err(|e| format!("json decode error: {e}"))?;
        return Ok(value);
    }

    let content = tokio::fs::read_to_string(source)
        .await
        .map_err(|e| format!("read error: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("json parse error: {e}"))
}

fn contains_multipart(op: &Map<String, Value>) -> bool {
    op.get("requestBody")
        .and_then(|rb| rb.get("content"))
        .and_then(Value::as_object)
        .is_some_and(|content| content.keys().any(|k| k.contains("multipart")))
}

fn param_key(value: &Value) -> Option<(String, String)> {
    let obj = value.as_object()?;
    let name = obj.get("name")?.as_str()?.to_string();
    let location = obj.get("in")?.as_str()?.to_string();
    Some((location, name))
}

fn parameter_schema(param: &Value) -> Value {
    let schema = param
        .get("schema")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "type": "string" }));

    if schema.get("$ref").is_some() {
        serde_json::json!({ "type": "string" })
    } else {
        schema
    }
}

fn method_to_annotations(method: &reqwest::Method) -> ToolAnnotations {
    match *method {
        reqwest::Method::GET | reqwest::Method::HEAD | reqwest::Method::OPTIONS => {
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(true)
        }
        reqwest::Method::PUT => ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(true)
            .open_world(true),
        reqwest::Method::PATCH => ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(true),
        reqwest::Method::DELETE => ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(true)
            .open_world(true),
        _ => ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(false)
            .open_world(true),
    }
}

fn generate_operation_name(method: &str, path: &str) -> String {
    let mut cleaned = path
        .trim_matches('/')
        .replace("/", "_")
        .replace(['{', '}', '.'], "");
    if cleaned.is_empty() {
        cleaned = "root".to_string();
    }
    format!("{}_{}", method.to_ascii_lowercase(), cleaned)
}

fn is_auto_injected_athlete_param(name: &str, location: &str, path: &str) -> bool {
    if location != "path" {
        return false;
    }
    if name == "athleteId" {
        return true;
    }
    name == "id" && path.contains("/athlete/{id}")
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
        other => other.to_string(),
    }
}

fn parse_tag_set(tags: Option<&str>) -> HashSet<String> {
    tags.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

fn extract_operation_tags(op: &Map<String, Value>) -> Vec<String> {
    op.get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn should_filter_operation(
    operation_tags: &[String],
    include_tags: &HashSet<String>,
    exclude_tags: &HashSet<String>,
) -> bool {
    if !include_tags.is_empty() {
        return !operation_tags
            .iter()
            .any(|tag| include_tags.contains(&tag.to_ascii_lowercase()));
    }

    if !exclude_tags.is_empty() {
        return operation_tags
            .iter()
            .any(|tag| exclude_tags.contains(&tag.to_ascii_lowercase()));
    }

    false
}

fn add_response_control_properties(schema_props: &mut Map<String, Value>) {
    schema_props.entry("compact".to_string()).or_insert_with(|| {
        serde_json::json!({
            "type": "boolean",
            "description": "Enable token-efficient response mode. When false, returns full response payload."
        })
    });

    schema_props.entry("fields".to_string()).or_insert_with(|| {
        serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
            "description": "Optional response field filter applied when compact=true."
        })
    });
}

fn build_internal_tool_schema(mut schema: Value) -> JsonObject {
    if !schema.is_object() {
        schema = serde_json::json!({ "type": "object", "properties": {} });
    }

    let Some(schema_obj) = schema.as_object_mut() else {
        return JsonObject::new();
    };

    if !schema_obj.contains_key("type") {
        schema_obj.insert("type".to_string(), Value::String("object".to_string()));
    }

    if !schema_obj
        .get("properties")
        .is_some_and(serde_json::Value::is_object)
    {
        schema_obj.insert("properties".to_string(), Value::Object(Map::new()));
    }

    if let Some(props) = schema_obj
        .get_mut("properties")
        .and_then(Value::as_object_mut)
    {
        add_response_control_properties(props);
    }

    serde_json::from_value(schema).unwrap_or_default()
}

pub fn internal_tools() -> Vec<Tool> {
    let set_webhook_schema = build_internal_tool_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "secret": { "type": "string" }
        },
        "required": ["secret"]
    }));

    let download_schema = build_internal_tool_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "activity_id": { "type": "string" },
            "output_path": { "type": "string" }
        },
        "required": ["activity_id"]
    }));

    let id_schema = build_internal_tool_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "download_id": { "type": "string" }
        },
        "required": ["download_id"]
    }));

    let webhook_schema = build_internal_tool_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "signature": { "type": "string" },
            "payload": { "type": "object" }
        },
        "required": ["signature", "payload"]
    }));

    vec![
        Tool::new(
            "set_webhook_secret",
            "Set HMAC secret for webhook verification.",
            Arc::new(set_webhook_schema),
        ),
        Tool::new(
            "start_download",
            "Start activity file download and return download_id.",
            Arc::new(download_schema.clone()),
        ),
        Tool::new(
            "get_download_status",
            "Get current status for a download_id.",
            Arc::new(id_schema.clone()),
        ),
        Tool::new(
            "cancel_download",
            "Cancel an in-progress download by download_id.",
            Arc::new(id_schema),
        ),
        Tool::new(
            "list_downloads",
            "List all tracked downloads.",
            Arc::new(build_internal_tool_schema(serde_json::json!({
                "type": "object",
                "properties": {}
            }))),
        ),
        Tool::new(
            "receive_webhook",
            "Verify and ingest webhook payload.",
            Arc::new(webhook_schema),
        ),
    ]
}

pub fn merge_tools(dynamic: Vec<Tool>, internal: Vec<Tool>) -> ListToolsResult {
    let mut by_name: BTreeMap<String, Tool> = BTreeMap::new();

    for tool in dynamic {
        by_name.insert(tool.name.to_string(), tool);
    }
    for tool in internal {
        by_name.insert(tool.name.to_string(), tool);
    }

    ListToolsResult {
        tools: by_name.into_values().collect(),
        next_cursor: None,
        meta: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn spec_with_tagged_operations() -> Value {
        serde_json::json!({
            "openapi": "3.0.0",
            "paths": {
                "/api/v1/athlete/{id}/wellness": {
                    "get": {
                        "operationId": "getWellness",
                        "tags": ["Wellness"],
                        "parameters": [
                            {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}
                        ]
                    }
                },
                "/api/v1/athlete/{id}/gear": {
                    "get": {
                        "operationId": "getGear",
                        "tags": ["Gear"],
                        "parameters": [
                            {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}
                        ]
                    }
                }
            }
        })
    }

    #[test]
    fn tag_filter_include_only_keeps_matching_operations() {
        let mut include = HashSet::new();
        include.insert("wellness".to_string());

        let registry = DynamicRegistry::from_openapi_with_tags(
            &spec_with_tagged_operations(),
            &include,
            &HashSet::new(),
        )
        .expect("registry should parse");

        assert!(registry.operation("getWellness").is_some());
        assert!(registry.operation("getGear").is_none());
    }

    #[test]
    fn tag_filter_exclude_removes_matching_operations() {
        let mut exclude = HashSet::new();
        exclude.insert("gear".to_string());

        let registry = DynamicRegistry::from_openapi_with_tags(
            &spec_with_tagged_operations(),
            &HashSet::new(),
            &exclude,
        )
        .expect("registry should parse");

        assert!(registry.operation("getWellness").is_some());
        assert!(registry.operation("getGear").is_none());
    }

    #[tokio::test]
    async fn cached_registry_is_used_when_refresh_fails() {
        let tmp_file =
            std::env::temp_dir().join(format!("intervals_openapi_{}.json", uuid::Uuid::new_v4()));

        let initial_spec = serde_json::to_string(&spec_with_tagged_operations())
            .expect("spec serialization should succeed");
        tokio::fs::write(&tmp_file, initial_spec)
            .await
            .expect("initial spec file should be written");

        let runtime = DynamicRuntime::new(
            "https://intervals.icu".to_string(),
            "i123".to_string(),
            "api_key".to_string(),
            Some(tmp_file.to_string_lossy().to_string()),
            None,
            None,
            Duration::ZERO,
        );

        let first = runtime
            .ensure_registry()
            .await
            .expect("first load should work");
        assert!(first.operation("getWellness").is_some());

        tokio::fs::write(&tmp_file, "{ this is not valid json")
            .await
            .expect("broken spec should be written");

        let second = runtime
            .ensure_registry()
            .await
            .expect("should use cached registry when refresh fails");
        assert!(second.operation("getWellness").is_some());

        let _ = tokio::fs::remove_file(&tmp_file).await;
    }

    #[tokio::test]
    async fn initial_load_sets_refresh_timestamp() {
        let tmp_file =
            std::env::temp_dir().join(format!("intervals_openapi_{}.json", uuid::Uuid::new_v4()));

        let initial_spec = serde_json::to_string(&spec_with_tagged_operations())
            .expect("spec serialization should succeed");
        tokio::fs::write(&tmp_file, initial_spec)
            .await
            .expect("initial spec file should be written");

        let runtime = DynamicRuntime::new(
            "https://intervals.icu".to_string(),
            "i123".to_string(),
            "api_key".to_string(),
            Some(tmp_file.to_string_lossy().to_string()),
            None,
            None,
            Duration::from_secs(300),
        );

        runtime
            .ensure_registry()
            .await
            .expect("initial load should work");

        let refresh_marker = runtime.last_refresh_attempt.lock().await;
        assert!(
            refresh_marker.is_some(),
            "initial load should set last_refresh_attempt"
        );

        let _ = tokio::fs::remove_file(&tmp_file).await;
    }

    #[test]
    fn parse_response_body_parses_json_with_wildcard_content_type() {
        let bytes = br#"{"ok":true,"items":[1,2,3]}"#;
        let parsed = parse_response_body(bytes, "*/*");

        assert_eq!(parsed["ok"], Value::Bool(true));
        assert_eq!(parsed["items"][0], Value::from(1));
    }

    #[test]
    fn parse_response_body_reports_non_utf8_binary_payloads() {
        let bytes = [0_u8, 159, 146, 150];
        let parsed = parse_response_body(&bytes, "application/octet-stream");

        assert_eq!(parsed["binary"], Value::Bool(true));
        assert_eq!(parsed["byte_length"], Value::from(bytes.len()));
    }

    #[tokio::test]
    async fn dispatch_openapi_parses_json_even_when_content_type_is_wildcard() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/activity/42/map"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "*/*")
                    .set_body_string(r#"{"type":"FeatureCollection","features":[]}"#),
            )
            .mount(&server)
            .await;

        let runtime = DynamicRuntime::new(
            server.uri(),
            "ath-1".to_string(),
            "secret".to_string(),
            None,
            None,
            None,
            Duration::from_secs(300),
        );

        let schema = JsonObject::new();
        let operation = DynamicOperation {
            name: "getActivityMap".to_string(),
            method: reqwest::Method::GET,
            path_template: "/api/v1/activity/{id}/map".to_string(),
            description: "Get activity map data".to_string(),
            params: vec![ParamSpec {
                name: "id".to_string(),
                location: ParamLocation::Path,
                auto_injected: false,
            }],
            has_json_body: false,
            tool: Tool::new("getActivityMap", "Get activity map", Arc::new(schema)),
        };

        let mut args = JsonObject::new();
        args.insert("id".to_string(), Value::String("42".to_string()));
        let result = runtime
            .dispatch_openapi(&operation, Some(&args))
            .await
            .expect("dispatch should succeed");

        let as_value = serde_json::to_value(&result).expect("result should serialize");
        assert_eq!(as_value["structuredContent"]["status"], Value::from(200));
        assert_eq!(
            as_value["structuredContent"]["body"]["type"],
            Value::String("FeatureCollection".to_string())
        );
    }

    #[tokio::test]
    async fn dispatch_openapi_training_plan_uses_auto_injected_athlete_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/athlete/ath-42/training-plan"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "*/*")
                    .set_body_string(r#"{"weeks":4,"items":[]}"#),
            )
            .mount(&server)
            .await;

        let runtime = DynamicRuntime::new(
            server.uri(),
            "ath-42".to_string(),
            "secret".to_string(),
            None,
            None,
            None,
            Duration::from_secs(300),
        );

        let schema = JsonObject::new();
        let operation = DynamicOperation {
            name: "getAthleteTrainingPlan".to_string(),
            method: reqwest::Method::GET,
            path_template: "/api/v1/athlete/{id}/training-plan".to_string(),
            description: "Get athlete training plan".to_string(),
            params: vec![ParamSpec {
                name: "id".to_string(),
                location: ParamLocation::Path,
                auto_injected: true,
            }],
            has_json_body: false,
            tool: Tool::new(
                "getAthleteTrainingPlan",
                "Get athlete training plan",
                Arc::new(schema),
            ),
        };

        let result = runtime
            .dispatch_openapi(&operation, Some(&JsonObject::new()))
            .await
            .expect("dispatch should succeed");

        let as_value = serde_json::to_value(&result).expect("result should serialize");
        assert_eq!(as_value["structuredContent"]["status"], Value::from(200));
        assert_eq!(
            as_value["structuredContent"]["body"]["weeks"],
            Value::from(4)
        );
    }

    #[test]
    fn dynamic_tool_schema_includes_compact_and_fields_controls() {
        let spec = serde_json::json!({
            "openapi": "3.0.0",
            "paths": {
                "/api/v1/activity/{id}/map": {
                    "get": {
                        "operationId": "getActivityMap",
                        "parameters": [
                            {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}
                        ]
                    }
                }
            }
        });

        let registry = DynamicRegistry::from_openapi(&spec).expect("registry should parse");
        let tool = registry
            .operation("getActivityMap")
            .expect("operation should exist")
            .tool
            .clone();

        let tool_json = serde_json::to_value(tool).expect("tool should serialize");
        let properties = &tool_json["inputSchema"]["properties"];

        assert_eq!(
            properties["compact"]["type"],
            Value::String("boolean".to_string())
        );
        assert_eq!(
            properties["fields"]["type"],
            Value::String("array".to_string())
        );
        assert_eq!(
            properties["fields"]["items"]["type"],
            Value::String("string".to_string())
        );
    }

    #[test]
    fn internal_tools_schema_includes_compact_and_fields_controls() {
        let tools = internal_tools();

        for tool in tools {
            let tool_json = serde_json::to_value(&tool).expect("tool should serialize");
            let properties = &tool_json["inputSchema"]["properties"];

            assert_eq!(
                properties["compact"]["type"],
                Value::String("boolean".to_string()),
                "tool {} must expose compact boolean",
                tool.name
            );
            assert_eq!(
                properties["fields"]["type"],
                Value::String("array".to_string()),
                "tool {} must expose fields array",
                tool.name
            );
        }
    }
}
