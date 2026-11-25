//! Server-side token registry — the authoritative record of every token the
//! server has issued.
//!
//! The registry is keyed by `jti` and is the single source of truth for the two
//! facts the JWT deliberately does not carry: a token's revocation state and the
//! athlete's upstream credential (stored encrypted at rest). The auth middleware
//! resolves credentials through it; the web UI manages tokens through it. See
//! ADR-0011.

use crate::state_store::{StateStore, StateStoreError};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// One issued token: the aggregate the registry owns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRecord {
    pub jti: String,
    pub athlete_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub revoked: bool,
    /// The athlete's upstream API key, encrypted with the master-key-derived
    /// AES-256-GCM key. Never embedded in the JWT. `None` for legacy records
    /// whose credential is still carried in the token claims.
    #[serde(default)]
    pub encrypted_api_key: Option<String>,
}

/// Token-registry operations can fail when the durable store is unavailable.
#[derive(Debug, Error)]
pub enum TokenRegistryError {
    #[error("token registry storage is unavailable")]
    Storage,
}

/// Authoritative registry of issued tokens.
///
/// SQLite-backed registries query the database for every operation. The explicit
/// in-memory constructor is intended for tests and non-durable embedding only.
#[derive(Clone)]
pub struct TokenRegistry {
    store: Option<StateStore>,
    records: Option<Arc<RwLock<HashMap<String, TokenRecord>>>>,
}

impl TokenRegistry {
    /// Use the shared SQLite state store as the registry's source of truth.
    pub(crate) fn from_store(store: StateStore) -> Self {
        Self {
            store: Some(store),
            records: None,
        }
    }

    /// Create an explicitly non-durable registry for tests or embedding.
    pub fn in_memory() -> Self {
        Self {
            store: None,
            records: Some(Arc::default()),
        }
    }

    /// Whether recorded tokens survive a process restart.
    pub fn is_persistent(&self) -> bool {
        self.store.is_some()
    }

