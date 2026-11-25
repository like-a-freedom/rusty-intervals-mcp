use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use intervals_icu_mcp::intents::idempotency::IdempotencyEntry;
use intervals_icu_mcp::intents::{
    IdempotencyCache, IdempotencyMiddleware, IntentError, IntentHandler, IntentOutput, IntentRouter,
};
use secrecy::SecretString;
use serde_json::{Value, json};

struct CountingHandler(Arc<AtomicUsize>);

#[async_trait]
impl IntentHandler for CountingHandler {
    fn name(&self) -> &'static str {
        "legacy_test"
    }
    fn description(&self) -> &'static str {
        "test"
    }
    fn input_schema(&self) -> Value {
        json!({})
    }
    fn requires_idempotency_token(&self) -> bool {
        true
    }
    fn extract_idempotency_token(&self, input: &Value) -> Option<String> {
        input.get("idempotency_token")?.as_str().map(str::to_owned)
    }
    async fn execute(
        &self,
        _input: Value,
        _client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(IntentOutput::markdown("executed"))
    }
}

#[tokio::test]
async fn old_raw_key_file_blocks_router_after_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old-cache.json");
    let entry = IdempotencyEntry::new(
        IntentOutput::markdown("private legacy result"),
        "old fingerprint".into(),
        std::time::Duration::from_secs(3600),
    );
    std::fs::write(
        &path,
        serde_json::to_vec(&json!([["old-token", entry]])).unwrap(),
    )
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let middleware = Arc::new(IdempotencyMiddleware::with_file(path.clone()));
    let router = IntentRouter::new(
        vec![Box::new(CountingHandler(calls.clone()))],
        Arc::new(
            ReqwestIntervalsClient::new(
                "https://example.invalid",
                "athlete".to_string(),
                SecretString::new("test-key".into()),
            )
            .unwrap(),
        ),
        middleware.clone(),
    );
    let result = router
        .route(
            "legacy_test",
            json!({"idempotency_token": "old-token"}),
            None,
        )
        .await;
    assert!(matches!(result, Err(IntentError::IdempotencyConflict(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    middleware.flush().await.unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(saved[0][1].get("owner").is_none());
}
