//! Runtime for dynamic OpenAPI tool registry with caching and refresh.

use crate::dynamic::parser::parse_openapi_spec;
use crate::dynamic::types::DynamicRegistry;
use rmcp::ErrorData;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

const OPENAPI_DEFAULT_PATH: &str = "/api/v1/docs";
const OPENAPI_FETCH_TIMEOUT_SECS: u64 = 10;

/// Configuration for the dynamic runtime.
#[derive(Clone, Debug)]
pub struct DynamicRuntimeConfig {
    pub base_url: String,
    pub athlete_id: String,
    pub api_key: String,
    pub spec_source: Option<String>,
    pub refresh_interval: Duration,
}

impl DynamicRuntimeConfig {
    /// Create configuration from environment variables.
    pub fn from_env() -> Self {
        let base_url = std::env::var("INTERVALS_ICU_BASE_URL")
            .unwrap_or_else(|_| "https://intervals.icu".to_string());
        let athlete_id = std::env::var("INTERVALS_ICU_ATHLETE_ID").unwrap_or_default();
        let api_key = std::env::var("INTERVALS_ICU_API_KEY").unwrap_or_default();
        let spec_source = std::env::var("INTERVALS_ICU_OPENAPI_SPEC").ok();
        let refresh_secs = std::env::var("INTERVALS_ICU_SPEC_REFRESH_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300);

        Self {
            base_url,
            athlete_id,
            api_key,
            spec_source,
            refresh_interval: Duration::from_secs(refresh_secs),
        }
    }

    /// Create a new configuration builder.
    pub fn builder() -> DynamicRuntimeConfigBuilder {
        DynamicRuntimeConfigBuilder::default()
    }
}

/// Builder for DynamicRuntimeConfig.
#[derive(Default)]
pub struct DynamicRuntimeConfigBuilder {
    base_url: Option<String>,
    athlete_id: Option<String>,
    api_key: Option<String>,
    spec_source: Option<String>,
    refresh_interval: Option<Duration>,
}

impl DynamicRuntimeConfigBuilder {
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn athlete_id(mut self, id: impl Into<String>) -> Self {
        self.athlete_id = Some(id.into());
        self
    }

    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn spec_source(mut self, source: impl Into<String>) -> Self {
        self.spec_source = Some(source.into());
        self
    }

    pub fn refresh_interval(mut self, interval: Duration) -> Self {
        self.refresh_interval = Some(interval);
        self
    }

    pub fn build(self) -> DynamicRuntimeConfig {
        DynamicRuntimeConfig {
            base_url: self
                .base_url
                .unwrap_or_else(|| "https://intervals.icu".to_string()),
            athlete_id: self.athlete_id.unwrap_or_default(),
            api_key: self.api_key.unwrap_or_default(),
            spec_source: self.spec_source,
            refresh_interval: self.refresh_interval.unwrap_or(Duration::from_secs(300)),
        }
    }
}

/// Runtime state for dynamic OpenAPI tool generation.
#[derive(Clone)]
pub struct DynamicRuntime {
    http_client: reqwest::Client,
    config: DynamicRuntimeConfig,
    last_refresh_attempt: Arc<Mutex<Option<Instant>>>,
    registry: Arc<RwLock<Option<Arc<DynamicRegistry>>>>,
    cached_tool_count: Arc<AtomicUsize>,
}

impl DynamicRuntime {
    /// Create a new dynamic runtime with the given configuration.
    pub fn new(config: DynamicRuntimeConfig) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            config,
            last_refresh_attempt: Arc::new(Mutex::new(None)),
            registry: Arc::new(RwLock::new(None)),
            cached_tool_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a new dynamic runtime from environment variables.
    pub fn from_env() -> Self {
        Self::new(DynamicRuntimeConfig::from_env())
    }

