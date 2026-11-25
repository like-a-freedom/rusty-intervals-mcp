use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::Duration;
use thiserror::Error;

const SCHEMA_VERSION: i64 = 2;
const BUSY_TIMEOUT: Duration = Duration::from_secs(1);
const CONNECTION_LOCK_TIMEOUT: Duration = Duration::from_millis(250);
const CONNECTION_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(5);
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
    #[error("state-store database connection remained busy past the lock deadline")]
    LockTimeout,
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
        if path == Path::new(":memory:") {
            return Err(StateStoreError::Configuration(
                "MCP_STATE_DB_PATH must be a durable file path".to_string(),
            ));
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        prepare_database_files(&path)?;
        // Interpret the configured path literally so SQLite opens the file we secured.
        let flags = OpenFlags::default() - OpenFlags::SQLITE_OPEN_URI;
        #[cfg(unix)]
        let flags = flags - OpenFlags::SQLITE_OPEN_CREATE;
        let mut connection = Connection::open_with_flags(&path, flags)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        let mode: String =
            connection.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(StateStoreError::Configuration(
                "state-store database requires WAL mode".to_string(),
            ));
        }
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Self::initialize_schema(&mut connection)?;
        connection.execute(
            "UPDATE idempotency_operations SET state = 'unknown' WHERE state = 'active'",
            [],
        )?;

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
            let deadline = std::time::Instant::now() + CONNECTION_LOCK_TIMEOUT;
            let mut connection = loop {
                match connection.try_lock() {
                    Ok(connection) => break connection,
                    Err(TryLockError::Poisoned(_)) => return Err(StateStoreError::Poisoned),
                    Err(TryLockError::WouldBlock) => {
                        let remaining =
                            deadline.saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            return Err(StateStoreError::LockTimeout);
                        }
                        std::thread::sleep(CONNECTION_LOCK_POLL_INTERVAL.min(remaining));
                    }
                }
            };
            secure_database_files(&path)?;
            operation(&mut connection)
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

