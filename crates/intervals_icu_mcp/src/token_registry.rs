//! Server-side token registry — the authoritative record of every token the
//! server has issued.
//!
//! The registry is keyed by `jti` and is the single source of truth for the two
//! facts the JWT deliberately does not carry: a token's revocation state and the
//! athlete's upstream credential (stored encrypted at rest). The auth middleware
//! resolves credentials through it; the web UI manages tokens through it. See
//! ADR-0011.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    /// AES-256-GCM key. Never embedded in the JWT. `None` for records written
    /// before ADR-0011 (`serde(default)`), which fall back to the credential
    /// embedded in the legacy token's claims.
    #[serde(default)]
    pub encrypted_api_key: Option<String>,
}

/// Shared, optionally-persistent registry of issued tokens.
#[derive(Clone, Default)]
pub struct TokenRegistry {
    records: Arc<RwLock<HashMap<String, TokenRecord>>>,
    path: Option<PathBuf>,
}

impl TokenRegistry {
    /// Load a registry from `path`, or start empty and in-memory when `None`.
    ///
    /// A missing or unreadable file is treated as an empty registry: the server
    /// must still start when the volume is not yet populated.
    pub fn load(path: Option<PathBuf>) -> Self {
        let records = path
            .as_deref()
            .and_then(read_records)
            .unwrap_or_default()
            .into_iter()
            .map(|record| (record.jti.clone(), record))
            .collect();
        Self {
            records: Arc::new(RwLock::new(records)),
            path,
        }
    }

    /// Whether recorded tokens survive a restart.
    pub fn is_persistent(&self) -> bool {
        self.path.is_some()
    }

    /// Record a newly issued token. Overwrites any record with the same `jti`.
    pub async fn record(&self, record: TokenRecord) {
        self.records
            .write()
            .await
            .insert(record.jti.clone(), record);
        self.persist().await;
    }

    /// Look a token up by its `jti`.
    pub async fn find(&self, jti: &str) -> Option<TokenRecord> {
        self.records.read().await.get(jti).cloned()
    }

    /// Mark a token revoked. Returns `false` when the `jti` is unknown.
    pub async fn revoke(&self, jti: &str) -> bool {
        let revoked = {
            let mut records = self.records.write().await;
            match records.get_mut(jti) {
                Some(record) => {
                    record.revoked = true;
                    true
                }
                None => false,
            }
        };
        if revoked {
            self.persist().await;
        }
        revoked
    }

    /// Every record, oldest first.
    pub async fn all(&self) -> Vec<TokenRecord> {
        let mut records: Vec<TokenRecord> = self.records.read().await.values().cloned().collect();
        records.sort_by(|a, b| a.issued_at.cmp(&b.issued_at));
        records
    }

    /// The records for one athlete, oldest first.
    pub async fn list_for_athlete(&self, athlete_id: &str) -> Vec<TokenRecord> {
        let mut records: Vec<TokenRecord> = self
            .records
            .read()
            .await
            .values()
            .filter(|record| record.athlete_id == athlete_id)
            .cloned()
            .collect();
        records.sort_by(|a, b| a.issued_at.cmp(&b.issued_at));
        records
    }

    /// Write the registry to disk when a path is configured. Best effort: a
    /// failed write is logged but never fails the caller's request.
    async fn persist(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let records = self.all().await;
        match serde_json::to_vec(&records) {
            Ok(data) => {
                if let Err(error) = tokio::fs::write(path, data).await {
                    tracing::warn!(path = %path.display(), %error, "token registry write failed");
                }
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "token registry serialization failed");
            }
        }
    }
}

/// Load records from `path`.
///
/// A missing file is a normal first run (no record). A file that exists but
/// cannot be read or parsed is an operator-actionable failure: warn, because
/// every token recorded in it will be rejected until the file is repaired.
fn read_records(path: &Path) -> Option<Vec<TokenRecord>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "token registry file is unreadable; starting empty — previously issued tokens will be rejected");
            return None;
        }
    };
    match serde_json::from_str(&content) {
        Ok(records) => Some(records),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "token registry file is corrupt; starting empty — previously issued tokens will be rejected");
            None
        }
    }
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
        let registry = TokenRegistry::load(None);
        registry
            .record(record("jti-1", "athlete-a", "2026-01-01T00:00:00Z", false))
            .await;

        let found = registry.find("jti-1").await.expect("record present");
        assert_eq!(found.athlete_id, "athlete-a");
        assert_eq!(found.encrypted_api_key.as_deref(), Some("encrypted-jti-1"));
        assert!(registry.find("missing").await.is_none());
    }

    #[tokio::test]
    async fn revoke_marks_known_and_reports_unknown() {
        let registry = TokenRegistry::load(None);
        registry
            .record(record("jti-1", "athlete-a", "2026-01-01T00:00:00Z", false))
            .await;

        assert!(registry.revoke("jti-1").await);
        assert!(registry.find("jti-1").await.expect("record").revoked);
        assert!(!registry.revoke("missing").await);
    }

    #[tokio::test]
    async fn lists_are_scoped_and_ordered_by_issue_time() {
        let registry = TokenRegistry::load(None);
        registry
            .record(record("late", "athlete-a", "2026-01-03T00:00:00Z", false))
            .await;
        registry
            .record(record("early", "athlete-a", "2026-01-01T00:00:00Z", false))
            .await;
        registry
            .record(record("other", "athlete-b", "2026-01-02T00:00:00Z", false))
            .await;

        let athlete_a = registry.list_for_athlete("athlete-a").await;
        assert_eq!(
            athlete_a.iter().map(|r| r.jti.as_str()).collect::<Vec<_>>(),
            vec!["early", "late"]
        );

        let all = registry.all().await;
        assert_eq!(
            all.iter().map(|r| r.jti.as_str()).collect::<Vec<_>>(),
            vec!["early", "other", "late"]
        );
    }

    #[tokio::test]
    async fn persists_and_reloads_from_disk() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested/tokens.json");

        {
            let registry = TokenRegistry::load(Some(path.clone()));
            registry
                .record(record("jti-1", "athlete-a", "2026-01-01T00:00:00Z", false))
                .await;
            assert!(registry.revoke("jti-1").await);
        }

        let reloaded = TokenRegistry::load(Some(path));
        let found = reloaded.find("jti-1").await.expect("record persisted");
        assert_eq!(found.athlete_id, "athlete-a");
        assert!(found.revoked);
        assert!(reloaded.is_persistent());
    }

    #[tokio::test]
    async fn missing_file_starts_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = TokenRegistry::load(Some(dir.path().join("absent.json")));
        assert!(registry.all().await.is_empty());
    }

    #[test]
    fn in_memory_registry_is_not_persistent() {
        assert!(!TokenRegistry::load(None).is_persistent());
    }

    #[test]
    fn legacy_records_without_api_key_deserialize() {
        // Registry files written before ADR-0011 have no `encrypted_api_key`.
        let json = r#"[{"jti":"old","athlete_id":"a","issued_at":"2026-01-01T00:00:00Z","expires_at":"2026-02-01T00:00:00Z","revoked":false}]"#;
        let records: Vec<TokenRecord> = serde_json::from_str(json).expect("legacy json parses");
        assert_eq!(records.len(), 1);
        assert!(records[0].encrypted_api_key.is_none());
    }

    #[tokio::test]
    async fn corrupt_file_starts_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("tokens.json");
        std::fs::write(&path, b"{not json").expect("write corrupt file");
        let registry = TokenRegistry::load(Some(path));
        assert!(registry.all().await.is_empty());
    }
}
