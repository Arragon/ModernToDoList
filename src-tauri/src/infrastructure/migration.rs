//! Schema migration runner for the ModernToDoList SQLite index database.
//!
//! Runs migrations in version order, tracking applied versions in
//! the `schema_migrations` table. Each migration is idempotent-safe
//! (uses `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`).
//!
//! This module also implements the **versioned Data-directory migration
//! framework** (RD-M10-025~027 / INH-1118):
//! - `Data/version.json` records the Data directory version and the settings
//!   schema version.
//! - [`migrate_data_dir`] applies sequential directory/settings migrations.
//! - [`rebuild_index_fallback`] implements the recovery policy: when a
//!   migration fails, delete `index.db` and rebuild the index from XML.

use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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

    #[error("IO error during data migration: {0}")]
    Io(#[from] std::io::Error),

    #[error("Data directory version {found} is newer than supported {supported}")]
    FutureDataDir { found: u32, supported: u32 },

    #[error("Corrupt Data/version.json: {reason}")]
    VersionFileCorrupt { reason: String },

    #[error("Corrupt Data/settings.json: {reason}")]
    SettingsCorrupt { reason: String },

    #[error("Index rebuild fallback failed: {reason}")]
    RebuildFailed { reason: String },
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

// ===========================================================================
// Versioned Data-directory migration framework (RD-M10-025~027 / INH-1118)
// ===========================================================================

/// File that records the Data directory's migration state.
pub const DATA_VERSION_FILE: &str = "version.json";
/// File holding user settings inside `Data/`.
pub const SETTINGS_FILE: &str = "settings.json";
/// Current Data directory version. Bump when adding a [`DataMigration`].
pub const CURRENT_DATA_DIR_VERSION: u32 = 3;
/// Current settings schema version, stored inside `Data/settings.json`.
pub const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 2;

/// Contents of `Data/version.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataDirVersionFile {
    pub data_dir_version: u32,
    pub settings_schema_version: u32,
    /// RFC 3339 timestamp of the last migration run.
    pub migrated_at: String,
}

