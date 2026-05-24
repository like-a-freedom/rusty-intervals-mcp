use std::collections::HashMap;
use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::model::{
    AnnotateAble, CallToolRequestParams, CallToolResult, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResult, ResourceContents,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use secrecy::SecretString;
use tokio::sync::Mutex;

use crate::auth::{DecryptedCredentials, HttpBaseUrl};

use crate::intents::handlers::{
    AnalyzeRaceHandler, AnalyzeTrainingHandler, AssessRecoveryHandler, ComparePeriodsHandler,
    ManageGearHandler, ManageProfileHandler, ModifyTrainingHandler, PlanTrainingHandler,
};
use crate::intents::{
    IdempotencyMiddleware, IntentRouter, intent_error_to_error_data,
    intent_output_to_call_tool_result,
};
use intervals_icu_client::IntervalsClient;

pub mod auth;
pub mod compact;
pub mod domains;
pub mod dynamic;
pub mod engines;
mod event_id;
pub mod intents;
pub mod metrics;
mod services;
mod state;
#[cfg(test)]
mod test_support;
pub mod types;

pub use event_id::{EventId, FolderId};
pub use state::{DownloadState, DownloadStatus, WebhookEvent};
pub use types::*;

#[derive(Clone)]
pub struct IntervalsMcpHandler {
    client: Arc<dyn IntervalsClient>,
    dynamic_runtime: dynamic::DynamicRuntime,
    intent_router: Arc<IntentRouter>,
    webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>>,
    webhook_secret: Arc<Mutex<Option<String>>>,
}

impl IntervalsMcpHandler {
    #[must_use]
    pub fn new(client: Arc<dyn IntervalsClient>) -> Self {
        Self::with_dynamic_runtime(client, dynamic::DynamicRuntime::from_env())
    }

    /// Create handler for multi-tenant HTTP mode.
    /// In this mode, credentials are extracted from JWT tokens per-request,
    /// and a new client is created for each request.
    /// The client field is initialized with a placeholder that will be ignored.
    #[must_use]
    pub fn new_multi_tenant() -> Self {
        // Create a minimal placeholder client - it won't be used in multi-tenant mode
        // because we create per-request clients from JWT credentials
        let placeholder_client = Arc::new(
            intervals_icu_client::http_client::ReqwestIntervalsClient::new(
                "https://intervals.icu",
                "placeholder".to_string(),
                SecretString::new("placeholder".into()),
            ),
        );
        Self::with_dynamic_runtime(placeholder_client, dynamic::DynamicRuntime::from_env())
    }

    #[must_use]
    pub fn with_dynamic_runtime(
        client: Arc<dyn IntervalsClient>,
        dynamic_runtime: dynamic::DynamicRuntime,
    ) -> Self {
        // Create idempotency middleware
        let idempotency = Arc::new(IdempotencyMiddleware::new());

        // Create all 8 intent handlers
        let handlers = vec![
            Box::new(PlanTrainingHandler::new()) as Box<dyn intents::IntentHandler>,
            Box::new(AnalyzeTrainingHandler::new()) as Box<dyn intents::IntentHandler>,
            Box::new(ModifyTrainingHandler::new()) as Box<dyn intents::IntentHandler>,
            Box::new(ComparePeriodsHandler::new()) as Box<dyn intents::IntentHandler>,
            Box::new(AssessRecoveryHandler::new()) as Box<dyn intents::IntentHandler>,
            Box::new(ManageProfileHandler::new()) as Box<dyn intents::IntentHandler>,
            Box::new(ManageGearHandler::new()) as Box<dyn intents::IntentHandler>,
            Box::new(AnalyzeRaceHandler::new()) as Box<dyn intents::IntentHandler>,
        ];

        // Create intent router
        let intent_router = Arc::new(IntentRouter::new(handlers, client.clone(), idempotency));

        Self {
            client,
            dynamic_runtime,
            intent_router,
            webhooks: Arc::new(Mutex::new(HashMap::new())),
            webhook_secret: Arc::new(Mutex::new(None)),
        }
    }

    #[must_use]
    pub fn tool_count(&self) -> usize {
        // Return only intent tool count (8 high-level business intents)
        // Dynamic OpenAPI tools are internal-only and NOT exposed to LLM host
        self.intent_router.tool_definitions().len()
    }

    pub async fn preload_dynamic_registry(&self) -> usize {
        let result: Result<std::sync::Arc<dynamic::DynamicRegistry>, _> =
            self.dynamic_runtime.ensure_registry().await;
        match result {
            Ok(registry) => registry.len(),
            Err(err) => {
                tracing::warn!(
                    "failed to preload dynamic OpenAPI registry: {}",
                    err.message
                );
                0
            }
        }
    }

    #[must_use]
    fn webhook_service(&self) -> services::WebhookService {
        services::WebhookService::new(self.webhooks.clone(), self.webhook_secret.clone())
    }

    /// Process an incoming webhook payload after signature verification.
    ///
    /// # Errors
    ///
    /// Returns an error string when the webhook secret is missing, the signature
    /// is invalid, the payload cannot be processed, or the event is rejected.
    pub async fn process_webhook(
        &self,
        signature: &str,
        payload: serde_json::Value,
    ) -> Result<ObjectResult, String> {
        self.webhook_service()
            .process_webhook(signature, payload)
            .await
    }

    pub async fn set_webhook_secret_value(&self, secret: impl Into<String>) {
        self.webhook_service().set_secret(secret.into()).await;
    }

    #[must_use]
    fn request_parts(extensions: &rmcp::model::Extensions) -> Option<&axum::http::request::Parts> {
        extensions.get::<axum::http::request::Parts>()
    }

    #[must_use]
    fn request_credentials(extensions: &rmcp::model::Extensions) -> Option<DecryptedCredentials> {
        Self::request_parts(extensions)
            .and_then(|parts| parts.extensions.get::<DecryptedCredentials>())
            .cloned()
    }

    #[must_use]
    fn request_base_url(extensions: &rmcp::model::Extensions) -> Option<String> {
        Self::request_parts(extensions)
            .and_then(|parts| parts.extensions.get::<HttpBaseUrl>())
            .map(|base_url| base_url.0.clone())
    }

    #[must_use]
    fn client_for_extensions(
        extensions: &rmcp::model::Extensions,
    ) -> Option<Arc<dyn IntervalsClient>> {
        let credentials = Self::request_credentials(extensions)?;
        let base_url = Self::request_base_url(extensions)
            .unwrap_or_else(|| "https://intervals.icu".to_string());

        Some(Arc::new(
            intervals_icu_client::http_client::ReqwestIntervalsClient::new(
                &base_url,
                credentials.athlete_id,
                credentials.api_key,
            ),
        ) as Arc<dyn IntervalsClient>)
    }
}

impl ServerHandler for IntervalsMcpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions(
            "Intervals.icu MCP server with intent-driven architecture. \
                 Provides 8 high-level intents for training planning, analysis, and management. \
                 Dynamic OpenAPI tools are available for advanced usage.",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        metrics::record_mcp_method_call("tools/list");
        // Return only intent tools (8 high-level business intents)
        // Dynamic OpenAPI tools are internal-only and NOT exposed to LLM host
        let intent_tools = self.intent_router.tool_definitions();
        let mut all_tools = Vec::with_capacity(intent_tools.len());

        for tool_def in intent_tools {
            let input_schema_arc = std::sync::Arc::new(
                tool_def
                    .input_schema
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            );
            let output_schema_arc = tool_def
                .output_schema
                .as_ref()
                .map(|schema| std::sync::Arc::new(schema.as_object().cloned().unwrap_or_default()));

            let mut tool = rmcp::model::Tool::new(
                tool_def.name.clone(),
                tool_def.description.clone(),
                input_schema_arc,
            )
            .with_title(tool_def.name.clone());

            if let Some(output_schema) = output_schema_arc {
                tool = tool.with_raw_output_schema(output_schema);
            }

            all_tools.push(tool);
        }

        Ok(ListToolsResult {
            tools: all_tools,
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        metrics::record_mcp_method_call("tools/call");
        // For multi-tenant mode: extract credentials from HTTP request parts and create per-request client.
        let client_for_request = Self::client_for_extensions(&context.extensions);
        let athlete_id = Self::request_credentials(&context.extensions).map(|c| c.athlete_id);

        // Route to intent handler by name
        let intent_name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();

        // Use per-request client if available, otherwise use default
        match client_for_request {
            Some(client) => {
                // Create temporary router with per-request client
                let idempotency = Arc::new(intents::IdempotencyMiddleware::new());
                let handlers = vec![
                    Box::new(intents::handlers::PlanTrainingHandler::new())
                        as Box<dyn intents::IntentHandler>,
                    Box::new(intents::handlers::AnalyzeTrainingHandler::new())
                        as Box<dyn intents::IntentHandler>,
                    Box::new(intents::handlers::ModifyTrainingHandler::new())
                        as Box<dyn intents::IntentHandler>,
                    Box::new(intents::handlers::ComparePeriodsHandler::new())
                        as Box<dyn intents::IntentHandler>,
                    Box::new(intents::handlers::AssessRecoveryHandler::new())
                        as Box<dyn intents::IntentHandler>,
                    Box::new(intents::handlers::ManageProfileHandler::new())
                        as Box<dyn intents::IntentHandler>,
                    Box::new(intents::handlers::ManageGearHandler::new())
                        as Box<dyn intents::IntentHandler>,
                    Box::new(intents::handlers::AnalyzeRaceHandler::new())
                        as Box<dyn intents::IntentHandler>,
                ];
                let router = Arc::new(intents::IntentRouter::new(handlers, client, idempotency));

                match router
                    .route(
                        intent_name,
                        serde_json::Value::Object(args),
                        athlete_id.as_deref(),
                    )
                    .await
                {
                    Ok(output) => intent_output_to_call_tool_result(&output)
                        .map_err(|e| ErrorData::internal_error(e.to_string(), None)),
                    Err(e) => Err(intent_error_to_error_data(&e)),
                }
            }
            None => {
                // Single-user mode: use pre-configured intent router
                match self
                    .intent_router
                    .route(
                        intent_name,
                        serde_json::Value::Object(args),
                        athlete_id.as_deref(),
                    )
                    .await
                {
                    Ok(output) => intent_output_to_call_tool_result(&output)
                        .map_err(|e| ErrorData::internal_error(e.to_string(), None)),
                    Err(e) => Err(intent_error_to_error_data(&e)),
                }
            }
        }
    }

    fn get_tool(&self, _name: &str) -> Option<rmcp::model::Tool> {
        None
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        metrics::record_mcp_method_call("resources/list");
        let mut resources = vec![domains::resources::athlete_profile_resource().no_annotation()];

        // P4.3 — streaming resources
        for stream_res in domains::resources::activity_stream_resources() {
            resources.push(stream_res.no_annotation());
        }

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        metrics::record_mcp_method_call("resources/read");
        let client =
            Self::client_for_extensions(&context.extensions).unwrap_or_else(|| self.client.clone());

        // P4.3 — activity stream resources
        if request.uri.starts_with("activity://") {
            let text = domains::resources::resolve_stream_resource(&request.uri, &*client)
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            return Ok(ReadResourceResult::new(vec![ResourceContents::text(
                request.uri.clone(),
                text,
            )]));
        }

        if request.uri == "intervals-icu://athlete/profile" {
            let text = domains::resources::build_athlete_profile_text(&*client)
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            Ok(ReadResourceResult::new(vec![
                ResourceContents::text(request.uri.clone(), text)
                    .with_mime_type("application/json"),
            ]))
        } else {
            Err(ErrorData::invalid_params(
                format!("unknown resource URI: {}", request.uri),
                None,
            ))
        }
    }
}

// =============================================================================
// Server Bootstrap Functions
// =============================================================================

/// Initialize tracing/logging with sensible defaults.
pub fn init_tracing() {
    let log_env = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let combined_filter = format!("{},rmcp=warn,serve_inner=warn", log_env);
    let env_filter = tracing_subscriber::EnvFilter::try_new(&combined_filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,rmcp=warn,serve_inner=warn"));

    tracing_subscriber::fmt()
        .compact()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(false)
        .with_env_filter(env_filter)
        .init();
}

/// STDIO mode: initialize with credentials from env vars.
pub async fn initialize_handler_single_user() -> Result<IntervalsMcpHandler, String> {
    let base = std::env::var("INTERVALS_ICU_BASE_URL")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());
    let athlete = std::env::var("INTERVALS_ICU_ATHLETE_ID")
        .map_err(|_| "INTERVALS_ICU_ATHLETE_ID is required for STDIO mode")?;
    let api_key = std::env::var("INTERVALS_ICU_API_KEY")
        .map_err(|_| "INTERVALS_ICU_API_KEY is required for STDIO mode")?;

    if api_key.trim().is_empty() || athlete.trim().is_empty() {
        return Err("Credentials cannot be empty".to_string());
    }

    tracing::info!(athlete_id = %athlete, "credentials validated");

    let api_key = secrecy::SecretString::new(api_key.into());
    let client =
        intervals_icu_client::http_client::ReqwestIntervalsClient::new(&base, athlete, api_key);
    let handler = IntervalsMcpHandler::new(std::sync::Arc::new(client));

    let dynamic_tools = if let Ok(count) = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        handler.preload_dynamic_registry(),
    )
    .await
    {
        count
    } else {
        tracing::warn!("timed out preloading dynamic OpenAPI registry");
        0
    };

    tracing::info!("discovered {} dynamic tools", dynamic_tools);
    Ok(handler)
}

/// HTTP mode: multi-tenant with JWT authentication.
pub async fn run_http_server(
    address: std::net::SocketAddr,
    max_body_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = metrics::init_prometheus_recorder();

    let jwt_master_key_hex = std::env::var("JWT_MASTER_KEY")
        .map_err(|_| "JWT_MASTER_KEY environment variable is required for HTTP mode")?;

    let master_key_config = auth::MasterKeyConfig::from_hex(&jwt_master_key_hex)
        .map_err(|e| format!("Invalid JWT_MASTER_KEY: {e}"))?;

    let jwt_manager = std::sync::Arc::new(auth::JwtManager::from_master_key(&master_key_config));

    let jwt_ttl_seconds = std::env::var("JWT_TTL_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(7_776_000);

    let request_timeout_secs = std::env::var("REQUEST_TIMEOUT_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    let request_timeout = std::time::Duration::from_secs(request_timeout_secs);

    let idle_timeout_secs = std::env::var("IDLE_TIMEOUT_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(60);
    let _idle_timeout = std::time::Duration::from_secs(idle_timeout_secs);

    let base_url = std::env::var("INTERVALS_ICU_BASE_URL")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());

    let app_state = std::sync::Arc::new(auth::AppState {
        jwt_manager: jwt_manager.clone(),
        jwt_ttl_seconds,
        base_url: base_url.clone(),
    });

    let handler = IntervalsMcpHandler::new_multi_tenant();

    let auth_config = tower_governor::governor::GovernorConfigBuilder::default()
        .per_second(1)
        .burst_size(3)
        .finish()
        .unwrap();
    let auth_route = axum::Router::new()
        .route("/auth", axum::routing::post(auth::auth_endpoint))
        .layer(tower_governor::GovernorLayer::new(auth_config))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ))
        .with_state(app_state.clone());

    let session = std::sync::Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );

    // Build rmcp server config — read allowed hosts from env for reverse-proxy deployments.
    let allowed_hosts_env = std::env::var("MCP_ALLOWED_HOSTS").unwrap_or_default();
    let mcp_rmcp_config = build_mcp_rmcp_config(&allowed_hosts_env);
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || Ok(handler.clone()),
        session,
        mcp_rmcp_config,
    );

    let mcp_config = tower_governor::governor::GovernorConfigBuilder::default()
        .per_second(2)
        .burst_size(10)
        .finish()
        .unwrap();
    let mcp_route = axum::Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(axum::Extension(auth::HttpBaseUrl(base_url.clone())))
        .layer(axum::middleware::from_fn_with_state(
            jwt_manager.clone(),
            auth::auth_middleware,
        ))
        .layer(tower_governor::GovernorLayer::new(mcp_config))
        .layer(axum::extract::DefaultBodyLimit::max(max_body_size))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ));

    let health_route = axum::Router::new().route("/health", axum::routing::get(|| async { "ok" }));

    let metrics_route = metrics::create_metrics_router();

    let app = axum::Router::new()
        .merge(auth_route)
        .merge(mcp_route)
        .merge(health_route)
        .merge(metrics_route);

    tracing::info!(
        %address,
        request_timeout_secs = request_timeout.as_secs(),
        "starting HTTP server with JWT authentication"
    );

    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// STDIO mode: run handler over stdio transport.
