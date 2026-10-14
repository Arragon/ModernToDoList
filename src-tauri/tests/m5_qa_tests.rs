//! M5 QA Integration Tests
//!
//! Tests covering the M5 task query layer and index query functionality.
//! QA-M5-001 through QA-M5-016

use std::fs;
use std::path::PathBuf;

fn setup_workspace(name: &str) -> (PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("mtdl_qa_m5_{}_{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();

    let metadata = serde_json::json!({
        "version": 1,
        "name": name,
        "id": uuid::Uuid::new_v4().to_string(),
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z",
        "documents": []
    });
    fs::write(
        dir.join("workspace.json"),
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();

    (dir, metadata["id"].as_str().unwrap().to_string())
}

fn setup_test_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    moderntodolist_lib::infrastructure::migration::run_migrations(&conn).unwrap();
    conn
}

fn seed_tasks(conn: &rusqlite::Connection) {
    use rusqlite::params;

    let doc_id = "doc-test-m5";
    let ws_id = "ws-test-m5";

    conn.execute(
        "INSERT INTO workspaces (id, name, root_path) VALUES (?1, ?2, ?3)",
        params![ws_id, "Test WS", "C:\\test"],
    ).unwrap();

    conn.execute(
        "INSERT INTO documents (id, workspace_id, file_path, doc_type, fingerprint) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![doc_id, ws_id, "test.xml", "xml", "fp1"],
    ).unwrap();

    // Root tasks
    conn.execute(
        "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, risk, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params!["T001", doc_id, "Root Task 1", 2, "In Progress", 50, 0, 0],
    ).unwrap();
    conn.execute(
        "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, risk, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params!["T002", doc_id, "Root Task 2", 0, "Not Started", 0, 0, 1],
    ).unwrap();

    // Child tasks
    conn.execute(
        "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, risk, parent_key, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params!["T001-1", doc_id, "Child 1.1", 3, "Completed", 100, 0, "T001", 0],
    ).unwrap();
    conn.execute(
        "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, risk, parent_key, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params!["T001-2", doc_id, "Child 1.2", 1, "Blocked", 0, 2, "T001", 1],
    ).unwrap();

    // Deep nested
    conn.execute(
        "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, risk, parent_key, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params!["T001-1-1", doc_id, "Deep Child", 4, "Not Started", 0, 0, "T001-1", 0],
    ).unwrap();

    // Tags
    conn.execute("INSERT INTO task_tags (task_key, document_id, tag) VALUES (?1, ?2, ?3)", params!["T001", doc_id, "urgent"]).unwrap();
    conn.execute("INSERT INTO task_tags (task_key, document_id, tag) VALUES (?1, ?2, ?3)", params!["T001", doc_id, "frontend"]).unwrap();
    conn.execute("INSERT INTO task_tags (task_key, document_id, tag) VALUES (?1, ?2, ?3)", params!["T002", doc_id, "backend"]).unwrap();

    // Dated task
    conn.execute(
        "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, risk, start_date, due_date, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params!["T003", doc_id, "Dated Task", 0, "Not Started", 0, 0, "2024-01-01", "2024-06-15", 2],
    ).unwrap();
}

// QA-M5-001: Task query returns all tasks
#[test]
fn qa_m5_001_query_all_tasks() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT task_key, title, parent_key, position FROM task_index ORDER BY position").unwrap();
    let rows: Vec<(String, String, Option<String>, i32)> = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    }).unwrap().filter_map(|r| r.ok()).collect();

    assert_eq!(rows.len(), 6, "Should have 6 tasks total");
    let root_tasks: Vec<_> = rows.iter().filter(|(_, _, p, _)| p.is_none()).collect();
    assert_eq!(root_tasks.len(), 3, "Should have 3 root tasks");
}

// QA-M5-002: Task query with parent filter
#[test]
fn qa_m5_002_query_children_of_parent() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT task_key, title FROM task_index WHERE parent_key = ?1 ORDER BY position").unwrap();
    let rows: Vec<(String, String)> = stmt.query_map(rusqlite::params!["T001"], |row| {
        Ok((row.get(0)?, row.get(1)?))
    }).unwrap().filter_map(|r| r.ok()).collect();

    assert_eq!(rows.len(), 2, "T001 should have 2 children");
    assert_eq!(rows[0].0, "T001-1");
    assert_eq!(rows[1].0, "T001-2");
}