impl DataDirVersionFile {
    fn new(data_dir_version: u32, settings_schema_version: u32) -> Self {
        Self {
            data_dir_version,
            settings_schema_version,
            migrated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// One sequential Data-directory migration step.
pub struct DataMigration {
    pub version: u32,
    pub description: &'static str,
    /// Transform the Data directory in place. Must be safe to re-run.
    pub apply: fn(&Path) -> MigrationResult<()>,
}

fn migrate_data_to_v1(data_dir: &Path) -> MigrationResult<()> {
    // v1: canonical portable layout — lists and logs directories.
    std::fs::create_dir_all(data_dir.join("lists"))?;
    std::fs::create_dir_all(data_dir.join("logs"))?;
    Ok(())
}

fn migrate_data_to_v2(data_dir: &Path) -> MigrationResult<()> {
    // v2: settings.json gains a schemaVersion (legacy flat files are upgraded).
    migrate_settings_file(data_dir)?;
    Ok(())
}

fn migrate_data_to_v3(data_dir: &Path) -> MigrationResult<()> {
    // v3: dedicated WebView2 user-data folder inside Data/.
    std::fs::create_dir_all(data_dir.join("webview2"))?;
    // Ensure settings are at the current schema even if v2 ran on an older build.
    migrate_settings_file(data_dir)?;
    Ok(())
}

/// The ordered list of Data-directory migrations (0 → CURRENT).
pub fn data_migrations() -> Vec<DataMigration> {
    vec![
        DataMigration {
            version: 1,
            description: "Create canonical Data layout (lists/, logs/)",
            apply: migrate_data_to_v1,
        },
        DataMigration {
            version: 2,
            description: "Introduce settings.json schemaVersion",
            apply: migrate_data_to_v2,
        },
        DataMigration {
            version: 3,
            description: "Add webview2/ user-data folder",
            apply: migrate_data_to_v3,
        },
    ]
}

/// Read the Data directory version. A missing `version.json` means a legacy
/// (pre-framework) directory and reports version 0.
pub fn detect_data_dir_version(data_dir: &Path) -> MigrationResult<DataDirVersionFile> {
    let path = data_dir.join(DATA_VERSION_FILE);
    if !path.exists() {
        return Ok(DataDirVersionFile::new(0, 0));
    }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text).map_err(|e| MigrationError::VersionFileCorrupt {
        reason: e.to_string(),
    })
}

fn write_version_file(data_dir: &Path, state: &DataDirVersionFile) -> MigrationResult<()> {
    // Write via temp file + rename so a crash cannot leave a torn version.json.
    let tmp = data_dir.join(format!("{DATA_VERSION_FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_string_pretty(state).expect("serializes"))?;
    std::fs::rename(&tmp, data_dir.join(DATA_VERSION_FILE))?;
    Ok(())
}

/// Read the current `schemaVersion` from `Data/settings.json` (0 if absent).
fn detect_settings_schema_version(data_dir: &Path) -> u32 {
    std::fs::read_to_string(data_dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("schemaVersion").and_then(|s| s.as_u64()).map(|n| n as u32))
        .unwrap_or(0)
}

/// Migrate a settings JSON value from its embedded `schemaVersion` (default 0)
/// to [`CURRENT_SETTINGS_SCHEMA_VERSION`] by applying sequential transforms.
pub fn migrate_settings_value(value: serde_json::Value) -> MigrationResult<serde_json::Value> {
    use serde_json::Value;

    let mut current = value
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;

    let mut v = value;
    while current < CURRENT_SETTINGS_SCHEMA_VERSION {
        let target = current + 1;
        match target {
            1 => {
                // 0 → 1: legacy untyped file becomes a JSON object with a
                // schemaVersion marker.
                if !v.is_object() {
                    v = Value::Object(serde_json::Map::new());
                }
                v.as_object_mut()
                    .expect("object")
                    .insert("schemaVersion".into(), Value::from(1));
            }
            2 => {
                // 1 → 2: flat legacy keys are nested into sections.
                let obj = v.as_object_mut().ok_or_else(|| {
                    MigrationError::SettingsCorrupt {
                        reason: "settings root is not an object".into(),
                    }
                })?;
                if let Some(secs) = obj.remove("autosave_secs") {
                    let session = obj
                        .entry("session")
                        .or_insert_with(|| Value::Object(serde_json::Map::new()));
                    if let Some(s) = session.as_object_mut() {
                        s.insert("autosaveSecs".into(), secs);
                    }
                }
                if let Some(theme) = obj.remove("theme") {
                    let appearance = obj
                        .entry("appearance")
                        .or_insert_with(|| Value::Object(serde_json::Map::new()));
                    if let Some(a) = appearance.as_object_mut() {
                        a.insert("theme".into(), theme);
                    }
                }
                obj.insert("schemaVersion".into(), Value::from(2));
            }
            _ => break,
        }
        current = target;
    }
    Ok(v)
}

/// Migrate `Data/settings.json` in place. Returns the final schema version.
/// A missing settings file is created at the current schema.
pub fn migrate_settings_file(data_dir: &Path) -> MigrationResult<u32> {
    let path = data_dir.join(SETTINGS_FILE);
    let value = if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text).map_err(|e| MigrationError::SettingsCorrupt {
            reason: e.to_string(),
        })?
    } else {
        serde_json::Value::Null
    };

    let migrated = migrate_settings_value(value)?;
    let tmp = data_dir.join(format!("{SETTINGS_FILE}.tmp"));
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&migrated).expect("serializes"),
    )?;
    std::fs::rename(&tmp, &path)?;
    Ok(CURRENT_SETTINGS_SCHEMA_VERSION)
}

