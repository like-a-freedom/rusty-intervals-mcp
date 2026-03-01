use std::collections::HashMap;
use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::model::{
    AnnotateAble, CallToolRequestParams, CallToolResult, GetPromptRequestParams, GetPromptResult,
    JsonObject, ListPromptsResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    Prompt, PromptArgument, ReadResourceRequestParams, ReadResourceResult, ResourceContents,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use tokio::sync::{Mutex, watch};

use intervals_icu_client::IntervalsClient;

pub mod compact;
pub mod domains;
pub mod dynamic;
mod event_id;
pub mod middleware;
pub mod prompts;
mod services;
mod state;
mod transforms;
pub mod types;

pub use event_id::{EventId, FolderId};
pub use middleware::LoggingMiddleware;
pub use state::{DownloadState, DownloadStatus, WebhookEvent};
pub use types::*;

#[derive(Clone)]
pub struct IntervalsMcpHandler {
    client: Arc<dyn IntervalsClient>,
    dynamic_runtime: dynamic::DynamicRuntime,
    downloads: Arc<Mutex<HashMap<String, DownloadStatus>>>,
    cancel_senders: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>>,
    webhook_secret: Arc<Mutex<Option<String>>>,
}

impl IntervalsMcpHandler {
    pub fn new(client: Arc<dyn IntervalsClient>) -> Self {
        Self {
            client,
            dynamic_runtime: dynamic::DynamicRuntime::from_env(),
            downloads: Arc::new(Mutex::new(HashMap::new())),
            cancel_senders: Arc::new(Mutex::new(HashMap::new())),
            webhooks: Arc::new(Mutex::new(HashMap::new())),
            webhook_secret: Arc::new(Mutex::new(None)),
        }
    }

    pub fn tool_count(&self) -> usize {
        self.dynamic_runtime.cached_tool_count() + dynamic::internal_tools().len()
    }

    pub fn prompt_count(&self) -> usize {
        7
    }

    pub async fn preload_dynamic_registry(&self) -> usize {
        match self.dynamic_runtime.ensure_registry().await {
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

    fn download_service(&self) -> services::DownloadService {
        services::DownloadService::new(self.downloads.clone(), self.cancel_senders.clone())
    }

    fn webhook_service(&self) -> services::WebhookService {
        services::WebhookService::new(self.webhooks.clone(), self.webhook_secret.clone())
    }

    fn compact_internal_result(args: &JsonObject, value: serde_json::Value) -> CallToolResult {
        let compact_enabled = args
            .get("compact")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let response_fields = args
            .get("fields")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            });

        let normalized = if compact_enabled {
            match &value {
                serde_json::Value::Object(_) => {
                    if let Some(fields) = response_fields.as_ref() {
                        crate::compact::filter_fields(&value, fields)
                    } else {
                        value
                    }
                }
                serde_json::Value::Array(_) => {
                    if let Some(fields) = response_fields.as_ref() {
                        crate::compact::filter_array_fields(&value, fields)
                    } else {
                        value
                    }
                }
                _ => value,
            }
        } else {
            value
        };

        CallToolResult::structured(normalized)
    }

    async fn call_internal_tool(
        &self,
        request: &CallToolRequestParams,
    ) -> Result<Option<CallToolResult>, ErrorData> {
        let args = request.arguments.clone().unwrap_or_default();

        match request.name.as_ref() {
            "set_webhook_secret" => {
                let secret = args
                    .get("secret")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ErrorData::invalid_params("missing required parameter: secret", None)
                    })?;
                self.webhook_service().set_secret(secret.to_string()).await;
                Ok(Some(Self::compact_internal_result(
                    &args,
                    serde_json::json!({"ok": true}),
                )))
            }
            "start_download" => {
                let activity_id = args
                    .get("activity_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ErrorData::invalid_params("missing required parameter: activity_id", None)
                    })?
                    .to_string();
                let output_path = args
                    .get("output_path")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned);
                let download_id = self
                    .download_service()
                    .start_download(self.client.clone(), activity_id, output_path)
                    .await;
                Ok(Some(Self::compact_internal_result(
                    &args,
                    serde_json::json!({"download_id": download_id}),
                )))
            }
            "get_download_status" => {
                let id = args
                    .get("download_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ErrorData::invalid_params("missing required parameter: download_id", None)
                    })?;
                let status = self
                    .download_service()
                    .get_status(id)
                    .await
                    .ok_or_else(|| ErrorData::invalid_params("download_id not found", None))?;
                Ok(Some(Self::compact_internal_result(
                    &args,
                    serde_json::to_value(status).map_err(|e| {
                        ErrorData::internal_error(format!("failed to serialize status: {e}"), None)
                    })?,
                )))
            }
            "cancel_download" => {
                let id = args
                    .get("download_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ErrorData::invalid_params("missing required parameter: download_id", None)
                    })?;
                let cancelled = self.download_service().cancel_download(id).await;
                Ok(Some(Self::compact_internal_result(
                    &args,
                    serde_json::json!({"cancelled": cancelled}),
                )))
            }
            "list_downloads" => {
                let downloads = self.download_service().list_downloads().await;
                Ok(Some(Self::compact_internal_result(
                    &args,
                    serde_json::to_value(downloads).map_err(|e| {
                        ErrorData::internal_error(
                            format!("failed to serialize downloads list: {e}"),
                            None,
                        )
                    })?,
                )))
            }
            "receive_webhook" => {
                let signature = args
                    .get("signature")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ErrorData::invalid_params("missing required parameter: signature", None)
                    })?;
                let payload = args.get("payload").cloned().ok_or_else(|| {
                    ErrorData::invalid_params("missing required parameter: payload", None)
                })?;
                let result = self
                    .webhook_service()
                    .process_webhook(signature, payload)
                    .await
                    .map_err(|e| {
                        ErrorData::internal_error(format!("webhook processing failed: {e}"), None)
                    })?;
                Ok(Some(Self::compact_internal_result(&args, result.value)))
            }
            _ => Ok(None),
        }
    }

    fn available_prompts() -> Vec<Prompt> {
        vec![
            Prompt::new(
                "analyze-recent-training",
                Some("Analyze recent training activities, load trends, and provide insights"),
                Some(vec![PromptArgument {
                    name: "days_back".to_string(),
                    title: Some("Days Back".to_string()),
                    description: Some("How many days to analyze".to_string()),
                    required: Some(false),
                }]),
            ),
            Prompt::new(
                "performance-analysis",
                Some("Analyze performance curves and training zones"),
                Some(vec![
                    PromptArgument {
                        name: "days_back".to_string(),
                        title: Some("Days Back".to_string()),
                        description: Some("How many days to analyze".to_string()),
                        required: Some(false),
                    },
                    PromptArgument {
                        name: "metric".to_string(),
                        title: Some("Metric".to_string()),
                        description: Some("Metric: power/hr/pace".to_string()),
                        required: Some(false),
                    },
                ]),
            ),
            Prompt::new(
                "activity-deep-dive",
                Some("Detailed analysis of a specific activity"),
                Some(vec![PromptArgument {
                    name: "activity_id".to_string(),
                    title: Some("Activity ID".to_string()),
                    description: Some("Intervals.icu activity id".to_string()),
                    required: Some(true),
                }]),
            ),
            Prompt::new(
                "recovery-check",
                Some("Assess recovery status and readiness to train"),
                Some(vec![PromptArgument {
                    name: "days_back".to_string(),
                    title: Some("Days Back".to_string()),
                    description: Some("How many days to analyze".to_string()),
                    required: Some(false),
                }]),
            ),
            Prompt::new(
                "training-plan-review",
                Some("Review planned workouts for the upcoming period"),
                Some(vec![PromptArgument {
                    name: "start_date".to_string(),
                    title: Some("Start Date".to_string()),
                    description: Some("ISO date YYYY-MM-DD".to_string()),
                    required: Some(false),
                }]),
            ),
            Prompt::new(
                "plan-training-week",
                Some("Create a training plan for the upcoming week"),
                Some(vec![
                    PromptArgument {
                        name: "start_date".to_string(),
                        title: Some("Start Date".to_string()),
                        description: Some("ISO date YYYY-MM-DD".to_string()),
                        required: Some(false),
                    },
                    PromptArgument {
                        name: "focus".to_string(),
                        title: Some("Focus".to_string()),
                        description: Some("Training focus".to_string()),
                        required: Some(false),
                    },
                ]),
            ),
            Prompt::new(
                "analyze-and-adapt-plan",
                Some("Analyze recent training and adapt current plan based on actual load"),
                Some(vec![
                    PromptArgument {
                        name: "period".to_string(),
                        title: Some("Period Label".to_string()),
                        description: Some("Human-friendly period label".to_string()),
                        required: Some(false),
                    },
                    PromptArgument {
                        name: "days_back".to_string(),
                        title: Some("Days Back".to_string()),
                        description: Some("How many days to analyze".to_string()),
                        required: Some(false),
                    },
                    PromptArgument {
                        name: "focus".to_string(),
                        title: Some("Focus".to_string()),
                        description: Some("Adaptation focus".to_string()),
                        required: Some(false),
                    },
                ]),
            ),
        ]
    }

    fn prompt_from_request(request: &GetPromptRequestParams) -> Result<GetPromptResult, ErrorData> {
        let args = request.arguments.clone().unwrap_or_default();
        let result = match request.name.as_str() {
            "analyze-recent-training" => {
                let days_back = args
                    .get("days_back")
                    .and_then(serde_json::Value::as_i64)
                    .map(|v| v.max(0) as u32)
                    .unwrap_or(30);
                prompts::analyze_recent_training_prompt(days_back)
            }
            "performance-analysis" => {
                let days_back = args
                    .get("days_back")
                    .and_then(serde_json::Value::as_i64)
                    .map(|v| v.max(0) as u32)
                    .unwrap_or(90);
                let metric = args
                    .get("metric")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| args.get("sport_type").and_then(serde_json::Value::as_str))
                    .unwrap_or("power");
                prompts::performance_analysis_prompt(metric, days_back)
            }
            "activity-deep-dive" => {
                let activity_id = args
                    .get("activity_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ErrorData::invalid_params("missing required argument: activity_id", None)
                    })?;
                prompts::activity_deep_dive_prompt(activity_id)
            }
            "recovery-check" => {
                let days_back = args
                    .get("days_back")
                    .and_then(serde_json::Value::as_i64)
                    .map(|v| v.max(0) as u32)
                    .unwrap_or(7);
                prompts::recovery_check_prompt(days_back)
            }
            "training-plan-review" => {
                let start_date = args
                    .get("start_date")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
                prompts::training_plan_review_prompt(&start_date)
            }
            "plan-training-week" => {
                let start_date = args
                    .get("start_date")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
                let focus = args
                    .get("focus")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("general fitness")
                    .to_string();
                prompts::plan_training_week_prompt(&start_date, &focus)
            }
            "analyze-and-adapt-plan" => {
                let days_back = args
                    .get("days_back")
                    .and_then(serde_json::Value::as_i64)
                    .map(|v| v.max(0) as i32)
                    .unwrap_or(30);
                let period = args
                    .get("period")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("the last {} days", days_back));
                let focus = args
                    .get("focus")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("balanced progression")
                    .to_string();
                prompts::analyze_and_adapt_plan_prompt(&period, &focus)
            }
            _ => {
                return Err(ErrorData::invalid_params(
                    format!("unknown prompt: {}", request.name),
                    None,
                ));
            }
        };

        Ok(result)
    }

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
}