    /// Record a newly issued token. An existing JTI is never overwritten.
    pub async fn record(&self, record: TokenRecord) -> Result<(), TokenRegistryError> {
        if let Some(store) = &self.store {
            store
                .run(move |connection| {
                    connection.execute(
                        "INSERT INTO token_registry
                         (jti, athlete_id, issued_at, expires_at, revoked, encrypted_api_key)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            record.jti,
                            record.athlete_id,
                            record.issued_at,
                            record.expires_at,
                            i64::from(record.revoked),
                            record.encrypted_api_key,
                        ],
                    )?;
                    Ok(())
                })
                .await
                .map_err(|_| TokenRegistryError::Storage)?;
        } else if let Some(records) = &self.records {
            match records.write().await.entry(record.jti.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(record);
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    return Err(TokenRegistryError::Storage);
                }
            }
        }
        Ok(())
    }

    /// Look a token up by its `jti`.
    pub async fn find(&self, jti: &str) -> Result<Option<TokenRecord>, TokenRegistryError> {
        if let Some(store) = &self.store {
            let jti = jti.to_owned();
            return store
                .run(move |connection| {
                    connection
                        .query_row(
                            "SELECT jti, athlete_id, issued_at, expires_at, revoked, encrypted_api_key
                             FROM token_registry WHERE jti = ?1",
                            params![jti],
                            token_record_from_row,
                        )
                        .optional()
                        .map_err(StateStoreError::from)
                })
                .await
                .map_err(|_| TokenRegistryError::Storage);
        }
        if let Some(records) = &self.records {
            return Ok(records.read().await.get(jti).cloned());
        }
        Ok(None)
    }

    /// Replace only the credential of a matching, non-revoked record.
    pub(crate) async fn update_credential(
        &self,
        jti: String,
        athlete_id: String,
        encrypted_api_key: String,
    ) -> Result<bool, TokenRegistryError> {
        if let Some(store) = &self.store {
            return store
                .run(move |connection| {
                    let updated = connection.execute(
                        "UPDATE token_registry SET encrypted_api_key = ?1
                         WHERE jti = ?2 AND athlete_id = ?3 AND revoked = 0",
                        params![encrypted_api_key, jti, athlete_id],
                    )?;
                    Ok(updated == 1)
                })
                .await
                .map_err(|_| TokenRegistryError::Storage);
        }
        if let Some(records) = &self.records {
            let mut records = records.write().await;
            if let Some(record) = records.get_mut(&jti)
                && record.athlete_id == athlete_id
                && !record.revoked
            {
                record.encrypted_api_key = Some(encrypted_api_key);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Mark a token revoked. Returns `false` when the `jti` is unknown.
    pub async fn revoke(&self, jti: &str) -> Result<bool, TokenRegistryError> {
        if let Some(store) = &self.store {
            let jti = jti.to_owned();
            return store
                .run(move |connection| {
                    let updated = connection.execute(
                        "UPDATE token_registry SET revoked = 1 WHERE jti = ?1",
                        params![jti],
                    )?;
                    Ok(updated == 1)
                })
                .await
                .map_err(|_| TokenRegistryError::Storage);
        }
        if let Some(records) = &self.records {
            let mut records = records.write().await;
            if let Some(record) = records.get_mut(jti) {
                record.revoked = true;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Every record, oldest first.
    pub async fn all(&self) -> Result<Vec<TokenRecord>, TokenRegistryError> {
        self.list(None).await
    }

    /// The records for one athlete, oldest first.
    pub async fn list_for_athlete(
        &self,
        athlete_id: &str,
    ) -> Result<Vec<TokenRecord>, TokenRegistryError> {
        self.list(Some(athlete_id)).await
    }

    async fn list(&self, athlete_id: Option<&str>) -> Result<Vec<TokenRecord>, TokenRegistryError> {
        if let Some(store) = &self.store {
            let athlete_id = athlete_id.map(str::to_owned);
            return store
                .run(move |connection| {
                    let mut records = Vec::new();
                    if let Some(athlete_id) = athlete_id {
                        let mut statement = connection.prepare(
                            "SELECT jti, athlete_id, issued_at, expires_at, revoked, encrypted_api_key
                             FROM token_registry WHERE athlete_id = ?1 ORDER BY issued_at ASC",
                        )?;
                        let rows = statement.query_map(params![athlete_id], token_record_from_row)?;
                        for row in rows {
                            records.push(row?);
                        }
                    } else {
                        let mut statement = connection.prepare(
                            "SELECT jti, athlete_id, issued_at, expires_at, revoked, encrypted_api_key
                             FROM token_registry ORDER BY issued_at ASC",
                        )?;
                        let rows = statement.query_map([], token_record_from_row)?;
                        for row in rows {
                            records.push(row?);
                        }
                    }
                    Ok(records)
                })
                .await
                .map_err(|_| TokenRegistryError::Storage);
        }
        let Some(records) = &self.records else {
            return Ok(Vec::new());
        };
        let mut selected: Vec<_> = records
            .read()
            .await
            .values()
            .filter(|record| athlete_id.is_none_or(|id| record.athlete_id == id))
            .cloned()
            .collect();
        selected.sort_by(|a, b| a.issued_at.cmp(&b.issued_at));
        Ok(selected)
    }
}

fn token_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TokenRecord> {
    Ok(TokenRecord {
        jti: row.get(0)?,
        athlete_id: row.get(1)?,
        issued_at: row.get(2)?,
        expires_at: row.get(3)?,
        revoked: row.get::<_, i64>(4)? != 0,
        encrypted_api_key: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(jti: &str, athlete_id: &str, issued_at: &str, revoked: bool) -> TokenRecord {
        TokenRecord {
            jti: jti.to_string(),
            athlete_id: athlete_id.to_string(),
            issued_at: issued_at.to_string(),
            expires_at: "2026-02-01T00:00:00Z".to_string(),
            revoked,
            encrypted_api_key: Some(format!("encrypted-{jti}")),
        }
    }

    #[tokio::test]
    async fn record_then_find_is_keyed_by_jti() {
        let registry = TokenRegistry::in_memory();
        registry
            .record(record("jti-1", "athlete-a", "2026-01-01T00:00:00Z", false))
            .await
            .expect("record token");

        let found = registry
            .find("jti-1")
            .await
            .expect("lookup succeeds")
            .expect("record present");
        assert_eq!(found.athlete_id, "athlete-a");
        assert_eq!(found.encrypted_api_key.as_deref(), Some("encrypted-jti-1"));
        assert!(
            registry
                .find("missing")
                .await
                .expect("lookup succeeds")
                .is_none()
        );
    }

    #[tokio::test]
    async fn in_memory_issuance_never_replaces_an_existing_jti() {
        let registry = TokenRegistry::in_memory();
        let original = record("same-jti", "athlete-a", "2026-01-01T00:00:00Z", false);
        let replacement = record("same-jti", "athlete-b", "2026-01-02T00:00:00Z", false);
        registry
            .record(original.clone())
            .await
            .expect("record token");

        assert!(registry.record(replacement.clone()).await.is_err());
        assert_eq!(
            registry.find("same-jti").await.unwrap(),
            Some(original.clone())
        );

        assert!(registry.revoke("same-jti").await.expect("revoke token"));
        assert!(registry.record(replacement).await.is_err());
        assert_eq!(
            registry.find("same-jti").await.unwrap(),
            Some(TokenRecord {
                revoked: true,
                ..original
            })
        );
    }

    #[tokio::test]
    async fn revoke_marks_known_and_reports_unknown() {
        let registry = TokenRegistry::in_memory();
        registry
            .record(record("jti-1", "athlete-a", "2026-01-01T00:00:00Z", false))
            .await
            .expect("record token");

        assert!(registry.revoke("jti-1").await.expect("revoke token"));
        assert!(
            registry
                .find("jti-1")
                .await
                .expect("lookup succeeds")
                .expect("record")
                .revoked
        );
        assert!(
            !registry
                .revoke("missing")
                .await
                .expect("revoke missing token")
        );
    }

    #[tokio::test]
    async fn lists_are_scoped_and_ordered_by_issue_time() {
        let registry = TokenRegistry::in_memory();
        registry
            .record(record("late", "athlete-a", "2026-01-03T00:00:00Z", false))
            .await
            .expect("record late token");
        registry
            .record(record("early", "athlete-a", "2026-01-01T00:00:00Z", false))
            .await
            .expect("record early token");
        registry
            .record(record("other", "athlete-b", "2026-01-02T00:00:00Z", false))
            .await
            .expect("record other token");

        let athlete_a = registry
            .list_for_athlete("athlete-a")
            .await
            .expect("list athlete tokens");
        assert_eq!(
            athlete_a.iter().map(|r| r.jti.as_str()).collect::<Vec<_>>(),
            vec!["early", "late"]
        );

        let all = registry.all().await.expect("list all tokens");
        assert_eq!(
            all.iter().map(|r| r.jti.as_str()).collect::<Vec<_>>(),
            vec!["early", "other", "late"]
        );
    }

    #[tokio::test]
    async fn concurrent_insertions_of_the_same_jti_have_one_winner() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store =
            StateStore::open(dir.path().join("state.sqlite3"), [6; 32]).expect("open state store");
        let registry = TokenRegistry::from_store(store);
        let first = registry.clone();
        let second = registry.clone();
        let gate = Arc::new(tokio::sync::Barrier::new(3));
        let first_gate = gate.clone();
        let second_gate = gate.clone();

        let first_insert = tokio::spawn(async move {
            first_gate.wait().await;
            first
                .record(record(
                    "racing-jti",
                    "athlete-a",
                    "2026-01-01T00:00:00Z",
                    false,
                ))
                .await
        });
        let second_insert = tokio::spawn(async move {
            second_gate.wait().await;
            second
                .record(record(
                    "racing-jti",
                    "athlete-a",
                    "2026-01-02T00:00:00Z",
                    false,
                ))
                .await
        });
        gate.wait().await;

        let results = [
            first_insert.await.expect("first task"),
            second_insert.await.expect("second task"),
        ];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        assert_eq!(registry.all().await.expect("list registry").len(), 1);
    }

    #[tokio::test]
    async fn cloned_sqlite_registries_observe_revocation_without_a_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store =
            StateStore::open(dir.path().join("state.sqlite3"), [6; 32]).expect("open state store");
        let first = TokenRegistry::from_store(store.clone());
        let second = TokenRegistry::from_store(store);
        first
            .record(record(
                "shared-jti",
                "athlete-a",
                "2026-01-01T00:00:00Z",
                false,
            ))
            .await
            .expect("record token");

        assert!(second.revoke("shared-jti").await.expect("revoke token"));
        assert!(
            first
                .find("shared-jti")
                .await
                .expect("read current token state")
                .expect("token exists")
                .revoked
        );
    }

    #[tokio::test]
    async fn persists_and_reloads_from_sqlite() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested/tokens.sqlite3");

        {
            let store = StateStore::open(path.clone(), [6; 32]).expect("open state store");
            let registry = TokenRegistry::from_store(store);
            registry
                .record(record("jti-1", "athlete-a", "2026-01-01T00:00:00Z", false))
                .await
                .expect("persist token");
            assert!(registry.revoke("jti-1").await.expect("persist revocation"));
        }

        let store = StateStore::open(path, [6; 32]).expect("reopen state store");
        let reloaded = TokenRegistry::from_store(store);
        let found = reloaded
            .find("jti-1")
            .await
            .expect("lookup succeeds")
            .expect("record persisted");
        assert_eq!(found.athlete_id, "athlete-a");
        assert!(found.revoked);
        assert!(reloaded.is_persistent());
    }

    #[tokio::test]
    async fn new_sqlite_registry_starts_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store =
            StateStore::open(dir.path().join("state.sqlite3"), [6; 32]).expect("open state store");
        let registry = TokenRegistry::from_store(store);
        assert!(registry.all().await.expect("list registry").is_empty());
    }

    #[test]
    fn in_memory_registry_is_not_persistent() {
        assert!(!TokenRegistry::in_memory().is_persistent());
    }

    #[test]
    fn legacy_records_without_api_key_deserialize() {
        // Legacy JWT claims may still supply the credential when no registry row exists.
        let json = r#"[{"jti":"old","athlete_id":"a","issued_at":"2026-01-01T00:00:00Z","expires_at":"2026-02-01T00:00:00Z","revoked":false}]"#;
        let records: Vec<TokenRecord> = serde_json::from_str(json).expect("legacy json parses");
        assert_eq!(records.len(), 1);
        assert!(records[0].encrypted_api_key.is_none());
    }

    #[test]
    fn existing_json_registry_file_is_not_overwritten_as_sqlite() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("tokens.json");
        let legacy_json = b"[{\"jti\":\"old\"}]";
        std::fs::write(&path, legacy_json).expect("write legacy registry");

        assert!(StateStore::open(path.clone(), [6; 32]).is_err());
        assert_eq!(
            std::fs::read(path).expect("read legacy registry"),
            legacy_json
        );
    }

    #[tokio::test]
    async fn sqlite_insert_failure_is_reported_without_creating_a_token() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store =
            StateStore::open(dir.path().join("state.sqlite3"), [6; 32]).expect("open state store");
        let registry = TokenRegistry::from_store(store.clone());
        store
            .run(|connection| {
                connection.execute_batch(
                    "CREATE TRIGGER reject_token_insert BEFORE INSERT ON token_registry
                     BEGIN SELECT RAISE(ABORT, 'injected insert failure'); END;",
                )?;
                Ok(())
            })
            .await
            .expect("install failure trigger");

        assert!(matches!(
            registry
                .record(record(
                    "jti-fail",
                    "athlete-a",
                    "2026-01-01T00:00:00Z",
                    false
                ))
                .await,
            Err(TokenRegistryError::Storage)
        ));
        assert!(
            registry
                .find("jti-fail")
                .await
                .expect("read after failed insert")
                .is_none()
        );
    }

    #[tokio::test]
    async fn sqlite_revoke_failure_is_reported_without_changing_token_state() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store =
            StateStore::open(dir.path().join("state.sqlite3"), [6; 32]).expect("open state store");
        let registry = TokenRegistry::from_store(store.clone());
        registry
            .record(record(
                "jti-revoke",
                "athlete-a",
                "2026-01-01T00:00:00Z",
                false,
            ))
            .await
            .expect("record token");
        store
            .run(|connection| {
                connection.execute_batch(
                    "CREATE TRIGGER reject_token_revoke BEFORE UPDATE OF revoked ON token_registry
                     BEGIN SELECT RAISE(ABORT, 'injected revoke failure'); END;",
                )?;
                Ok(())
            })
            .await
            .expect("install failure trigger");

        assert!(matches!(
            registry.revoke("jti-revoke").await,
            Err(TokenRegistryError::Storage)
        ));
        assert!(
            !registry
                .find("jti-revoke")
                .await
                .expect("read token after failed revoke")
                .expect("token remains present")
                .revoked
        );
    }

    #[tokio::test]
    async fn sqlite_rejects_reusing_a_revoked_jti_without_reviving_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store =
            StateStore::open(dir.path().join("state.sqlite3"), [6; 32]).expect("open state store");
        let registry = TokenRegistry::from_store(store);
        registry
            .record(record(
                "same-jti",
                "athlete-a",
                "2026-01-01T00:00:00Z",
                true,
            ))
            .await
            .expect("record revoked token");

        assert!(matches!(
            registry
                .record(record(
                    "same-jti",
                    "athlete-a",
                    "2026-01-02T00:00:00Z",
                    false
                ))
                .await,
            Err(TokenRegistryError::Storage)
        ));
        assert!(
            registry
                .find("same-jti")
                .await
                .expect("lookup token")
                .expect("revoked token remains present")
                .revoked
        );
    }

    #[tokio::test]
    async fn sqlite_lookup_failure_is_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store =
            StateStore::open(dir.path().join("state.sqlite3"), [6; 32]).expect("open state store");
        let registry = TokenRegistry::from_store(store.clone());
        store
            .run(|connection| {
                connection.execute_batch("DROP TABLE token_registry;")?;
                Ok(())
            })
            .await
            .expect("remove token table to inject read failure");

        assert!(matches!(
            registry.find("jti-read-fail").await,
            Err(TokenRegistryError::Storage)
        ));
    }
}
