use super::types::IntentOutput;
use crate::state_store::{StateStore, StateStoreError};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::time::Duration;
use thiserror::Error;

const MAX_UNCERTAIN_OPERATIONS: i64 = 1024;
const DEFAULT_COMPLETED_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const SCOPE_HASH_DOMAIN: &[u8] = b"intervals-icu-mcp:operation-scope:v1\0";
const FINGERPRINT_HASH_DOMAIN: &[u8] = b"intervals-icu-mcp:operation-fingerprint:v1\0";
const OWNER_HASH_DOMAIN: &[u8] = b"intervals-icu-mcp:operation-owner:v1\0";

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
    CapacityExhausted,
    #[error("stored operation result is invalid")]
    InvalidResult,
}

pub(crate) enum Reservation {
    Reserved,
    Replay(IntentOutput),
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
    ) -> Result<Reservation, OperationJournalError> {
        let scope_hash = digest(SCOPE_HASH_DOMAIN, token.as_bytes());
        let fingerprint_hash = digest(FINGERPRINT_HASH_DOMAIN, fingerprint.as_bytes());
        let owner_hash = owner.map(|value| digest(OWNER_HASH_DOMAIN, value.as_bytes()).to_vec());
        let now = now_millis();
        let reserved_at = now;
        let max_uncertain = MAX_UNCERTAIN_OPERATIONS;

        let record = self
            .store
            .run(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
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
                        "unknown" => StoredReservation::Unknown,
                        "completed" => StoredReservation::Replay {
                            nonce: nonce.ok_or(StateStoreError::InvalidTransition)?,
                            ciphertext: ciphertext.ok_or(StateStoreError::InvalidTransition)?,
                        },
                        _ => return Err(StateStoreError::InvalidTransition),
                    };
                    transaction.commit()?;
                    return Ok(reservation);
                }

                let uncertain_count: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM idempotency_operations WHERE state = 'unknown'",
                    [],
                    |row| row.get(0),
                )?;
                if uncertain_count >= max_uncertain {
                    transaction.commit()?;
                    return Ok(StoredReservation::CapacityExhausted);
                }

                transaction.execute(
                    "INSERT INTO idempotency_operations
                     (scope_hash, fingerprint_hash, owner_hash, state, created_at, expires_at, result_nonce, result_ciphertext)
                     VALUES (?1, ?2, ?3, 'unknown', ?4, NULL, NULL, NULL)",
                    params![
                        scope_hash.as_slice(),
                        fingerprint_hash.as_slice(),
                        owner_hash,
                        reserved_at,
                    ],
                )?;
                transaction.commit()?;
                Ok(StoredReservation::Reserved)
            })
            .await?;

        match record {
            StoredReservation::Reserved => Ok(Reservation::Reserved),
            StoredReservation::Unknown => Err(OperationJournalError::UnknownOutcome),
            StoredReservation::OwnerMismatch => Err(OperationJournalError::OwnerMismatch),
            StoredReservation::FingerprintMismatch => {
                Err(OperationJournalError::FingerprintMismatch)
            }
            StoredReservation::CapacityExhausted => Err(OperationJournalError::CapacityExhausted),
            StoredReservation::Replay { nonce, ciphertext } => {
                let plaintext = self
                    .store
                    .decrypt(&scope_hash, &nonce, &ciphertext)
                    .map_err(OperationJournalError::Store)?;
                let output = serde_json::from_slice(&plaintext)
                    .map_err(|_| OperationJournalError::InvalidResult)?;
                Ok(Reservation::Replay(output))
            }
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
        let plaintext =
            serde_json::to_vec(output).map_err(|_| OperationJournalError::InvalidResult)?;
        let (nonce, ciphertext) = self
            .store
            .encrypt(&scope_hash, &plaintext)
            .map_err(OperationJournalError::Store)?;
        let completed_at = now_millis();
        let expires_at = completed_at.saturating_add(duration_millis(self.completed_ttl));

        self.store
            .run(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let updated = transaction.execute(
                    "UPDATE idempotency_operations
                     SET state = 'completed', expires_at = ?1, result_nonce = ?2, result_ciphertext = ?3
                     WHERE scope_hash = ?4 AND fingerprint_hash = ?5 AND owner_hash IS ?6 AND state = 'unknown'",
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
            .await?;
        Ok(())
    }
}

#[derive(Debug)]
enum StoredReservation {
    Reserved,
    Unknown,
    OwnerMismatch,
    FingerprintMismatch,
    CapacityExhausted,
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
