//! # Intervals.icu MCP Server
//!
//! Model Context Protocol server that exposes Intervals.icu training data
//! as AI-agent tools (intents).  Built on top of `intervals_icu_client`.
//!
//! ## Layout
//!
//! - [`engines`] — domain logic: analysis, metrics, fetch orchestration
//! - [`intents`] — MCP tool handlers (intents) and routing
//! - [`auth`] — JWT-based authentication for multi-tenant HTTP mode
//! - [`domains`] — domain types and value objects
//! - [`metrics`] — Prometheus metrics recording
//! - [`dynamic`] — dynamic OpenAPI-driven tool registry

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    ResourceContents, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use secrecy::SecretString;

use crate::auth::{DecryptedCredentials, HttpBaseUrl};

use crate::intents::handlers::{
    AnalyzeRaceHandler, AnalyzeTrainingHandler, AssessRecoveryHandler, ComparePeriodsHandler,
    ManageGearHandler, ManageProfileHandler, ModifyTrainingHandler, PlanTrainingHandler,
    TrackProgressHandler,
};
use crate::intents::{
    IdempotencyMiddleware, IntentRouter, intent_error_to_error_data,
    intent_output_to_call_tool_result,
};
use intervals_icu_client::IntervalsClient;

pub mod auth;
pub mod auth_ui;
pub mod compact;
pub mod content;
pub mod domains;
pub mod dynamic;
pub mod engines;
pub mod intents;
pub mod metrics;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

fn all_intent_handlers() -> Vec<Box<dyn intents::IntentHandler>> {
    vec![
        Box::new(PlanTrainingHandler::new()) as Box<dyn intents::IntentHandler>,
        Box::new(AnalyzeTrainingHandler::new()) as Box<dyn intents::IntentHandler>,
        Box::new(ModifyTrainingHandler::new()) as Box<dyn intents::IntentHandler>,
        Box::new(ComparePeriodsHandler::new()) as Box<dyn intents::IntentHandler>,
        Box::new(AssessRecoveryHandler::new()) as Box<dyn intents::IntentHandler>,
        Box::new(ManageProfileHandler::new()) as Box<dyn intents::IntentHandler>,
        Box::new(ManageGearHandler::new()) as Box<dyn intents::IntentHandler>,
        Box::new(AnalyzeRaceHandler::new()) as Box<dyn intents::IntentHandler>,
        Box::new(TrackProgressHandler::new()) as Box<dyn intents::IntentHandler>,
    ]
}

#[derive(Clone)]
pub struct IntervalsMcpHandler {
    client: Arc<dyn IntervalsClient>,
    dynamic_runtime: dynamic::DynamicRuntime,
    intent_router: Arc<IntentRouter>,
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
    pub fn new_multi_tenant() -> Result<Self, Box<dyn std::error::Error>> {
        // Create a minimal placeholder client - it won't be used in multi-tenant mode
        // because we create per-request clients from JWT credentials
        let placeholder_client = Arc::new(
            intervals_icu_client::http_client::ReqwestIntervalsClient::new(
                "https://intervals.icu",
                "placeholder".to_string(),
                SecretString::new("placeholder".into()),
            )?,
        );
        Ok(Self::with_dynamic_runtime(
            placeholder_client,
            dynamic::DynamicRuntime::from_env(),
        ))
    }

    #[must_use]
    pub fn with_dynamic_runtime(
        client: Arc<dyn IntervalsClient>,
        dynamic_runtime: dynamic::DynamicRuntime,
    ) -> Self {
        // Create idempotency middleware
        let idempotency = Arc::new(IdempotencyMiddleware::new());

        // Phase 2B (ADR-0001): when dynamic dispatch is enabled via
        // env var, wrap the typed client in `DynamicClientAdapter` so
        // mapped trait methods dispatch through the OpenAPI runtime;
        // everything else falls back to the typed path. Disabled by
        // default — flip with `INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED=1`
        // after Phase 2C proves a real call site.
        let client: Arc<dyn IntervalsClient> = if dynamic_dispatch_enabled() {
            Arc::new(dynamic::DynamicClientAdapter::new(
                Arc::new(dynamic_runtime.clone()),
                client,
            ))
        } else {
            client
        };

        let handlers = all_intent_handlers();

        // Create intent router
        let intent_router = Arc::new(IntentRouter::new(handlers, client.clone(), idempotency));

        Self {
            client,
            dynamic_runtime,
            intent_router,
        }
    }

