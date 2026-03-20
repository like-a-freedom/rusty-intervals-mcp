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
use tokio::sync::Mutex;

use crate::intents::handlers::{
    AnalyzeRaceHandler, AnalyzeTrainingHandler, AssessRecoveryHandler, ComparePeriodsHandler,
    ManageGearHandler, ManageProfileHandler, ModifyTrainingHandler, PlanTrainingHandler,
};
use crate::intents::{
    IdempotencyMiddleware, IntentRouter, intent_error_to_error_data,
    intent_output_to_call_tool_result,
};
use intervals_icu_client::IntervalsClient;

pub mod compact;
pub mod domains;
pub mod dynamic;
pub mod engines;
mod event_id;
pub mod intents;
pub mod prompts;
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
    pub fn new(client: Arc<dyn IntervalsClient>) -> Self {
        Self::with_dynamic_runtime(client, dynamic::DynamicRuntime::from_env())
    }

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
            client: client.clone(),
            dynamic_runtime,
            intent_router,
            webhooks: Arc::new(Mutex::new(HashMap::new())),
            webhook_secret: Arc::new(Mutex::new(None)),
        }
    }

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

    fn webhook_service(&self) -> services::WebhookService {
        services::WebhookService::new(self.webhooks.clone(), self.webhook_secret.clone())
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
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        // Route to intent handler by name
        let intent_name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();

        match self
            .intent_router
            .route(intent_name, serde_json::Value::Object(args))
            .await
        {
            Ok(output) => intent_output_to_call_tool_result(&output)
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

#[cfg(test)]
mod tests {
    use super::*;
    use intervals_icu_client::{AthleteProfile, Event, EventCategory, IntervalsError};
    use uuid::Uuid;

    fn test_handler() -> IntervalsMcpHandler {
        let runtime =
            dynamic::DynamicRuntime::new(dynamic::DynamicRuntimeConfig::builder().build());
        IntervalsMcpHandler::with_dynamic_runtime(Arc::new(MockClient), runtime)
    }

    fn mock_event(event_id: Option<&str>) -> Event {
        Event {
            id: event_id.map(str::to_owned),
            start_date_local: "2026-03-04".to_string(),
            name: "Mock event".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        }
    }

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
                start_date_local: "2026-03-04".to_string(),
            }])
        }

        async fn create_event(
            &self,
            event: intervals_icu_client::Event,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            Ok(event)
        }
        async fn get_event(
            &self,
            event_id: &str,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            Ok(mock_event(Some(event_id)))
        }
        async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> {
            Ok(())
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

        async fn update_wellness_bulk(
            &self,
            _entries: &[serde_json::Value],
        ) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn get_weather_config(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_weather_config(
            &self,
            _config: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn list_routes(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }

        async fn get_route(
            &self,
            _route_id: i64,
            _include_path: bool,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_route(
            &self,
            _route_id: i64,
            _route: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_route_similarity(
            &self,
            _route_id: i64,
            _other_id: i64,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }
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
        let handler = IntervalsMcpHandler::with_dynamic_runtime(Arc::new(MockClient), runtime);

        // Preload should load the registry
        let count = handler.preload_dynamic_registry().await;
        assert!(count > 0, "should load at least one tool from test spec");

        // Cleanup
        let _ = tokio::fs::remove_file(&tmp_file).await;
    }
}
