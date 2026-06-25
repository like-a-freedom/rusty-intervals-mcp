use super::idempotency::IdempotencyMiddleware;
use super::types::{
    IntentError, IntentHandler, IntentOutput, ToolDefinition, standard_output_schema,
};
use crate::metrics;
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
    pub async fn route(
        &self,
        name: &str,
        input: Value,
        athlete_id: Option<&str>,
    ) -> Result<IntentOutput, IntentError> {
        let start = std::time::Instant::now();
        info!(
            tool = name,
            athlete_id = athlete_id.unwrap_or("unknown"),
            "Tool call started"
        );

        debug!("Routing intent: {}", name);
        let handler = self
            .handlers
            .get(name)
            .ok_or_else(|| IntentError::UnknownIntent(name.to_string()))?;

        let result = if handler.requires_idempotency_token() {
            if let Some(token) = handler.extract_idempotency_token(&input) {
                let dry_run = input
                    .get("dry_run")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                if dry_run {
                    handler.execute(input, self.client.clone(), None).await
                } else {
                    let request_fingerprint = fingerprint_request(name, &input);
                    self.idempotency
                        .execute_with_idempotency(&token, &request_fingerprint, || async {
                            handler
                                .execute(input.clone(), self.client.clone(), None)
                                .await
                        })
                        .await
                }
            } else {
                Err(IntentError::validation("Idempotency token required"))
            }
        } else {
            handler.execute(input, self.client.clone(), None).await
        };

        let duration = start.elapsed().as_secs_f64();
        let success = result.is_ok();

        // Record metrics
        metrics::record_tool_call(name, success, duration);

        // Track athlete activity for observability (no high-cardinality labels)
        if let Some(aid) = athlete_id {
            metrics::record_athlete_activity(aid);
        }

        info!(
            tool = name,
            athlete_id = athlete_id.unwrap_or("unknown"),
            duration_secs = duration,
            "Tool call completed"
        );

        result
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
    use serde_json::{Value, json};
    use std::sync::Once;

    fn setup_tracing() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_new("info")
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .try_init();
        });
    }

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

    use crate::test_support::mock::MockIntervalsClient;

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
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());

        let router = IntentRouter::new(handlers, client, idempotency);
        assert_eq!(router.intent_names().len(), 1);
        assert!(router.intent_names().contains(&"test_handler"));
    }

    #[tokio::test]
    async fn router_routes_to_known_handler() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({"test": "data"});
        let result = router.route("test_handler", input, None).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn router_returns_error_for_unknown_handler() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({});
        let result = router.route("unknown_handler", input, None).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IntentError::UnknownIntent(_)));
    }

    #[test]
    fn router_tool_definitions_returns_all_handlers() {
        let handlers = vec![
            Box::new(MockIntentHandler::new("handler1", false)) as Box<dyn IntentHandler>,
            Box::new(MockIntentHandler::new("handler2", false)) as Box<dyn IntentHandler>,
        ];
        let client = Arc::new(MockIntervalsClient::default());
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
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        assert!(router.get_handler("test_handler").is_some());
    }

    #[test]
    fn router_get_handler_returns_none_for_unknown() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        assert!(router.get_handler("unknown").is_none());
    }

    #[test]
    fn router_debug_format() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
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
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({
            "idempotency_token": "token1",
            "dry_run": true
        });
        let result = router.route("idempotent_handler", input, None).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_idempotency_missing_token_returns_error() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("idempotent_handler", true))
                as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({
            "dry_run": false
        });
        let result = router.route("idempotent_handler", input, None).await;

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
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        // First call
        let input = json!({
            "idempotency_token": "same-token"
        });
        let result1 = router.route("counting_handler", input.clone(), None).await;
        assert!(result1.is_ok());

        // Second call with same token should use cache
        let result2 = router.route("counting_handler", input, None).await;
        assert!(result2.is_ok());

        // Reset for other tests
        CALL_COUNT.store(0, Ordering::SeqCst);
    }

    #[test]
    fn router_tool_definitions_with_empty_handlers() {
        let handlers: Vec<Box<dyn IntentHandler>> = vec![];
        let client = Arc::new(MockIntervalsClient::default());
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
        let client = Arc::new(MockIntervalsClient::default());
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
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        assert_eq!(router.intent_names().len(), 10);
    }

    #[tokio::test]
    async fn router_route_non_idempotent_handler() {
        let handlers = vec![
            Box::new(MockIntentHandler::new("non_idempotent", false)) as Box<dyn IntentHandler>
        ];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({"data": "test"});
        let result = router.route("non_idempotent", input, None).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_route_idempotent_handler_without_token() {
        let handlers = vec![
            Box::new(MockIntentHandler::new("requires_token", true)) as Box<dyn IntentHandler>
        ];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({});
        let result = router.route("requires_token", input, None).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }

    // ------------------------------------------------------------------
    // route() — error propagation
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn router_route_execution_error_non_idempotent() {
        struct FailingHandler;

        #[async_trait]
        impl IntentHandler for FailingHandler {
            fn name(&self) -> &'static str {
                "failing_handler"
            }
            fn description(&self) -> &'static str {
                "Always fails"
            }
            fn input_schema(&self) -> serde_json::Value {
                json!({})
            }
            fn requires_idempotency_token(&self) -> bool {
                false
            }
            async fn execute(
                &self,
                _input: Value,
                _client: Arc<dyn IntervalsClient>,
                _idempotency_cache: Option<&IdempotencyCache>,
            ) -> Result<IntentOutput, IntentError> {
                Err(IntentError::internal("Simulated failure"))
            }
        }

        let handlers = vec![Box::new(FailingHandler) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let result = router.route("failing_handler", json!({}), None).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IntentError::InternalError(_)));
    }

    #[tokio::test]
    async fn router_route_idempotent_execution_error_inside_closure() {
        struct FailingIdempotentHandler;

        #[async_trait]
        impl IntentHandler for FailingIdempotentHandler {
            fn name(&self) -> &'static str {
                "failing_idempotent"
            }
            fn description(&self) -> &'static str {
                "Fails inside idempotency closure"
            }
            fn input_schema(&self) -> serde_json::Value {
                json!({})
            }
            fn requires_idempotency_token(&self) -> bool {
                true
            }
            fn extract_idempotency_token(&self, input: &Value) -> Option<String> {
                input
                    .get("idempotency_token")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
            }
            async fn execute(
                &self,
                _input: Value,
                _client: Arc<dyn IntervalsClient>,
                _idempotency_cache: Option<&IdempotencyCache>,
            ) -> Result<IntentOutput, IntentError> {
                Err(IntentError::api("API failure inside closure"))
            }
        }

        let handlers = vec![Box::new(FailingIdempotentHandler) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({"idempotency_token": "fail-token"});
        let result = router.route("failing_idempotent", input, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ApiClientError(_)
        ));
    }

    #[tokio::test]
    async fn router_route_dry_run_execution_error() {
        struct FailingDryRunHandler;

        #[async_trait]
        impl IntentHandler for FailingDryRunHandler {
            fn name(&self) -> &'static str {
                "failing_dry_run"
            }
            fn description(&self) -> &'static str {
                "Fails on dry run execution"
            }
            fn input_schema(&self) -> serde_json::Value {
                json!({})
            }
            fn requires_idempotency_token(&self) -> bool {
                true
            }
            fn extract_idempotency_token(&self, input: &Value) -> Option<String> {
                input
                    .get("idempotency_token")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
            }
            async fn execute(
                &self,
                _input: Value,
                _client: Arc<dyn IntervalsClient>,
                _idempotency_cache: Option<&IdempotencyCache>,
            ) -> Result<IntentOutput, IntentError> {
                Err(IntentError::internal("Dry run failure"))
            }
        }

        let handlers = vec![Box::new(FailingDryRunHandler) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({
            "idempotency_token": "dry-run-token",
            "dry_run": true
        });
        let result = router.route("failing_dry_run", input, None).await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // route() — athlete_id = Some(...) branch
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn router_route_with_athlete_id_non_idempotent() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("test_handler", false)) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let result = router
            .route("test_handler", json!({}), Some("athlete_42"))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_route_with_athlete_id_idempotent_dry_run() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("idempotent_handler", true))
                as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({
            "idempotency_token": "athlete-token",
            "dry_run": true
        });
        let result = router
            .route("idempotent_handler", input, Some("athlete_99"))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_route_with_athlete_id_idempotent_execute() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("idempotent_handler", true))
                as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({"idempotency_token": "athlete-exec-token"});
        let result = router
            .route("idempotent_handler", input, Some("athlete_77"))
            .await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // route() — idempotency fingerprint conflict through router
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn router_idempotency_conflict_different_fingerprint() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("idempotent_handler", true))
                as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input1 = json!({
            "idempotency_token": "conflict-token",
            "data": "first"
        });
        let result1 = router.route("idempotent_handler", input1, None).await;
        assert!(result1.is_ok());

        let input2 = json!({
            "idempotency_token": "conflict-token",
            "data": "second"
        });
        let result2 = router.route("idempotent_handler", input2, None).await;
        assert!(result2.is_err());
        assert!(matches!(
            result2.unwrap_err(),
            IntentError::IdempotencyConflict(_)
        ));
    }

    // ------------------------------------------------------------------
    // route() — dry_run edge cases (non-bool, null, explicit false)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn router_route_dry_run_false_explicit() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("idempotent_handler", true))
                as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({
            "idempotency_token": "explicit-false",
            "dry_run": false
        });
        let result = router.route("idempotent_handler", input, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_route_dry_run_null() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("idempotent_handler", true))
                as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({
            "idempotency_token": "null-dry-run",
            "dry_run": null
        });
        let result = router.route("idempotent_handler", input, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_route_dry_run_non_bool() {
        let handlers =
            vec![Box::new(MockIntentHandler::new("idempotent_handler", true))
                as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input = json!({
            "idempotency_token": "string-dry-run",
            "dry_run": "yes"
        });
        let result = router.route("idempotent_handler", input, None).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // fingerprint_request edge cases
    // ------------------------------------------------------------------

    #[test]
    fn fingerprint_request_empty_object() {
        let fp = fingerprint_request("test", &json!({}));
        assert_eq!(fp, "test:{}");
    }

    #[test]
    fn fingerprint_request_empty_array() {
        let fp = fingerprint_request("test", &json!({"items": []}));
        assert!(fp.contains("[]"));
    }

    #[test]
    fn fingerprint_request_deep_nesting() {
        let a = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "value": 42
                    }
                }
            }
        });
        let b = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "value": 42
                    }
                }
            }
        });
        assert_eq!(
            fingerprint_request("test", &a),
            fingerprint_request("test", &b)
        );
    }

    #[test]
    fn fingerprint_request_different_deep_values() {
        let a = json!({
            "level1": {
                "level2": {
                    "value": "a"
                }
            }
        });
        let b = json!({
            "level1": {
                "level2": {
                    "value": "b"
                }
            }
        });
        assert_ne!(
            fingerprint_request("test", &a),
            fingerprint_request("test", &b)
        );
    }

    #[test]
    fn fingerprint_request_mixed_nesting() {
        let a = json!({
            "outer": [
                {"key": "value"},
                [1, 2, {"nested": true}]
            ]
        });
        let b = json!({
            "outer": [
                {"key": "value"},
                [1, 2, {"nested": true}]
            ]
        });
        assert_eq!(
            fingerprint_request("test", &a),
            fingerprint_request("test", &b)
        );
    }

    // ------------------------------------------------------------------
    // router — concurrent idempotency safety
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn router_idempotency_cache_hit_after_miss() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        struct CountHandler;

        #[async_trait]
        impl IntentHandler for CountHandler {
            fn name(&self) -> &'static str {
                "count_handler"
            }
            fn description(&self) -> &'static str {
                "Counts executions"
            }
            fn input_schema(&self) -> serde_json::Value {
                json!({})
            }
            fn requires_idempotency_token(&self) -> bool {
                true
            }
            fn extract_idempotency_token(&self, input: &Value) -> Option<String> {
                input
                    .get("idempotency_token")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
            }
            async fn execute(
                &self,
                _input: Value,
                _client: Arc<dyn IntervalsClient>,
                _idempotency_cache: Option<&IdempotencyCache>,
            ) -> Result<IntentOutput, IntentError> {
                COUNTER.fetch_add(1, Ordering::SeqCst);
                Ok(IntentOutput::new(vec![ContentBlock::text("done")]))
            }
        }

        let handlers = vec![Box::new(CountHandler) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let input1 = json!({"idempotency_token": "count-token", "dry_run": false});
        let r1 = router.route("count_handler", input1.clone(), None).await;
        assert!(r1.is_ok());

        let input2 = json!({"idempotency_token": "count-token", "dry_run": false});
        let r2 = router.route("count_handler", input2, None).await;
        assert!(r2.is_ok());

        assert_eq!(
            COUNTER.load(Ordering::SeqCst),
            1,
            "handler should execute only once"
        );
        COUNTER.store(0, Ordering::SeqCst);
    }

    // ------------------------------------------------------------------
    // route() — exercises MockClient methods via handler
    // ------------------------------------------------------------------

    struct ClientExercisingHandler;

    #[async_trait]
    impl IntentHandler for ClientExercisingHandler {
        fn name(&self) -> &'static str {
            "client_exerciser"
        }
        fn description(&self) -> &'static str {
            "Exercises multiple client methods"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({})
        }
        fn requires_idempotency_token(&self) -> bool {
            false
        }
        async fn execute(
            &self,
            _input: Value,
            client: Arc<dyn IntervalsClient>,
            _idempotency_cache: Option<&IdempotencyCache>,
        ) -> Result<IntentOutput, IntentError> {
            let _ = client.get_athlete_profile().await;
            let _ = client.get_recent_activities(None, None).await;
            let _ = client.get_events(None, None).await;
            let _ = client.get_gear_list().await;
            let _ = client.get_sport_settings().await;
            let _ = client.get_fitness_summary().await;
            let _ = client.get_workout_library().await;
            let _ = client.list_routes().await;
            let _ = client.get_event("evt_1").await;
            let _ = client.get_route(1, false).await;
            let _ = client.get_route_similarity(1, 2).await;
            let _ = client.get_weather_config().await;
            let _ = client.get_hr_curves(None, "cycling").await;
            let _ = client.get_pace_curves(None, "running").await;
            let _ = client.get_power_curves(None, "cycling").await;
            let _ = client.get_wellness(None).await;
            let _ = client.get_upcoming_workouts(None, None, None).await;
            let _ = client.get_hr_histogram("act_1").await;
            let _ = client.get_power_histogram("act_1").await;
            let _ = client.get_pace_histogram("act_1").await;
            let _ = client.get_gap_histogram("act_1").await;
            let _ = client.get_activity_details("act_1").await;
            let _ = client.get_activity_intervals("act_1").await;
            let _ = client.get_activity_streams("act_1", None).await;
            let _ = client.search_activities("run", None).await;
            let _ = client.get_best_efforts("act_1", None).await;
            let _ = client.get_activities_around("act_1", None, None).await;
            let _ = client
                .search_intervals(30, 120, 80, 120, None, None, None, None)
                .await;
            Ok(IntentOutput::new(vec![ContentBlock::text("done")]))
        }
    }

    #[tokio::test]
    async fn router_route_exercises_client_methods() {
        setup_tracing();
        let handlers = vec![Box::new(ClientExercisingHandler) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let result = router.route("client_exerciser", json!({}), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_route_exercises_client_methods_with_athlete_id() {
        setup_tracing();
        let handlers = vec![Box::new(ClientExercisingHandler) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        let result = router
            .route("client_exerciser", json!({}), Some("athlete_123"))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn router_idempotency_cache_hit_after_error_not_cached() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static TRY_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct FailOnceHandler;

        #[async_trait]
        impl IntentHandler for FailOnceHandler {
            fn name(&self) -> &'static str {
                "fail_once"
            }
            fn description(&self) -> &'static str {
                "Fails on first call, succeeds on second"
            }
            fn input_schema(&self) -> serde_json::Value {
                json!({})
            }
            fn requires_idempotency_token(&self) -> bool {
                true
            }
            fn extract_idempotency_token(&self, input: &Value) -> Option<String> {
                input
                    .get("idempotency_token")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
            }
            async fn execute(
                &self,
                _input: Value,
                _client: Arc<dyn IntervalsClient>,
                _idempotency_cache: Option<&IdempotencyCache>,
            ) -> Result<IntentOutput, IntentError> {
                let attempt = TRY_COUNT.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    Err(IntentError::internal("First attempt fails"))
                } else {
                    Ok(IntentOutput::new(vec![ContentBlock::text("success")]))
                }
            }
        }

        let handlers = vec![Box::new(FailOnceHandler) as Box<dyn IntentHandler>];
        let client = Arc::new(MockIntervalsClient::default());
        let idempotency = Arc::new(IdempotencyMiddleware::new());
        let router = IntentRouter::new(handlers, client, idempotency);

        // First call fails — result is NOT cached
        let input1 = json!({"idempotency_token": "fail-once-token"});
        let r1 = router.route("fail_once", input1, None).await;
        assert!(r1.is_err());

        // Second call with same token but NOT cached (previous error wasn't stored)
        // so it retries — this time succeeds
        let input2 = json!({"idempotency_token": "fail-once-token"});
        let r2 = router.route("fail_once", input2, None).await;
        assert!(r2.is_ok());

        assert_eq!(
            TRY_COUNT.load(Ordering::SeqCst),
            2,
            "should execute twice (first fails, second succeeds)"
        );
        TRY_COUNT.store(0, Ordering::SeqCst);
    }
}