// QA-M5-003: Task query with status filter
#[test]
fn qa_m5_003_query_filter_by_status() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT task_key FROM task_index WHERE status = ?1").unwrap();
    let rows: Vec<String> = stmt.query_map(rusqlite::params!["Completed"], |row| row.get(0))
        .unwrap().filter_map(|r| r.ok()).collect();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0], "T001-1");
}

// QA-M5-004: Task query with document filter
#[test]
fn qa_m5_004_query_filter_by_document() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT COUNT(*) FROM task_index WHERE document_id = ?1").unwrap();
    let count: i32 = stmt.query_row(rusqlite::params!["doc-test-m5"], |row| row.get(0)).unwrap();

    assert_eq!(count, 6);
}

// QA-M5-005: Task tags retrieval
#[test]
fn qa_m5_005_get_task_tags() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT tag FROM task_tags WHERE task_key = ?1 AND document_id = ?2 ORDER BY tag").unwrap();
    let tags: Vec<String> = stmt.query_map(rusqlite::params!["T001", "doc-test-m5"], |row| row.get(0))
        .unwrap().filter_map(|r| r.ok()).collect();

    assert_eq!(tags, vec!["frontend", "urgent"]);
}

// QA-M5-006: Deep nested task hierarchy
#[test]
fn qa_m5_006_deep_nested_hierarchy() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    // T001-1-1 -> parent is T001-1
    let mut stmt = conn.prepare("SELECT parent_key FROM task_index WHERE task_key = ?1").unwrap();
    let parent: Option<String> = stmt.query_row(rusqlite::params!["T001-1-1"], |row| row.get(0)).unwrap();
    assert_eq!(parent, Some("T001-1".to_string()));

    // T001-1 -> parent is T001
    let parent2: Option<String> = stmt.query_row(rusqlite::params!["T001-1"], |row| row.get(0)).unwrap();
    assert_eq!(parent2, Some("T001".to_string()));
}

// QA-M5-007: Task ordering by position
#[test]
fn qa_m5_007_task_ordering() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT task_key FROM task_index WHERE parent_key IS NULL ORDER BY position, task_key").unwrap();
    let keys: Vec<String> = stmt.query_map([], |row| row.get(0))
        .unwrap().filter_map(|r| r.ok()).collect();

    assert_eq!(keys[0], "T001");
    assert_eq!(keys[1], "T002");
    assert_eq!(keys[2], "T003");
}

// QA-M5-008: Priority values preserved
#[test]
fn qa_m5_008_priority_values() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT task_key, priority FROM task_index ORDER BY task_key").unwrap();
    let rows: Vec<(String, i32)> = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap().filter_map(|r| r.ok()).collect();

    let t001 = rows.iter().find(|(k, _)| k == "T001").unwrap();
    assert_eq!(t001.1, 2);
    let deep = rows.iter().find(|(k, _)| k == "T001-1-1").unwrap();
    assert_eq!(deep.1, 4);
}

// QA-M5-009: Date fields preserved
#[test]
fn qa_m5_009_date_fields() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT start_date, due_date FROM task_index WHERE task_key = ?1").unwrap();
    let (start, due): (Option<String>, Option<String>) = stmt.query_row(rusqlite::params!["T003"], |row| {
        Ok((row.get(0)?, row.get(1)?))
    }).unwrap();

    assert_eq!(start, Some("2024-01-01".to_string()));
    assert_eq!(due, Some("2024-06-15".to_string()));
}

// QA-M5-010: Empty query returns empty
#[test]
fn qa_m5_010_empty_query() {
    let conn = setup_test_db();

    let mut stmt = conn.prepare("SELECT COUNT(*) FROM task_index").unwrap();
    let count: i32 = stmt.query_row([], |row| row.get(0)).unwrap();
    assert_eq!(count, 0);
}

// QA-M5-011: Tags for task with no tags
#[test]
fn qa_m5_011_no_tags_returns_empty() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT tag FROM task_tags WHERE task_key = ?1 AND document_id = ?2").unwrap();
    let tags: Vec<String> = stmt.query_map(rusqlite::params!["T003", "doc-test-m5"], |row| row.get(0))
        .unwrap().filter_map(|r| r.ok()).collect();

    assert!(tags.is_empty());
}

