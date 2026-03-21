use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use crate::{ObjectResult, WebhookEvent};

#[derive(Clone)]
pub struct WebhookService {
    webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>>,
    webhook_secret: Arc<Mutex<Option<String>>>,
}

impl WebhookService {
    pub fn new(
        webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>>,
        webhook_secret: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            webhooks,
            webhook_secret,
        }
    }

    pub async fn set_secret(&self, secret: impl Into<String>) {
        let mut s = self.webhook_secret.lock().await;
        *s = Some(secret.into());
    }

    pub async fn process_webhook(
        &self,
        signature: &str,
        payload: serde_json::Value,
    ) -> Result<ObjectResult, String> {
        let secret_opt = self.webhook_secret.lock().await.clone();
        let secret = secret_opt.ok_or_else(|| "webhook secret not set".to_string())?;

        verify_signature(&secret, signature, &payload)?;

        let id = extract_event_id(&payload);
        let evt = WebhookEvent {
            id: id.clone(),
            payload: payload.clone(),
            received_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        let mut store = self.webhooks.lock().await;
        if store.contains_key(&id) {
            return Ok(ObjectResult {
                value: serde_json::json!({ "duplicate": true }),
            });
        }

        store.insert(id.clone(), evt);
        Ok(ObjectResult {
            value: serde_json::json!({ "ok": true, "id": id }),
        })
    }
}

fn verify_signature(
    secret: &str,
    signature: &str,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let mut mac: Hmac<Sha256> =
        Hmac::new_from_slice(secret.as_bytes()).map_err(|e| e.to_string())?;
    let body = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
    mac.update(&body);
    let expected = mac.finalize().into_bytes();
    let sig_bytes = hex::decode(signature).map_err(|e| e.to_string())?;
    if expected.as_slice() != sig_bytes.as_slice() {
        return Err("signature mismatch".into());
    }
    Ok(())
}