/// Apply all pending Data-directory migrations sequentially.
///
/// Returns the number of migrations applied. Version state is stamped after
/// each successful step so an interrupted run resumes where it stopped.
pub fn migrate_data_dir_with(
    data_dir: &Path,
    migrations: &[DataMigration],
) -> MigrationResult<u32> {
    std::fs::create_dir_all(data_dir)?;
    let state = detect_data_dir_version(data_dir)?;

    if state.data_dir_version > CURRENT_DATA_DIR_VERSION {
        return Err(MigrationError::FutureDataDir {
            found: state.data_dir_version,
            supported: CURRENT_DATA_DIR_VERSION,
        });
    }

    let mut ordered: Vec<&DataMigration> = migrations.iter().collect();
    ordered.sort_by_key(|m| m.version);

    let mut version = state.data_dir_version;
    let mut applied = 0;
    for m in ordered {
        if m.version <= version {
            continue;
        }
        (m.apply)(data_dir).map_err(|e| MigrationError::MigrationFailed {
            version: m.version,
            reason: e.to_string(),
        })?;
        version = m.version;
        write_version_file(
            data_dir,
            &DataDirVersionFile::new(version, detect_settings_schema_version(data_dir)),
        )?;
        log::info!("Applied data-dir migration v{}: {}", m.version, m.description);
        applied += 1;
    }

    // Keep settings schema in sync even when no dir migrations were pending.
    let settings_version = migrate_settings_file(data_dir)?;
    write_version_file(
        data_dir,
        &DataDirVersionFile::new(version, settings_version),
    )?;

    Ok(applied)
}

/// Migrate the Data directory using the built-in migration list.
pub fn migrate_data_dir(data_dir: &Path) -> MigrationResult<u32> {
    migrate_data_dir_with(data_dir, &data_migrations())
}

/// Index-rebuild fallback (RD-M10-027): delete `index.db` (plus WAL/SHM
/// sidecars) and rebuild it from the given XML documents.
///
/// The index is derived data — deleting it never loses business data.
/// The workspace/documents rows the foreign keys require are re-registered
/// (`INSERT OR IGNORE`) before indexing, so the rebuilt database is
/// immediately consistent. Returns the number of indexed tasks.
pub fn rebuild_index_fallback(
    data_dir: &Path,
    workspace_id: &str,
    workspace_root: &Path,
    documents: &[(String, PathBuf)],
) -> MigrationResult<usize> {
    for suffix in ["index.db", "index.db-wal", "index.db-shm"] {
        let f = data_dir.join(suffix);
        match std::fs::remove_file(&f) {
            Ok(()) => log::warn!("Index rebuild fallback: removed {}", suffix),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(MigrationError::RebuildFailed {
                    reason: format!("cannot remove {suffix}: {e}"),
                })
            }
        }
    }

    std::fs::create_dir_all(data_dir)?;
    let conn = Connection::open(data_dir.join("index.db")).map_err(|e| {
        MigrationError::RebuildFailed {
            reason: format!("cannot recreate index.db: {e}"),
        }
    })?;
    run_migrations(&conn)?;

    // Re-register the workspace/document identity rows the FKs need.
    conn.execute(
        "INSERT OR IGNORE INTO workspaces (id, name, root_path) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            workspace_id,
            workspace_id,
            workspace_root.to_string_lossy()
        ],
    )?;
    for (doc_id, path) in documents {
        conn.execute(
            "INSERT OR IGNORE INTO documents (id, workspace_id, file_path) VALUES (?1, ?2, ?3)",
            rusqlite::params![doc_id, workspace_id, path.to_string_lossy()],
        )?;
    }

    super::indexer::rebuild_index(&conn, documents, None, None).map_err(|e| {
        MigrationError::RebuildFailed {
            reason: e.to_string(),
        }
    })
}

/// Result of [`migrate_or_rebuild`].
#[derive(Debug)]
pub enum DataMigrationOutcome {
    /// All migrations succeeded; `applied` is the number of steps run.
    Migrated { applied: u32 },
    /// A migration failed; the index was deleted and rebuilt from XML.
    RebuiltAfterFailure {
        failed_version: u32,
        failure: String,
        tasks_indexed: usize,
    },
}