    #[must_use]
    pub fn tool_count(&self) -> usize {
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
            )
            .ok()?,
        ) as Arc<dyn IntervalsClient>)
    }
}

impl ServerHandler for IntervalsMcpHandler {
    async fn initialize(
        &self,
        request: rmcp::model::InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ServerConfig, ErrorData> {
        metrics::record_mcp_session("started");
        context.peer.set_peer_info(request);
        Ok(self.get_info())
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions(
            "Intervals.icu MCP server with intent-driven architecture. \
                 Provides 9 high-level intents for training planning, analysis, and management. \
                 The dynamic OpenAPI registry is private implementation state and is not exposed as MCP tools.",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        metrics::record_mcp_method_call("tools/list");

        // The public MCP surface is the curated intent catalogue only.
        let intent_tools = self.intent_router.tool_definitions();
        let mut public_tools: Vec<rmcp::model::Tool> = Vec::with_capacity(intent_tools.len());

        for tool_def in &intent_tools {
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

            public_tools.push(tool);
        }

        Ok(ListToolsResult::with_all_items(public_tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        metrics::record_mcp_method_call("tools/call");
        let client_for_request = Self::client_for_extensions(&context.extensions);
        let athlete_id = Self::request_credentials(&context.extensions).map(|c| c.athlete_id);

        let intent_name = request.name.as_ref();
        let args: rmcp::model::JsonObject = request.arguments.unwrap_or_default();

        // Route exclusively through the intent layer. Dynamic OpenAPI operations
        // are private implementation capabilities, never MCP tool names.
        let intent_result = match &client_for_request {
            Some(client) => {
                let idempotency = Arc::new(intents::IdempotencyMiddleware::new());
                let handlers = all_intent_handlers();
                let router = Arc::new(intents::IntentRouter::new(
                    handlers,
                    client.clone(),
                    idempotency,
                ));
                router
                    .route(
                        intent_name,
                        serde_json::Value::Object(args.clone()),
                        athlete_id.as_deref(),
                    )
                    .await
            }
            None => {
                self.intent_router
                    .route(
                        intent_name,
                        serde_json::Value::Object(args.clone()),
                        athlete_id.as_deref(),
                    )
                    .await
            }
        };

        match intent_result {
            Ok(output) => intent_output_to_call_tool_result(&output)
                .map(CallToolResponse::from)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None)),
            Err(e) => Err(intent_error_to_error_data(&e)),
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
        let mut resources = vec![domains::resources::athlete_profile_resource()];

        // P4.3 — streaming resources
        for stream_res in domains::resources::activity_stream_resources() {
            resources.push(stream_res);
        }

        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
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
            )])
            .into());
        }

        if request.uri == "intervals-icu://athlete/profile" {
            let text = domains::resources::build_athlete_profile_text(&*client)
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            Ok(ReadResourceResult::new(vec![
                ResourceContents::text(request.uri.clone(), text)
                    .with_mime_type("application/json"),
            ])
            .into())
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
        intervals_icu_client::http_client::ReqwestIntervalsClient::new(&base, athlete, api_key)
            .map_err(|e| format!("failed to create client: {e}"))?;
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
    let idle_timeout = std::time::Duration::from_secs(idle_timeout_secs);

    let (mcp_rate_per_second, mcp_burst_size) = parse_rate_limit_values(
        std::env::var("MCP_RATE_LIMIT_PER_SECOND").ok().as_deref(),
        std::env::var("MCP_RATE_LIMIT_BURST").ok().as_deref(),
    );

    let base_url = std::env::var("INTERVALS_ICU_BASE_URL")
        .unwrap_or_else(|_| "https://intervals.icu".to_string());

    let revoked_jtis: auth::TokenRevocationSet = Arc::new(tokio::sync::RwLock::new(HashSet::new()));

    let app_state = std::sync::Arc::new(auth::AppState {
        jwt_manager: jwt_manager.clone(),
        jwt_ttl_seconds,
        base_url: base_url.clone(),
        revoked_jtis: revoked_jtis.clone(),
    });

    let handler = IntervalsMcpHandler::new_multi_tenant().expect("new_multi_tenant");

    let registry_path = std::env::var("MCP_TOKEN_REGISTRY_PATH")
        .ok()
        .map(PathBuf::from);
    let cookie_secure = std::env::var("MCP_COOKIE_SECURE")
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);
    let ui_state = auth_ui::UiState::new(
        app_state.clone(),
        revoked_jtis.clone(),
        registry_path,
        cookie_secure,
    );

    let ui_config = tower_governor::governor::GovernorConfigBuilder::default()
        .per_second(2)
        .burst_size(5)
        .finish()
        .unwrap();

    let ui_route = axum::Router::new()
        .route("/ui", axum::routing::get(auth_ui::ui_home))
        .route("/ui/token", axum::routing::post(auth_ui::ui_create_token))
        .route("/ui/tokens", axum::routing::get(auth_ui::ui_list_tokens))
        .route(
            "/ui/revoke/{jti}",
            axum::routing::post(auth_ui::ui_revoke_token),
        )
        .route("/ui/static/css", axum::routing::get(auth_ui::serve_css))
        .layer(tower_governor::GovernorLayer::new(ui_config))
        .with_state(ui_state);

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
        .per_second(mcp_rate_per_second)
        .burst_size(mcp_burst_size)
        .key_extractor(AthleteKeyExtractor)
        .finish()
        .unwrap();
    let mcp_governor = tower_governor::GovernorLayer::new(mcp_config).error_handler(|err| {
        if let tower_governor::errors::GovernorError::TooManyRequests { wait_time, .. } = &err {
            metrics::record_rate_limited("mcp");
            tracing::warn!(wait_time, "MCP rate limit exceeded");
        }
        err.into_response().map(axum::body::Body::from)
    });
    let mcp_route = axum::Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(axum::Extension(auth::HttpBaseUrl(base_url.clone())))
        .layer(mcp_governor)
        .layer(axum::middleware::from_fn_with_state(
            app_state.clone(),
            auth::auth_middleware,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(max_body_size))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ));

    let health_route = axum::Router::new().route("/health", axum::routing::get(|| async { "ok" }));

    let metrics_route = metrics::create_metrics_router();

    let app = axum::Router::new()
        .route(
            "/",
            axum::routing::get(|| async { axum::response::Redirect::to("/ui") }),
        )
        .merge(auth_route)
        .merge(ui_route)
        .merge(mcp_route)
        .merge(health_route)
        .merge(metrics_route);

    tracing::info!(
        %address,
        request_timeout_secs = request_timeout.as_secs(),
        "starting HTTP server with JWT authentication"
    );

    // Create TCP socket with keepalive to prevent idle connection drops
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(address),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    configure_tcp_keepalive(&socket, idle_timeout)?;
    socket.set_nonblocking(true)?;
    let sock_addr = socket2::SockAddr::from(address);
    socket.bind(&sock_addr)?;
    socket.listen(128)?;
    // Convert socket2::Socket → std::net::TcpListener → tokio::net::TcpListener
    let std_listener: std::net::TcpListener = socket.into();
    let listener = tokio::net::TcpListener::from_std(std_listener)?;

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

