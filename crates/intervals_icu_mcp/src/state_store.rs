use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use rusqlite::{Connection, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;

const SCHEMA_VERSION: i64 = 2;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const CREATE_IDEMPOTENCY_TABLE_V2: &str = "CREATE TABLE idempotency_operations (
    scope_hash BLOB PRIMARY KEY NOT NULL CHECK(length(scope_hash) = 32),
    tenant_hash BLOB CHECK(tenant_hash IS NULL OR length(tenant_hash) = 32),
    fingerprint_hash BLOB NOT NULL CHECK(length(fingerprint_hash) = 32),
    owner_hash BLOB,
    state TEXT NOT NULL CHECK(state IN ('active', 'unknown', 'completed')),
    created_at INTEGER NOT NULL,
    expires_at INTEGER,
    result_nonce BLOB,
    result_ciphertext BLOB,
    CHECK(
        (state IN ('active', 'unknown') AND expires_at IS NULL AND result_nonce IS NULL AND result_ciphertext IS NULL)
        OR
        (state = 'completed' AND expires_at IS NOT NULL AND result_nonce IS NOT NULL AND result_ciphertext IS NOT NULL)
    )
);";

#[derive(Debug, Error)]
pub(crate) enum StateStoreError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("state-store I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("state-store task failed: {0}")]
    Join(String),
    #[error("state-store configuration is invalid: {0}")]
    Configuration(String),
    #[error("unsupported state-store schema version {0}")]
    SchemaVersion(i64),
    #[error("state-store encryption failed")]
    Encryption,
    #[error("state-store encryption key must be 64 hexadecimal characters")]
    InvalidEncryptionKey,
    #[error("state-store database connection lock is poisoned")]
    Poisoned,
    #[error("state-store record transition did not match the expected state")]
    InvalidTransition,
}

#[derive(Clone)]
pub(crate) struct StateStore {
    connection: Arc<Mutex<Connection>>,
    path: PathBuf,
    encryption_key: Arc<[u8; 32]>,
}

impl StateStore {
    pub(crate) fn open(path: PathBuf, encryption_key: [u8; 32]) -> Result<Self, StateStoreError> {
        if path.as_os_str().is_empty() {
            return Err(StateStoreError::Configuration(
                "MCP_STATE_DB_PATH cannot be empty".to_string(),
            ));
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Self::initialize_schema(&mut connection)?;
        set_database_sidecar_permissions(&path)?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            path,
            encryption_key: Arc::new(encryption_key),
        })
    }

    pub(crate) fn from_env() -> Result<Self, StateStoreError> {
        let path = std::env::var("MCP_STATE_DB_PATH").map_err(|_| {
            StateStoreError::Configuration(
                "MCP_STATE_DB_PATH is required for durable mutation safety".to_string(),
            )
        })?;
        let key = std::env::var("MCP_STATE_ENCRYPTION_KEY").map_err(|_| {
            StateStoreError::Configuration(
                "MCP_STATE_ENCRYPTION_KEY is required to encrypt saved operation results"
                    .to_string(),
            )
        })?;
        let encryption_key = decode_encryption_key(&key)?;
        Self::open(PathBuf::from(path), encryption_key)
    }

    fn initialize_schema(connection: &mut Connection) -> Result<(), StateStoreError> {
        let schema_version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if schema_version > SCHEMA_VERSION {
            return Err(StateStoreError::SchemaVersion(schema_version));
        }
        if schema_version == SCHEMA_VERSION {
            return Ok(());
        }

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if schema_version == 1 {
            transaction.execute_batch(
                "ALTER TABLE idempotency_operations RENAME TO idempotency_operations_v1;",
            )?;
            transaction.execute_batch(CREATE_IDEMPOTENCY_TABLE_V2)?;
            transaction.execute_batch(
                "INSERT INTO idempotency_operations (
                    scope_hash, tenant_hash, fingerprint_hash, owner_hash, state, created_at,
                    expires_at, result_nonce, result_ciphertext
                )
                SELECT scope_hash, NULL, fingerprint_hash, owner_hash, state, created_at,
                    expires_at, result_nonce, result_ciphertext
                FROM idempotency_operations_v1;
                DROP TABLE idempotency_operations_v1;
                CREATE INDEX idempotency_operations_state_created
                    ON idempotency_operations(state, created_at);
                PRAGMA user_version = 2;",
            )?;
        } else {
            transaction.execute_batch(CREATE_IDEMPOTENCY_TABLE_V2)?;
            transaction.execute_batch(
                "CREATE INDEX idempotency_operations_state_created
                    ON idempotency_operations(state, created_at);
                CREATE TABLE token_registry (
                    jti TEXT PRIMARY KEY NOT NULL,
                    athlete_id TEXT NOT NULL,
                    issued_at TEXT NOT NULL,
                    expires_at TEXT NOT NULL,
                    revoked INTEGER NOT NULL CHECK(revoked IN (0, 1)),
                    encrypted_api_key TEXT
                );
                PRAGMA user_version = 2;",
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) async fn run<T, F>(&self, operation: F) -> Result<T, StateStoreError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StateStoreError> + Send + 'static,
    {
        let connection = Arc::clone(&self.connection);
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|_| StateStoreError::Poisoned)?;
            let operation_result = operation(&mut connection);
            let permission_result = set_database_sidecar_permissions(&path);
            permission_result?;
            operation_result
        })
        .await
        .map_err(|error| StateStoreError::Join(error.to_string()))?
    }

    pub(crate) fn encrypt(
        &self,
        scope_hash: &[u8; 32],
        plaintext: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), StateStoreError> {
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|_| StateStoreError::Encryption)?;
        let cipher = Aes256Gcm::new_from_slice(self.encryption_key.as_ref())
            .map_err(|_| StateStoreError::Encryption)?;
        let nonce = Nonce::from(nonce);
        let ciphertext = cipher
            .encrypt(
                &nonce,
                aes_gcm::aead::Payload {
                    msg: plaintext,
                    aad: scope_hash,
                },
            )
            .map_err(|_| StateStoreError::Encryption)?;
        Ok((nonce.to_vec(), ciphertext))
    }

    pub(crate) fn decrypt(
        &self,
        scope_hash: &[u8; 32],
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, StateStoreError> {
        let nonce: &[u8; 12] = nonce.try_into().map_err(|_| StateStoreError::Encryption)?;
        let nonce = Nonce::from(*nonce);
        let cipher = Aes256Gcm::new_from_slice(self.encryption_key.as_ref())
            .map_err(|_| StateStoreError::Encryption)?;
        cipher
            .decrypt(
                &nonce,
                aes_gcm::aead::Payload {
                    msg: ciphertext,
                    aad: scope_hash,
                },
            )
            .map_err(|_| StateStoreError::Encryption)
    }
}

