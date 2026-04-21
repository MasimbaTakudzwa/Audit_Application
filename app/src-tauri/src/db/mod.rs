//! SQLCipher-backed database state.
//!
//! The connection is held behind `Mutex<Option<Connection>>` because the DB
//! file is encrypted at rest and stays closed until the user signs in. On
//! onboarding/login we recover the raw 32-byte master key from the identity
//! vault, open the DB, and issue `PRAGMA key = x'<hex>'` before any other
//! access. On logout we drop the connection, which re-locks the file.

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub mod migrations;

pub struct DbState {
    inner: Mutex<Option<Connection>>,
}

impl DbState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    pub fn is_open(&self) -> bool {
        self.inner.lock().is_some()
    }

    /// Opens the SQLCipher DB at `path` using `master_key` as the page key,
    /// applies pragmas, runs migrations, and stores the connection.
    ///
    /// The `PRAGMA key` statement MUST be the first thing run on the
    /// connection — SQLCipher rejects keying after any other access. Using
    /// `execute_batch` with the raw-key blob literal avoids any quoting
    /// surprises from rusqlite's `pragma_update` code path.
    pub fn open_with_key(&self, path: &Path, master_key: &[u8; 32]) -> AppResult<()> {
        let conn = Connection::open(path)?;

        let key_literal = format!("PRAGMA key = \"x'{}'\";", hex::encode(master_key));
        conn.execute_batch(&key_literal)?;

        // A read against sqlite_master forces SQLCipher to attempt to decrypt
        // page 1. Wrong key → SQLITE_NOTADB. This converts a silently-bad
        // key into an error we can surface instead of letting the later
        // migration code hit a confusing parse error.
        let _: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(_, _) => {
                    AppError::Crypto("database key rejected".into())
                }
                other => AppError::Database(other),
            })?;

        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        migrations::run(&conn)?;

        *self.inner.lock() = Some(conn);
        Ok(())
    }

    pub fn close(&self) {
        *self.inner.lock() = None;
    }

    pub fn with<T>(
        &self,
        f: impl FnOnce(&Connection) -> AppResult<T>,
    ) -> AppResult<T> {
        let guard = self.inner.lock();
        let conn = guard
            .as_ref()
            .ok_or_else(|| AppError::Unauthorised("database is locked".into()))?;
        f(conn)
    }

    pub fn with_mut<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut guard = self.inner.lock();
        let conn = guard
            .as_mut()
            .ok_or_else(|| AppError::Unauthorised("database is locked".into()))?;
        f(conn)
    }
}

impl Default for DbState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn tmp_path(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.join(format!("audit-test-{stamp}-{suffix}.db"))
    }

    #[test]
    fn keyed_db_migrations_and_roundtrip() {
        let path = tmp_path("roundtrip");
        let key = [7u8; 32];

        let state = DbState::new();
        state.open_with_key(&path, &key).unwrap();

        // Built-in roles are seeded by migration 0002_identity.
        let role_count: i64 = state
            .with(|conn| {
                Ok(conn
                    .query_row("SELECT COUNT(*) FROM Role", [], |r| r.get(0))?)
            })
            .unwrap();
        assert_eq!(role_count, 5);

        state
            .with(|conn| {
                conn.execute(
                    "INSERT INTO Firm (id, name, country, default_locale, created_at)
                     VALUES (?1, 'Acme', 'ZW', 'en-GB', 0)",
                    params!["firm-1"],
                )?;
                Ok(())
            })
            .unwrap();
        state.close();

        // Reopen with correct key — should see the previously inserted row.
        state.open_with_key(&path, &key).unwrap();
        let firm_name: String = state
            .with(|conn| {
                Ok(conn
                    .query_row("SELECT name FROM Firm WHERE id = ?1", params!["firm-1"], |r| r.get(0))?)
            })
            .unwrap();
        assert_eq!(firm_name, "Acme");
        state.close();

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn wrong_key_rejected() {
        let path = tmp_path("wrongkey");
        let good = [1u8; 32];
        let bad = [2u8; 32];

        let state = DbState::new();
        state.open_with_key(&path, &good).unwrap();
        state.close();

        let err = state.open_with_key(&path, &bad).unwrap_err();
        assert!(matches!(err, AppError::Crypto(_)));

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn with_closed_db_returns_unauthorised() {
        let state = DbState::new();
        let err = state.with(|_| Ok(())).unwrap_err();
        assert!(matches!(err, AppError::Unauthorised(_)));
    }
}
