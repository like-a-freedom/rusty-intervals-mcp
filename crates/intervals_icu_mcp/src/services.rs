use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, mpsc, watch};
use uuid::Uuid;

use crate::{DownloadState, DownloadStatus, ObjectResult, WebhookEvent};
use intervals_icu_client::IntervalsClient;

#[derive(Clone)]
pub struct DownloadService {
    downloads: Arc<Mutex<HashMap<String, DownloadStatus>>>,
    cancel_senders: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
}

impl DownloadService {
    pub fn new(
        downloads: Arc<Mutex<HashMap<String, DownloadStatus>>>,
        cancel_senders: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    ) -> Self {
        Self {
            downloads,
            cancel_senders,
        }
    }

    pub async fn start_download(
        &self,
        client: Arc<dyn IntervalsClient>,
        activity_id: String,
        output_path: Option<String>,
    ) -> String {
        let id = Uuid::new_v4().to_string();

        let status = DownloadStatus {
            id: id.clone(),
            activity_id: activity_id.clone(),
            state: DownloadState::Pending,
            bytes_downloaded: 0,
            total_bytes: None,
            path: None,
        };

        {
            let mut map = self.downloads.lock().await;
            map.insert(id.clone(), status);
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        {
            let mut canc = self.cancel_senders.lock().await;
            canc.insert(id.clone(), cancel_tx);
        }

        let downloads = self.downloads.clone();
        let id_clone_for_task = id.clone();
        let params_activity = activity_id.clone();
        let path_opt_clone = output_path.clone();

        tokio::spawn(async move {
            {
                let mut map = downloads.lock().await;
                if let Some(s) = map.get_mut(&id_clone_for_task) {
                    s.state = DownloadState::InProgress;
                }
            }

            let (tx, mut rx) = mpsc::channel(8);
            let out_path = path_opt_clone.map(std::path::PathBuf::from);
            let download_fut = client.download_activity_file_with_progress(
                &params_activity,
                out_path,
                tx,
                cancel_rx,
            );

            let downloads_clone = downloads.clone();
            let id_clone = id_clone_for_task.clone();
            let progress_handle = tokio::spawn(async move {
                while let Some(pr) = rx.recv().await {
                    let mut map = downloads_clone.lock().await;
                    if let Some(s) = map.get_mut(&id_clone) {
                        s.bytes_downloaded = pr.bytes_downloaded;
                        s.total_bytes = pr.total_bytes;
                    }
                }
            });

            match download_fut.await {
                Ok(path) => {
                    let mut map = downloads.lock().await;
                    if let Some(s) = map.get_mut(&id_clone_for_task) {
                        s.state = DownloadState::Completed;
                        s.path = path;
                    }
                }
                Err(e) => {
                    let mut map = downloads.lock().await;
                    if let Some(s) = map.get_mut(&id_clone_for_task) {
                        s.state = DownloadState::Failed(e.to_string());
                    }
                }
            }

            let _ = progress_handle.await;
        });

        id
    }

    pub async fn get_status(&self, download_id: &str) -> Option<DownloadStatus> {
        let map = self.downloads.lock().await;
        map.get(download_id).cloned()
    }

    pub async fn list_downloads(&self) -> Vec<DownloadStatus> {
        let map = self.downloads.lock().await;
        map.values().cloned().collect()
    }

    pub async fn cancel_download(&self, download_id: &str) -> bool {
        let canc = self.cancel_senders.lock().await;
        if let Some(tx) = canc.get(download_id) {
            let _ = tx.send(true);
            let mut map = self.downloads.lock().await;
            if let Some(s) = map.get_mut(download_id) {
                s.state = DownloadState::Cancelled;
            }
            true
        } else {
            false
        }
    }
}

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
