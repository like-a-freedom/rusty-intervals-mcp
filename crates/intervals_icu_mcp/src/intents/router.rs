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
    use super::fingerprint_request;
    use serde_json::json;

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
}