fn prepare_database_files(path: &Path) -> Result<(), StateStoreError> {
    #[cfg(unix)]
    {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;

        // SQLite derives new journal, WAL and SHM permissions from the database file.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = std::fs::symlink_metadata(path)?;
                if !metadata.is_file() {
                    return Err(StateStoreError::Configuration(
                        "state-store database must be a regular file".to_string(),
                    ));
                }
                // Reject an old JSON journal or other non-SQLite file before changing its mode.
                if metadata.len() != 0 {
                    let mut header = [0; 16];
                    let mut file = std::fs::File::open(path)?;
                    if metadata.len() < header.len() as u64 {
                        return Err(invalid_database_header());
                    }
                    file.read_exact(&mut header)?;
                    if &header != b"SQLite format 3\0" {
                        return Err(invalid_database_header());
                    }
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    secure_database_files(path)
}

#[cfg(unix)]
fn invalid_database_header() -> StateStoreError {
    StateStoreError::Configuration(
        "the existing state-store file is not SQLite; no legacy history was imported".to_string(),
    )
}

#[cfg(unix)]
fn secure_database_files(path: &Path) -> Result<(), StateStoreError> {
    set_private_permissions(path, false)?;
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        set_private_permissions(Path::new(&sidecar), true)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_permissions(path: &Path, allow_missing: bool) -> Result<(), StateStoreError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Err(StateStoreError::Configuration(
            "state-store database and sidecars must be regular files".to_string(),
        ));
    }
    if metadata.permissions().mode() & 0o7777 != 0o600 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn secure_database_files(_path: &Path) -> Result<(), StateStoreError> {
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

    #[tokio::test]
    async fn reopening_state_store_classifies_active_operations_as_unknown() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("state.sqlite3");
        let store = StateStore::open(path.clone(), [4; 32]).expect("open state store");
        store
            .run(|connection| {
                connection.execute(
                    "INSERT INTO idempotency_operations (
                        scope_hash, tenant_hash, fingerprint_hash, owner_hash, state, created_at
                    ) VALUES (?1, ?2, ?3, NULL, 'active', 123)",
                    rusqlite::params![vec![1_u8; 32], vec![2_u8; 32], vec![3_u8; 32]],
                )?;
                Ok(())
            })
            .await
            .expect("insert active operation");
        drop(store);

        let reopened = StateStore::open(path, [4; 32]).expect("reopen state store");
        let state: String = reopened
            .connection
            .lock()
            .expect("connection lock")
            .query_row(
                "SELECT state FROM idempotency_operations WHERE scope_hash = ?1",
                rusqlite::params![vec![1_u8; 32]],
                |row| row.get(0),
            )
            .expect("read recovered operation");
        assert_eq!(state, "unknown");
    }

    #[tokio::test]
    async fn state_store_bounds_waiting_for_the_connection_lock() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let store =
            StateStore::open(dir.path().join("state.sqlite3"), [4; 32]).expect("open state store");
        let (lock_acquired_tx, lock_acquired_rx) = std::sync::mpsc::channel();
        let (release_lock_tx, release_lock_rx) = std::sync::mpsc::channel();
        let lock_store = store.clone();
        let lock_thread = std::thread::spawn(move || {
            let _connection_guard = lock_store.connection.lock().expect("connection lock");
            lock_acquired_tx.send(()).expect("signal acquired lock");
            release_lock_rx.recv().expect("release connection lock");
        });
        lock_acquired_rx.recv().expect("wait for connection lock");
        let operation_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started_by_operation = operation_started.clone();

        let result = tokio::time::timeout(
            Duration::from_secs(3),
            store.run(move |_| {
                started_by_operation.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }),
        )
        .await;
        let run_result = result.expect("connection lock wait must be bounded");
        let operation_returned_error = run_result.is_err();
        release_lock_tx.send(()).expect("release connection lock");
        lock_thread.join().expect("join connection lock holder");

        assert!(
            operation_returned_error,
            "timed out connection lock must return an error"
        );
        assert!(!operation_started.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sqlite_files_are_private_before_mutation() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let store = StateStore::open(path.clone(), [4; 32]).unwrap();
        let files = [
            path,
            dir.path().join("state.sqlite3-wal"),
            dir.path().join("state.sqlite3-shm"),
        ];
        for file in &files {
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        store
            .run(move |connection| {
                for file in files {
                    assert_eq!(std::fs::metadata(file)?.permissions().mode() & 0o777, 0o600);
                }
                connection.execute(
                    "INSERT INTO token_registry VALUES ('id', 'athlete', 'now', 'later', 0, NULL)",
                    [],
                )?;
                Ok(())
            })
            .await
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sqlite_commit_is_not_reported_as_failure_when_path_becomes_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original");
        let moved = dir.path().join("moved");
        let store = StateStore::open(original.join("state.sqlite3"), [4; 32]).unwrap();
        let original_in_operation = original.clone();
        let moved_in_operation = moved.clone();
        let result = store
            .run(move |connection| {
                let transaction = connection.transaction()?;
                transaction.execute(
                    "INSERT INTO token_registry VALUES ('id', 'athlete', 'now', 'later', 0, NULL)",
                    [],
                )?;
                transaction.commit()?;
                std::fs::rename(original_in_operation, moved_in_operation)?;
                Ok(())
            })
            .await;
        std::fs::rename(moved, original).unwrap();
        let count = store
            .run(|connection| {
                Ok(
                    connection.query_row("SELECT COUNT(*) FROM token_registry", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                )
            })
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert!(
            result.is_ok(),
            "the committed insert was reported as failed: {result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn sqlite_open_rejects_invalid_sidecar_before_initializing_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let sidecar = dir.path().join("state.sqlite3-journal");
        std::fs::create_dir(&sidecar).unwrap();
        assert!(matches!(
            StateStore::open(path.clone(), [4; 32]),
            Err(StateStoreError::Configuration(_))
        ));
        std::fs::remove_dir(sidecar).unwrap();
        let connection = Connection::open(path).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 0);
    }

    #[test]
    fn sqlite_open_creates_private_database_and_schema() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested/state.sqlite3");
        let store = StateStore::open(path.clone(), [4; 32]).expect("open store");
        assert!(path.exists());
        let connection = store.connection.lock().expect("connection lock");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read schema version");
        assert_eq!(version, SCHEMA_VERSION);
        let mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["state.sqlite3", "state.sqlite3-wal", "state.sqlite3-shm"] {
                let metadata = std::fs::metadata(path.with_file_name(name)).unwrap();
                assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            }
        }
    }
}
