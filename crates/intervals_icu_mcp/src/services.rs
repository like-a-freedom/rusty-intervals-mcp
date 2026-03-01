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
    fn verify_signature_rejects_invalid_signature() {
        let payload = serde_json::json!({"id": "evt-1"});
        let result = verify_signature("secret", "deadbeef", &payload);
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "signature mismatch");
    }
}
