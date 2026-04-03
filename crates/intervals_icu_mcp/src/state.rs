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

    // ========================================================================
    // DownloadState Tests
    // ========================================================================

    #[test]
    fn test_download_state_pending() {
        let state = DownloadState::Pending;
        assert!(matches!(state, DownloadState::Pending));
    }

    #[test]
    fn test_download_state_in_progress() {
        let state = DownloadState::InProgress;
        assert!(matches!(state, DownloadState::InProgress));
    }

    #[test]
    fn test_download_state_completed() {
        let state = DownloadState::Completed;
        assert!(matches!(state, DownloadState::Completed));
    }

    #[test]
    fn test_download_state_failed() {
        let state = DownloadState::Failed("Error message".into());
        assert!(matches!(state, DownloadState::Failed(msg) if msg == "Error message"));
    }

    #[test]
    fn test_download_state_cancelled() {
        let state = DownloadState::Cancelled;
        assert!(matches!(state, DownloadState::Cancelled));
    }

    #[test]
    fn test_download_state_clone() {
        let state = DownloadState::Failed("test".into());
        let cloned = state.clone();
        assert!(matches!(cloned, DownloadState::Failed(msg) if msg == "test"));
    }

    #[test]
    fn test_download_state_debug() {
        let state = DownloadState::InProgress;
        let debug = format!("{:?}", state);
        assert!(debug.contains("InProgress"));
    }

    // ========================================================================
    // DownloadStatus Tests
    // ========================================================================

    #[test]
    fn test_download_status_new() {
        let status = DownloadStatus {
            id: "dl_123".into(),
            activity_id: "act_456".into(),
            state: DownloadState::Pending,
            bytes_downloaded: 0,
            total_bytes: Some(1024),
            path: None,
        };

        assert_eq!(status.id, "dl_123");
        assert_eq!(status.activity_id, "act_456");
        assert!(matches!(status.state, DownloadState::Pending));
        assert_eq!(status.bytes_downloaded, 0);
        assert_eq!(status.total_bytes, Some(1024));
        assert!(status.path.is_none());
    }

    #[test]
    fn test_download_status_completed() {
        let status = DownloadStatus {
            id: "dl_789".into(),
            activity_id: "act_000".into(),
            state: DownloadState::Completed,
            bytes_downloaded: 2048,
            total_bytes: Some(2048),
            path: Some("/path/to/file.fit".into()),
        };

        assert_eq!(status.bytes_downloaded, 2048);
        assert_eq!(status.total_bytes, Some(2048));
        assert_eq!(status.path, Some("/path/to/file.fit".into()));
    }

    #[test]
    fn test_download_status_clone() {
        let status = DownloadStatus {
            id: "dl_clone".into(),
            activity_id: "act_clone".into(),
            state: DownloadState::InProgress,
            bytes_downloaded: 512,
            total_bytes: None,
            path: None,
        };

        let cloned = status.clone();
        assert_eq!(cloned.id, status.id);
        assert_eq!(cloned.bytes_downloaded, status.bytes_downloaded);
    }

    #[test]
    fn test_download_status_debug() {
        let status = DownloadStatus {
            id: "dl_debug".into(),
            activity_id: "act_debug".into(),
            state: DownloadState::Completed,
            bytes_downloaded: 100,
            total_bytes: Some(100),
            path: Some("/test".into()),
        };

        let debug = format!("{:?}", status);
        assert!(debug.contains("DownloadStatus"));
        assert!(debug.contains("dl_debug"));
    }

    // ========================================================================
    // WebhookEvent Tests
    // ========================================================================

    #[test]
    fn test_webhook_event_new() {
        let event = WebhookEvent {
            id: "webhook_123".into(),
            payload: serde_json::json!({"key": "value"}),
            received_at: 1234567890,
        };

        assert_eq!(event.id, "webhook_123");
        assert_eq!(event.payload["key"], "value");
        assert_eq!(event.received_at, 1_234_567_890);
    }

    #[test]
    fn test_webhook_event_empty_payload() {
        let event = WebhookEvent {
            id: "webhook_empty".into(),
            payload: serde_json::json!({}),
            received_at: 0,
        };

        assert!(event.payload.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_webhook_event_clone() {
        let event = WebhookEvent {
            id: "webhook_clone".into(),
            payload: serde_json::json!({"data": [1, 2, 3]}),
            received_at: 9_999_999_999,
        };

        let cloned = event.clone();
        assert_eq!(cloned.id, event.id);
        assert_eq!(cloned.payload, event.payload);
        assert_eq!(cloned.received_at, event.received_at);
    }

    #[test]
    fn test_webhook_event_debug() {
        let event = WebhookEvent {
            id: "webhook_debug".into(),
            payload: serde_json::json!(null),
            received_at: 111,
        };

        let debug = format!("{:?}", event);
        assert!(debug.contains("WebhookEvent"));
        assert!(debug.contains("webhook_debug"));
    }
}
