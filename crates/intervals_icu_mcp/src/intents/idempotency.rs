use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::journal::{OperationJournal, OperationJournalError, Reservation};
use super::types::{IntentError, IntentOutput};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyEntry {
    pub result: IntentOutput,
    pub request_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub created_at: DateTime<Local>,
    pub expires_at: DateTime<Local>,
}

impl IdempotencyEntry {
    pub fn new(result: IntentOutput, request_fingerprint: String, ttl: Duration) -> Self {
        let now = Local::now();
        let expires =
            now + chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::days(1));
        Self {
            result,
            request_fingerprint,
            owner: None,
            created_at: now,
            expires_at: expires,
        }
    }
    pub fn is_expired(&self) -> bool {
        Local::now() > self.expires_at
    }
}

#[derive(Debug, Clone, Default)]
pub struct IdempotencyStats {
    pub hits: u64,
    pub misses: u64,
    pub sets: u64,
    pub evictions: u64,
    pub expired_count: u64,
}

const MAX_UNCERTAIN_OPERATIONS: usize = 1024;
type Digest256 = [u8; 32];

#[derive(Clone)]
pub struct IdempotencyMiddleware {
    cache: Arc<RwLock<CacheInner>>,
    in_flight: Arc<std::sync::Mutex<HashMap<Digest256, std::sync::Weak<tokio::sync::Mutex<()>>>>>,
    uncertain: Arc<std::sync::Mutex<HashMap<Digest256, Digest256>>>,
    default_ttl: Duration,
    persistence_path: Option<PathBuf>,
    journal: Option<OperationJournal>,
}

struct CacheInner {
    entries: HashMap<String, IdempotencyEntry>,
    stats: IdempotencyStats,
}

async fn mark_cancelled_reservation_unknown(
    journal: &OperationJournal,
    token: &str,
    request_fingerprint: &str,
    owner: Option<&str>,
) {
    if let Err(error) = journal
        .mark_unknown(token, request_fingerprint, owner)
        .await
    {
        tracing::warn!(error = %error, "failed to persist cancelled operation reservation as unknown");
    }
}

async fn reserve_with_cancellation(
    journal: &OperationJournal,
    token: &str,
    request_fingerprint: &str,
    owner: Option<&str>,
    tenant_scope: Option<&str>,
) -> Result<(Reservation, tokio::sync::oneshot::Sender<()>), IntentError> {
    let journal = journal.clone();
    let token = token.to_owned();
    let request_fingerprint = request_fingerprint.to_owned();
    let owner = owner.map(str::to_owned);
    let tenant_scope = tenant_scope.map(str::to_owned);
    let (reservation_tx, reservation_rx) = tokio::sync::oneshot::channel();
    let (dispatch_ack_tx, dispatch_ack_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let reservation = journal
            .reserve(
                &token,
                &request_fingerprint,
                owner.as_deref(),
                tenant_scope.as_deref(),
            )
            .await;
        let was_reserved = matches!(&reservation, Ok(Reservation::Reserved));

        if reservation_tx.send(reservation).is_err() {
            if was_reserved {
                mark_cancelled_reservation_unknown(
                    &journal,
                    &token,
                    &request_fingerprint,
                    owner.as_deref(),
                )
                .await;
            }
            return;
        }

        if was_reserved && dispatch_ack_rx.await.is_err() {
            mark_cancelled_reservation_unknown(
                &journal,
                &token,
                &request_fingerprint,
                owner.as_deref(),
            )
            .await;
        }
    });

    let reservation = reservation_rx
        .await
        .map_err(|_| {
            IntentError::InternalError(
                "The operation reservation was interrupted. Do not retry automatically; investigate the upstream system.".to_string(),
            )
        })?
        .map_err(map_reservation_error)?;
    Ok((reservation, dispatch_ack_tx))
}

impl IdempotencyMiddleware {
    pub fn new() -> Self {
        Self::with_ttl(Duration::from_secs(86400))
    }
    pub fn with_ttl(ttl: Duration) -> Self {
        Self::with_parts(ttl, None, HashMap::new(), None)
    }

    pub(crate) fn with_journal(journal: OperationJournal) -> Self {
        Self::with_parts(
            Duration::from_secs(86400),
            None,
            HashMap::new(),
            Some(journal),
        )
    }

    fn with_parts(
        default_ttl: Duration,
        persistence_path: Option<PathBuf>,
        entries: HashMap<String, IdempotencyEntry>,
        journal: Option<OperationJournal>,
    ) -> Self {
        Self {
            cache: Arc::new(RwLock::new(CacheInner {
                entries,
                stats: IdempotencyStats::default(),
            })),
            in_flight: Arc::default(),
            uncertain: Arc::default(),
            default_ttl,
            persistence_path,
            journal,
        }
    }