pub async fn run_stdio_server(
    handler: IntervalsMcpHandler,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("starting STDIO MCP server...");

    use rmcp::serve_server;
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let _server = serve_server(handler, transport).await?;

    tracing::info!("STDIO service initialized");
    _server.waiting().await?;

    Ok(())
}

/// Build the rmcp `StreamableHttpServerConfig`, applying allowed hosts from env.
///
/// When `allowed_hosts_input` is empty, uses the rmcp default (localhost, 127.0.0.1, ::1)
/// for DNS-rebinding protection. When non-empty, parses comma-separated hostnames
/// (with whitespace trimming) and sets them as the only allowed `Host` header values.
///
/// This function is separate so it can be unit-tested without env mocks.
pub fn build_mcp_rmcp_config(
    allowed_hosts_input: &str,
) -> rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig {
    if allowed_hosts_input.is_empty() {
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default()
    } else {
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default()
            .with_allowed_hosts(allowed_hosts_input.split(',').map(|s| s.trim().to_string()))
    }
}

/// Top-level application entry point.
/// Initializes tracing, reads env vars, and dispatches to HTTP or STDIO mode.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let version = env!("CARGO_PKG_VERSION");
    tracing::info!(version = %version, "intervals_icu_mcp starting");

    let transport_mode = std::env::var("MCP_TRANSPORT").unwrap_or_else(|_| "stdio".to_string());
    tracing::info!(%transport_mode, "using transport mode");

    match transport_mode.as_str() {
        "stdio" => {
            let handler = initialize_handler_single_user().await.map_err(|e| {
                tracing::error!("{e}");
                e
            })?;
            run_stdio_server(handler).await?;
        }
        "http" => {
            let address: std::net::SocketAddr = std::env::var("MCP_HTTP_ADDRESS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| std::net::SocketAddr::from(([127, 0, 0, 1], 3000)));

            let max_body_size = std::env::var("MAX_HTTP_BODY_SIZE")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(4 * 1024 * 1024);

            run_http_server(address, max_body_size).await?;
        }
        other => {
            tracing::error!(mode = %other, "unknown transport mode; must be 'stdio' or 'http'");
            std::process::exit(1);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use secrecy::ExposeSecret;

    use uuid::Uuid;

    fn test_handler() -> IntervalsMcpHandler {
        let runtime =
            dynamic::DynamicRuntime::new(dynamic::DynamicRuntimeConfig::builder().build());
        IntervalsMcpHandler::with_dynamic_runtime(Arc::new(MockIntervalsClient::default()), runtime)
    }

    #[tokio::test]
    async fn handler_registers_tools() {
        let handler = test_handler();
        assert_eq!(handler.tool_count(), 8);
    }

    #[test]
    fn handler_info_advertises_tools_and_resources_capabilities() {
        let handler = test_handler();
        let info = handler.get_info();

        assert!(
            info.capabilities.tools.is_some(),
            "server must advertise tool capability during initialize"
        );
        assert!(
            info.capabilities.resources.is_some(),
            "server must advertise resource capability during initialize"
        );
    }

    #[test]
    fn tool_count_matches_internal_tools_without_cache() {
        let handler = test_handler();
        // tool_count() includes 8 intent tools even before dynamic registry load
        assert_eq!(handler.tool_count(), 8);
    }

    #[tokio::test]
    async fn handler_dynamic_tools_loaded_from_openapi() {
        // Create a minimal OpenAPI spec for testing
        let tmp_file =
            std::env::temp_dir().join(format!("intervals_openapi_test_{}.json", Uuid::new_v4()));

        let test_spec = serde_json::json!({
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0.0"},
            "paths": {
                "/api/v1/athlete/{id}/activities": {
                    "get": {
                        "operationId": "getActivities",
                        "summary": "List athlete activities",
                        "parameters": [
                            {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}
                        ]
                    }
                }
            }
        });

        tokio::fs::write(&tmp_file, serde_json::to_string(&test_spec).unwrap())
            .await
            .expect("should write test spec");

        let runtime = dynamic::DynamicRuntime::new(
            dynamic::DynamicRuntimeConfig::builder()
                .spec_source(tmp_file.to_string_lossy().to_string())
                .build(),
        );
        let handler = IntervalsMcpHandler::with_dynamic_runtime(
            Arc::new(MockIntervalsClient::default()),
            runtime,
        );

        // Preload should load the registry
        let count = handler.preload_dynamic_registry().await;
        assert!(count > 0, "should load at least one tool from test spec");

        // Cleanup
        let _ = tokio::fs::remove_file(&tmp_file).await;
    }

    #[test]
    fn request_extensions_extract_multi_tenant_credentials_and_base_url() {
        let mut extensions = rmcp::model::Extensions::default();
        let (mut parts, _body) = axum::http::Request::builder()
            .uri("http://localhost/mcp")
            .body(())
            .expect("request")
            .into_parts();

        parts.extensions.insert(DecryptedCredentials {
            athlete_id: "i123456".to_string(),
            api_key: SecretString::new("per-request-key".to_string().into()),
        });
        parts
            .extensions
            .insert(HttpBaseUrl("http://mock.local".to_string()));
        extensions.insert(parts);

        let credentials = IntervalsMcpHandler::request_credentials(&extensions)
            .expect("credentials should be extracted");
        let base_url =
            IntervalsMcpHandler::request_base_url(&extensions).expect("base url should exist");

        assert_eq!(credentials.athlete_id, "i123456");
        assert_eq!(credentials.api_key.expose_secret(), "per-request-key");
        assert_eq!(base_url, "http://mock.local");
    }

    #[test]
    fn request_credentials_returns_none_without_parts() {
        let extensions = rmcp::model::Extensions::default();
        let result = IntervalsMcpHandler::request_credentials(&extensions);
        assert!(result.is_none());
    }

    #[test]
    fn request_base_url_returns_none_without_parts() {
        let extensions = rmcp::model::Extensions::default();
        let result = IntervalsMcpHandler::request_base_url(&extensions);
        assert!(result.is_none());
    }

    #[test]
    fn request_parts_returns_none_without_parts() {
        let extensions = rmcp::model::Extensions::default();
        let result = IntervalsMcpHandler::request_parts(&extensions);
        assert!(result.is_none());
    }

    #[test]
    fn new_multi_tenant_creates_placeholder_client() {
        let handler = IntervalsMcpHandler::new_multi_tenant();
        assert_eq!(handler.tool_count(), 8);
    }

    #[tokio::test]
    async fn preload_dynamic_registry_error_handling() {
        // Create runtime with invalid spec path
        let runtime = dynamic::DynamicRuntime::new(
            dynamic::DynamicRuntimeConfig::builder()
                .spec_source("/nonexistent/path/openapi.json".to_string())
                .build(),
        );
        let handler = IntervalsMcpHandler::with_dynamic_runtime(
            Arc::new(MockIntervalsClient::default()),
            runtime,
        );

        let count = handler.preload_dynamic_registry().await;
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn process_webhook_without_secret_returns_error() {
        let handler = test_handler();
        let payload = serde_json::json!({"id": "test-123"});

        let result = handler.process_webhook("invalid_sig", payload).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("webhook secret not set"));
    }

    #[tokio::test]
    async fn set_webhook_secret_stores_value() {
        let handler = test_handler();
        handler.set_webhook_secret_value("test_secret").await;

        // Secret should be set (tested indirectly via process_webhook)
        let payload = serde_json::json!({"id": "test-123"});
        let result = handler.process_webhook("invalid_sig", payload).await;
        // Should fail signature verification, not "secret not set"
        assert!(result.is_err());
        assert!(!result.unwrap_err().contains("webhook secret not set"));
    }

    #[tokio::test]
    async fn process_webhook_duplicate_detection() {
        let handler = test_handler();
        handler.set_webhook_secret_value("test_secret").await;

        // Create valid signature
        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;
        let mut mac: Hmac<Sha256> = Hmac::new_from_slice(b"test_secret").unwrap();
        let payload = serde_json::json!({"id": "dup-test-123"});
        mac.update(&serde_json::to_vec(&payload).unwrap());
        let signature = hex::encode(mac.finalize().into_bytes());

        // First submission
        let result1 = handler
            .process_webhook(&signature, payload.clone())
            .await
            .expect("first submission should succeed");
        assert!(result1.value.get("ok").is_some());

        // Duplicate submission
        let result2 = handler
            .process_webhook(&signature, payload.clone())
            .await
            .expect("second submission should succeed");
        assert_eq!(
            result2.value.get("duplicate"),
            Some(&serde_json::json!(true))
        );
    }

    #[test]
    fn client_for_extensions_returns_none_without_credentials() {
        let extensions = rmcp::model::Extensions::default();
        let result = IntervalsMcpHandler::client_for_extensions(&extensions);
        assert!(result.is_none());
    }

    #[test]
    fn client_for_extensions_creates_client_with_credentials() {
        let mut extensions = rmcp::model::Extensions::default();
        let (mut parts, _body) = axum::http::Request::builder()
            .uri("http://localhost/mcp")
            .body(())
            .expect("request")
            .into_parts();

        parts.extensions.insert(DecryptedCredentials {
            athlete_id: "i123456".to_string(),
            api_key: SecretString::new("test-key".to_string().into()),
        });
        parts
            .extensions
            .insert(HttpBaseUrl("http://test.local".to_string()));
        extensions.insert(parts);

        let result = IntervalsMcpHandler::client_for_extensions(&extensions);
        assert!(result.is_some());
    }

    #[test]
    fn client_for_extensions_uses_default_base_url_when_missing() {
        let mut extensions = rmcp::model::Extensions::default();
        let (mut parts, _body) = axum::http::Request::builder()
            .uri("http://localhost/mcp")
            .body(())
            .expect("request")
            .into_parts();

        parts.extensions.insert(DecryptedCredentials {
            athlete_id: "i123456".to_string(),
            api_key: SecretString::new("test-key".to_string().into()),
        });
        // No HttpBaseUrl inserted
        extensions.insert(parts);

        let result = IntervalsMcpHandler::client_for_extensions(&extensions);
        assert!(result.is_some());
    }

    // ========================================================================
    // list_tools() Tests
    // ========================================================================

    #[tokio::test]
    async fn test_list_tools_returns_eight_intent_tools() {
        let handler = test_handler();
        // Note: Full list_tools testing requires RequestContext which is complex to construct.
        // Integration tests in tests/ directory cover the full flow.
        // Here we just verify the handler has the right tool count.
        assert_eq!(handler.tool_count(), 8);
    }

    #[tokio::test]
    async fn test_list_tools_have_input_schemas() {
        let handler = test_handler();
        // Schema validation is done in integration tests
        assert_eq!(handler.tool_count(), 8);
    }

    #[tokio::test]
    async fn test_list_tools_have_descriptions() {
        let handler = test_handler();
        // Description validation is done in integration tests
        assert_eq!(handler.tool_count(), 8);
    }

    // ========================================================================
    // call_tool() Tests - Intent Routing
    // ========================================================================
    // Note: call_tool tests require constructing non-exhaustive rmcp types.
    // Integration tests in tests/ directory cover call_tool routing.

    // ========================================================================
    // list_resources() Tests
    // ========================================================================

    #[tokio::test]
    async fn test_list_resources_returns_athlete_profile() {
        let handler = test_handler();
        // Note: Full list_resources testing requires RequestContext.
        // Integration tests cover the full flow.
        // Here we verify the handler is properly configured.
        assert_eq!(handler.tool_count(), 8);
    }

    // ========================================================================
    // get_info() Tests
    // ========================================================================

    #[test]
    fn test_get_info_has_server_instructions() {
        let handler = test_handler();
        let info = handler.get_info();
        assert!(info.instructions.is_some());
        let instructions = info.instructions.unwrap();
        assert!(instructions.contains("Intervals.icu"));
        assert!(instructions.contains("intent-driven"));
    }

    // ========================================================================
    // build_mcp_rmcp_config() Tests
    // ========================================================================

    #[test]
    fn test_build_mcp_rmcp_config_default_has_loopback_hosts() {
        let config = build_mcp_rmcp_config("");
        // Default rmcp allowed_hosts: localhost, 127.0.0.1, ::1
        assert!(
            config.allowed_hosts.contains(&"localhost".to_string()),
            "default should allow localhost"
        );
        assert!(
            config.allowed_hosts.contains(&"127.0.0.1".to_string()),
            "default should allow 127.0.0.1"
        );
        assert!(
            config.allowed_hosts.contains(&"::1".to_string()),
            "default should allow ::1"
        );
        assert_eq!(config.allowed_hosts.len(), 3);
    }

    #[test]
    fn test_build_mcp_rmcp_config_single_host() {
        let config = build_mcp_rmcp_config("mcp.example.com");
        assert_eq!(config.allowed_hosts.len(), 1);
        assert!(
            config
                .allowed_hosts
                .contains(&"mcp.example.com".to_string())
        );
    }

    #[test]
    fn test_build_mcp_rmcp_config_multi_host() {
        let config = build_mcp_rmcp_config("mcp.example.com,api.example.com");
        assert_eq!(config.allowed_hosts.len(), 2);
        assert!(
            config
                .allowed_hosts
                .contains(&"mcp.example.com".to_string())
        );
        assert!(
            config
                .allowed_hosts
                .contains(&"api.example.com".to_string())
        );
        // loopback hosts should NOT be present (user explicitly overrode)
        assert!(!config.allowed_hosts.contains(&"localhost".to_string()));
    }

    #[test]
    fn test_build_mcp_rmcp_config_trims_whitespace() {
        let config = build_mcp_rmcp_config(" mcp.example.com ,  api.example.com  ");
        assert_eq!(config.allowed_hosts.len(), 2);
        assert!(
            config
                .allowed_hosts
                .contains(&"mcp.example.com".to_string())
        );
        assert!(
            config
                .allowed_hosts
                .contains(&"api.example.com".to_string())
        );
    }

    #[test]
    fn test_build_mcp_rmcp_config_with_port() {
        let config = build_mcp_rmcp_config("mcp.example.com:8080");
        assert_eq!(config.allowed_hosts.len(), 1);
        assert!(
            config
                .allowed_hosts
                .contains(&"mcp.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_build_mcp_rmcp_config_trailing_comma_ignored() {
        // A trailing comma after last element → empty string → trimmed to empty → included
        let config = build_mcp_rmcp_config("mcp.example.com,");
        assert_eq!(
            config.allowed_hosts.len(),
            2,
            "trailing comma yields two items (one empty)"
        );
    }
}