/// Rate limit key extractor that uses `athlete_id` from JWT credentials
/// as the rate-limiting key. Falls back to client IP for unauthenticated requests.
#[derive(Debug, Clone)]
struct AthleteKeyExtractor;

impl tower_governor::key_extractor::KeyExtractor for AthleteKeyExtractor {
    type Key = String;

    fn extract<T>(
        &self,
        req: &axum::http::Request<T>,
    ) -> Result<String, tower_governor::errors::GovernorError> {
        // Prefer athlete_id from JWT credentials (set by auth_middleware)
        if let Some(creds) = req.extensions().get::<auth::DecryptedCredentials>() {
            return Ok(creds.athlete_id.clone());
        }
        // Fallback to client IP for unauthenticated endpoints
        if let Some(addr) = req
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        {
            return Ok(format!("ip:{}", addr.0.ip()));
        }
        Err(tower_governor::errors::GovernorError::UnableToExtractKey)
    }
}

/// Check whether dynamic upstream dispatch is enabled.
///
/// When `true`, [`IntervalsMcpHandler::with_dynamic_runtime`] wraps the
/// typed client in [`dynamic::DynamicClientAdapter`], routing mapped
/// trait methods through the live OpenAPI registry. Default: `false`.
/// Phase 2B of ADR-0001; flip to `1` after Phase 2C proves a real call
/// site.
fn dynamic_dispatch_enabled() -> bool {
    matches!(
        std::env::var("INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

/// Parse rate limit config from optional string values.
/// Returns `(per_second, burst_size)` with defaults of 5 and 15.
fn parse_rate_limit_values(per_second: Option<&str>, burst_size: Option<&str>) -> (u64, u32) {
    let rate = match per_second
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&v| v > 0)
    {
        Some(v) => v,
        None => {
            if per_second.is_some() {
                tracing::warn!(
                    value = ?per_second,
                    "MCP_RATE_LIMIT_PER_SECOND is invalid or zero, using default 5"
                );
            }
            5
        }
    };
    let burst = match burst_size
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&v| v > 0)
    {
        Some(v) => v,
        None => {
            if burst_size.is_some() {
                tracing::warn!(
                    value = ?burst_size,
                    "MCP_RATE_LIMIT_BURST is invalid or zero, using default 15"
                );
            }
            15
        }
    };
    (rate, burst)
}

/// Configure TCP keepalive on a socket2::Socket.
///
/// Sets idle time before sending keepalive probes and interval between probes.
/// OS defaults are used for retry count.
fn configure_tcp_keepalive(
    socket: &socket2::Socket,
    idle: std::time::Duration,
) -> Result<(), std::io::Error> {
    socket.set_keepalive(true)?;
    // The OS rejects zero values for keepalive time; clamp to at least 1 second.
    let idle = if idle.is_zero() {
        std::time::Duration::from_secs(1)
    } else {
        idle
    };
    let ka = socket2::TcpKeepalive::new()
        .with_time(idle)
        .with_interval(std::time::Duration::from_secs(10));
    socket.set_tcp_keepalive(&ka)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // Inline tests for private symbols of `lib.rs`.
    use crate::auth::{DecryptedCredentials, HttpBaseUrl};
    use crate::{
        AthleteKeyExtractor, IntervalsMcpHandler, configure_tcp_keepalive, parse_rate_limit_values,
    };
    use secrecy::{ExposeSecret, SecretString};

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
    // ── AthleteKeyExtractor tests ──────────────────────────────────────

    #[test]
    fn athlete_key_extractor_extracts_athlete_id_from_credentials() {
        use tower_governor::key_extractor::KeyExtractor;

        let mut req = axum::http::Request::builder()
            .uri("http://localhost/mcp")
            .body(())
            .unwrap();
        req.extensions_mut().insert(DecryptedCredentials {
            athlete_id: "athlete-42".to_string(),
            api_key: SecretString::new("secret-key".to_string().into()),
        });

        let key = AthleteKeyExtractor.extract(&req).unwrap();
        assert_eq!(key, "athlete-42");
    }

    #[test]
    fn athlete_key_extractor_falls_back_to_ip_when_no_credentials() {
        use tower_governor::key_extractor::KeyExtractor;

        let addr: std::net::SocketAddr = "10.0.0.1:54321".parse().unwrap();
        let mut req = axum::http::Request::builder()
            .uri("http://localhost/mcp")
            .body(())
            .unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(addr));

        let key = AthleteKeyExtractor.extract(&req).unwrap();
        assert_eq!(key, "ip:10.0.0.1");
    }

    #[test]
    fn athlete_key_extractor_returns_error_when_no_key_available() {
        use tower_governor::key_extractor::KeyExtractor;

        let req = axum::http::Request::builder()
            .uri("http://localhost/mcp")
            .body(())
            .unwrap();

        let result = AthleteKeyExtractor.extract(&req);
        assert!(result.is_err());
    }

    #[test]
    fn athlete_key_extractor_prefers_credentials_over_ip() {
        use tower_governor::key_extractor::KeyExtractor;

        let addr: std::net::SocketAddr = "10.0.0.1:54321".parse().unwrap();
        let mut req = axum::http::Request::builder()
            .uri("http://localhost/mcp")
            .body(())
            .unwrap();
        req.extensions_mut().insert(DecryptedCredentials {
            athlete_id: "athlete-99".to_string(),
            api_key: SecretString::new("key".to_string().into()),
        });
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(addr));

        let key = AthleteKeyExtractor.extract(&req).unwrap();
        assert_eq!(key, "athlete-99");
    }
    // ── Rate limit config parsing tests ─────────────────────────────────

    #[test]
    fn parse_rate_limit_both_none_returns_defaults() {
        let (rate, burst) = parse_rate_limit_values(None, None);
        assert_eq!(rate, 5);
        assert_eq!(burst, 15);
    }

    #[test]
    fn parse_rate_limit_valid_values() {
        let (rate, burst) = parse_rate_limit_values(Some("10"), Some("30"));
        assert_eq!(rate, 10);
        assert_eq!(burst, 30);
    }

    #[test]
    fn parse_rate_limit_invalid_rate_returns_default() {
        let (rate, _) = parse_rate_limit_values(Some("not-a-number"), Some("30"));
        assert_eq!(rate, 5);
    }

    #[test]
    fn parse_rate_limit_invalid_burst_returns_default() {
        let (_, burst) = parse_rate_limit_values(Some("10"), Some("also-not"));
        assert_eq!(burst, 15);
    }

    #[test]
    fn parse_rate_limit_zero_rate_returns_default() {
        let (rate, _) = parse_rate_limit_values(Some("0"), Some("30"));
        assert_eq!(rate, 5);
    }

    #[test]
    fn parse_rate_limit_zero_burst_returns_default() {
        let (_, burst) = parse_rate_limit_values(Some("10"), Some("0"));
        assert_eq!(burst, 15);
    }

    #[test]
    fn parse_rate_limit_mixed_valid_invalid() {
        let (rate, burst) = parse_rate_limit_values(Some("8"), None);
        assert_eq!(rate, 8);
        assert_eq!(burst, 15);
    }

    // ── TCP keepalive tests ────────────────────────────────────────────

    #[test]
    fn configure_tcp_keepalive_sets_keepalive_on_socket() {
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let socket = socket2::Socket::new(
            socket2::Domain::for_address(addr),
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )
        .unwrap();

        let idle = std::time::Duration::from_secs(60);
        configure_tcp_keepalive(&socket, idle).unwrap();

        // Verify keepalive is enabled by checking the socket option
        assert!(
            socket.keepalive().unwrap(),
            "TCP keepalive should be enabled on the socket"
        );
    }

    #[test]
    fn configure_tcp_keepalive_with_zero_duration_still_sets_keepalive() {
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let socket = socket2::Socket::new(
            socket2::Domain::for_address(addr),
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )
        .unwrap();

        let idle = std::time::Duration::from_secs(0);
        configure_tcp_keepalive(&socket, idle).unwrap();

        // Even with 0 duration, keepalive should be set (OS will use minimum)
        assert!(socket.keepalive().unwrap());
    }

    // ── dynamic_dispatch_enabled tests ─────────────────────────────────

    #[test]
    fn dynamic_dispatch_enabled_returns_false_by_default() {
        let _guard = crate::test_support::EnvVarGuard::acquire_blocking(&[
            "INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED",
        ]);
        unsafe {
            std::env::remove_var("INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED");
        }
        assert!(!super::dynamic_dispatch_enabled());
    }

    #[test]
    fn dynamic_dispatch_enabled_true_value() {
        let _guard = crate::test_support::EnvVarGuard::acquire_blocking(&[
            "INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED",
        ]);
        unsafe {
            std::env::set_var("INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED", "1");
        }
        assert!(super::dynamic_dispatch_enabled());
    }

    #[test]
    fn dynamic_dispatch_enabled_true_string() {
        let _guard = crate::test_support::EnvVarGuard::acquire_blocking(&[
            "INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED",
        ]);
        unsafe {
            std::env::set_var("INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED", "true");
        }
        assert!(super::dynamic_dispatch_enabled());
    }

    #[test]
    fn dynamic_dispatch_enabled_true_uppercase() {
        let _guard = crate::test_support::EnvVarGuard::acquire_blocking(&[
            "INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED",
        ]);
        unsafe {
            std::env::set_var("INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED", "TRUE");
        }
        assert!(super::dynamic_dispatch_enabled());
    }

    #[test]
    fn dynamic_dispatch_enabled_other_value_returns_false() {
        let _guard = crate::test_support::EnvVarGuard::acquire_blocking(&[
            "INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED",
        ]);
        unsafe {
            std::env::set_var("INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED", "yes");
        }
        assert!(!super::dynamic_dispatch_enabled());
    }

    // ── build_mcp_rmcp_config tests ───────────────────────────────────

    #[test]
    fn build_mcp_rmcp_config_empty_uses_default() {
        let config = super::build_mcp_rmcp_config("");
        // Default config should work without panicking
        let _ = config;
    }

    #[test]
    fn build_mcp_rmcp_config_with_hosts() {
        let config = super::build_mcp_rmcp_config("example.com, api.example.com");
        let _ = config;
    }

    // ── all_intent_handlers tests ──────────────────────────────────────

    #[test]
    fn all_intent_handlers_returns_nine_handlers() {
        let handlers = super::all_intent_handlers();
        assert_eq!(handlers.len(), 9);
    }

    #[test]
    fn all_intent_handlers_have_distinct_names() {
        let handlers = super::all_intent_handlers();
        let names: Vec<String> = handlers.iter().map(|h| h.name().to_string()).collect();
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "handler names should be unique");
    }

    // ── IntervalsMcpHandler construction tests ─────────────────────────

    #[test]
    fn handler_new_returns_valid_handler() {
        use intervals_icu_client::IntervalsClient;
        use std::sync::Arc;

        let client: Arc<dyn IntervalsClient> =
            Arc::new(crate::test_support::mock::MockIntervalsClient::default());
        let handler = IntervalsMcpHandler::new(client);
        assert!(handler.tool_count() > 0);
    }

    #[test]
    fn handler_tool_count_matches_all_intent_handlers() {
        use intervals_icu_client::IntervalsClient;
        use std::sync::Arc;

        let client: Arc<dyn IntervalsClient> =
            Arc::new(crate::test_support::mock::MockIntervalsClient::default());
        let handler = IntervalsMcpHandler::new(client);
        assert_eq!(handler.tool_count(), 9);
    }

    #[test]
    fn handler_get_tool_returns_none() {
        use intervals_icu_client::IntervalsClient;
        use rmcp::ServerHandler;
        use std::sync::Arc;

        let client: Arc<dyn IntervalsClient> =
            Arc::new(crate::test_support::mock::MockIntervalsClient::default());
        let handler = IntervalsMcpHandler::new(client);
        assert!(handler.get_tool("any_name").is_none());
    }

    #[test]
    fn handler_get_info_returns_server_info() {
        use intervals_icu_client::IntervalsClient;
        use rmcp::ServerHandler;
        use std::sync::Arc;

        let client: Arc<dyn IntervalsClient> =
            Arc::new(crate::test_support::mock::MockIntervalsClient::default());
        let handler = IntervalsMcpHandler::new(client);
        let info = handler.get_info();
        let info_str = format!("{info:?}");
        assert!(
            !info_str.is_empty(),
            "get_info should return non-empty debug representation"
        );
    }

    #[tokio::test]
    async fn handler_preload_dynamic_registry_does_not_panic() {
        use intervals_icu_client::IntervalsClient;
        use std::sync::Arc;

        let client: Arc<dyn IntervalsClient> =
            Arc::new(crate::test_support::mock::MockIntervalsClient::default());
        let handler = IntervalsMcpHandler::new(client);
        let _count = handler.preload_dynamic_registry().await;
    }

    #[test]
    fn handler_new_multi_tenant_succeeds() {
        let result = IntervalsMcpHandler::new_multi_tenant();
        assert!(result.is_ok());
        let handler = result.unwrap();
        assert!(handler.tool_count() > 0);
    }

    #[test]
    fn handler_with_dynamic_runtime_dispatch_enabled_branch() {
        use intervals_icu_client::IntervalsClient;
        use std::sync::Arc;

        let _guard = crate::test_support::EnvVarGuard::acquire_blocking(&[
            "INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED",
        ]);
        unsafe {
            std::env::set_var("INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED", "1");
        }
        let client: Arc<dyn IntervalsClient> =
            Arc::new(crate::test_support::mock::MockIntervalsClient::default());
        let handler = IntervalsMcpHandler::with_dynamic_runtime(
            client,
            crate::dynamic::DynamicRuntime::from_env(),
        );
        assert!(handler.tool_count() > 0);
    }
}
