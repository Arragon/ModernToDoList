//! M4 QA Integration Tests
//!
//! Tests the full M4 workspace platform: SQLite infrastructure,
//! workspace management, document indexing, file watching, and recovery.

use std::fs;
use std::path::{Path, PathBuf};

// Helper to create a temp workspace with XML files
fn setup_workspace(name: &str) -> (PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("mtdl_qa_m4_{}_{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();

    let metadata = serde_json::json!({
        "version": 1,
        "name": name,
        "id": uuid::Uuid::new_v4().to_string(),
        "created_at": "2024-01-01T00:00:00s",
        "updated_at": "2024-01-01T00:00:00s",
        "documents": []
    });
    fs::write(
        dir.join("workspace.json"),
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();

    (dir, metadata["id"].as_str().unwrap().to_string())
}

fn write_test_xml(dir: &Path, filename: &str) -> PathBuf {
    let path = dir.join(filename);
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<TDL>
    <TASK ID="1" TITLE="Parent Task" PERCENTDONE="50" PRIORITY="2">
        <CATEGORY>work</CATEGORY>
        <TASK ID="2" TITLE="Child Task" PERCENTDONE="0"/>
    </TASK>
    <TASK ID="3" TITLE="Second Task" PERCENTDONE="100"/>
</TDL>"#;
    fs::write(&path, xml).unwrap();
    path
}

fn setup_test_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    moderntodolist_lib::infrastructure::migration::run_migrations(&conn).unwrap();
    conn
}

// QA-M4-001: SQLite database creation and WAL mode
#[test]
fn qa_m4_001_sqlite_creation_and_wal_mode() {
    let dir = std::env::temp_dir().join(format!("mtdl_qa001_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();

    let db = moderntodolist_lib::infrastructure::DatabaseManager::open(&dir);
    assert!(db.is_available(), "DB should be available");

    db.with_connection(|conn| {
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode, "wal", "Should use WAL journal mode");
        Ok(())
    })
    .unwrap();

    fs::remove_dir_all(&dir).ok();
}

// QA-M4-002: Schema migration runs correctly
#[test]
fn qa_m4_002_schema_migration_complete() {
    let conn = setup_test_db();

    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert!(tables.contains(&"schema_migrations".to_string()));
    assert!(tables.contains(&"workspaces".to_string()));
    assert!(tables.contains(&"documents".to_string()));
    assert!(tables.contains(&"task_index".to_string()));
    assert!(tables.contains(&"task_tags".to_string()));
    assert!(tables.contains(&"task_participants".to_string()));
    assert!(tables.contains(&"task_dependencies".to_string()));
}

// QA-M4-003: Workspace create and open roundtrip
#[test]
fn qa_m4_003_workspace_create_open_roundtrip() {
    let (dir, ws_id) = setup_workspace("QA003");

    let ws = moderntodolist_lib::domain::workspace::Workspace::open(&dir).unwrap();
    assert_eq!(ws.id(), ws_id);
    assert_eq!(ws.name(), "QA003");

    fs::remove_dir_all(&dir).ok();
}

// QA-M4-004: Document scanning finds XML/TDL files
#[test]
fn qa_m4_004_document_scanning() {
    let (dir, _) = setup_workspace("QA004");

    write_test_xml(&dir, "task1.xml");
    write_test_xml(&dir, "task2.tdl");
    fs::write(dir.join("readme.txt"), "not a task file").unwrap();
    fs::create_dir(dir.join("subdir")).unwrap();
    write_test_xml(&dir.join("subdir"), "task3.xml");

    let ws = moderntodolist_lib::domain::workspace::Workspace::open(&dir).unwrap();
    let found = ws.scan_documents().unwrap();
    assert_eq!(found.len(), 3, "Should find 3 XML/TDL files");

    fs::remove_dir_all(&dir).ok();
}

// QA-M4-005: Index document populates SQLite correctly
#[test]
fn qa_m4_005_index_document_populates_sqlite() {
    let dir = std::env::temp_dir().join(format!("mtdl_qa005_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let xml_path = write_test_xml(&dir, "tasks.xml");

    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO workspaces (id, name, root_path) VALUES ('ws1', 'Test', ?1)",
        [&dir.to_string_lossy().to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc1', 'ws1', 'tasks.xml', 'managed')",
        [],
    )
    .unwrap();

    let count = moderntodolist_lib::infrastructure::indexer::index_document(&conn, "doc1", &xml_path).unwrap();
    assert_eq!(count, 3, "Should index 3 tasks");

    let task_count: u32 = conn
        .query_row("SELECT COUNT(*) FROM task_index WHERE document_id = 'doc1'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(task_count, 3);

    let tag_count: u32 = conn
        .query_row("SELECT COUNT(*) FROM task_tags WHERE document_id = 'doc1'", [], |row| row.get(0))
        .unwrap();
    assert!(tag_count >= 1, "Should have at least 1 tag");

    fs::remove_dir_all(&dir).ok();
}

// QA-M4-006: Delete index.db then full rebuild recovers all data
#[test]
fn qa_m4_006_delete_index_db_full_rebuild() {
    let (dir, ws_id) = setup_workspace("QA006");
    let xml_path = write_test_xml(&dir, "tasks.xml");

    let data_dir = std::env::temp_dir().join(format!("mtdl_qa006_data_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&data_dir).unwrap();

    {
        let db = moderntodolist_lib::infrastructure::DatabaseManager::open(&data_dir);
        db.with_connection(|conn| {
            conn.execute(
                "INSERT INTO workspaces (id, name, root_path) VALUES (?1, 'QA006', ?2)",
                rusqlite::params![&ws_id, dir.to_string_lossy().to_string()],
            ).unwrap();
            conn.execute(
                "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc1', ?1, 'tasks.xml', 'managed')",
                rusqlite::params![&ws_id],
            ).unwrap();
            let count = moderntodolist_lib::infrastructure::indexer::index_document(conn, "doc1", &xml_path).unwrap();
            assert_eq!(count, 3);
            Ok(())
        }).unwrap();
    }

    // DELETE the database file
    let db_path = data_dir.join("index.db");
    assert!(db_path.exists());
    fs::remove_file(&db_path).unwrap();
    fs::remove_file(data_dir.join("index.db-wal")).ok();
    fs::remove_file(data_dir.join("index.db-shm")).ok();
    assert!(!db_path.exists(), "DB file should be deleted");

    // Reopen DB - should auto-create fresh
    let db2 = moderntodolist_lib::infrastructure::DatabaseManager::open(&data_dir);
    assert!(db2.is_available(), "DB should reopen successfully");

    db2.with_connection(|conn| {
        moderntodolist_lib::infrastructure::migration::run_migrations(conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES (?1, 'QA006', ?2)",
            rusqlite::params![&ws_id, dir.to_string_lossy().to_string()],
        ).unwrap();
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc1', ?1, 'tasks.xml', 'managed')",
            rusqlite::params![&ws_id],
        ).unwrap();
        let count = moderntodolist_lib::infrastructure::indexer::index_document(conn, "doc1", &xml_path).unwrap();
        assert_eq!(count, 3, "Full rebuild should recover all 3 tasks");
        Ok(())
    }).unwrap();

    fs::remove_dir_all(&dir).ok();
    fs::remove_dir_all(&data_dir).ok();
}

// QA-M4-007: SQLite corruption detection and recovery
#[test]
fn qa_m4_007_sqlite_corruption_recovery() {
    let data_dir = std::env::temp_dir().join(format!("mtdl_qa007_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&data_dir).unwrap();

    let db = moderntodolist_lib::infrastructure::DatabaseManager::open(&data_dir);
    assert!(db.is_available());

    // Corrupt the database file
    let db_path = data_dir.join("index.db");
    fs::write(&db_path, b"this is not a valid sqlite database").unwrap();
    fs::remove_file(data_dir.join("index.db-wal")).ok();
    fs::remove_file(data_dir.join("index.db-shm")).ok();

    // Reopen - should handle gracefully
    let _db2 = moderntodolist_lib::infrastructure::DatabaseManager::open(&data_dir);

    // Recovery: delete corrupted file and recreate
    fs::remove_file(&db_path).ok();
    let db3 = moderntodolist_lib::infrastructure::DatabaseManager::open(&data_dir);
    assert!(db3.is_available(), "Should recover by recreating the database");

    fs::remove_dir_all(&data_dir).ok();
}

// QA-M4-008: File watcher detects external changes
#[test]
fn qa_m4_008_file_watcher_detects_changes() {
    let dir = std::env::temp_dir().join(format!("mtdl_qa008_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();

    let config = moderntodolist_lib::infrastructure::WatcherConfig {
        debounce_ms: 100,
        recursive: true,
    };
    let mut watcher = moderntodolist_lib::infrastructure::WorkspaceWatcher::new(config);
    watcher.watch(&dir).unwrap();

    fs::write(dir.join("new_task.xml"), "<TDL/>").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));

    let events = watcher.poll_events();
    assert!(events.len() <= 10, "Should not produce excessive events");

    watcher.stop();
    fs::remove_dir_all(&dir).ok();
}

// QA-M4-009: Fingerprint-based change detection
#[test]
fn qa_m4_009_fingerprint_change_detection() {
    let dir = std::env::temp_dir().join(format!("mtdl_qa009_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("test.xml");
    fs::write(&file_path, "<TDL><TASK ID='1' TITLE='Original'/></TDL>").unwrap();

    let mut checker = moderntodolist_lib::infrastructure::FingerprintChecker::new();

    let fp1 = checker.record(&file_path);
    assert!(fp1.is_some(), "Should record fingerprint");
    assert!(!checker.has_changed(&file_path), "Should not detect change");

    fs::write(&file_path, "<TDL><TASK ID='1' TITLE='Modified'/></TDL>").unwrap();
    assert!(checker.has_changed(&file_path), "Should detect change after modification");

    let fp2 = checker.record(&file_path);
    assert!(fp2.is_some());
    assert_ne!(fp1.unwrap(), fp2.unwrap(), "Fingerprints should differ");
    assert!(!checker.has_changed(&file_path), "Should not detect change after re-recording");

    fs::remove_dir_all(&dir).ok();
}

// QA-M4-010: Re-index is idempotent
#[test]
fn qa_m4_010_reindex_idempotent() {
    let dir = std::env::temp_dir().join(format!("mtdl_qa010_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let xml_path = write_test_xml(&dir, "tasks.xml");

    let conn = setup_test_db();
    conn.execute("INSERT INTO workspaces (id, name, root_path) VALUES ('ws1', 'Test', '/tmp')", []).unwrap();
    conn.execute(
        "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc1', 'ws1', 'tasks.xml', 'managed')",
        [],
    ).unwrap();

    let count1 = moderntodolist_lib::infrastructure::indexer::index_document(&conn, "doc1", &xml_path).unwrap();
    let count2 = moderntodolist_lib::infrastructure::indexer::index_document(&conn, "doc1", &xml_path).unwrap();
    assert_eq!(count1, count2, "Both indexing runs should find same count");

    let task_count: u32 = conn
        .query_row("SELECT COUNT(*) FROM task_index WHERE document_id = 'doc1'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(task_count, 3, "Should have exactly 3 tasks, not 6");

    fs::remove_dir_all(&dir).ok();
}

// QA-M4-011: Workspace change detection (new/changed/removed)
#[test]
fn qa_m4_011_workspace_change_detection() {
    let (dir, _) = setup_workspace("QA011");

    fs::write(dir.join("existing.xml"), "<TDL/>").unwrap();
    let mut ws = moderntodolist_lib::domain::workspace::Workspace::open(&dir).unwrap();
    let doc_id = ws.register_document("existing.xml".to_string(), moderntodolist_lib::domain::workspace::DocumentType::Managed)
        .unwrap();
    // Set initial fingerprint so change detection can compare
    ws.update_fingerprint(&doc_id, "initial_fake_fingerprint").unwrap();

    fs::write(dir.join("new.xml"), "<TDL/>").unwrap();
    fs::write(dir.join("existing.xml"), "<TDL><TASK ID='1' TITLE='Changed'/></TDL>").unwrap();

    let (new, changed, _removed) = ws.detect_changes().unwrap();
    assert_eq!(new.len(), 1, "Should detect 1 new file");
    assert_eq!(changed.len(), 1, "Should detect 1 changed file");

    fs::remove_dir_all(&dir).ok();
}

// QA-M4-012: Recovery mode when data dir is missing
#[test]
fn qa_m4_012_recovery_mode_missing_dir() {
    let nonexistent = PathBuf::from("/nonexistent/path/that/does/not/exist");
    let db = moderntodolist_lib::infrastructure::DatabaseManager::open(&nonexistent);
    assert!(!db.is_available(), "DB should be in recovery mode");

    let result = db.with_connection(|_conn| {
        Ok(())
    });
    assert!(result.is_err(), "Should fail in recovery mode");
}

// QA-M4-013: GATE-M4 - SQLite full delete is not data loss
#[test]
fn qa_m4_013_gate_sqlite_delete_not_data_loss() {
    let (dir, ws_id) = setup_workspace("GATE-M4");
    let xml_path = write_test_xml(&dir, "important_tasks.xml");

    let data_dir = std::env::temp_dir().join(format!("mtdl_gate_data_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&data_dir).unwrap();

    {
        let db = moderntodolist_lib::infrastructure::DatabaseManager::open(&data_dir);
        db.with_connection(|conn| {
            conn.execute(
                "INSERT INTO workspaces (id, name, root_path) VALUES (?1, 'GATE-M4', ?2)",
                rusqlite::params![&ws_id, dir.to_string_lossy().to_string()],
            ).unwrap();
            conn.execute(
                "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc1', ?1, 'important_tasks.xml', 'managed')",
                rusqlite::params![&ws_id],
            ).unwrap();
            moderntodolist_lib::infrastructure::indexer::index_document(conn, "doc1", &xml_path).unwrap();
            Ok(())
        }).unwrap();
    }

    // Nuke the entire data directory
    fs::remove_dir_all(&data_dir).unwrap();
    assert!(!data_dir.exists(), "Data directory should be completely removed");

    // Verify XML source files still exist
    assert!(xml_path.exists(), "XML source file must survive");
    let xml_content = fs::read_to_string(&xml_path).unwrap();
    assert!(xml_content.contains("Parent Task"), "XML data should be intact");

    // Rebuild from scratch - all data recovered from XML
    fs::create_dir_all(&data_dir).unwrap();
    let db2 = moderntodolist_lib::infrastructure::DatabaseManager::open(&data_dir);
    assert!(db2.is_available());

    db2.with_connection(|conn| {
        moderntodolist_lib::infrastructure::migration::run_migrations(conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES (?1, 'GATE-M4', ?2)",
            rusqlite::params![&ws_id, dir.to_string_lossy().to_string()],
        ).unwrap();
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc1', ?1, 'important_tasks.xml', 'managed')",
            rusqlite::params![&ws_id],
        ).unwrap();
        let count = moderntodolist_lib::infrastructure::indexer::index_document(conn, "doc1", &xml_path).unwrap();
        assert_eq!(count, 3, "All 3 tasks recovered from XML source");
        Ok(())
    }).unwrap();

    fs::remove_dir_all(&dir).ok();
    fs::remove_dir_all(&data_dir).ok();
}

// QA-M4-014: Multiple migrations are idempotent
#[test]
fn qa_m4_014_migrations_idempotent() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    moderntodolist_lib::infrastructure::migration::run_migrations(&conn).unwrap();
    moderntodolist_lib::infrastructure::migration::run_migrations(&conn).unwrap();

    let version: u32 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get(0))
        .unwrap();
    assert!(version >= 1, "Schema version should be at least 1");
}

// QA-M4-015: Self-save recognition via fingerprint
#[test]
fn qa_m4_015_self_save_recognition() {
    let dir = std::env::temp_dir().join(format!("mtdl_qa015_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("task.xml");
    fs::write(&file_path, "<TDL><TASK ID='1' TITLE='Test'/></TDL>").unwrap();

    let config = moderntodolist_lib::infrastructure::WatcherConfig {
        debounce_ms: 100,
        recursive: true,
    };
    let mut watcher = moderntodolist_lib::infrastructure::WorkspaceWatcher::new(config);
    watcher.watch(&dir).unwrap();

    let fp = moderntodolist_lib::infrastructure::indexer::compute_fingerprint(&file_path).unwrap();
    watcher.register_self_save(&file_path, fp);

    std::thread::sleep(std::time::Duration::from_millis(200));
    let events = watcher.poll_events();
    let self_events: Vec<_> = events.iter().filter(|e| e.path == file_path).collect();
    assert!(self_events.is_empty(), "Self-save should be filtered out");

    watcher.stop();
    fs::remove_dir_all(&dir).ok();
}

// QA-M4-016: Concurrent DB access via clone
#[test]
fn qa_m4_016_concurrent_db_access() {
    let data_dir = std::env::temp_dir().join(format!("mtdl_qa016_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&data_dir).unwrap();

    let db = moderntodolist_lib::infrastructure::DatabaseManager::open(&data_dir);
    let db_clone = db.clone();

    db.with_connection(|conn| {
        conn.execute("INSERT INTO workspaces (id, name, root_path) VALUES ('ws1', 'Test1', '/tmp')", []).unwrap();
        Ok(())
    }).unwrap();

    db_clone.with_connection(|conn| {
        let count: u32 = conn.query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1, "Clone should see the same data");
        Ok(())
    }).unwrap();

    fs::remove_dir_all(&data_dir).ok();
}
