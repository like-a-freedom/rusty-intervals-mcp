use super::types::IntentOutput;
use crate::metrics;
use crate::state_store::{StateStore, StateStoreError};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::time::Duration;
use thiserror::Error;

const MAX_GLOBAL_UNRESOLVED_OPERATIONS: i64 = 1024;
const MAX_ACTIVE_PER_TENANT: i64 = 16;
const MAX_UNKNOWN_PER_TENANT: i64 = 128;
const MAX_COMPLETED_PER_TENANT: i64 = 2048;
const DEFAULT_COMPLETED_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const SCOPE_HASH_DOMAIN: &[u8] = b"intervals-icu-mcp:operation-scope:v1\0";
const TENANT_HASH_DOMAIN: &[u8] = b"intervals-icu-mcp:operation-tenant:v1\0";
const FINGERPRINT_HASH_DOMAIN: &[u8] = b"intervals-icu-mcp:operation-fingerprint:v1\0";
const OWNER_HASH_DOMAIN: &[u8] = b"intervals-icu-mcp:operation-owner:v1\0";

#[derive(Debug, Clone, Copy)]
pub(crate) enum QuotaKind {
    ActivePerTenant,
    UnknownPerTenant,
    UnresolvedGlobal,
    CompletedPerTenant,
}

impl QuotaKind {
    const fn metric_label(self) -> &'static str {
        match self {
            Self::ActivePerTenant => "active_per_tenant",
            Self::UnknownPerTenant => "unknown_per_tenant",
            Self::UnresolvedGlobal => "unresolved_global",
            Self::CompletedPerTenant => "completed_per_tenant",
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum OperationJournalError {
    #[error("operation storage failed")]
    Store(#[from] StateStoreError),
    #[error("idempotency token is unavailable")]
    OwnerMismatch,
    #[error("idempotency token was already used for a different request")]
    FingerprintMismatch,
    #[error("the previous operation has an unknown outcome")]
    UnknownOutcome,
    #[error("operation journal capacity is exhausted")]
    CapacityExhausted(QuotaKind),
    #[error("stored operation result is invalid")]
    InvalidResult,
}

pub(crate) enum Reservation {
    Reserved,
    Replay(Box<IntentOutput>),
}

#[derive(Clone)]
pub(crate) struct OperationJournal {
    store: StateStore,
    completed_ttl: Duration,
}

impl OperationJournal {
    pub(crate) fn new(store: StateStore) -> Self {
        Self {
            store,
            completed_ttl: DEFAULT_COMPLETED_TTL,
        }
    }

    pub(crate) async fn reserve(
        &self,
        token: &str,
        fingerprint: &str,
        owner: Option<&str>,
        tenant_scope: Option<&str>,
    ) -> Result<Reservation, OperationJournalError> {
        let scope_hash = digest(SCOPE_HASH_DOMAIN, token.as_bytes());
        let fingerprint_hash = digest(FINGERPRINT_HASH_DOMAIN, fingerprint.as_bytes());
        let owner_hash = owner.map(|value| digest(OWNER_HASH_DOMAIN, value.as_bytes()).to_vec());
        let tenant_hash =
            tenant_scope.map(|value| digest(TENANT_HASH_DOMAIN, value.as_bytes()).to_vec());
        let now = now_millis();
        let reserved_at = now;

        let record = self
            .store
            .run(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                transaction.execute(
                    "DELETE FROM idempotency_operations WHERE state = 'completed' AND expires_at <= ?1",
                    params![now],
                )?;
                let mut record = transaction
                    .query_row(
                        "SELECT fingerprint_hash, owner_hash, state, expires_at, result_nonce, result_ciphertext
                         FROM idempotency_operations WHERE scope_hash = ?1",
                        params![scope_hash.as_slice()],
                        |row| {
                            Ok((
                                row.get::<_, Vec<u8>>(0)?,
                                row.get::<_, Option<Vec<u8>>>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<i64>>(3)?,
                                row.get::<_, Option<Vec<u8>>>(4)?,
                                row.get::<_, Option<Vec<u8>>>(5)?,
                            ))
                        },
                    )
                    .optional()?;

                if record.as_ref().is_some_and(|(_, _, state, expires_at, _, _)| {
                    state == "completed" && expires_at.is_some_and(|expiry| expiry <= now)
                }) {
                    transaction.execute(
                        "DELETE FROM idempotency_operations WHERE scope_hash = ?1",
                        params![scope_hash.as_slice()],
                    )?;
                    record = None;
                }

                if let Some((stored_fingerprint, stored_owner, state, _, nonce, ciphertext)) = record
                {
                    if stored_owner != owner_hash {
                        transaction.commit()?;
                        return Ok(StoredReservation::OwnerMismatch);
                    }
                    if stored_fingerprint != fingerprint_hash {
                        transaction.commit()?;
                        return Ok(StoredReservation::FingerprintMismatch);
                    }
                    let reservation = match state.as_str() {
                        "active" | "unknown" => StoredReservation::Unknown,
                        "completed" => StoredReservation::Replay {
                            nonce: nonce.ok_or(StateStoreError::InvalidTransition)?,
                            ciphertext: ciphertext.ok_or(StateStoreError::InvalidTransition)?,
                        },
                        _ => return Err(StateStoreError::InvalidTransition),
                    };
                    transaction.commit()?;
                    return Ok(reservation);
                }

                let tenant_hash_ref = tenant_hash.as_deref();
                let (active_count, unknown_count, completed_capacity_count): (i64, i64, i64) =
                    transaction.query_row(
                        "SELECT
                            (SELECT COUNT(*) FROM idempotency_operations
                             WHERE state = 'active' AND tenant_hash IS ?1),
                            (SELECT COUNT(*) FROM idempotency_operations
                             WHERE state = 'unknown' AND tenant_hash IS ?1),
                            (SELECT COUNT(*) FROM idempotency_operations
                             WHERE tenant_hash IS ?1 AND
                               (state = 'active' OR (state = 'completed' AND expires_at > ?2)))",
                        params![tenant_hash_ref, now],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?;
                if active_count >= MAX_ACTIVE_PER_TENANT {
                    transaction.commit()?;
                    return Ok(StoredReservation::CapacityExhausted(
                        QuotaKind::ActivePerTenant,
                    ));
                }
                if unknown_count >= MAX_UNKNOWN_PER_TENANT {
                    transaction.commit()?;
                    return Ok(StoredReservation::CapacityExhausted(
                        QuotaKind::UnknownPerTenant,
                    ));
                }
                if completed_capacity_count >= MAX_COMPLETED_PER_TENANT {
                    transaction.commit()?;
                    return Ok(StoredReservation::CapacityExhausted(
                        QuotaKind::CompletedPerTenant,
                    ));
                }

                let unresolved_count: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM idempotency_operations WHERE state IN ('active', 'unknown')",
                    [],
                    |row| row.get(0),
                )?;
                if unresolved_count >= MAX_GLOBAL_UNRESOLVED_OPERATIONS {
                    transaction.commit()?;
                    return Ok(StoredReservation::CapacityExhausted(
                        QuotaKind::UnresolvedGlobal,
                    ));
                }

                transaction.execute(
                    "INSERT INTO idempotency_operations
                     (scope_hash, tenant_hash, fingerprint_hash, owner_hash, state, created_at, expires_at, result_nonce, result_ciphertext)
                     VALUES (?1, ?2, ?3, ?4, 'active', ?5, NULL, NULL, NULL)",
                    params![
                        scope_hash.as_slice(),
                        tenant_hash,
                        fingerprint_hash.as_slice(),
                        owner_hash,
                        reserved_at,
                    ],
                )?;
                transaction.commit()?;
                Ok(StoredReservation::Reserved)
            })
            .await
            .map_err(|error| {
                metrics::record_operation_journal_error();
                OperationJournalError::Store(error)
            })?;

        match record {
            StoredReservation::Reserved => Ok(Reservation::Reserved),
            StoredReservation::Unknown => Err(OperationJournalError::UnknownOutcome),
            StoredReservation::OwnerMismatch => Err(OperationJournalError::OwnerMismatch),
            StoredReservation::FingerprintMismatch => {
                Err(OperationJournalError::FingerprintMismatch)
            }
            StoredReservation::CapacityExhausted(kind) => {
                metrics::record_operation_journal_quota_rejected(kind.metric_label());
                Err(OperationJournalError::CapacityExhausted(kind))
            }
            StoredReservation::Replay { nonce, ciphertext } => {
                let plaintext = self
                    .store
                    .decrypt(&scope_hash, &nonce, &ciphertext)
                    .map_err(|error| {
                        metrics::record_operation_journal_error();
                        OperationJournalError::Store(error)
                    })?;
                let output = serde_json::from_slice(&plaintext).map_err(|_| {
                    metrics::record_operation_journal_error();
                    OperationJournalError::InvalidResult
                })?;
                Ok(Reservation::Replay(Box::new(output)))
            }
        }
    }

    pub(crate) async fn mark_unknown(
        &self,
        token: &str,
        fingerprint: &str,
        owner: Option<&str>,
    ) -> Result<(), OperationJournalError> {
        let scope_hash = digest(SCOPE_HASH_DOMAIN, token.as_bytes());
        let fingerprint_hash = digest(FINGERPRINT_HASH_DOMAIN, fingerprint.as_bytes());
        let owner_hash = owner.map(|value| digest(OWNER_HASH_DOMAIN, value.as_bytes()).to_vec());

        self.store
            .run(move |connection| {
                let updated = connection.execute(
                    "UPDATE idempotency_operations SET state = 'unknown'
                     WHERE scope_hash = ?1 AND fingerprint_hash = ?2 AND owner_hash IS ?3 AND state = 'active'",
                    params![scope_hash.as_slice(), fingerprint_hash.as_slice(), owner_hash],
                )?;
                if updated != 1 {
                    return Err(StateStoreError::InvalidTransition);
                }
                Ok(())
            })
            .await
            .map_err(|error| {
                metrics::record_operation_journal_error();
                OperationJournalError::Store(error)
            })?;
        self.refresh_unknown_metrics().await;
        Ok(())
    }

    pub(crate) async fn refresh_unknown_metrics(&self) {
        let snapshot = self
            .store
            .run(|connection| {
                connection
                    .query_row(
                        "SELECT COUNT(*), MIN(created_at) FROM idempotency_operations WHERE state = 'unknown'",
                        [],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
                    )
                    .map_err(Into::into)
            })
            .await;

        match snapshot {
            Ok((count, oldest_created_at)) => {
                let oldest_age_seconds = oldest_created_at
                    .map(|created_at| now_millis().saturating_sub(created_at).max(0) as u64 / 1_000)
                    .unwrap_or(0);
                metrics::set_operation_journal_unknown_snapshot(
                    u64::try_from(count).unwrap_or_default(),
                    oldest_age_seconds,
                );
            }
            Err(_) => metrics::record_operation_journal_error(),
        }
    }

    pub(crate) async fn complete(
        &self,
        token: &str,
        fingerprint: &str,
        owner: Option<&str>,
        output: &IntentOutput,
    ) -> Result<(), OperationJournalError> {
        let scope_hash = digest(SCOPE_HASH_DOMAIN, token.as_bytes());
        let fingerprint_hash = digest(FINGERPRINT_HASH_DOMAIN, fingerprint.as_bytes());
        let owner_hash = owner.map(|value| digest(OWNER_HASH_DOMAIN, value.as_bytes()).to_vec());
        let plaintext = serde_json::to_vec(output).map_err(|_| {
            metrics::record_operation_journal_error();
            OperationJournalError::InvalidResult
        })?;
        let (nonce, ciphertext) = self
            .store
            .encrypt(&scope_hash, &plaintext)
            .map_err(|error| {
                metrics::record_operation_journal_error();
                OperationJournalError::Store(error)
            })?;
        let completed_at = now_millis();
        let expires_at = completed_at.saturating_add(duration_millis(self.completed_ttl));

        self.store
            .run(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let updated = transaction.execute(
                    "UPDATE idempotency_operations
                     SET state = 'completed', expires_at = ?1, result_nonce = ?2, result_ciphertext = ?3
                     WHERE scope_hash = ?4 AND fingerprint_hash = ?5 AND owner_hash IS ?6 AND state = 'active'",
                    params![
                        expires_at,
                        nonce,
                        ciphertext,
                        scope_hash.as_slice(),
                        fingerprint_hash.as_slice(),
                        owner_hash,
                    ],
                )?;
                if updated != 1 {
                    return Err(StateStoreError::InvalidTransition);
                }
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|error| {
                metrics::record_operation_journal_error();
                OperationJournalError::Store(error)
            })?;
        Ok(())
    }
}

#[derive(Debug)]
enum StoredReservation {
    Reserved,
    Unknown,
    OwnerMismatch,
    FingerprintMismatch,
    CapacityExhausted(QuotaKind),
    Replay { nonce: Vec<u8>, ciphertext: Vec<u8> },
}

fn digest(domain: &[u8], value: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(value);
    hasher.finalize().into()
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn duration_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn reservation_does_not_embed_completed_output() {
        assert!(std::mem::size_of::<Reservation>() < std::mem::size_of::<IntentOutput>());
    }

    #[tokio::test]
    async fn global_unresolved_limit_counts_legacy_rows_without_tenant_hash() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().join("operations.sqlite3"), [3; 32]).unwrap();
        let seed_store = store.clone();
        seed_store
            .run(|connection| {
                let transaction = connection.transaction()?;
                for index in 0..MAX_GLOBAL_UNRESOLVED_OPERATIONS {
                    let mut scope_hash = [0u8; 32];
                    scope_hash[..8].copy_from_slice(&(index as u64).to_be_bytes());
                    transaction.execute(
                        "INSERT INTO idempotency_operations
                         (scope_hash, fingerprint_hash, state, created_at)
                         VALUES (?1, ?2, 'unknown', 0)",
                        params![scope_hash.as_slice(), [0u8; 32].as_slice()],
                    )?;
                }
                transaction.commit()?;
                Ok(())
            })
            .await
            .unwrap();

        let journal = OperationJournal::new(store);
        let result = journal
            .reserve("new-operation", "fingerprint", None, Some("new-tenant"))
            .await;

        assert!(matches!(
            result,
            Err(OperationJournalError::CapacityExhausted(
                QuotaKind::UnresolvedGlobal
            ))
        ));
    }
}
