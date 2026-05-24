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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_state_failed_holds_message() {
        let state = DownloadState::Failed("Error message".into());
        assert!(matches!(state, DownloadState::Failed(msg) if msg == "Error message"));
    }

    #[test]
    fn download_status_construction() {
        let status = DownloadStatus {
            id: "dl_123".into(),
            activity_id: "act_456".into(),
            state: DownloadState::Pending,
            bytes_downloaded: 0,
            total_bytes: Some(1024),
            path: None,
        };
        assert!(matches!(status.state, DownloadState::Pending));
        assert_eq!(status.total_bytes, Some(1024));
        assert!(status.path.is_none());
    }

    #[test]
    fn webhook_event_holds_data() {
        let event = WebhookEvent {
            id: "webhook_123".into(),
            payload: serde_json::json!({"key": "value"}),
            received_at: 1234567890,
        };
        assert_eq!(event.payload["key"], "value");
        assert_eq!(event.received_at, 1_234_567_890);
    }
}
