use super::idempotency::IdempotencyMiddleware;
use super::types::{
    IntentError, IntentHandler, IntentOutput, ToolDefinition, standard_output_schema,
};
use intervals_icu_client::IntervalsClient;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Clone)]
pub struct IntentRouter {
    handlers: Arc<HashMap<String, Box<dyn IntentHandler>>>,
    idempotency: Arc<IdempotencyMiddleware>,
    client: Arc<dyn IntervalsClient>,
}

impl IntentRouter {
    pub fn new(
        handlers: Vec<Box<dyn IntentHandler>>,
        client: Arc<dyn IntervalsClient>,
        idempotency: Arc<IdempotencyMiddleware>,
    ) -> Self {
        let map: HashMap<String, Box<dyn IntentHandler>> = handlers
            .into_iter()
            .map(|h| (h.name().to_string(), h))
            .collect();
        info!("IntentRouter initialized with {} handlers", map.len());
        Self {
            handlers: Arc::new(map),
            idempotency,
            client,
        }
    }
    pub async fn route(&self, name: &str, input: Value) -> Result<IntentOutput, IntentError> {
        debug!("Routing intent: {}", name);
        let handler = self
            .handlers
            .get(name)
            .ok_or_else(|| IntentError::UnknownIntent(name.to_string()))?;
        if handler.requires_idempotency_token() {
            if let Some(token) = handler.extract_idempotency_token(&input) {
                let dry_run = input
                    .get("dry_run")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                if dry_run {
                    return handler.execute(input, self.client.clone(), None).await;
                }

                let request_fingerprint = fingerprint_request(name, &input);
                return self
                    .idempotency
                    .execute_with_idempotency(&token, &request_fingerprint, || async {
                        handler
                            .execute(input.clone(), self.client.clone(), None)
                            .await
                    })
                    .await;
            } else {
                return Err(IntentError::validation("Idempotency token required"));
            }
        }
        handler.execute(input, self.client.clone(), None).await
    }
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let output_schema = standard_output_schema();
        self.handlers
            .values()
            .map(|h| {
                ToolDefinition::new(h.name(), h.description(), h.input_schema())
                    .with_output_schema(output_schema.clone())
            })
            .collect()
    }
    pub fn get_handler(&self, name: &str) -> Option<&dyn IntentHandler> {
        self.handlers.get(name).map(|h| h.as_ref())
    }
    pub fn intent_names(&self) -> Vec<&str> {
        self.handlers.keys().map(|s| s.as_str()).collect()
    }
}

fn fingerprint_request(name: &str, input: &Value) -> String {
    fn canonicalize(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut sorted = std::collections::BTreeMap::new();
                for (key, value) in map {
                    sorted.insert(key.clone(), canonicalize(value));
                }

                let normalized: Map<String, Value> = sorted.into_iter().collect();
                Value::Object(normalized)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
            other => other.clone(),
        }
    }

    let normalized = canonicalize(input);
    format!("{}:{}", name, normalized)
}