impl ServerHandler for IntervalsMcpHandler {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            instructions: Some(
                "Intervals.icu MCP server with dynamic OpenAPI tool generation and compact-aware responses."
                    .into(),
            ),
            capabilities: rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
            ..Default::default()
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let registry: Result<Arc<dynamic::DynamicRegistry>, ErrorData> =
            self.dynamic_runtime.ensure_registry().await;
        let dynamic_tools = match registry {
            Ok(r) => r.list_tools(),
            Err(err) => {
                tracing::warn!("dynamic OpenAPI registry unavailable: {}", err.message);
                Vec::new()
            }
        };
        Ok(dynamic::merge_tools(
            dynamic_tools,
            dynamic::internal_tools(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Some(result) = self.call_internal_tool(&request).await? {
            return Ok(result);
        }
        let registry: Arc<dynamic::DynamicRegistry> =
            self.dynamic_runtime.ensure_registry().await?;
        let op = registry.operation(request.name.as_ref()).ok_or_else(|| {
            ErrorData::invalid_params(
                format!(
                    "unknown tool '{}': not found in dynamic OpenAPI registry",
                    request.name
                ),
                None,
            )
        })?;

        self.dynamic_runtime
            .dispatch_openapi(op, request.arguments.as_ref())
            .await
    }

    fn get_tool(&self, _name: &str) -> Option<rmcp::model::Tool> {
        None
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(ListPromptsResult {
            prompts: Self::available_prompts(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, ErrorData> {
        Self::prompt_from_request(&request)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let res = domains::resources::athlete_profile_resource().no_annotation();

        Ok(ListResourcesResult {
            resources: vec![res],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        if request.uri == "intervals-icu://athlete/profile" {
            let text = domains::resources::build_athlete_profile_text(&*self.client)
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            Ok(ReadResourceResult {
                contents: vec![ResourceContents::TextResourceContents {
                    uri: request.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                    meta: None,
                }],
            })
        } else {
            Err(ErrorData::invalid_params(
                format!("unknown resource URI: {}", request.uri),
                None,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use intervals_icu_client::{AthleteProfile, IntervalsError};

    struct MockClient;

    #[async_trait::async_trait]
    impl IntervalsClient for MockClient {
        async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
            Ok(AthleteProfile {
                id: "ath1".to_string(),
                name: Some("Tester".to_string()),
            })
        }

        async fn get_recent_activities(
            &self,
            _limit: Option<u32>,
            _days_back: Option<i32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
            Ok(vec![intervals_icu_client::ActivitySummary {
                id: "a1".to_string(),
                name: Some("Run".to_string()),
            }])
        }

        async fn create_event(
            &self,
            _event: intervals_icu_client::Event,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            unimplemented!()
        }
        async fn get_event(
            &self,
            _event_id: &str,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            unimplemented!()
        }
        async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> {
            unimplemented!()
        }
        async fn get_events(
            &self,
            _days_back: Option<i32>,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }
        async fn bulk_create_events(
            &self,
            _events: Vec<intervals_icu_client::Event>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }
        async fn get_activity_streams(
            &self,
            _activity_id: &str,
            _streams: Option<Vec<String>>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_activity_intervals(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_best_efforts(
            &self,
            _activity_id: &str,
            _options: Option<intervals_icu_client::BestEffortsOptions>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_activity_details(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn search_activities(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
            Ok(vec![])
        }
        async fn search_activities_full(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
            Ok("id,name\n1,Run".to_string())
        }
        async fn update_activity(
            &self,
            _activity_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn download_activity_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn download_activity_file_with_progress(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
            _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
            _cancel_rx: tokio::sync::watch::Receiver<bool>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn download_fit_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn download_gpx_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }
        async fn get_gear_list(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_sport_settings(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_power_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_gap_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn delete_activity(&self, _activity_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn get_activities_around(
            &self,
            _activity_id: &str,
            _limit: Option<u32>,
            _route_id: Option<i64>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn search_intervals(
            &self,
            _min_secs: u32,
            _max_secs: u32,
            _min_intensity: u32,
            _max_intensity: u32,
            _interval_type: Option<String>,
            _min_reps: Option<u32>,
            _max_reps: Option<u32>,
            _limit: Option<u32>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_power_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_hr_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_pace_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_wellness(
            &self,
            _days_back: Option<i32>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_wellness_for_date(
            &self,
            _date: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_wellness(
            &self,
            _date: &str,
            _data: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_upcoming_workouts(
            &self,
            _days_ahead: Option<u32>,
            _limit: Option<u32>,
            _category: Option<String>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn update_event(
            &self,
            _event_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn bulk_delete_events(&self, _event_ids: Vec<String>) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn duplicate_event(
            &self,
            _event_id: &str,
            _num_copies: Option<u32>,
            _weeks_between: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }
        async fn get_hr_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_pace_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_workout_library(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn get_workouts_in_folder(
            &self,
            _folder_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn create_folder(
            &self,
            _folder: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_folder(
            &self,
            _folder_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_folder(&self, _folder_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn create_gear(
            &self,
            _gear: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_gear(
            &self,
            _gear_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_gear(&self, _gear_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
        async fn create_gear_reminder(
            &self,
            _gear_id: &str,
            _reminder: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_gear_reminder(
            &self,
            _gear_id: &str,
            _reminder_id: &str,
            _reset: bool,
            _snooze_days: u32,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_sport_settings(
            &self,
            _sport_type: &str,
            _recalc_hr_zones: bool,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn apply_sport_settings(
            &self,
            _sport_type: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn create_sport_settings(
            &self,
            _settings: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn handler_registers_tools_and_prompts() {
        let handler = IntervalsMcpHandler::new(Arc::new(MockClient));
        assert!(handler.tool_count() > 0);
        assert_eq!(handler.prompt_count(), 7);
    }

    #[test]
    fn tool_count_matches_internal_tools_without_cache() {
        let handler = IntervalsMcpHandler::new(Arc::new(MockClient));
        assert_eq!(handler.tool_count(), dynamic::internal_tools().len());
    }

    #[test]
    fn compact_internal_result_filters_array_fields_when_enabled() {
        let mut args = JsonObject::new();
        args.insert("compact".to_string(), serde_json::Value::Bool(true));
        args.insert("fields".to_string(), serde_json::json!(["id", "status"]));

        let result = IntervalsMcpHandler::compact_internal_result(
            &args,
            serde_json::json!([
                {"id": "d1", "status": "running", "extra": 1},
                {"id": "d2", "status": "done", "extra": 2}
            ]),
        );

        let value = serde_json::to_value(result).expect("result should serialize");
        let content = &value["structuredContent"];

        assert_eq!(
            content[0]["id"],
            serde_json::Value::String("d1".to_string())
        );
        assert_eq!(
            content[0]["status"],
            serde_json::Value::String("running".to_string())
        );
        assert!(content[0].get("extra").is_none());
    }
}