fn extract_event_id(payload: &serde_json::Value) -> String {
    if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
        return id.to_string();
    }

    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("ts-{}", since.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_event_id_prefers_payload_id() {
        let payload = serde_json::json!({"id": "evt-1"});
        assert_eq!(extract_event_id(&payload), "evt-1");
    }

    #[test]
    fn extract_event_id_fallback_to_timestamp() {
        let payload = serde_json::json!({"type": "activity", "data": "test"});
        let result = extract_event_id(&payload);
        assert!(result.starts_with("ts-"));
    }

    #[test]
    fn extract_event_id_null_id_fallback() {
        let payload = serde_json::json!({"id": null});
        let result = extract_event_id(&payload);
        assert!(result.starts_with("ts-"));
    }

    #[test]
    fn extract_event_id_numeric_id_fallback() {
        let payload = serde_json::json!({"id": 12345});
        let result = extract_event_id(&payload);
        assert!(result.starts_with("ts-"));
    }

    #[test]
    fn verify_signature_rejects_invalid_signature() {
        let payload = serde_json::json!({"id": "evt-1"});
        let result = verify_signature("secret", "deadbeef", &payload);
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "signature mismatch");
    }

    #[test]
    fn verify_signature_accepts_valid_signature() {
        let payload = serde_json::json!({"id": "evt-1"});
        let mut mac: Hmac<Sha256> = Hmac::new_from_slice(b"secret").unwrap();
        mac.update(&serde_json::to_vec(&payload).unwrap());
        let signature = hex::encode(mac.finalize().into_bytes());

        let result = verify_signature("secret", &signature, &payload);
        assert!(result.is_ok());
    }

    #[test]
    fn verify_signature_empty_secret_fails() {
        let payload = serde_json::json!({"id": "evt-1"});
        let result = verify_signature("", "deadbeef", &payload);
        assert!(result.is_err());
    }

    #[test]
    fn verify_signature_invalid_hex_fails() {
        let payload = serde_json::json!({"id": "evt-1"});
        let result = verify_signature("secret", "not_validhex", &payload);
        assert!(result.is_err());
    }

    #[test]
    fn verify_signature_tampered_payload_fails() {
        let payload = serde_json::json!({"id": "evt-1"});
        let mut mac: Hmac<Sha256> = Hmac::new_from_slice(b"secret").unwrap();
        mac.update(&serde_json::to_vec(&payload).unwrap());
        let signature = hex::encode(mac.finalize().into_bytes());

        // Tamper with payload
        let tampered = serde_json::json!({"id": "evt-2"});
        let result = verify_signature("secret", &signature, &tampered);
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "signature mismatch");
    }

    #[tokio::test]
    async fn webhook_service_new() {
        let webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let secret: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let service = WebhookService::new(webhooks.clone(), secret.clone());

        assert!(Arc::ptr_eq(&service.webhooks, &webhooks));
        assert!(Arc::ptr_eq(&service.webhook_secret, &secret));
    }

    #[tokio::test]
    async fn webhook_service_set_secret() {
        let webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let secret: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let service = WebhookService::new(webhooks.clone(), secret.clone());

        service.set_secret("new_secret").await;

        let stored = secret.lock().await;
        assert_eq!(stored.as_ref().unwrap(), "new_secret");
    }

    #[tokio::test]
    async fn webhook_service_process_without_secret() {
        let webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let secret: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let service = WebhookService::new(webhooks, secret);

        let payload = serde_json::json!({"id": "test-1"});
        let result = service.process_webhook("invalid", payload).await;

        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "webhook secret not set");
    }

    #[tokio::test]
    async fn webhook_service_process_valid() {
        let webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let secret: Arc<Mutex<Option<String>>> =
            Arc::new(Mutex::new(Some("test_secret".to_string())));
        let service = WebhookService::new(webhooks.clone(), secret);

        let payload = serde_json::json!({"id": "test-1", "type": "activity.created"});
        let mut mac: Hmac<Sha256> = Hmac::new_from_slice(b"test_secret").unwrap();
        mac.update(&serde_json::to_vec(&payload).unwrap());
        let signature = hex::encode(mac.finalize().into_bytes());

        let result = service.process_webhook(&signature, payload.clone()).await;
        assert!(result.is_ok());
        let result_value = result.unwrap();
        assert_eq!(result_value.value.get("ok"), Some(&serde_json::json!(true)));
        assert!(result_value.value.get("id").is_some());

        // Verify stored in webhooks
        let id = result_value
            .value
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let stored = webhooks.lock().await;
        assert!(stored.contains_key(&id));
        let event = stored.get(&id).unwrap();
        assert_eq!(event.id, id);
        assert_eq!(event.payload, payload);
    }

    #[tokio::test]
    async fn webhook_service_process_duplicate() {
        let webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let secret: Arc<Mutex<Option<String>>> =
            Arc::new(Mutex::new(Some("test_secret".to_string())));
        let service = WebhookService::new(webhooks.clone(), secret);

        let payload = serde_json::json!({"id": "dup-test", "type": "activity.created"});
        let mut mac: Hmac<Sha256> = Hmac::new_from_slice(b"test_secret").unwrap();
        mac.update(&serde_json::to_vec(&payload).unwrap());
        let signature = hex::encode(mac.finalize().into_bytes());

        // First submission
        let result1 = service.process_webhook(&signature, payload.clone()).await;
        assert!(result1.is_ok());

        // Duplicate submission
        let result2 = service.process_webhook(&signature, payload.clone()).await;
        assert!(result2.is_ok());
        let result2_value = result2.unwrap();
        assert_eq!(
            result2_value.value.get("duplicate"),
            Some(&serde_json::json!(true))
        );
    }

    #[tokio::test]
    async fn webhook_service_process_invalid_signature() {
        let webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let secret: Arc<Mutex<Option<String>>> =
            Arc::new(Mutex::new(Some("test_secret".to_string())));
        let service = WebhookService::new(webhooks, secret);

        let payload = serde_json::json!({"id": "test-1"});
        let result = service.process_webhook("invalid_signature", payload).await;

        assert!(result.is_err());
        // hex::decode error for invalid hex string or signature mismatch
        let err_msg = result.err().unwrap();
        assert!(
            err_msg.contains("Odd number of digits")
                || err_msg.contains("signature mismatch")
                || err_msg.contains("Invalid hex")
        );
    }

    #[tokio::test]
    async fn webhook_service_clone() {
        let webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let secret: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let service = WebhookService::new(webhooks, secret);
        let cloned = service.clone();

        // Both should share the same underlying state
        service.set_secret("shared_secret").await;
        let stored = cloned.webhook_secret.lock().await;
        assert_eq!(stored.as_ref().unwrap(), "shared_secret");
    }
}
