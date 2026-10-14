//! Schema definitions for the ModernToDoList SQLite index database.
//!
//! All table DDL statements are centralized here. The migration runner
//! uses these to create or upgrade the database schema.

/// Current schema version. Increment when adding new migrations.
pub const SCHEMA_VERSION: u32 = 2;

/// SQL to create the `schema_migrations` tracking table.
pub const CREATE_SCHEMA_MIGRATIONS: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version     INTEGER PRIMARY KEY,
    applied_at  TEXT NOT NULL DEFAULT (datetime('now'))
)
"#;

/// SQL to create the `workspaces` table.
pub const CREATE_WORKSPACES: &str = r#"
CREATE TABLE IF NOT EXISTS workspaces (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    root_path   TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
)
"#;

/// SQL to create the `documents` table.
pub const CREATE_DOCUMENTS: &str = r#"
CREATE TABLE IF NOT EXISTS documents (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    file_path       TEXT NOT NULL,
    doc_type        TEXT NOT NULL DEFAULT 'managed',
    fingerprint     TEXT,
    last_indexed    TEXT,
    UNIQUE(workspace_id, file_path)
)
"#;

/// SQL to create the `task_index` table — the primary search/index store.
pub const CREATE_TASK_INDEX: &str = r#"
CREATE TABLE IF NOT EXISTS task_index (
    task_key        TEXT NOT NULL,
    document_id     TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    title           TEXT NOT NULL DEFAULT '',
    priority        INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'TODO',
    percent_done    REAL NOT NULL DEFAULT 0.0,
    risk            INTEGER NOT NULL DEFAULT 0,
    start_date      TEXT,
    due_date        TEXT,
    completed_date  TEXT,
    parent_key      TEXT,
    position        INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT,
    PRIMARY KEY (task_key, document_id)
)
"#;

/// SQL to create the `task_tags` table.
pub const CREATE_TASK_TAGS: &str = r#"
CREATE TABLE IF NOT EXISTS task_tags (
    task_key    TEXT NOT NULL,
    document_id TEXT NOT NULL,
    tag         TEXT NOT NULL,
    PRIMARY KEY (task_key, document_id, tag),
    FOREIGN KEY (task_key, document_id) REFERENCES task_index(task_key, document_id) ON DELETE CASCADE
)
"#;

/// SQL to create the `task_participants` table.
pub const CREATE_TASK_PARTICIPANTS: &str = r#"
CREATE TABLE IF NOT EXISTS task_participants (
    task_key        TEXT NOT NULL,
    document_id     TEXT NOT NULL,
    participant     TEXT NOT NULL,
    role            TEXT NOT NULL DEFAULT 'allocated_to',
    PRIMARY KEY (task_key, document_id, participant, role),
    FOREIGN KEY (task_key, document_id) REFERENCES task_index(task_key, document_id) ON DELETE CASCADE
)
"#;

/// SQL to create the `task_dependencies` table.
pub const CREATE_TASK_DEPENDENCIES: &str = r#"
CREATE TABLE IF NOT EXISTS task_dependencies (
    task_key        TEXT NOT NULL,
    document_id     TEXT NOT NULL,
    depends_on_key  TEXT NOT NULL,
    dep_type        TEXT NOT NULL DEFAULT 'local',
    raw_ref         TEXT,
    PRIMARY KEY (task_key, document_id, depends_on_key),
    FOREIGN KEY (task_key, document_id) REFERENCES task_index(task_key, document_id) ON DELETE CASCADE
)
"#;

/// SQL to create the `attachments_index` table.
pub const CREATE_ATTACHMENTS_INDEX: &str = r#"
CREATE TABLE IF NOT EXISTS attachments_index (
    id              TEXT PRIMARY KEY,
    task_key        TEXT NOT NULL,
    document_id     TEXT NOT NULL,
    file_path       TEXT NOT NULL,
    file_name       TEXT NOT NULL,
    file_size       INTEGER,
    fingerprint     TEXT,
    attachment_type TEXT NOT NULL DEFAULT 'managed',
    FOREIGN KEY (task_key, document_id) REFERENCES task_index(task_key, document_id) ON DELETE CASCADE
)
"#;

/// SQL to create the `progress_links_index` table.
pub const CREATE_PROGRESS_LINKS_INDEX: &str = r#"
CREATE TABLE IF NOT EXISTS progress_links_index (
    id              TEXT PRIMARY KEY,
    task_key        TEXT NOT NULL,
    document_id     TEXT NOT NULL,
    url             TEXT NOT NULL,
    provider        TEXT,
    FOREIGN KEY (task_key, document_id) REFERENCES task_index(task_key, document_id) ON DELETE CASCADE
)
"#;

/// SQL to create the `saved_views` table.
pub const CREATE_SAVED_VIEWS: &str = r#"
CREATE TABLE IF NOT EXISTS saved_views (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    predicates_json TEXT NOT NULL DEFAULT '{}',
    sort_order      INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
)
"#;

/// SQL to create the `ui_state` table — key-value store for UI persistence.
pub const CREATE_UI_STATE: &str = r#"
CREATE TABLE IF NOT EXISTS ui_state (
    key         TEXT PRIMARY KEY,
    value_json  TEXT NOT NULL DEFAULT '{}',
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
)
"#;

/// SQL to create the `recovery_records` table.
pub const CREATE_RECOVERY_RECORDS: &str = r#"
CREATE TABLE IF NOT EXISTS recovery_records (
    id              TEXT PRIMARY KEY,
    record_type     TEXT NOT NULL,
    payload_json    TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    resolved_at     TEXT
)
"#;

