//! Schema migration runner for the ModernToDoList SQLite index database.
//!
//! Runs migrations in version order, tracking applied versions in
//! the `schema_migrations` table. Each migration is idempotent-safe
//! (uses `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`).

use rusqlite::{Connection, Transaction};
use thiserror::Error;

use super::schema;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Schema version {found} is newer than supported {supported}")]
    FutureSchema { found: u32, supported: u32 },

    #[error("Migration {version} failed: {reason}")]
    MigrationFailed { version: u32, reason: String },
}

pub type MigrationResult<T> = Result<T, MigrationError>;

/// Run all pending migrations on the given connection.
///
/// Returns the number of migrations applied. If the database schema
/// is already at the current version, returns 0.
pub fn run_migrations(conn: &Connection) -> MigrationResult<u32> {
    // Ensure the tracking table exists before we query it.
    conn.execute_batch(schema::CREATE_SCHEMA_MIGRATIONS)?;

    let current = get_current_version(conn)?;
    let supported = schema::SCHEMA_VERSION;

    if current > supported {
        return Err(MigrationError::FutureSchema {
            found: current,
            supported,
        });
    }

    let migrations = schema::migrations();
    let mut applied = 0;

    for (version, description, statements) in &migrations {
        if *version <= current {
            continue;
        }

        // Run each migration inside a transaction.
        let tx = conn.unchecked_transaction()?;
        apply_migration(&tx, *version, description, statements)?;
        tx.commit()?;
        applied += 1;
    }

    Ok(applied)
}

/// Get the current schema version from the database.
/// Returns 0 if no migrations have been applied yet.
pub fn get_current_version(conn: &Connection) -> MigrationResult<u32> {
    let result = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get::<_, u32>(0),
    );
    Ok(result.unwrap_or(0))
}

fn apply_migration(
    tx: &Transaction,
    version: u32,
    description: &str,
    statements: &[&str],
) -> MigrationResult<()> {
    for (i, sql) in statements.iter().enumerate() {
        tx.execute_batch(sql).map_err(|e| MigrationError::MigrationFailed {
            version,
            reason: format!("statement {} failed: {}", i, e),
        })?;
    }

    // Record the migration.
    tx.execute(
        "INSERT INTO schema_migrations (version) VALUES (?1)",
        [version],
    )?;

    log::info!("Applied migration v{}: {}", version, description);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_memory_db() -> Connection {
        Connection::open_in_memory().expect("in-memory DB")
    }

    #[test]
    fn fresh_database_runs_all_migrations() {
        let conn = new_memory_db();
        let applied = run_migrations(&conn).unwrap();
        assert!(applied > 0, "should apply at least one migration");
        assert_eq!(get_current_version(&conn).unwrap(), schema::SCHEMA_VERSION);
    }

    #[test]
    fn running_migrations_twice_is_idempotent() {
        let conn = new_memory_db();
        let first = run_migrations(&conn).unwrap();
        let second = run_migrations(&conn).unwrap();
        assert!(first > 0);
        assert_eq!(second, 0);
    }

    #[test]
    fn all_core_tables_exist_after_migration() {
        let conn = new_memory_db();
        run_migrations(&conn).unwrap();

        let tables = vec![
            "schema_migrations",
            "workspaces",
            "documents",
            "task_index",
            "task_tags",
            "task_participants",
            "task_dependencies",
            "attachments_index",
            "progress_links_index",
            "saved_views",
            "ui_state",
            "recovery_records",
        ];

        for table in tables {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            assert!(exists, "table '{}' should exist after migration", table);
        }
    }

    #[test]
    fn all_indexes_exist_after_migration() {
        let conn = new_memory_db();
        run_migrations(&conn).unwrap();

        let indexes = vec![
            "idx_task_index_parent",
            "idx_task_index_document",
            "idx_task_tags_tag",
            "idx_task_participants_participant",
            "idx_task_dependencies_target",
        ];

        for idx in indexes {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name=?1",
                    [idx],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            assert!(exists, "index '{}' should exist after migration", idx);
        }
    }

    #[test]
    fn workspaces_table_accepts_insert() {
        let conn = new_memory_db();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES (?1, ?2, ?3)",
            ["ws-001", "Test Workspace", "/tmp/test"],
        )
        .unwrap();

        let name: String = conn
            .query_row("SELECT name FROM workspaces WHERE id = ?1", ["ws-001"], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(name, "Test Workspace");
    }

    #[test]
    fn task_index_cascade_delete_on_document() {
        let conn = new_memory_db();
        run_migrations(&conn).unwrap();

        // Insert workspace + document
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES (?1, ?2, ?3)",
            ["ws-001", "Test", "/tmp"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path) VALUES (?1, ?2, ?3)",
            ["doc-001", "ws-001", "/tmp/test.xml"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_index (task_key, document_id, title) VALUES (?1, ?2, ?3)",
            ["TK-1", "doc-001", "Test Task"],
        )
        .unwrap();

        // Delete document — task_index row should cascade
        conn.execute("DELETE FROM documents WHERE id = ?1", ["doc-001"])
            .unwrap();

        let count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_index WHERE task_key = ?1",
                ["TK-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "task should be cascade-deleted");
    }

    #[test]
    fn future_schema_version_is_rejected() {
        let conn = new_memory_db();
        run_migrations(&conn).unwrap();

        // Manually bump the version to something in the future
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [9999],
        )
        .unwrap();

        let result = run_migrations(&conn);
        assert!(result.is_err());
        match result.unwrap_err() {
            MigrationError::FutureSchema { found, supported } => {
                assert_eq!(found, 9999);
                assert_eq!(supported, schema::SCHEMA_VERSION);
            }
            other => panic!("expected FutureSchema, got: {:?}", other),
        }
    }
}
