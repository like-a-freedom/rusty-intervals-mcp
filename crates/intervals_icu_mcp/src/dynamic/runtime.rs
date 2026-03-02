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
    pub include_tags: HashSet<String>,
    pub exclude_tags: HashSet<String>,
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
        let include_tags_raw = std::env::var("INTERVALS_INCLUDE_TAGS").ok();
        let exclude_tags_raw = std::env::var("INTERVALS_EXCLUDE_TAGS").ok();
        let refresh_secs = std::env::var("INTERVALS_ICU_SPEC_REFRESH_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300);

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
            base_url,
            athlete_id,
            api_key,
            spec_source,
            include_tags,
            exclude_tags,
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
    include_tags: HashSet<String>,
    exclude_tags: HashSet<String>,
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

    pub fn include_tag(mut self, tag: impl Into<String>) -> Self {
        self.include_tags.insert(tag.into());
        self
    }

    pub fn exclude_tag(mut self, tag: impl Into<String>) -> Self {
        self.exclude_tags.insert(tag.into());
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
            include_tags: self.include_tags,
            exclude_tags: self.exclude_tags,
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
        let use_exclude = self.config.include_tags.is_empty();
        let empty_set = HashSet::new();
        let parsed = parse_openapi_spec(
            &spec,
            &self.config.include_tags,
            if use_exclude {
                &self.config.exclude_tags
            } else {
                &empty_set
            },
        )?;
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

fn parse_tag_set(input: Option<&str>) -> HashSet<String> {
    input
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
