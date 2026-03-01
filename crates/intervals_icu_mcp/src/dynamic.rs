use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rmcp::ErrorData;
use rmcp::model::{CallToolResult, Content, JsonObject, ListToolsResult, Tool, ToolAnnotations};
use serde_json::{Map, Value};

const OPENAPI_DEFAULT_PATH: &str = "/api/v1/docs";

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
    pub fn empty() -> Self {
        Self {
            operations: HashMap::new(),
        }
    }

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
    registry: Arc<tokio::sync::RwLock<Option<Arc<DynamicRegistry>>>>,
    cached_tool_count: Arc<AtomicUsize>,
}

impl DynamicRuntime {
    pub fn from_env() -> Self {
        let base_url = std::env::var("INTERVALS_ICU_BASE_URL")
            .unwrap_or_else(|_| "https://intervals.icu".to_string());
        let athlete_id = std::env::var("INTERVALS_ICU_ATHLETE_ID").unwrap_or_default();
        let api_key = std::env::var("INTERVALS_ICU_API_KEY").unwrap_or_default();
        let spec_source = std::env::var("INTERVALS_ICU_OPENAPI_SPEC").ok();

        Self {
            http_client: reqwest::Client::new(),
            base_url,
            athlete_id,
            api_key,
            spec_source,
            registry: Arc::new(tokio::sync::RwLock::new(None)),
            cached_tool_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn cached_tool_count(&self) -> usize {
        self.cached_tool_count.load(Ordering::Relaxed)
    }

    pub async fn ensure_registry(&self) -> Result<Arc<DynamicRegistry>, ErrorData> {
        if let Some(existing) = self.registry.read().await.clone() {
            return Ok(existing);
        }

        let spec = self.load_spec().await.map_err(|e| {
            ErrorData::internal_error(format!("failed to load OpenAPI spec: {e}"), None)
        })?;
        let parsed = DynamicRegistry::from_openapi(&spec).map_err(|e| {
            ErrorData::internal_error(format!("failed to parse OpenAPI spec: {e}"), None)
        })?;

        let parsed = Arc::new(parsed);
        self.cached_tool_count
            .store(parsed.len(), Ordering::Relaxed);

        let mut write = self.registry.write().await;
        *write = Some(parsed.clone());
        Ok(parsed)
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

        let body_json = if content_type.contains("json") {
            serde_json::from_slice::<Value>(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        } else {
            Value::String(String::from_utf8_lossy(&bytes).into_owned())
        };

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
            "body": normalized_body,
        });

        if status.is_success() {
            Ok(CallToolResult::structured(wrapper))
        } else {
            Ok(CallToolResult::structured_error(wrapper))
        }
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

#[derive(Debug, Clone)]
pub struct CompatAlias {
    pub alias_name: Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub target_operation: Cow<'static, str>,
    pub input_schema: Arc<JsonObject>,
}

impl CompatAlias {
    pub fn as_tool(&self) -> Tool {
        Tool::new(
            self.alias_name.clone(),
            self.description.clone(),
            self.input_schema.clone(),
        )
    }
}

pub fn default_compat_aliases() -> Vec<CompatAlias> {
    let profile_schema: JsonObject = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {}
    }))
    .unwrap_or_default();

    let recent_schema: JsonObject = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
            "days_back": { "type": "integer", "minimum": 0 }
        }
    }))
    .unwrap_or_default();

    vec![
        CompatAlias {
            alias_name: Cow::Borrowed("get_athlete_profile"),
            description: Cow::Borrowed(
                "Compatibility alias. Get athlete profile including id and name.",
            ),
            target_operation: Cow::Borrowed("getAthleteProfile"),
            input_schema: Arc::new(profile_schema),
        },
        CompatAlias {
            alias_name: Cow::Borrowed("get_recent_activities"),
            description: Cow::Borrowed(
                "Compatibility alias. Get recent activities with optional limit and days_back.",
            ),
            target_operation: Cow::Borrowed("listActivities"),
            input_schema: Arc::new(recent_schema),
        },
    ]
}

pub fn internal_tools() -> Vec<Tool> {
    let set_webhook_schema: JsonObject = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "secret": { "type": "string" }
        },
        "required": ["secret"]
    }))
    .unwrap_or_default();

    let download_schema: JsonObject = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "activity_id": { "type": "string" },
            "output_path": { "type": "string" }
        },
        "required": ["activity_id"]
    }))
    .unwrap_or_default();

    let id_schema: JsonObject = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "download_id": { "type": "string" }
        },
        "required": ["download_id"]
    }))
    .unwrap_or_default();

    let webhook_schema: JsonObject = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "signature": { "type": "string" },
            "payload": { "type": "object" }
        },
        "required": ["signature", "payload"]
    }))
    .unwrap_or_default();

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
            Arc::new(JsonObject::new()),
        ),
        Tool::new(
            "receive_webhook",
            "Verify and ingest webhook payload.",
            Arc::new(webhook_schema),
        ),
    ]
}

pub fn merge_tools(
    dynamic: Vec<Tool>,
    aliases: &[CompatAlias],
    internal: Vec<Tool>,
) -> ListToolsResult {
    let mut by_name: BTreeMap<String, Tool> = BTreeMap::new();

    for tool in dynamic {
        by_name.insert(tool.name.to_string(), tool);
    }
    for alias in aliases {
        let t = alias.as_tool();
        by_name.insert(t.name.to_string(), t);
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

pub fn result_text(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text.into())])
}
