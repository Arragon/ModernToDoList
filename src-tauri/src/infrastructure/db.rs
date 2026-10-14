//! SQLite database connection manager for the ModernToDoList index database.
//!
//! Manages the lifecycle of the `Data/index.db` SQLite database, including:
//! - Opening with WAL journal mode (or fallback to DELETE for UNC paths)
//! - Running schema migrations on first open
//! - Graceful degradation when the database is unavailable
//! - Busy timeout configuration for concurrent access

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

use super::migration;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] migration::MigrationError),

    #[error("Database directory does not exist: {0}")]
    DataDirMissing(PathBuf),

    #[error("Database unavailable, entering Index Recovery Mode")]
    Unavailable,
}

pub type DatabaseResult<T> = Result<T, DatabaseError>;

/// The operational mode of the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseMode {
    /// Normal operation with WAL journaling.
    WalMode,
    /// Fallback to DELETE journal (e.g., UNC/network paths).
    DeleteJournal,
    /// Database could not be opened; XML is still accessible.
    RecoveryMode,
}

/// Manages the SQLite index database connection.
///
/// The database is a derived index — deleting it does not lose business data.
/// XML/TDL files remain the source of truth.
pub struct DatabaseManager {
    conn: Arc<Mutex<Option<Connection>>>,
    db_path: PathBuf,
    mode: DatabaseMode,
}

impl DatabaseManager {
    /// Open or create the index database at the given path.
    ///
    /// The `data_dir` should be the portable `Data/` directory.
    /// The database file will be `data_dir/index.db`.
    ///
    /// If the database cannot be opened (permissions, corruption, etc.),
    /// the manager enters `RecoveryMode` and all queries will fail gracefully.
    pub fn open(data_dir: &Path) -> Self {
        let db_path = data_dir.join("index.db");

        if !data_dir.exists() {
            log::warn!("Data directory missing: {:?}, entering RecoveryMode", data_dir);
            return Self {
                conn: Arc::new(Mutex::new(None)),
                db_path,
                mode: DatabaseMode::RecoveryMode,
            };
        }

        match Self::try_open(&db_path) {
            Ok((conn, mode)) => {
                // Run migrations
                if let Err(e) = migration::run_migrations(&conn) {
                    log::error!("Migration failed: {}, entering RecoveryMode", e);
                    return Self {
                        conn: Arc::new(Mutex::new(None)),
                        db_path,
                        mode: DatabaseMode::RecoveryMode,
                    };
                }
                log::info!("Database opened at {:?} in {:?} mode", db_path, mode);
                Self {
                    conn: Arc::new(Mutex::new(Some(conn))),
                    db_path,
                    mode,
                }
            }
            Err(e) => {
                log::error!("Cannot open database: {}, entering RecoveryMode", e);
                Self {
                    conn: Arc::new(Mutex::new(None)),
                    db_path,
                    mode: DatabaseMode::RecoveryMode,
                }
            }
        }
    }

    /// Try to open the database with WAL mode first, then fall back to DELETE journal.
    fn try_open(db_path: &Path) -> DatabaseResult<(Connection, DatabaseMode)> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;

        let conn = Connection::open_with_flags(db_path, flags)?;

        // Set busy timeout to 5 seconds
        conn.execute_batch("PRAGMA busy_timeout = 5000;")?;

        // Try WAL mode first
        match conn.execute_batch("PRAGMA journal_mode = WAL;") {
            Ok(_) => {
                // Verify WAL was actually set
                let journal_mode: String = conn.query_row(
                    "PRAGMA journal_mode",
                    [],
                    |row| row.get(0),
                )?;
                if journal_mode.to_lowercase() == "wal" {
                    // Enable foreign keys
                    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
                    return Ok((conn, DatabaseMode::WalMode));
                }
            }
            Err(e) => {
                log::warn!("WAL mode failed (possibly UNC path): {}, trying DELETE journal", e);
            }
        }