    pub fn with_file(path: PathBuf) -> Self {
        let entries = Self::load_from_file(&path);
        Self::with_parts(Duration::from_secs(86400), Some(path), entries, None)
    }

    pub fn with_ttl_and_file(ttl: Duration, path: PathBuf) -> Self {
        let entries = Self::load_from_file(&path);
        Self::with_parts(ttl, Some(path), entries, None)
    }

    pub async fn flush(&self) -> std::io::Result<()> {
        let Some(ref path) = self.persistence_path else {
            return Ok(());
        };
        let cache = self.cache.read().await;
        let serializable: Vec<_> = cache
            .entries
            .iter()
            .filter(|(_, e)| !e.is_expired())
            .map(|(k, e)| (k.clone(), e.clone()))
            .collect();
        let json = serde_json::to_vec(&serializable)?;

        let tmp_path = path.with_extension("tmp");
        tokio::fs::write(&tmp_path, &json).await?;
        tokio::fs::rename(&tmp_path, path).await?;
        Ok(())
    }

    fn load_from_file(path: &std::path::Path) -> HashMap<String, IdempotencyEntry> {
        let Ok(data) = std::fs::read_to_string(path) else {
            return HashMap::new();
        };
        let entries: Vec<(String, IdempotencyEntry)> =
            serde_json::from_str(&data).unwrap_or_default();
        entries
            .into_iter()
            .filter(|(_, e)| !e.is_expired())
            .collect()
    }
    pub async fn execute_with_idempotency<F, Fut>(
        &self,
        token: &str,
        request_fingerprint: &str,
        action: F,
    ) -> Result<IntentOutput, IntentError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<IntentOutput, IntentError>>,
    {
        self.execute_for_owner(token, request_fingerprint, None, action)
            .await
    }

    pub(crate) async fn execute_for_owner<F, Fut>(
        &self,
        token: &str,
        request_fingerprint: &str,
        owner: Option<&str>,
        action: F,
    ) -> Result<IntentOutput, IntentError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<IntentOutput, IntentError>>,
    {
        self.execute_for_owner_with_legacy(token, request_fingerprint, owner, None, None, action)
            .await
    }

