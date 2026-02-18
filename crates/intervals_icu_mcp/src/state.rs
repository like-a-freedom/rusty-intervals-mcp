use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub enum DownloadState {
    Pending,
    InProgress,
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct DownloadStatus {
    pub id: String,
    pub activity_id: String,
    pub state: DownloadState,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub path: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct WebhookEvent {
    pub id: String,
    pub payload: serde_json::Value,
    pub received_at: u64,
}