        // Fallback to DELETE journal
        conn.execute_batch("PRAGMA journal_mode = DELETE;")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok((conn, DatabaseMode::DeleteJournal))
    }

    /// Get the current database mode.
    pub fn mode(&self) -> DatabaseMode {
        self.mode
    }

    /// Get the database file path.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Check if the database is available for queries.
    pub fn is_available(&self) -> bool {
        self.mode != DatabaseMode::RecoveryMode
    }

    /// Execute a function with a reference to the database connection.
    ///
    /// Returns `Err(DatabaseError::Unavailable)` if the database is in RecoveryMode.
    pub fn with_connection<F, R>(&self, f: F) -> DatabaseResult<R>
    where
        F: FnOnce(&Connection) -> DatabaseResult<R>,
    {
        let guard = self.conn.lock().map_err(|_| DatabaseError::Unavailable)?;
        match guard.as_ref() {
            Some(conn) => f(conn),
            None => Err(DatabaseError::Unavailable),
        }
    }

    /// Execute a function with a mutable reference to the database connection.
    pub fn with_connection_mut<F, R>(&self, f: F) -> DatabaseResult<R>
    where
        F: FnOnce(&Connection) -> DatabaseResult<R>,
    {
        // rusqlite::Connection methods like execute don't need &mut self,
        // so we use the same pattern.
        self.with_connection(f)
    }

    /// Get the current schema version.
    pub fn schema_version(&self) -> DatabaseResult<u32> {
        self.with_connection(|conn| {
            migration::get_current_version(conn).map_err(DatabaseError::from)
        })
    }

    /// Delete the database file and all associated files (WAL, SHM).
    /// After this call, the manager enters RecoveryMode.
    ///
    /// This is used for the "delete index.db and rebuild" workflow.
    pub fn delete_database(&mut self) -> DatabaseResult<()> {
        // Drop the connection first
        {
            let mut guard = self.conn.lock().map_err(|_| DatabaseError::Unavailable)?;
            *guard = None;
        }

        // Delete the database file and associated files
        let _ = std::fs::remove_file(&self.db_path);
        let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));

        self.mode = DatabaseMode::RecoveryMode;
        log::info!("Database deleted: {:?}", self.db_path);
        Ok(())
    }

    /// Re-open the database after it was deleted or closed.
    /// Runs migrations on the fresh database.
    pub fn reopen(&mut self) -> DatabaseResult<()> {
        let data_dir = self.db_path.parent().ok_or(DatabaseError::Unavailable)?;
        if !data_dir.exists() {
            return Err(DatabaseError::DataDirMissing(data_dir.to_path_buf()));
        }

        match Self::try_open(&self.db_path) {
            Ok((conn, mode)) => {
                migration::run_migrations(&conn)?;
                self.mode = mode;
                let mut guard = self.conn.lock().map_err(|_| DatabaseError::Unavailable)?;
                *guard = Some(conn);
                Ok(())
            }
            Err(e) => {
                self.mode = DatabaseMode::RecoveryMode;
                Err(e)
            }
        }
    }
}

impl Clone for DatabaseManager {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
            db_path: self.db_path.clone(),
            mode: self.mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_data_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mtdl_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn open_creates_database_in_data_dir() {
        let dir = temp_data_dir();
        let mgr = DatabaseManager::open(&dir);
        assert!(mgr.is_available());
        assert_eq!(mgr.mode(), DatabaseMode::WalMode);
        assert!(dir.join("index.db").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_runs_migrations() {
        let dir = temp_data_dir();
        let mgr = DatabaseManager::open(&dir);
        assert!(mgr.is_available());
        let version = mgr.schema_version().unwrap();
        assert_eq!(version, super::super::schema::SCHEMA_VERSION);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_data_dir_enters_recovery_mode() {
        let dir = PathBuf::from("/nonexistent/path/that/does/not/exist");
        let mgr = DatabaseManager::open(&dir);
        assert!(!mgr.is_available());
        assert_eq!(mgr.mode(), DatabaseMode::RecoveryMode);
    }

    #[test]
    fn with_connection_fails_in_recovery_mode() {
        let dir = PathBuf::from("/nonexistent/path");
        let mgr = DatabaseManager::open(&dir);
        let result = mgr.with_connection(|conn| {
            conn.execute("SELECT 1", []).map_err(DatabaseError::from)?;
            Ok(())
        });
        assert!(result.is_err());
    }

    #[test]
    fn delete_and_reopen_database() {
        let dir = temp_data_dir();
        let mut mgr = DatabaseManager::open(&dir);
        assert!(mgr.is_available());

        // Delete
        mgr.delete_database().unwrap();
        assert!(!mgr.is_available());
        assert!(!dir.join("index.db").exists());

        // Reopen
        mgr.reopen().unwrap();
        assert!(mgr.is_available());
        assert!(dir.join("index.db").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_shares_connection() {
        let dir = temp_data_dir();
        let mgr1 = DatabaseManager::open(&dir);
        let mgr2 = mgr1.clone();

        // Both should see the same state
        assert!(mgr1.is_available());
        assert!(mgr2.is_available());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn foreign_keys_are_enabled() {
        let dir = temp_data_dir();
        let mgr = DatabaseManager::open(&dir);
        mgr.with_connection(|conn| {
            let fk: bool = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
            assert!(fk, "foreign keys should be enabled");
            Ok(())
        }).unwrap();
        fs::remove_dir_all(&dir).ok();
    }
}