    pub(crate) async fn execute_for_owner_with_legacy<F, Fut>(
        &self,
        token: &str,
        request_fingerprint: &str,
        owner: Option<&str>,
        legacy_token: Option<&str>,
        tenant_scope: Option<&str>,
        action: F,
    ) -> Result<IntentOutput, IntentError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<IntentOutput, IntentError>>,
    {
        let token_hash: Digest256 = Sha256::digest(token.as_bytes()).into();
        let fingerprint_hash: Digest256 = Sha256::digest(request_fingerprint.as_bytes()).into();
        let lock = {
            let mut in_flight = self.in_flight.lock().unwrap_or_else(|err| err.into_inner());
            in_flight.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = in_flight
                .get(&token_hash)
                .and_then(std::sync::Weak::upgrade)
            {
                lock
            } else {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                in_flight.insert(token_hash, Arc::downgrade(&lock));
                lock
            }
        };
        let _guard = lock.lock().await;
        if let Some(legacy_token) = legacy_token {
            let cache = self.cache.read().await;
            if cache
                .entries
                .get(legacy_token)
                .is_some_and(|entry| !entry.is_expired())
            {
                return Err(IntentError::IdempotencyConflict(
                    "idempotency token is unavailable".to_string(),
                ));
            }
        }
        if let Some(journal) = &self.journal {
            return self
                .execute_with_journal(
                    journal,
                    token,
                    request_fingerprint,
                    owner,
                    tenant_scope,
                    action,
                )
                .await;
        }
        if let Some(entry) = self
            .get_for_owner(token, request_fingerprint, owner)
            .await?
        {
            let mut cache = self.cache.write().await;
            cache.stats.hits += 1;
            crate::metrics::record_idempotency("hit");
            return Ok(entry);
        }
        {
            let mut uncertain = self.uncertain.lock().unwrap_or_else(|err| err.into_inner());
            if let Some(fingerprint) = uncertain.get(&token_hash) {
                if fingerprint != &fingerprint_hash {
                    return Err(IntentError::IdempotencyConflict(format!(
                        "token '{token}' was already used for a different request"
                    )));
                }
                return Err(IntentError::InternalError(
                    "The previous request has an unknown outcome. Check upstream before taking further action; do not retry this request automatically.".to_string(),
                ));
            }
            if uncertain.len() >= MAX_UNCERTAIN_OPERATIONS {
                return Err(IntentError::InternalError(
                    "Idempotency capacity exhausted; no action was started. Check unresolved upstream outcomes before restarting the server.".to_string(),
                ));
            }
            // Once started, cancellation, panic, or an error cannot prove upstream did not write.
            uncertain.insert(token_hash, fingerprint_hash);
        }
        let result = action().await?;
        self.set_for_owner(token, request_fingerprint, owner, &result)
            .await;
        self.uncertain
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&token_hash);
        {
            let mut cache = self.cache.write().await;
            cache.stats.misses += 1;
            cache.stats.sets += 1;
            crate::metrics::record_idempotency("miss");
        }
        Ok(result)
    }
    async fn execute_with_journal<F, Fut>(
        &self,
        journal: &OperationJournal,
        token: &str,
        request_fingerprint: &str,
        owner: Option<&str>,
        tenant_scope: Option<&str>,
        action: F,
    ) -> Result<IntentOutput, IntentError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<IntentOutput, IntentError>>,
    {
        let (reservation, dispatch_ack) =
            reserve_with_cancellation(journal, token, request_fingerprint, owner, tenant_scope)
                .await?;

        match reservation {
            Reservation::Replay(output) => {
                self.cache.write().await.stats.hits += 1;
                crate::metrics::record_idempotency("hit");
                Ok(*output)
            }
            Reservation::Reserved => {
                if dispatch_ack.send(()).is_err() {
                    return Err(IntentError::InternalError(
                        "The operation reservation could not be acknowledged. Do not retry automatically; investigate the upstream system.".to_string(),
                    ));
                }

                let output = match action().await {
                    Ok(output) => output,
                    Err(action_error) => {
                        if journal
                            .mark_unknown(token, request_fingerprint, owner)
                            .await
                            .is_err()
                        {
                            return Err(IntentError::InternalError(
                                "The upstream operation may have succeeded, but its uncertain outcome could not be recorded. Do not retry automatically; investigate the upstream system.".to_string(),
                            ));
                        }
                        return Err(action_error);
                    }
                };
                if journal
                    .complete(token, request_fingerprint, owner, &output)
                    .await
                    .is_err()
                {
                    let _ = journal
                        .mark_unknown(token, request_fingerprint, owner)
                        .await;
                    return Err(IntentError::InternalError(
                        "The upstream operation may have succeeded, and its outcome could not be confirmed or durably recorded. Do not retry automatically; investigate the upstream system.".to_string(),
                    ));
                }
                let mut cache = self.cache.write().await;
                cache.stats.misses += 1;
                cache.stats.sets += 1;
                crate::metrics::record_idempotency("miss");
                Ok(output)
            }
        }
    }

    pub async fn get(
        &self,
        token: &str,
        request_fingerprint: &str,
    ) -> Result<Option<IntentOutput>, IntentError> {
        self.get_for_owner(token, request_fingerprint, None).await
    }

    async fn get_for_owner(
        &self,
        token: &str,
        request_fingerprint: &str,
        owner: Option<&str>,
    ) -> Result<Option<IntentOutput>, IntentError> {
        let cache = self.cache.read().await;
        if let Some(entry) = cache.entries.get(token) {
            if entry.is_expired() {
                drop(cache);
                let mut cache = self.cache.write().await;
                cache.entries.remove(token);
                cache.stats.expired_count += 1;
                return Ok(None);
            }
            if entry.owner.as_deref() != owner {
                return Err(IntentError::IdempotencyConflict(
                    "idempotency token is unavailable".to_string(),
                ));
            }
            if entry.request_fingerprint != request_fingerprint {
                crate::metrics::record_idempotency("conflict");
                return Err(IntentError::IdempotencyConflict(format!(
                    "token '{}' was already used for a different request; generate a new idempotency token",
                    token
                )));
            }
            return Ok(Some(entry.result.clone()));
        }
        Ok(None)
    }
    pub async fn set(&self, token: &str, request_fingerprint: &str, result: &IntentOutput) {
        self.set_for_owner(token, request_fingerprint, None, result)
            .await;
    }

    async fn set_for_owner(
        &self,
        token: &str,
        request_fingerprint: &str,
        owner: Option<&str>,
        result: &IntentOutput,
    ) {
        let mut cache = self.cache.write().await;
        let mut entry = IdempotencyEntry::new(
            result.clone(),
            request_fingerprint.to_string(),
            self.default_ttl,
        );
        entry.owner = owner.map(str::to_owned);
        cache.entries.insert(token.to_string(), entry);
    }
    pub async fn get_stats(&self) -> IdempotencyStats {
        let cache = self.cache.read().await;
        cache.stats.clone()
    }
}

fn map_reservation_error(error: OperationJournalError) -> IntentError {
    match error {
        OperationJournalError::OwnerMismatch => {
            IntentError::IdempotencyConflict("idempotency token is unavailable".to_string())
        }
        OperationJournalError::FingerprintMismatch => IntentError::IdempotencyConflict(
            "idempotency token was already used for a different request".to_string(),
        ),
        OperationJournalError::UnknownOutcome => IntentError::InternalError(
            "The previous request has an unknown outcome. Check upstream before taking further action; do not retry this request automatically.".to_string(),
        ),
        OperationJournalError::CapacityExhausted(_) => IntentError::InternalError(
            "Idempotency capacity exhausted; no action was started. Check unresolved upstream outcomes before restarting the server.".to_string(),
        ),
        OperationJournalError::Store(_) | OperationJournalError::InvalidResult => {
            IntentError::InternalError(
                "The durable operation journal is unavailable or invalid; no action was started.".to_string(),
            )
        }
    }
}