    /// Get the cached tool count.
    pub fn cached_tool_count(&self) -> usize {
        self.cached_tool_count.load(Ordering::Relaxed)
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    /// Get the athlete ID.
    pub fn athlete_id(&self) -> &str {
        &self.config.athlete_id
    }

    /// Get the API key.
    pub fn api_key(&self) -> &str {
        &self.config.api_key
    }

    /// Dispatch a dynamic operation.
    pub async fn dispatch_openapi(
        &self,
        operation: &crate::dynamic::types::DynamicOperation,
        arguments: Option<&rmcp::model::JsonObject>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        crate::dynamic::dispatch::dispatch_operation(
            &self.http_client,
            &self.config.base_url,
            &self.config.athlete_id,
            &self.config.api_key,
            operation,
            arguments,
        )
        .await
    }

    /// Ensure the registry is loaded, refreshing if necessary.
    pub async fn ensure_registry(&self) -> Result<Arc<DynamicRegistry>, ErrorData> {
        if let Some(existing) = self.registry.read().await.clone() {
            if !self.should_attempt_refresh().await {
                tracing::debug!(
                    refresh_interval_secs = self.config.refresh_interval.as_secs(),
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
        if self.config.refresh_interval.is_zero() {
            return true;
        }

        let mut guard = self.last_refresh_attempt.lock().await;
        let now = Instant::now();

        match *guard {
            Some(last) if now.duration_since(last) < self.config.refresh_interval => false,
            _ => {
                *guard = Some(now);
                true
            }
        }
    }

    async fn try_build_registry(&self) -> Result<Arc<DynamicRegistry>, String> {
        let spec = self.load_spec().await?;
        let parsed = parse_openapi_spec(&spec, &HashSet::new(), &HashSet::new())?;
        Ok(Arc::new(parsed))
    }

    async fn load_spec(&self) -> Result<serde_json::Value, String> {
        if let Some(source) = &self.config.spec_source {
            return load_spec_from_source(&self.http_client, source).await;
        }

        let remote = format!(
            "{}{}",
            self.config.base_url.trim_end_matches('/'),
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

async fn load_spec_from_source(
    http: &reqwest::Client,
    source: &str,
) -> Result<serde_json::Value, String> {
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
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("json decode error: {e}"))?;
        return Ok(value);
    }

    let content = tokio::fs::read_to_string(source)
        .await
        .map_err(|e| format!("read error: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("json parse error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // DynamicRuntimeConfig Tests
    // ========================================================================

    #[test]
    fn test_config_from_env_defaults() {
        // Clear env vars to get defaults
        unsafe {
            std::env::remove_var("INTERVALS_ICU_BASE_URL");
            std::env::remove_var("INTERVALS_ICU_ATHLETE_ID");
            std::env::remove_var("INTERVALS_ICU_API_KEY");
            std::env::remove_var("INTERVALS_ICU_OPENAPI_SPEC");
            std::env::remove_var("INTERVALS_ICU_SPEC_REFRESH_SECS");
        }

        let config = DynamicRuntimeConfig::from_env();
        assert_eq!(config.base_url, "https://intervals.icu");
        assert_eq!(config.athlete_id, "");
        assert_eq!(config.api_key, "");
        assert_eq!(config.refresh_interval, Duration::from_secs(300));
    }

    #[test]
    fn test_config_builder() {
        let config = DynamicRuntimeConfig::builder()
            .base_url("https://test.example.com")
            .athlete_id("12345")
            .api_key("test-key")
            .refresh_interval(Duration::from_secs(600))
            .build();

        assert_eq!(config.base_url, "https://test.example.com");
        assert_eq!(config.athlete_id, "12345");
        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.refresh_interval, Duration::from_secs(600));
    }

    #[test]
    fn test_config_builder_defaults() {
        let config = DynamicRuntimeConfig::builder().build();
        assert_eq!(config.base_url, "https://intervals.icu");
    }

    #[test]
    fn test_config_builder_fluent() {
        let config = DynamicRuntimeConfig::builder()
            .base_url("https://a.com")
            .athlete_id("1")
            .api_key("key")
            .spec_source("https://spec.com")
            .refresh_interval(Duration::from_secs(100))
            .build();

        assert_eq!(config.base_url, "https://a.com");
        assert_eq!(config.athlete_id, "1");
        assert_eq!(config.api_key, "key");
        assert_eq!(config.spec_source, Some("https://spec.com".to_string()));
        assert_eq!(config.refresh_interval, Duration::from_secs(100));
    }

    // ========================================================================
    // DynamicRuntime Tests
    // ========================================================================

    #[test]
    fn test_runtime_new() {
        let config = DynamicRuntimeConfig::builder()
            .base_url("https://test.com")
            .athlete_id("123")
            .api_key("key")
            .build();

        let runtime = DynamicRuntime::new(config);
        assert_eq!(runtime.base_url(), "https://test.com");
        assert_eq!(runtime.athlete_id(), "123");
        assert_eq!(runtime.api_key(), "key");
        assert_eq!(runtime.cached_tool_count(), 0);
    }

    #[test]
    fn test_runtime_from_env() {
        // Clear env vars first
        unsafe {
            std::env::remove_var("INTERVALS_ICU_BASE_URL");
            std::env::remove_var("INTERVALS_ICU_ATHLETE_ID");
            std::env::remove_var("INTERVALS_ICU_API_KEY");
        }

        let runtime = DynamicRuntime::from_env();
        assert_eq!(runtime.base_url(), "https://intervals.icu");
    }

    #[test]
    fn test_runtime_cached_tool_count() {
        use std::sync::atomic::Ordering;

        let config = DynamicRuntimeConfig::builder().build();
        let runtime = DynamicRuntime::new(config);

        assert_eq!(runtime.cached_tool_count(), 0);

        // Manually update count for testing
        runtime.cached_tool_count.store(5, Ordering::Relaxed);
        assert_eq!(runtime.cached_tool_count(), 5);
    }

    #[tokio::test]
    async fn test_runtime_ensure_registry_empty() {
        // A missing explicit spec source should fail without using remote fallback logic.
        let config = DynamicRuntimeConfig::builder()
            .spec_source("/nonexistent/path/spec.json")
            .build();

        let runtime = DynamicRuntime::new(config);

        let result = runtime.ensure_registry().await;
        let error = result.expect_err("missing explicit spec source should fail");
        assert!(
            error.message.contains("failed to build OpenAPI registry"),
            "unexpected error message: {}",
            error.message
        );
        assert!(
            error.message.contains("read error"),
            "unexpected error message: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn test_runtime_try_build_registry() {
        let spec_source = default_local_spec_path();
        let config = DynamicRuntimeConfig::builder()
            .spec_source(spec_source.to_string_lossy())
            .build();

        let runtime = DynamicRuntime::new(config);

        let result = runtime.try_build_registry().await;
        let registry = result.expect("local checked-in spec should build a registry");
        assert!(
            !registry.is_empty(),
            "registry should contain dynamic operations"
        );
    }

    // ========================================================================
    // Helper Function Tests
    // ========================================================================

    #[test]
    fn test_default_local_spec_path() {
        let path = default_local_spec_path();

        // Should end with docs/intervals_icu_api.json
        assert!(path.ends_with("docs/intervals_icu_api.json"));
    }

    #[test]
    fn test_default_local_spec_path_structure() {
        let path = default_local_spec_path();

        // Should contain the workspace structure
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("intervals_icu_api.json"));
    }

    #[tokio::test]
    async fn test_load_spec_from_source_invalid_url() {
        let http = reqwest::Client::new();

        // Invalid URL should fail
        let result = load_spec_from_source(&http, "https://invalid.nonexistent.domain.test").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_load_spec_from_source_file_not_found() {
        let http = reqwest::Client::new();

        // File that doesn't exist
        let result = load_spec_from_source(&http, "/nonexistent/path/spec.json").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("read error"));
    }

    #[tokio::test]
    async fn test_load_spec_from_source_local_default_spec() {
        let http = reqwest::Client::new();
        let spec_source = default_local_spec_path();

        let result = load_spec_from_source(&http, &spec_source.to_string_lossy()).await;
        let spec = result.expect("checked-in local OpenAPI spec should load");

        let openapi_version = spec
            .get("openapi")
            .and_then(serde_json::Value::as_str)
            .expect("spec should declare an OpenAPI version");
        assert!(
            openapi_version.starts_with("3.0."),
            "unexpected OpenAPI version: {openapi_version}"
        );
        assert!(
            spec.get("paths")
                .and_then(serde_json::Value::as_object)
                .map(|paths| !paths.is_empty())
                .unwrap_or(false),
            "spec should contain at least one path"
        );
    }

    #[test]
    fn test_config_refresh_interval_zero() {
        let config = DynamicRuntimeConfig::builder()
            .refresh_interval(Duration::from_secs(0))
            .build();

        assert_eq!(config.refresh_interval, Duration::from_secs(0));
    }

    #[test]
    fn test_config_with_spec_source() {
        let config = DynamicRuntimeConfig::builder()
            .spec_source("https://example.com/spec.json")
            .build();

        assert_eq!(
            config.spec_source,
            Some("https://example.com/spec.json".to_string())
        );
    }

    #[test]
    fn test_config_clone() {
        let config = DynamicRuntimeConfig::builder()
            .base_url("https://test.com")
            .athlete_id("123")
            .api_key("key")
            .build();

        let cloned = config.clone();
        assert_eq!(cloned.base_url, config.base_url);
        assert_eq!(cloned.athlete_id, config.athlete_id);
        assert_eq!(cloned.api_key, config.api_key);
    }

    #[test]
    fn test_runtime_clone() {
        let config = DynamicRuntimeConfig::builder()
            .base_url("https://test.com")
            .build();

        let runtime = DynamicRuntime::new(config);
        let cloned = runtime.clone();

        assert_eq!(cloned.base_url(), runtime.base_url());
        assert_eq!(cloned.athlete_id(), runtime.athlete_id());
    }

    #[tokio::test]
    async fn test_runtime_registry_initial_state() {
        let config = DynamicRuntimeConfig::builder().build();
        let runtime = DynamicRuntime::new(config);

        // Initial state should have no registry
        let registry_guard = runtime.registry.read().await;
        assert!(registry_guard.is_none());
    }

    #[tokio::test]
    async fn test_runtime_last_refresh_initial_state() {
        let config = DynamicRuntimeConfig::builder().build();
        let runtime = DynamicRuntime::new(config);

        // Initial state should have no last refresh
        let refresh_guard = runtime.last_refresh_attempt.lock().await;
        assert!(refresh_guard.is_none());
    }

    #[test]
    fn test_constants() {
        assert_eq!(OPENAPI_DEFAULT_PATH, "/api/v1/docs");
        assert_eq!(OPENAPI_FETCH_TIMEOUT_SECS, 10);
    }
}