/// SQL to create the `task_search_plain` table (M9, RD-M9-001~013).
///
/// A plain (non-FTS) mirror of the searchable text for every task.
/// It always works — even when the SQLite build lacks FTS5 — and powers
/// the LIKE-based substring fallback required for CJK queries (the
/// unicode61 tokenizer does not segment Chinese/Japanese/Korean text).
///
/// Like every other table here, it is a DISPOSABLE derived cache that can
/// be rebuilt from an XML scan at any time. It is never a business source
/// of truth.
pub const CREATE_TASK_SEARCH_PLAIN: &str = r#"
CREATE TABLE IF NOT EXISTS task_search_plain (
    document_id TEXT NOT NULL,
    task_id     TEXT NOT NULL,
    title       TEXT NOT NULL DEFAULT '',
    body        TEXT NOT NULL DEFAULT '',
    tags        TEXT NOT NULL DEFAULT '',
    participants TEXT NOT NULL DEFAULT '',
    attachments TEXT NOT NULL DEFAULT '',
    links       TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (document_id, task_id)
)
"#;

/// SQL to create the `task_search_fts` FTS5 virtual table (M9, RD-M9-001).
///
/// `document_id` and `task_id` are UNINDEXED identity columns used to join
/// back to `task_index`. The indexed columns are: title, body (description
/// plain text), tags, participants, attachments (display names) and links
/// (progress-link labels).
///
/// NOTE: this statement is part of migration v2. The bundled rusqlite 0.32
/// (libsqlite3-sys `bundled`) compiles SQLite with `-DSQLITE_ENABLE_FTS5`,
/// so FTS5 is guaranteed available for this project's builds. If it is ever
/// unavailable at runtime, `infrastructure::search_fts::ensure_search_schema`
/// detects that and degrades to the LIKE fallback over `task_search_plain`.
pub const CREATE_TASK_SEARCH_FTS: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS task_search_fts USING fts5(
    document_id UNINDEXED,
    task_id UNINDEXED,
    title,
    body,
    tags,
    participants,
    attachments,
    links,
    tokenize = 'unicode61'
)
"#;

/// Index: CJK LIKE-fallback scans on title.
pub const CREATE_IDX_SEARCH_PLAIN_TITLE: &str = r#"
CREATE INDEX IF NOT EXISTS idx_task_search_plain_title
ON task_search_plain(title)
"#;

/// Index: per-document clearing of the plain search table.
pub const CREATE_IDX_SEARCH_PLAIN_DOC: &str = r#"
CREATE INDEX IF NOT EXISTS idx_task_search_plain_doc
ON task_search_plain(document_id)
"#;

/// Index: task lookup by parent for tree reconstruction.
pub const CREATE_IDX_TASK_PARENT: &str = r#"
CREATE INDEX IF NOT EXISTS idx_task_index_parent
ON task_index(document_id, parent_key, position)
"#;

/// Index: task lookup by document for full-document scans.
pub const CREATE_IDX_TASK_DOCUMENT: &str = r#"
CREATE INDEX IF NOT EXISTS idx_task_index_document
ON task_index(document_id)
"#;

/// Index: tag search.
pub const CREATE_IDX_TAGS: &str = r#"
CREATE INDEX IF NOT EXISTS idx_task_tags_tag
ON task_tags(tag)
"#;

/// Index: participant search.
pub const CREATE_IDX_PARTICIPANTS: &str = r#"
CREATE INDEX IF NOT EXISTS idx_task_participants_participant
ON task_participants(participant)
"#;

/// Index: dependency reverse lookup.
pub const CREATE_IDX_DEPENDENCIES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_task_dependencies_target
ON task_dependencies(document_id, depends_on_key)
"#;

/// All migration SQL statements in order. Each entry is `(version, description, sql)`.
pub fn migrations() -> Vec<(u32, &'static str, Vec<&'static str>)> {
    vec![
        (
            1,
            "Initial schema: all core tables and indexes",
            vec![
                CREATE_SCHEMA_MIGRATIONS,
                CREATE_WORKSPACES,
                CREATE_DOCUMENTS,
                CREATE_TASK_INDEX,
                CREATE_TASK_TAGS,
                CREATE_TASK_PARTICIPANTS,
                CREATE_TASK_DEPENDENCIES,
                CREATE_ATTACHMENTS_INDEX,
                CREATE_PROGRESS_LINKS_INDEX,
                CREATE_SAVED_VIEWS,
                CREATE_UI_STATE,
                CREATE_RECOVERY_RECORDS,
                CREATE_IDX_TASK_PARENT,
                CREATE_IDX_TASK_DOCUMENT,
                CREATE_IDX_TAGS,
                CREATE_IDX_PARTICIPANTS,
                CREATE_IDX_DEPENDENCIES,
            ],
        ),
        (
            2,
            "M9 search: FTS5 task_search_fts + LIKE-fallback task_search_plain",
            vec![
                CREATE_TASK_SEARCH_PLAIN,
                CREATE_TASK_SEARCH_FTS,
                CREATE_IDX_SEARCH_PLAIN_TITLE,
                CREATE_IDX_SEARCH_PLAIN_DOC,
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_is_positive() {
        assert!(SCHEMA_VERSION > 0);
    }

    #[test]
    fn migrations_are_ordered() {
        let m = migrations();
        for i in 1..m.len() {
            assert!(m[i].0 > m[i - 1].0, "migration versions must be strictly increasing");
        }
    }

    #[test]
    fn migrations_start_at_version_1() {
        assert_eq!(migrations()[0].0, 1);
    }

    #[test]
    fn first_migration_creates_schema_migrations_table() {
        let stmts = &migrations()[0].2;
        assert!(stmts[0].contains("schema_migrations"));
    }
}