/// Full RD-M10-025~027 pipeline: migrate the Data directory; on any
/// migration failure, fall back to deleting `index.db` and rebuilding it
/// from the provided XML documents.
pub fn migrate_or_rebuild(
    data_dir: &Path,
    workspace_id: &str,
    workspace_root: &Path,
    documents: &[(String, PathBuf)],
) -> MigrationResult<DataMigrationOutcome> {
    match migrate_data_dir(data_dir) {
        Ok(applied) => Ok(DataMigrationOutcome::Migrated { applied }),
        Err(err) => {
            let failed_version = match &err {
                MigrationError::MigrationFailed { version, .. } => *version,
                _ => 0,
            };
            log::error!("Data migration failed ({err}); rebuilding index from XML");
            let tasks_indexed =
                rebuild_index_fallback(data_dir, workspace_id, workspace_root, documents)?;
            Ok(DataMigrationOutcome::RebuiltAfterFailure {
                failed_version,
                failure: err.to_string(),
                tasks_indexed,
            })
        }
    }
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

    // -----------------------------------------------------------------------
    // Data-directory migration framework (RD-M10-025~027)
    // -----------------------------------------------------------------------

    fn sample_xml(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(
            &path,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <TODOLIST PROJECTNAME=\"P\" NEXTUNIQUEID=\"3\" FILEVERSION=\"43\" APPVER=\"9.0.14.0\" FILEFORMAT=\"12\">\n\
             <TASK ID=\"1\" TITLE=\"Task One\" POS=\"0\" POSSTRING=\"1\">\n\
             <TASK ID=\"2\" TITLE=\"Task Two\" POS=\"0\" POSSTRING=\"1.1\">\n\
             </TASK>\n\
             </TASK>\n\
             </TODOLIST>\n",
        )
        .unwrap();
        path
    }

    #[test]
    fn data_dir_migrates_forward_across_three_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Data");

        // Fresh directory starts at version 0.
        let v = detect_data_dir_version(&data).unwrap();
        assert_eq!(v.data_dir_version, 0);

        let applied = migrate_data_dir(&data).unwrap();
        assert_eq!(applied, 3, "should apply v1, v2 and v3");

        let v = detect_data_dir_version(&data).unwrap();
        assert_eq!(v.data_dir_version, CURRENT_DATA_DIR_VERSION);
        assert_eq!(v.settings_schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);

        assert!(data.join("lists").is_dir());
        assert!(data.join("logs").is_dir());
        assert!(data.join("webview2").is_dir());

        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(data.join(SETTINGS_FILE)).unwrap())
                .unwrap();
        assert_eq!(settings["schemaVersion"], 2);
    }

    #[test]
    fn data_dir_migration_is_idempotent_and_resumable() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Data");

        // Apply only the first two migrations.
        let partial: Vec<DataMigration> =
            data_migrations().into_iter().filter(|m| m.version <= 2).collect();
        assert_eq!(migrate_data_dir_with(&data, &partial).unwrap(), 2);
        assert_eq!(detect_data_dir_version(&data).unwrap().data_dir_version, 2);

        // Resume with the full list: only v3 is applied.
        assert_eq!(migrate_data_dir(&data).unwrap(), 1);
        // Third run: nothing to do.
        assert_eq!(migrate_data_dir(&data).unwrap(), 0);
        assert_eq!(
            detect_data_dir_version(&data).unwrap().data_dir_version,
            CURRENT_DATA_DIR_VERSION
        );
    }

    #[test]
    fn future_data_dir_version_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            data.join(DATA_VERSION_FILE),
            r#"{"dataDirVersion":99,"settingsSchemaVersion":99,"migratedAt":"now"}"#,
        )
        .unwrap();

        match migrate_data_dir(&data) {
            Err(MigrationError::FutureDataDir { found, supported }) => {
                assert_eq!(found, 99);
                assert_eq!(supported, CURRENT_DATA_DIR_VERSION);
            }
            other => panic!("expected FutureDataDir, got {:?}", other.is_ok()),
        }
    }

    #[test]
    fn legacy_settings_are_upgraded_not_lost() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            data.join(SETTINGS_FILE),
            r#"{"theme":"dark","autosave_secs":30,"customKey":"keep-me"}"#,
        )
        .unwrap();

        migrate_data_dir(&data).unwrap();

        let s: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(data.join(SETTINGS_FILE)).unwrap())
                .unwrap();
        assert_eq!(s["schemaVersion"], 2);
        assert_eq!(s["appearance"]["theme"], "dark");
        assert_eq!(s["session"]["autosaveSecs"], 30);
        assert_eq!(s["customKey"], "keep-me", "unknown keys must be preserved");
        assert!(s.get("theme").is_none(), "flat key moved into section");
    }

    #[test]
    fn corrupt_version_file_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join(DATA_VERSION_FILE), "{ broken").unwrap();
        assert!(matches!(
            detect_data_dir_version(&data),
            Err(MigrationError::VersionFileCorrupt { .. })
        ));
    }

    #[test]
    fn failed_migration_triggers_index_rebuild_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Data");
        std::fs::create_dir_all(&data).unwrap();

        // Pre-create an index.db with junk data, as a previous version would.
        {
            let conn = Connection::open(data.join("index.db")).unwrap();
            run_migrations(&conn).unwrap();
            conn.execute(
                "INSERT INTO workspaces (id, name, root_path) VALUES ('stale','Stale','x')",
                [],
            )
            .unwrap();
        }
        let xml = sample_xml(tmp.path(), "tasks.xml");

        // A migration list whose v2 step always fails.
        fn failing(_data_dir: &Path) -> MigrationResult<()> {
            Err(MigrationError::MigrationFailed {
                version: 2,
                reason: "simulated failure".into(),
            })
        }
        let migrations = vec![
            DataMigration {
                version: 1,
                description: "layout",
                apply: migrate_data_to_v1,
            },
            DataMigration {
                version: 2,
                description: "boom",
                apply: failing,
            },
        ];
        let err = migrate_data_dir_with(&data, &migrations).unwrap_err();
        assert!(matches!(err, MigrationError::MigrationFailed { version: 2, .. }));

        // Fallback: delete index.db, rebuild from XML.
        let docs = vec![("doc-1".to_string(), xml.clone())];
        let tasks = rebuild_index_fallback(&data, "ws-mig", tmp.path(), &docs).unwrap();
        assert_eq!(tasks, 2, "both fixture tasks re-indexed");

        // Fresh database: stale row gone, task rows present.
        let conn = Connection::open(data.join("index.db")).unwrap();
        let stale: u32 = conn
            .query_row("SELECT COUNT(*) FROM workspaces WHERE id='stale'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stale, 0);
        let indexed: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_index WHERE document_id='doc-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 2);
    }

    #[test]
    fn migrate_or_rebuild_reports_outcome() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Data");
        let xml = sample_xml(tmp.path(), "tasks.xml");

        let outcome =
            migrate_or_rebuild(&data, "ws-m10", tmp.path(), &[("doc-1".to_string(), xml)])
                .unwrap();
        match outcome {
            DataMigrationOutcome::Migrated { applied } => assert_eq!(applied, 3),
            DataMigrationOutcome::RebuiltAfterFailure { .. } => {
                panic!("healthy migration must not trigger rebuild")
            }
        }
        // Second run applies nothing.
        match migrate_or_rebuild(&data, "ws-m10", tmp.path(), &[]).unwrap() {
            DataMigrationOutcome::Migrated { applied } => assert_eq!(applied, 0),
            _ => panic!("expected Migrated"),
        }
    }
}