// QA-M5-012: Delete parent leaves orphaned children
#[test]
fn qa_m5_012_delete_parent_orphans_children() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    conn.execute("DELETE FROM task_index WHERE task_key = ?1", rusqlite::params!["T001"]).unwrap();

    let mut stmt = conn.prepare("SELECT COUNT(*) FROM task_index").unwrap();
    let remaining: i32 = stmt.query_row([], |row| row.get(0)).unwrap();

    // T001 deleted; T001-1, T001-2, T001-1-1, T002, T003 remain = 5
    // Wait, we had 6 tasks: T001, T002, T003, T001-1, T001-2, T001-1-1
    // Delete T001 => 5 remain
    assert_eq!(remaining, 5);
}

// QA-M5-013: Query with limit
#[test]
fn qa_m5_013_query_with_limit() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT task_key FROM task_index ORDER BY task_key LIMIT ?1").unwrap();
    let rows: Vec<String> = stmt.query_map(rusqlite::params![3], |row| row.get(0))
        .unwrap().filter_map(|r| r.ok()).collect();

    assert_eq!(rows.len(), 3);
}

// QA-M5-014: Percent done tracking
#[test]
fn qa_m5_014_percent_done() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    let mut stmt = conn.prepare("SELECT task_key, percent_done FROM task_index ORDER BY task_key").unwrap();
    let rows: Vec<(String, f64)> = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap().filter_map(|r| r.ok()).collect();

    let t001 = rows.iter().find(|(k, _)| k == "T001").unwrap();
    assert_eq!(t001.1 as i32, 50);
    let completed = rows.iter().find(|(k, _)| k == "T001-1").unwrap();
    assert_eq!(completed.1 as i32, 100);
}

// QA-M5-015: Index rebuild preserves workspace
#[test]
fn qa_m5_015_rebuild_preserves_workspace() {
    let conn = setup_test_db();
    seed_tasks(&conn);

    // Clear task index (simulates rebuild)
    conn.execute("DELETE FROM task_index", []).unwrap();

    // Workspace should still exist
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM workspaces WHERE id = ?1").unwrap();
    let ws_count: i32 = stmt.query_row(rusqlite::params!["ws-test-m5"], |row| row.get(0)).unwrap();
    assert_eq!(ws_count, 1);

    // Task index should be empty
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM task_index").unwrap();
    let task_count: i32 = stmt.query_row([], |row| row.get(0)).unwrap();
    assert_eq!(task_count, 0);
}

// QA-M5-016: GATE-M5 - index is disposable, workspace.json survives
#[test]
fn qa_m5_016_gate_index_disposable() {
    let (dir, ws_id) = setup_workspace("GateM5Test");
    let ws_meta_path = dir.join("workspace.json");
    assert!(ws_meta_path.exists());

    // Create and seed DB
    {
        let db = moderntodolist_lib::infrastructure::db::DatabaseManager::open(&dir);
        db.with_connection(|conn| {
            conn.execute(
                "INSERT INTO workspaces (id, name, root_path) VALUES (?1, ?2, ?3)",
                rusqlite::params![ws_id, "GateM5Test", dir.to_string_lossy().to_string()],
            )?;
            conn.execute(
                "INSERT INTO documents (id, workspace_id, file_path, doc_type, fingerprint) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["doc1", ws_id, "test.xml", "xml", "fp1"],
            )?;
            conn.execute(
                "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, risk, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params!["T1", "doc1", "Task", 0, "Not Started", 0, 0, 0],
            )?;
            Ok::<_, moderntodolist_lib::infrastructure::db::DatabaseError>(())
        }).unwrap();
    }

    // Delete the database entirely
    let db_path = dir.join("index.db");
    std::fs::remove_file(&db_path).unwrap();
    let _ = std::fs::remove_file(dir.join("index.db-wal"));
    let _ = std::fs::remove_file(dir.join("index.db-shm"));

    // workspace.json must survive
    assert!(ws_meta_path.exists(), "workspace.json must survive DB deletion");

    // Workspace can be re-opened
    let ws = moderntodolist_lib::domain::workspace::Workspace::open(&dir).unwrap();
    assert_eq!(ws.name(), "GateM5Test");

    // Recreate DB - should be empty
    let db2 = moderntodolist_lib::infrastructure::db::DatabaseManager::open(&dir);
    let count = db2.with_connection(|conn| {
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM task_index")?;
        let c: i32 = stmt.query_row([], |row| row.get(0))?;
        Ok(c)
    }).unwrap();
    assert_eq!(count, 0);

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}
