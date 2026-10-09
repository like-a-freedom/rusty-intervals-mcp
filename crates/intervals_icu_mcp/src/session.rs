//! Session-lifecycle logging decorator around rmcp's `LocalSessionManager`.
//!
//! Tracks per-session start time and client identity (from the `initialize`
//! handshake) so session closes are auditable — reconnect storms per client
//! become visible via lifetime + client_name on the close event.

use rmcp::transport::streamable_http_server::SessionManager;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::session::{ServerSseMessage, SessionId};
use std::collections::HashMap;
use std::sync::Mutex;

/// One tracked session: when it started and who the client said it is.
#[derive(Debug)]
struct SessionMeta {
    started: std::time::Instant,
    client_name: String,
    client_version: String,
}

/// Decorator over `LocalSessionManager` adding session lifecycle logging.
///
/// Decorated points:
/// - `create_session` — starts the per-session clock;
/// - `initialize_session` — captures `clientInfo` (self-reported);
/// - `close_session` — emits `MCP session closed` with lifetime, so
///   reconnect storms become visible per client.
///
/// Note: sessions that silently expire by idle timeout never call
/// `close_session` (rmcp fires it only for the DELETE request); that case
/// stays invisible by design.
pub struct LoggingSessionManager {
    inner: LocalSessionManager,
    sessions: Mutex<HashMap<SessionId, SessionMeta>>,
}

impl LoggingSessionManager {
    pub fn new(inner: LocalSessionManager) -> Self {
        Self {
            inner,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn track_created(&self, id: &SessionId) {
        let mut sessions = match self.sessions.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        sessions.insert(
            id.clone(),
            SessionMeta {
                started: std::time::Instant::now(),
                client_name: "unknown".to_string(),
                client_version: "unknown".to_string(),
            },
        );
    }

    fn take_meta(&self, id: &SessionId) -> Option<SessionMeta> {
        let mut sessions = match self.sessions.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        sessions.remove(id)
    }

    /// Extract `clientInfo` from an initialize JSON-RPC message into the
    /// session record. Messages that are not initialize requests are ignored.
    fn capture_client_identity(&self, id: &SessionId, message: &rmcp::model::ClientJsonRpcMessage) {
        let client = match message {
            rmcp::model::JsonRpcMessage::Request(request) => match &request.request {
                rmcp::model::ClientRequest::InitializeRequest(init) => Some((
                    init.params.client_info.name.clone(),
                    init.params.client_info.version.clone(),
                )),
                _ => None,
            },
            _ => None,
        };
        if let Some((name, version)) = client
            && let Ok(mut sessions) = self.sessions.lock()
            && let Some(meta) = sessions.get_mut(id)
        {
            meta.client_name = name;
            meta.client_version = version;
        }
    }
}

impl SessionManager for LoggingSessionManager {
    type Error = <LocalSessionManager as SessionManager>::Error;
    type Transport = <LocalSessionManager as SessionManager>::Transport;

    async fn create_session(&self) -> Result<(SessionId, Self::Transport), Self::Error> {
        let (id, transport) = self.inner.create_session().await?;
        self.track_created(&id);
        Ok((id, transport))
    }

    async fn initialize_session(
        &self,
        id: &SessionId,
        message: rmcp::model::ClientJsonRpcMessage,
    ) -> Result<rmcp::model::ServerJsonRpcMessage, Self::Error> {
        // Client identity is self-reported in the initialize request; capture
        // it before the message moves into the session transport.
        self.capture_client_identity(id, &message);
        self.inner.initialize_session(id, message).await
    }

    async fn has_session(&self, id: &SessionId) -> Result<bool, Self::Error> {
        self.inner.has_session(id).await
    }

    async fn close_session(&self, id: &SessionId) -> Result<(), Self::Error> {
        let result = self.inner.close_session(id).await;
        if result.is_ok()
            && let Some(meta) = self.take_meta(id)
        {
            crate::metrics::record_mcp_session("ended");
            tracing::info!(
                session_id = %id,
                client_name = %meta.client_name,
                client_version = %meta.client_version,
                lifetime_ms = meta.started.elapsed().as_millis(),
                "MCP session closed"
            );
        }
        result
    }

    async fn create_stream(
        &self,
        id: &SessionId,
        message: rmcp::model::ClientJsonRpcMessage,
    ) -> Result<impl futures::Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error>
    {
        self.inner.create_stream(id, message).await
    }

    async fn accept_message(
        &self,
        id: &SessionId,
        message: rmcp::model::ClientJsonRpcMessage,
    ) -> Result<(), Self::Error> {
        self.inner.accept_message(id, message).await
    }

    async fn create_standalone_stream(
        &self,
        id: &SessionId,
    ) -> Result<impl futures::Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error>
    {
        self.inner.create_standalone_stream(id).await
    }

    async fn resume(
        &self,
        id: &SessionId,
        last_event_id: String,
    ) -> Result<impl futures::Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error>
    {
        self.inner.resume(id, last_event_id).await
    }
}

#[cfg(test)]
mod tests {
    use rmcp::model::{ClientJsonRpcMessage, ClientRequest, InitializeRequest, RequestId};

    use super::*;

    fn initialize_message(name: &str, version: &str) -> ClientJsonRpcMessage {
        let mut params = rmcp::model::InitializeRequestParams::default();
        params.client_info.name = name.to_string();
        params.client_info.version = version.to_string();
        ClientJsonRpcMessage::request(
            ClientRequest::InitializeRequest(InitializeRequest::new(params)),
            RequestId::Number(1),
        )
    }

    #[tokio::test]
    async fn close_session_reports_lifetime_and_client() {
        let manager = LoggingSessionManager::new(LocalSessionManager::default());
        let (id, _transport) = manager.create_session().await.expect("session created");
        manager.capture_client_identity(&id, &initialize_message("zed", "0.209.5"));

        manager.close_session(&id).await.expect("close ok");

        let sessions = manager.sessions.lock().expect("lock");
        assert!(
            sessions.is_empty(),
            "closed session must be untracked, got {sessions:?}"
        );
    }

    #[tokio::test]
    async fn client_identity_is_captured_at_initialize() {
        let manager = LoggingSessionManager::new(LocalSessionManager::default());
        let (id, _transport) = manager.create_session().await.expect("session created");
        manager.capture_client_identity(&id, &initialize_message("codex", "1.2.3"));

        {
            let sessions = manager.sessions.lock().expect("lock");
            let meta = sessions.get(&id).expect("session tracked");
            assert_eq!(meta.client_name, "codex");
            assert_eq!(meta.client_version, "1.2.3");
        }
        manager.close_session(&id).await.expect("close ok");
    }

    #[tokio::test]
    async fn close_unknown_session_is_a_noop() {
        // LocalSessionManager treats closing an absent id as success; the
        // decorator must preserve that and leave its own bookkeeping empty.
        let manager = LoggingSessionManager::new(LocalSessionManager::default());
        let unknown: SessionId = "does-not-exist".into();
        assert!(manager.close_session(&unknown).await.is_ok());
        assert!(manager.sessions.lock().expect("lock").is_empty());
    }
}