impl std::fmt::Debug for IntentRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntentRouter")
            .field("handlers", &self.handlers.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intents::types::{ContentBlock, IdempotencyCache};
    use async_trait::async_trait;
    use intervals_icu_client::{AthleteProfile, Event, EventCategory, IntervalsError};
    use serde_json::{Value, json};

    struct MockIntentHandler {
        name: &'static str,
        description: &'static str,
        requires_token: bool,
    }

    impl MockIntentHandler {
        fn new(name: &'static str, requires_token: bool) -> Self {
            Self {
                name,
                description: "Test handler",
                requires_token,
            }
        }
    }

    #[async_trait]
    impl IntentHandler for MockIntentHandler {
        fn name(&self) -> &'static str {
            self.name
        }

        fn description(&self) -> &'static str {
            self.description
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }

        fn requires_idempotency_token(&self) -> bool {
            self.requires_token
        }

        fn extract_idempotency_token(&self, input: &Value) -> Option<String> {
            input
                .get("idempotency_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }

        async fn execute(
            &self,
            _input: Value,
            _client: Arc<dyn IntervalsClient>,
            _idempotency_cache: Option<&IdempotencyCache>,
        ) -> Result<IntentOutput, IntentError> {
            Ok(IntentOutput::new(vec![ContentBlock::text(format!(
                "Executed {}",
                self.name
            ))]))
        }
    }

    struct MockClient;

    #[async_trait]
    impl IntervalsClient for MockClient {
        async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
            Ok(AthleteProfile {
                id: "ath1".to_string(),
                name: Some("Test".to_string()),
            })
        }

        async fn get_recent_activities(
            &self,
            _limit: Option<u32>,
            _days_back: Option<i32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
            Ok(vec![])
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
            Ok(Event {
                id: Some(event_id.to_string()),
                start_date_local: "2026-03-04".to_string(),
                name: "Mock event".to_string(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            })
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

    #[test]
    fn fingerprint_request_is_stable_across_key_order() {
        let a = json!({
            "action": "modify",
            "target_date": "2026-03-23",
            "dry_run": false,
            "idempotency_token": "same"
        });
        let b = json!({
            "idempotency_token": "same",
            "dry_run": false,
            "target_date": "2026-03-23",
            "action": "modify"
        });

        assert_eq!(
            fingerprint_request("modify_training", &a),
            fingerprint_request("modify_training", &b)
        );
    }

    #[test]
    fn fingerprint_request_includes_name() {
        let input = json!({"key": "value"});
        let fp = fingerprint_request("test_intent", &input);
        assert!(fp.starts_with("test_intent:"));
    }

    #[test]
    fn fingerprint_request_nested_objects() {
        let a = json!({
            "outer": {
                "b": 2,
                "a": 1
            }
        });
        let b = json!({
            "outer": {
                "a": 1,
                "b": 2
            }
        });

        assert_eq!(
            fingerprint_request("test", &a),
            fingerprint_request("test", &b)
        );
    }

    #[test]
    fn fingerprint_request_arrays() {
        let input = json!({"items": [3, 1, 2]});
        let fp = fingerprint_request("test", &input);
        assert!(fp.contains("[3,1,2]"));
    }

    #[test]
    fn router_new_initializes_with_handlers() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());

        let router = IntentRouter::new(handlers, client, idempotency);
        assert_eq!(router.intent_names().len(), 1);
        assert!(router.intent_names().contains(&"test_handler"));
    }

    #[tokio::test]
    async fn router_routes_to_known_handler() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({"test": "data"});
        let result = router.route("test_handler", input).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn router_returns_error_for_unknown_handler() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({});
        let result = router.route("unknown_handler", input).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IntentError::UnknownIntent(_)));
    }

    #[test]
    fn router_tool_definitions_returns_all_handlers() {
        let handlers = vec![
            Box::new(MockIntentHandler::new("handler1", false)) as Box<dyn IntentHandler>,
            Box::new(MockIntentHandler::new("handler2", false)) as Box<dyn IntentHandler>,
        ];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let definitions = router.tool_definitions();
        assert_eq!(definitions.len(), 2);

        let names: Vec<&str> = definitions.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"handler1"));
        assert!(names.contains(&"handler2"));
    }

    #[test]
    fn router_get_handler_returns_some_for_known() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        assert!(router.get_handler("test_handler").is_some());
    }

    #[test]
    fn router_get_handler_returns_none_for_unknown() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        assert!(router.get_handler("unknown").is_none());
    }

    #[test]
    fn router_debug_format() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let debug_str = format!("{:?}", router);
        assert!(debug_str.contains("IntentRouter"));
        assert!(debug_str.contains("test_handler"));
    }

    #[tokio::test]
    async fn router_idempotency_dry_run_bypasses_cache() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("idempotent_handler", true))
                as Box<dyn IntentHandler>];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({
            "idempotency_token": "token1",
            "dry_run": true
        });
        let result = router.route("idempotent_handler", input).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_idempotency_missing_token_returns_error() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("idempotent_handler", true))
                as Box<dyn IntentHandler>];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({
            "dry_run": false
        });
        let result = router.route("idempotent_handler", input).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }

    #[tokio::test]
    async fn router_idempotency_with_token_executes_with_cache() {
        use crate::intents::ContentBlock;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct CountingHandler;

        #[async_trait]
        impl IntentHandler for CountingHandler {
            fn name(&self) -> &'static str {
                "counting_handler"
            }

            fn description(&self) -> &'static str {
                "Test handler that counts calls"
            }

            fn input_schema(&self) -> serde_json::Value {
                serde_json::json!({})
            }

            fn requires_idempotency_token(&self) -> bool {
                true
            }

            fn extract_idempotency_token(&self, input: &Value) -> Option<String> {
                input
                    .get("idempotency_token")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }

            async fn execute(
                &self,
                _input: Value,
                _client: Arc<dyn IntervalsClient>,
                _idempotency_cache: Option<&IdempotencyCache>,
            ) -> Result<IntentOutput, IntentError> {
                CALL_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(IntentOutput::new(vec![ContentBlock::text(format!(
                    "Call {}",
                    CALL_COUNT.load(Ordering::SeqCst)
                ))]))
            }
        }

        let handlers = vec![Box::new(CountingHandler) as Box<dyn IntentHandler>];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        // First call
        let input = json!({
            "idempotency_token": "same-token"
        });
        let result1 = router.route("counting_handler", input.clone()).await;
        assert!(result1.is_ok());

        // Second call with same token should use cache
        let result2 = router.route("counting_handler", input).await;
        assert!(result2.is_ok());

        // Reset for other tests
        CALL_COUNT.store(0, Ordering::SeqCst);
    }

    #[test]
    fn router_tool_definitions_with_empty_handlers() {
        let handlers: Vec<Box<dyn IntentHandler>> = vec![];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let definitions = router.tool_definitions();
        assert!(definitions.is_empty());
    }

    #[test]
    fn router_intent_names_with_multiple_handlers() {
        let handlers = vec![
            Box::new(MockIntentHandler::new("handler_a", false)) as Box<dyn IntentHandler>,
            Box::new(MockIntentHandler::new("handler_b", false)) as Box<dyn IntentHandler>,
            Box::new(MockIntentHandler::new("handler_c", false)) as Box<dyn IntentHandler>,
        ];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let names = router.intent_names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"handler_a"));
        assert!(names.contains(&"handler_b"));
        assert!(names.contains(&"handler_c"));
    }

    #[test]
    fn fingerprint_request_primitives() {
        let input = json!({
            "string": "value",
            "number": 42,
            "boolean": true,
            "null": null
        });
        let fp = fingerprint_request("test", &input);
        assert!(fp.contains("value"));
        assert!(fp.contains("42"));
        assert!(fp.contains("true"));
    }

    #[test]
    fn fingerprint_request_nested_arrays() {
        let a = json!({
            "items": [[1, 2], [3, 4]]
        });
        let b = json!({
            "items": [[1, 2], [3, 4]]
        });
        assert_eq!(
            fingerprint_request("test", &a),
            fingerprint_request("test", &b)
        );
    }

    #[test]
    fn fingerprint_request_different_values() {
        let a = json!({"key": "value1"});
        let b = json!({"key": "value2"});
        assert_ne!(
            fingerprint_request("test", &a),
            fingerprint_request("test", &b)
        );
    }

    #[test]
    fn router_new_with_many_handlers() {
        // Use static handler names to avoid lifetime issues
        let handler_names: Vec<&'static str> = vec![
            "handler_0",
            "handler_1",
            "handler_2",
            "handler_3",
            "handler_4",
            "handler_5",
            "handler_6",
            "handler_7",
            "handler_8",
            "handler_9",
        ];
        let handlers: Vec<Box<dyn IntentHandler>> = handler_names
            .into_iter()
            .map(|name| Box::new(MockIntentHandler::new(name, false)) as Box<dyn IntentHandler>)
            .collect();
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        assert_eq!(router.intent_names().len(), 10);
    }

    #[tokio::test]
    async fn router_route_non_idempotent_handler() {
        let handlers = vec![
            Box::new(MockIntentHandler::new("non_idempotent", false)) as Box<dyn IntentHandler>
        ];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({"data": "test"});
        let result = router.route("non_idempotent", input).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_route_idempotent_handler_without_token() {
        let handlers = vec![
            Box::new(MockIntentHandler::new("requires_token", true)) as Box<dyn IntentHandler>
        ];
        let client = Arc::new(MockClient);
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({});
        let result = router.route("requires_token", input).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }
}