fn decode_encryption_key(value: &str) -> Result<[u8; 32], StateStoreError> {
    let decoded = hex::decode(value).map_err(|_| StateStoreError::InvalidEncryptionKey)?;
    decoded
        .try_into()
        .map_err(|_| StateStoreError::InvalidEncryptionKey)
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), StateStoreError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<(), StateStoreError> {
    Ok(())
}

#[cfg(unix)]
fn set_database_sidecar_permissions(path: &Path) -> Result<(), StateStoreError> {
    use std::os::unix::fs::PermissionsExt;
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        if sidecar.exists() {
            std::fs::set_permissions(sidecar, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    set_private_permissions(path)
}

#[cfg(not(unix))]
fn set_database_sidecar_permissions(_path: &Path) -> Result<(), StateStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encryption_key_requires_exactly_32_bytes_of_hex() {
        assert!(decode_encryption_key(&"ab".repeat(32)).is_ok());
        assert!(decode_encryption_key("not-hex").is_err());
        assert!(decode_encryption_key(&"ab".repeat(31)).is_err());
    }

    #[test]
    fn schema_v1_migration_preserves_unknown_operations_without_tenant_scope() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("state.sqlite3");
        let connection = Connection::open(&path).expect("create v1 database");
        connection
            .execute_batch(
                "CREATE TABLE idempotency_operations (
                    scope_hash BLOB PRIMARY KEY NOT NULL CHECK(length(scope_hash) = 32),
                    fingerprint_hash BLOB NOT NULL CHECK(length(fingerprint_hash) = 32),
                    owner_hash BLOB,
                    state TEXT NOT NULL CHECK(state IN ('unknown', 'completed')),
                    created_at INTEGER NOT NULL,
                    expires_at INTEGER,
                    result_nonce BLOB,
                    result_ciphertext BLOB,
                    CHECK(
                        (state = 'unknown' AND expires_at IS NULL AND result_nonce IS NULL AND result_ciphertext IS NULL)
                        OR
                        (state = 'completed' AND expires_at IS NOT NULL AND result_nonce IS NOT NULL AND result_ciphertext IS NOT NULL)
                    )
                );
                CREATE INDEX idempotency_operations_state_created
                    ON idempotency_operations(state, created_at);
                CREATE TABLE token_registry (
                    jti TEXT PRIMARY KEY NOT NULL,
                    athlete_id TEXT NOT NULL,
                    issued_at TEXT NOT NULL,
                    expires_at TEXT NOT NULL,
                    revoked INTEGER NOT NULL CHECK(revoked IN (0, 1)),
                    encrypted_api_key TEXT
                );
                INSERT INTO idempotency_operations (
                    scope_hash, fingerprint_hash, owner_hash, state, created_at
                ) VALUES (zeroblob(32), zeroblob(32), NULL, 'unknown', 123);
                PRAGMA user_version = 1;",
            )
            .expect("create v1 schema and record");
        drop(connection);

        let store = StateStore::open(path, [4; 32]).expect("migrate v1 database");
        let (version, record): (i64, (Option<Vec<u8>>, String, i64)) = store
            .connection
            .lock()
            .expect("connection lock")
            .query_row(
                "SELECT (SELECT user_version FROM pragma_user_version), tenant_hash, state, created_at
                 FROM idempotency_operations",
                [],
                |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?))),
            )
            .expect("read migrated record");

        assert_eq!(version, 2);
        assert_eq!(record.0, None);
        assert_eq!(record.1, "unknown");
        assert_eq!(record.2, 123);
    }

    #[test]
    fn sqlite_open_creates_private_database_and_schema() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested/state.sqlite3");
        let store = StateStore::open(path.clone(), [4; 32]).expect("open store");
        assert!(path.exists());
        let version: i64 = store
            .connection
            .lock()
            .expect("connection lock")
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read schema version");
        assert_eq!(version, SCHEMA_VERSION);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