impl Default for IdempotencyMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

pub fn generate_idempotency_token(intent: &str, params: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(intent.as_bytes());
    for p in params {
        hasher.update(p.as_bytes());
    }
    format!("{}_{}", intent, hex::encode(&hasher.finalize()[..8]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intents::journal::OperationJournal;
    use crate::state_store::StateStore;

    #[tokio::test]
    async fn concurrent_owners_share_lock_and_only_winner_can_replay() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let middleware = IdempotencyMiddleware::new();
        let calls = AtomicUsize::new(0);
        let action = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok(IntentOutput::markdown("private"))
        };
        let (a, b) = tokio::join!(
            middleware.execute_for_owner("token", "fingerprint", Some("a"), action),
            middleware.execute_for_owner("token", "fingerprint", Some("b"), action),
        );
        assert_ne!(a.is_ok(), b.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ownerless_persisted_entry_is_not_replayed_to_http() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        let old = IdempotencyMiddleware::with_file(path.clone());
        old.set("token", "fingerprint", &IntentOutput::markdown("private"))
            .await;
        old.flush().await.unwrap();
        let middleware = IdempotencyMiddleware::with_file(path);
        let called = std::sync::atomic::AtomicBool::new(false);
        let result = middleware
            .execute_for_owner("token", "fingerprint", Some("a"), || async {
                called.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate"))
            })
            .await;
        assert!(matches!(result, Err(IntentError::IdempotencyConflict(_))));
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn errors_and_cancellation_block_other_owners() {
        let middleware = IdempotencyMiddleware::new();
        let first = middleware
            .execute_for_owner("error", "fp", Some("a"), || async {
                Err(IntentError::InternalError("failure".into()))
            })
            .await;
        assert!(first.is_err());
        let (tx, rx) = tokio::sync::oneshot::channel();
        let clone = middleware.clone();
        let task = tokio::spawn(async move {
            clone
                .execute_for_owner("cancel", "fp", Some("a"), || async move {
                    tx.send(()).unwrap();
                    std::future::pending::<Result<IntentOutput, IntentError>>().await
                })
                .await
        });
        rx.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        for token in ["error", "cancel"] {
            for owner in ["a", "b"] {
                let result = middleware
                    .execute_for_owner(token, "fp", Some(owner), || async {
                        Ok(IntentOutput::markdown("duplicate"))
                    })
                    .await;
                assert!(matches!(result, Err(IntentError::InternalError(_))));
            }
        }
    }

    #[tokio::test]
    async fn concurrent_requests_with_the_same_token_execute_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let middleware = IdempotencyMiddleware::new();
        let calls = AtomicUsize::new(0);
        let action = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok(IntentOutput::markdown("created"))
        };
        let (first, second) = tokio::join!(
            middleware.execute_with_idempotency("token", "fingerprint", action),
            middleware.execute_with_idempotency("token", "fingerprint", action),
        );
        assert!(first.is_ok());
        assert!(second.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelled_write_is_not_repeated() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let middleware = IdempotencyMiddleware::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let first = {
            let middleware = middleware.clone();
            let calls = calls.clone();
            tokio::spawn(async move {
                middleware
                    .execute_with_idempotency("token", "fingerprint", || async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        started_tx.send(()).unwrap();
                        std::future::pending::<Result<IntentOutput, IntentError>>().await
                    })
                    .await
            })
        };
        started_rx.await.unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            middleware.execute_with_idempotency("token", "fingerprint", || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate"))
            }),
        )
        .await
        .expect("retry must not block");
        assert!(
            result.is_err(),
            "unknown outcome must not be reported as success"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let conflict = middleware
            .execute_with_idempotency("token", "other", || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate"))
            })
            .await;
        assert!(matches!(conflict, Err(IntentError::IdempotencyConflict(_))));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn uncertain_capacity_fails_closed_without_evicting() {
        let middleware = IdempotencyMiddleware::new();
        middleware
            .set("cached", "fp", &IntentOutput::markdown("ok"))
            .await;
        for i in 0..1024 {
            let token = format!("token-{i}");
            let result = middleware
                .execute_with_idempotency(&token, "fp", || async {
                    Err(IntentError::InternalError("unknown".into()))
                })
                .await;
            assert!(result.is_err());
        }
        let result = middleware
            .execute_with_idempotency("overflow", "fp", || async {
                Ok(IntentOutput::markdown("must not run"))
            })
            .await;
        assert!(
            result.is_err(),
            "capacity exhaustion must reject new actions"
        );
        let retry = middleware
            .execute_with_idempotency("token-0", "fp", || async {
                panic!("uncertain entry must not be evicted")
            })
            .await;
        assert!(retry.is_err());
        let cached = middleware
            .execute_with_idempotency("cached", "fp", || async {
                panic!("cached success must not execute again")
            })
            .await;
        assert!(cached.is_ok());
    }

    #[tokio::test]
    async fn successful_action_releases_reserved_capacity() {
        let middleware = IdempotencyMiddleware::new();
        for i in 0..1025 {
            let token = format!("success-{i}");
            assert!(
                middleware
                    .execute_with_idempotency(&token, "fp", || async {
                        Ok(IntentOutput::markdown("ok"))
                    })
                    .await
                    .is_ok()
            );
        }
        assert!(middleware.uncertain.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn active_actions_reserve_capacity() {
        let middleware = IdempotencyMiddleware::new();
        let mut tasks = Vec::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        for i in 0..1024 {
            let middleware = middleware.clone();
            let tx = tx.clone();
            tasks.push(tokio::spawn(async move {
                middleware
                    .execute_with_idempotency(&format!("active-{i}"), "fp", || async move {
                        tx.send(()).unwrap();
                        std::future::pending::<Result<IntentOutput, IntentError>>().await
                    })
                    .await
            }));
        }
        for _ in 0..1024 {
            rx.recv().await.unwrap();
        }
        let result = middleware
            .execute_with_idempotency("overflow", "fp", || async {
                panic!("capacity must be reserved before actions start")
            })
            .await;
        assert!(result.is_err());
        for task in tasks {
            task.abort();
            let _ = task.await;
        }
        let retry = middleware
            .execute_with_idempotency("active-0", "fp", || async {
                panic!("cancelled active operation must remain uncertain")
            })
            .await;
        assert!(retry.is_err());
    }

    #[tokio::test]
    async fn unknown_outcome_remains_blocked_after_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("idempotency.sqlite3");
        let store = StateStore::open(path.clone(), [7; 32]).unwrap();
        let middleware = IdempotencyMiddleware::with_journal(OperationJournal::new(store));
        let first = middleware
            .execute_with_idempotency("token", "fp", || async {
                Err(IntentError::InternalError("response lost".into()))
            })
            .await;
        assert!(first.is_err());

        let restarted_store = StateStore::open(path, [7; 32]).unwrap();
        let restarted = IdempotencyMiddleware::with_journal(OperationJournal::new(restarted_store));
        let called = std::sync::atomic::AtomicBool::new(false);
        let retry = restarted
            .execute_with_idempotency("token", "fp", || async {
                called.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate"))
            })
            .await;
        assert!(retry.is_err(), "an unknown outcome must remain blocked");
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn sqlite_process_death_after_upstream_acceptance_never_replays_write() {
        if let Ok(path) = std::env::var("DURABLE_JOURNAL_CRASH_CHILD_PATH") {
            let store = StateStore::open(path.clone().into(), [4; 32]).expect("open child journal");
            let middleware = IdempotencyMiddleware::with_journal(OperationJournal::new(store));
            let marker = format!("{path}.side-effect-applied");
            let _ = middleware
                .execute_with_idempotency("process-crash", "fingerprint", || async move {
                    std::fs::write(marker, b"upstream accepted").expect("write side-effect marker");
                    std::process::exit(77);
                })
                .await;
            panic!("child must terminate after the simulated upstream effect");
        }

        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("operations.sqlite3");
        let marker = dir.path().join("operations.sqlite3.side-effect-applied");
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "intents::idempotency::tests::sqlite_process_death_after_upstream_acceptance_never_replays_write",
                "--nocapture",
            ])
            .env("DURABLE_JOURNAL_CRASH_CHILD_PATH", &path)
            .output()
            .expect("run child test process");
        assert_eq!(output.status.code(), Some(77));
        assert_eq!(
            std::fs::read(marker).expect("side effect marker"),
            b"upstream accepted"
        );

        let store = StateStore::open(path, [4; 32]).expect("reopen journal");
        let middleware = IdempotencyMiddleware::with_journal(OperationJournal::new(store));
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let retry = middleware
            .execute_with_idempotency("process-crash", "fingerprint", || async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate write"))
            })
            .await;
        assert!(retry.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn sqlite_reservations_are_atomic_across_independent_connections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("operations.sqlite3");
        let first_store = StateStore::open(path.clone(), [9; 32]).unwrap();
        let second_store = StateStore::open(path, [9; 32]).unwrap();
        let first = IdempotencyMiddleware::with_journal(OperationJournal::new(first_store));
        let second = IdempotencyMiddleware::with_journal(OperationJournal::new(second_store));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let worker = {
            let calls = calls.clone();
            tokio::spawn(async move {
                first
                    .execute_with_idempotency("same", "fp", || async move {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        started_tx.send(()).unwrap();
                        std::future::pending::<Result<IntentOutput, IntentError>>().await
                    })
                    .await
            })
        };
        started_rx.await.unwrap();

        let retry = second
            .execute_with_idempotency("same", "fp", || async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate"))
            })
            .await;
        assert!(retry.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn sqlite_completed_result_is_private_to_its_original_owner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("operations.sqlite3");
        let first_store = StateStore::open(path.clone(), [3; 32]).unwrap();
        let first = IdempotencyMiddleware::with_journal(OperationJournal::new(first_store));
        let result = first
            .execute_for_owner("same", "fp", Some("owner-a"), || async {
                Ok(IntentOutput::markdown("private result"))
            })
            .await
            .unwrap();
        assert!(matches!(
            &result.content[0],
            crate::content::ContentBlock::Markdown { markdown } if markdown == "private result"
        ));

        let second_store = StateStore::open(path, [3; 32]).unwrap();
        let second = IdempotencyMiddleware::with_journal(OperationJournal::new(second_store));
        let replayed = second
            .execute_for_owner("same", "fp", Some("owner-a"), || async {
                panic!("a completed result must be replayed without executing again")
            })
            .await
            .unwrap();
        assert!(matches!(
            &replayed.content[0],
            crate::content::ContentBlock::Markdown { markdown } if markdown == "private result"
        ));

        let called = std::sync::atomic::AtomicBool::new(false);
        let replay = second
            .execute_for_owner("same", "fp", Some("owner-b"), || async {
                called.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(IntentOutput::markdown("leaked result"))
            })
            .await;
        assert!(matches!(replay, Err(IntentError::IdempotencyConflict(_))));
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancelled_sqlite_reservation_is_unknown_after_pending_reservation_finishes() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().join("operations.sqlite3"), [5; 32]).unwrap();
        let lock_store = store.clone();
        let (lock_started_tx, lock_started_rx) = tokio::sync::oneshot::channel();
        let (release_lock_tx, release_lock_rx) = std::sync::mpsc::channel();
        let lock_holder = tokio::spawn(async move {
            lock_store
                .run(move |_| {
                    lock_started_tx.send(()).unwrap();
                    release_lock_rx.recv().unwrap();
                    Ok(())
                })
                .await
        });
        lock_started_rx.await.unwrap();

        let middleware = IdempotencyMiddleware::with_journal(OperationJournal::new(store.clone()));
        let calls = Arc::new(AtomicUsize::new(0));
        let request = {
            let middleware = middleware.clone();
            let calls = calls.clone();
            tokio::spawn(async move {
                middleware
                    .execute_with_idempotency("same", "fp", || async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(IntentOutput::markdown("must not run"))
                    })
                    .await
            })
        };
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());

        release_lock_tx.send(()).unwrap();
        lock_holder.await.unwrap().unwrap();

        let mut state = String::from("missing");
        for _ in 0..100 {
            state = store
                .run(|connection| {
                    connection
                        .query_row(
                            "SELECT COALESCE((SELECT state FROM idempotency_operations LIMIT 1), 'missing')",
                            [],
                            |row| row.get::<_, String>(0),
                        )
                        .map_err(Into::into)
                })
                .await
                .unwrap();
            if state != "missing" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(state, "unknown");

        let retry = middleware
            .execute_with_idempotency("same", "fp", || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate"))
            })
            .await;
        assert!(retry.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn sqlite_reservation_is_active_while_the_action_runs() {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().join("operations.sqlite3"), [5; 32]).unwrap();
        let action_store = store.clone();
        let middleware = IdempotencyMiddleware::with_journal(OperationJournal::new(store));

        middleware
            .execute_with_idempotency("same", "fp", move || async move {
                let state = action_store
                    .run(|connection| {
                        connection
                            .query_row("SELECT state FROM idempotency_operations", [], |row| {
                                row.get::<_, String>(0)
                            })
                            .map_err(Into::into)
                    })
                    .await
                    .unwrap();
                assert_eq!(state, "active");
                Ok(IntentOutput::markdown("confirmed result"))
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn sqlite_action_error_transitions_active_reservation_to_unknown() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().join("operations.sqlite3"), [6; 32]).unwrap();
        let action_store = store.clone();
        let middleware = IdempotencyMiddleware::with_journal(OperationJournal::new(store.clone()));

        let result = middleware
            .execute_with_idempotency("failed", "fingerprint", move || async move {
                let state = action_store
                    .run(|connection| {
                        connection
                            .query_row("SELECT state FROM idempotency_operations", [], |row| {
                                row.get::<_, String>(0)
                            })
                            .map_err(Into::into)
                    })
                    .await
                    .expect("read reservation state");
                assert_eq!(state, "active");
                Err(IntentError::InternalError("upstream failed".to_string()))
            })
            .await;
        assert!(result.is_err());

        let state = store
            .run(|connection| {
                connection
                    .query_row("SELECT state FROM idempotency_operations", [], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(state, "unknown");
    }

    #[tokio::test]
    async fn failed_unknown_transition_still_returns_non_retryable_error() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().join("operations.sqlite3"), [7; 32]).unwrap();
        store
            .run(|connection| {
                connection.execute_batch(
                    "CREATE TRIGGER fail_unknown_transition BEFORE UPDATE OF state ON idempotency_operations WHEN NEW.state = 'unknown' BEGIN SELECT RAISE(FAIL, 'injected unknown transition failure'); END;",
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let middleware = IdempotencyMiddleware::with_journal(OperationJournal::new(store));

        let first = middleware
            .execute_with_idempotency("failed", "fingerprint", || async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(IntentError::InternalError(
                    "upstream response lost".to_string(),
                ))
            })
            .await
            .unwrap_err();
        assert!(matches!(
            first,
            IntentError::InternalError(message) if message.contains("Do not retry automatically")
        ));

        let retry = middleware
            .execute_with_idempotency("failed", "fingerprint", || async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(IntentOutput::markdown("must not run again"))
            })
            .await;
        assert!(retry.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_sqlite_reservation_never_starts_the_action() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("operations.sqlite3");
        let store = StateStore::open(path, [5; 32]).unwrap();
        store
            .run(|connection| {
                connection.execute_batch(
                    "CREATE TRIGGER fail_operation_reservation BEFORE INSERT ON idempotency_operations BEGIN SELECT RAISE(FAIL, 'injected reservation failure'); END;", 
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let middleware = IdempotencyMiddleware::with_journal(OperationJournal::new(store));
        let calls = std::sync::atomic::AtomicUsize::new(0);
        assert!(
            middleware
                .execute_with_idempotency("same", "fp", || async {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(IntentOutput::markdown("must not run"))
                })
                .await
                .is_err()
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_sqlite_completion_transition_keeps_operation_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("operations.sqlite3");
        let store = StateStore::open(path.clone(), [5; 32]).unwrap();
        store
            .run(|connection| {
                connection.execute_batch(
                    "CREATE TRIGGER fail_operation_completion BEFORE UPDATE OF state ON idempotency_operations WHEN NEW.state = 'completed' BEGIN SELECT RAISE(FAIL, 'injected transition failure'); END;", 
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let inspect_store = store.clone();
        let middleware = IdempotencyMiddleware::with_journal(OperationJournal::new(store));
        let calls = std::sync::atomic::AtomicUsize::new(0);
        assert!(
            middleware
                .execute_with_idempotency("same", "fp", || async {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(IntentOutput::markdown("confirmed result"))
                })
                .await
                .is_err()
        );
        let state = inspect_store
            .run(|connection| {
                connection
                    .query_row("SELECT state FROM idempotency_operations", [], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(state, "unknown");

        let restarted_store = StateStore::open(path, [5; 32]).unwrap();
        let restarted = IdempotencyMiddleware::with_journal(OperationJournal::new(restarted_store));
        let retry = restarted
            .execute_with_idempotency("same", "fp", || async {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate"))
            })
            .await;
        assert!(retry.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn sqlite_refuses_corrupt_database_without_overwriting_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old-idempotency.json");
        let legacy_data = b"legacy operation records";
        std::fs::write(&path, legacy_data).unwrap();

        assert!(StateStore::open(path.clone(), [1; 32]).is_err());
        assert_eq!(std::fs::read(path).unwrap(), legacy_data);
    }

    #[test]
    fn sqlite_refuses_unknown_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future-schema.sqlite3");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "user_version", 999_i64)
            .unwrap();
        drop(connection);

        assert!(StateStore::open(path, [1; 32]).is_err());
    }

    #[tokio::test]
    async fn action_error_leaves_unknown_outcome() {
        let middleware = IdempotencyMiddleware::new();
        let first = middleware
            .execute_with_idempotency("token", "fp", || async {
                Err(IntentError::InternalError("upstream failed".into()))
            })
            .await;
        assert!(
            matches!(first, Err(IntentError::InternalError(message)) if message == "upstream failed")
        );
        let retry = middleware
            .execute_with_idempotency("token", "fp", || async {
                panic!("must not repeat a possibly completed write")
            })
            .await;
        assert!(
            matches!(retry, Err(IntentError::InternalError(message)) if message.contains("unknown outcome"))
        );
    }

    #[tokio::test]
    async fn transport_failure_after_possible_dispatch_blocks_retry() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let middleware = IdempotencyMiddleware::new();
        let calls = AtomicUsize::new(0);
        let first = middleware
            .execute_with_idempotency("transport", "fp", || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(IntentError::api("connection reset after dispatch"))
            })
            .await;
        assert!(first.is_err());

        let retry = middleware
            .execute_with_idempotency("transport", "fp", || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate"))
            })
            .await;
        assert!(retry.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn response_decode_failure_after_possible_dispatch_blocks_retry() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let middleware = IdempotencyMiddleware::new();
        let calls = AtomicUsize::new(0);
        let first = middleware
            .execute_with_idempotency("decode", "fp", || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(IntentError::internal(
                    "upstream response could not be decoded after dispatch",
                ))
            })
            .await;
        assert!(first.is_err());

        let retry = middleware
            .execute_with_idempotency("decode", "fp", || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(IntentOutput::markdown("duplicate"))
            })
            .await;
        assert!(retry.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn validation_error_after_action_starts_remains_unknown() {
        let middleware = IdempotencyMiddleware::new();
        let first = middleware
            .execute_with_idempotency("token", "fp", || async {
                Err(IntentError::validation("possibly after a write"))
            })
            .await;
        assert!(matches!(first, Err(IntentError::ValidationError(_))));
        let retry = middleware
            .execute_with_idempotency("token", "fp", || async {
                panic!("a validation error alone does not prove no write occurred")
            })
            .await;
        assert!(
            matches!(retry, Err(IntentError::InternalError(message)) if message.contains("unknown outcome"))
        );
    }

    #[tokio::test]
    async fn action_panic_leaves_unknown_outcome() {
        let middleware = IdempotencyMiddleware::new();
        let worker = middleware.clone();
        let first = tokio::spawn(async move {
            worker
                .execute_with_idempotency("token", "fp", || async { panic!("action panic") })
                .await
        });
        assert!(first.await.unwrap_err().is_panic());
        let retry = tokio::time::timeout(
            Duration::from_secs(1),
            middleware.execute_with_idempotency("token", "fp", || async {
                panic!("must not repeat a possibly completed write")
            }),
        )
        .await
        .expect("retry must not block");
        assert!(
            matches!(retry, Err(IntentError::InternalError(message)) if message.contains("unknown outcome"))
        );
    }

    #[tokio::test]
    async fn test_cache() {
        let m = IdempotencyMiddleware::new();
        let out = IntentOutput::markdown("test");
        m.set("t", "fingerprint-a", &out).await;
        assert!(m.get("t", "fingerprint-a").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_rejects_different_fingerprint_for_same_token() {
        let m = IdempotencyMiddleware::new();
        let out = IntentOutput::markdown("test");
        m.set("t", "fingerprint-a", &out).await;

        let err = m.get("t", "fingerprint-b").await.unwrap_err();
        assert!(matches!(err, IntentError::IdempotencyConflict(_)));
    }
    #[test]
    fn test_token_gen() {
        let t1 = generate_idempotency_token("test", &["a"]);
        let t2 = generate_idempotency_token("test", &["a"]);
        assert_eq!(t1, t2);
    }

    #[tokio::test]
    async fn idempotency_survives_reload_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("idempotency.json");

        let m1 = IdempotencyMiddleware::with_file(path.clone());
        let out = IntentOutput::markdown("test result");

        // Verify in-memory works first
        m1.set("tok1", "fp-a", &out).await;
        let in_mem = m1.get("tok1", "fp-a").await.unwrap();
        assert!(in_mem.is_some(), "should be in memory after set");

        // Flush and check file
        m1.flush().await.unwrap();
        let file_content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !file_content.is_empty(),
            "file should not be empty after flush"
        );

        // Reload
        let m2 = IdempotencyMiddleware::with_file(path);
        let loaded = m2.get("tok1", "fp-a").await.unwrap();
        assert!(loaded.is_some(), "expected cached output after reload");
    }

    #[tokio::test]
    async fn expired_entries_not_restored_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("idempotency.json");

        let m = IdempotencyMiddleware::with_ttl_and_file(Duration::from_millis(1), path.clone());
        m.set("tok1", "fp-a", &IntentOutput::markdown("x")).await;
        m.flush().await.unwrap();

        tokio::time::sleep(Duration::from_millis(10)).await;

        let m2 = IdempotencyMiddleware::with_file(path);
        let loaded = m2.get("tok1", "fp-a").await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn flush_without_file_is_noop() {
        let m = IdempotencyMiddleware::new();
        m.set("tok1", "fp-a", &IntentOutput::markdown("x")).await;
        assert!(m.flush().await.is_ok());
    }

    #[tokio::test]
    async fn missing_file_returns_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let m = IdempotencyMiddleware::with_file(path);
        assert!(m.get("tok1", "fp-a").await.unwrap().is_none());
    }
}
