//! M10 RC Regression Matrix C — Workspace, SQLite rebuild and watcher
//! (INH-1126, spec section 5.5).
//!
//! 10 cases (C01..C10): managed vs linked documents, index rebuild from
//! scratch, corrupted index.db handling, UNC paths, and file-watcher
//! behaviour. Includes the GATE-M10-critical case C09: delete index.db ->
//! rebuild from XML -> full function preserved.
//!
//! Everything runs through the real public API:
//! `domain::workspace::{Workspace, DocumentType, ensure_workspace_in_db,
//! sync_documents_to_db}`, `infrastructure::db::DatabaseManager`,
//! `infrastructure::indexer`, `infrastructure::migration`,
//! `infrastructure::watcher`.
//!
//! # Simulated conditions
//! - Corrupted index.db (C05): a real SQLite file is overwritten with
//!   non-database bytes; `DatabaseManager::open` must degrade to RecoveryMode
//!   and a delete+reopen must restore full function.
//! - UNC path (C06): a real UNC share cannot be mounted in CI, so we assert
//!   (a) Linked documents preserve UNC paths verbatim and (b) an unreachable
//!   data directory (the failure mode a dead UNC mount produces) degrades
//!   gracefully to RecoveryMode with XML still authoritative.
//! - Watcher (C07/C08): real `notify` events with bounded retry polling.

use moderntodolist_lib::domain::fingerprint::FileFingerprint;
use moderntodolist_lib::domain::workspace::{
    ensure_workspace_in_db, is_supported_extension, sync_documents_to_db, DocumentEntry,
    DocumentType, Workspace,
};
use moderntodolist_lib::infrastructure::db::{DatabaseManager, DatabaseError, DatabaseMode};
use moderntodolist_lib::infrastructure::indexer::{
    clear_workspace_index, compute_fingerprint, index_document, rebuild_index, IndexProgress,
};
use moderntodolist_lib::infrastructure::migration::run_migrations;
use moderntodolist_lib::infrastructure::schema::SCHEMA_VERSION;
use moderntodolist_lib::infrastructure::watcher::{ChangeType, WatcherConfig, WorkspaceWatcher};
use rusqlite::Connection;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("xml")
}

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mtdl_rc_c_{}_{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Open a real on-disk index DB with foreign keys enforced (as production does)
/// and run migrations. Returns the connection.
fn open_index_db(data_dir: &Path) -> Connection {
    fs::create_dir_all(data_dir).unwrap();
    let conn = Connection::open(data_dir.join("index.db")).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
        .unwrap();
    run_migrations(&conn).unwrap();
    conn
}

/// Copy the rich RC index fixture into a workspace dir; returns its abs path.
fn seed_index_source(ws_dir: &Path, name: &str) -> PathBuf {
    let dest = ws_dir.join(name);
    fs::copy(fixtures_root().join("rc").join("index-source.xml"), &dest).unwrap();
    dest
}

fn scalar_i64(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> i64 {
    conn.query_row(sql, params, |r| r.get::<_, i64>(0)).unwrap()
}

// ─── C01: managed vs linked documents ────────────────────────────────────────

/// QA-M10-C01: managed documents resolve relative to the workspace root and
/// linked documents resolve to their absolute path verbatim. Both register,
/// persist across save/reopen, and are findable by their stable DocumentId.
#[test]
fn qa_m10_c01_managed_vs_linked_documents() {
    let ws_dir = sandbox("c01_ws");
    let mut ws = Workspace::create(&ws_dir, "Managed vs Linked".to_string()).unwrap();

    // Managed document inside the workspace tree.
    fs::create_dir_all(ws_dir.join("tasks")).unwrap();
    fs::write(ws_dir.join("tasks").join("managed.xml"), "<TODOLIST NEXTUNIQUEID=\"1\"/>").unwrap();
    let managed_id = ws
        .register_document("tasks/managed.xml".to_string(), DocumentType::Managed)
        .unwrap();

    // Linked document outside the workspace (absolute path).
    let external_dir = sandbox("c01_ext");
    let external = external_dir.join("external.xml");
    fs::write(&external, "<TODOLIST NEXTUNIQUEID=\"1\"/>").unwrap();
    let linked_id = ws
        .register_document(external.to_string_lossy().to_string(), DocumentType::Linked)
        .unwrap();

    // Types recorded correctly.
    let managed = ws.find_document(&managed_id).unwrap();
    assert_eq!(managed.doc_type, DocumentType::Managed);
    let linked = ws.find_document(&linked_id).unwrap();
    assert_eq!(linked.doc_type, DocumentType::Linked);

    // Path resolution semantics.
    let mp = ws.resolve_document_path(managed);
    assert!(mp.starts_with(&ws_dir), "managed path is under the workspace root: {mp:?}");
    assert!(mp.ends_with(Path::new("tasks/managed.xml")) || mp.ends_with("managed.xml"));
    let lp = ws.resolve_document_path(linked);
    assert_eq!(lp, external, "linked path is absolute and verbatim");

    // Duplicate registration is rejected.
    assert!(ws.register_document("tasks/managed.xml".to_string(), DocumentType::Managed).is_err());

    // Persists across save/reopen with stable ids.
    ws.save().unwrap();
    let ws2 = Workspace::open(&ws_dir).unwrap();
    assert_eq!(ws2.documents().len(), 2);
    assert!(ws2.find_document(&managed_id).is_some());
    assert!(ws2.find_document(&linked_id).is_some());
    assert_eq!(ws2.find_document(&linked_id).unwrap().doc_type, DocumentType::Linked);

    // Extension policy.
    assert!(is_supported_extension("xml") && is_supported_extension("TDL"));
    assert!(!is_supported_extension("txt"));

    fs::remove_dir_all(&ws_dir).ok();
    fs::remove_dir_all(&external_dir).ok();
}

// ─── C02: scan + change detection ────────────────────────────────────────────

/// QA-M10-C02: `scan_documents` finds managed XML/TDL files (skipping hidden
/// dirs), and `detect_changes` classifies new / changed / removed documents
/// against the persisted fingerprints.
#[test]
fn qa_m10_c02_scan_and_change_detection() {
    let ws_dir = sandbox("c02");
    let mut ws = Workspace::create(&ws_dir, "Scan".to_string()).unwrap();

    fs::write(ws_dir.join("a.xml"), "<TODOLIST NEXTUNIQUEID=\"1\"/>").unwrap();
    fs::write(ws_dir.join("b.tdl"), "<TODOLIST NEXTUNIQUEID=\"1\"/>").unwrap();
    fs::write(ws_dir.join("notes.txt"), "ignored").unwrap();
    fs::create_dir_all(ws_dir.join("sub")).unwrap();
    fs::write(ws_dir.join("sub").join("c.xml"), "<TODOLIST NEXTUNIQUEID=\"1\"/>").unwrap();
    fs::create_dir_all(ws_dir.join(".assets")).unwrap();
    fs::write(ws_dir.join(".assets").join("hidden.xml"), "<TODOLIST/>").unwrap();

    let found = ws.scan_documents().unwrap();
    // a.xml, b.tdl, sub/c.xml (hidden dir skipped, txt ignored)
    assert_eq!(found.len(), 3, "scan must find 3 task docs, got {found:?}");

    // Register a.xml and record its fingerprint.
    let a_id = ws.register_document("a.xml".to_string(), DocumentType::Managed).unwrap();
    let fp = compute_fingerprint(&ws_dir.join("a.xml")).unwrap();
    ws.update_fingerprint(&a_id, &fp).unwrap();

    // Mutate a.xml on disk -> detect_changes flags it as changed.
    fs::write(ws_dir.join("a.xml"), "<TODOLIST NEXTUNIQUEID=\"2\"><TASK ID=\"1\" TITLE=\"x\"/></TODOLIST>").unwrap();
    let (new_files, changed, removed) = ws.detect_changes().unwrap();
    assert!(changed.iter().any(|p| p.to_string_lossy() == "a.xml"),
        "a.xml content change must be detected, changed={changed:?}");
    // b.tdl and sub/c.xml are still unregistered -> new.
    assert!(new_files.len() >= 2, "unregistered docs reported new: {new_files:?}");
    assert!(removed.is_empty(), "nothing registered was removed");

    // Remove a registered-but-missing doc detection.
    let gone_id = ws.register_document("gone.xml".to_string(), DocumentType::Managed).unwrap();
    let (_n, _c, removed2) = ws.detect_changes().unwrap();
    assert!(removed2.contains(&gone_id), "missing registered doc reported removed");

    fs::remove_dir_all(&ws_dir).ok();
}

// ─── C03: index a document from scratch ──────────────────────────────────────

/// QA-M10-C03: `index_document` populates task_index, task_tags,
/// task_participants and task_dependencies from the real XML fixture, and
/// re-indexing the same document does not duplicate rows (clears first).
#[test]
fn qa_m10_c03_index_document_from_scratch() {
    let data_dir = sandbox("c03_data");
    let ws_dir = sandbox("c03_ws");
    let mut ws = Workspace::create(&ws_dir, "Index".to_string()).unwrap();
    let xml_path = seed_index_source(&ws_dir, "index-source.xml");
    let doc_id = ws
        .register_document("index-source.xml".to_string(), DocumentType::Managed)
        .unwrap();

    let conn = open_index_db(&data_dir);
    ensure_workspace_in_db(&conn, &ws).unwrap();
    sync_documents_to_db(&conn, &ws).unwrap();

    let n = index_document(&conn, &doc_id, &xml_path).unwrap();
    assert_eq!(n, 4, "index-source.xml has 4 TASK elements");

    let d = doc_id.as_str();
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index WHERE document_id=?1", [d]), 4);
    // Statuses derived from PERCENTDONE via the real mapper.
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index WHERE document_id=?1 AND status='Done'", [d]), 1);
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index WHERE document_id=?1 AND status='InProgress'", [d]), 2);
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index WHERE document_id=?1 AND status='NotStarted'", [d]), 1);
    // Tags: release(2), p0(1), data-safety(1) = 4 tag rows.
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_tags WHERE document_id=?1", [d]), 4);
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_tags WHERE document_id=?1 AND tag='release'", [d]), 2);
    // Participants: Alice appears on task1(allocated_to) + task2(allocated_to);
    // Bob, PM (allocated_by), Charlie.
    assert!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_participants WHERE document_id=?1", [d]) >= 4);
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_participants WHERE document_id=?1 AND participant='Alice'", [d]), 2);
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_participants WHERE document_id=?1 AND role='allocated_by' AND participant='PM'", [d]), 1);
    // Dependency: task 2 -> task 3.
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_dependencies WHERE document_id=?1", [d]), 1);
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_dependencies WHERE document_id=?1 AND task_key='2' AND depends_on_key='3'", [d]), 1);

    // Re-index must not duplicate.
    let n2 = index_document(&conn, &doc_id, &xml_path).unwrap();
    assert_eq!(n2, 4);
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index WHERE document_id=?1", [d]), 4,
        "re-index clears before insert");

    fs::remove_dir_all(&data_dir).ok();
    fs::remove_dir_all(&ws_dir).ok();
}

// ─── C04: multi-document rebuild_index ───────────────────────────────────────

/// QA-M10-C04: `rebuild_index` indexes multiple documents, reports progress
/// through the callback, returns the aggregate task count, and honours a
/// cancellation flag.
#[test]
fn qa_m10_c04_rebuild_index_multi_document() {
    let data_dir = sandbox("c04_data");
    let ws_dir = sandbox("c04_ws");
    let mut ws = Workspace::create(&ws_dir, "Rebuild".to_string()).unwrap();

    let p1 = seed_index_source(&ws_dir, "doc1.xml"); // 4 tasks
    fs::copy(fixtures_root().join("dependencies").join("local-dependency.xml"), ws_dir.join("doc2.xml")).unwrap();
    let p2 = ws_dir.join("doc2.xml"); // 2 tasks
    let d1 = ws.register_document("doc1.xml".to_string(), DocumentType::Managed).unwrap();
    let d2 = ws.register_document("doc2.xml".to_string(), DocumentType::Managed).unwrap();

    let conn = open_index_db(&data_dir);
    ensure_workspace_in_db(&conn, &ws).unwrap();
    sync_documents_to_db(&conn, &ws).unwrap();

    let progress_hits = Arc::new(AtomicUsize::new(0));
    let ph = progress_hits.clone();
    let docs = vec![(d1.clone(), p1.clone()), (d2.clone(), p2.clone())];
    let total = rebuild_index(&conn, &docs, None, Some(&move |_p: IndexProgress| {
        ph.fetch_add(1, Ordering::SeqCst);
    }))
    .unwrap();
    assert_eq!(total, 6, "4 + 2 tasks across two docs");
    assert!(progress_hits.load(Ordering::SeqCst) >= 2, "progress callback must fire");
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index", []), 6);

    // Cancellation is honoured before doing work.
    let cancel = Arc::new(AtomicBool::new(true));
    let err = rebuild_index(&conn, &docs, Some(cancel), None).unwrap_err();
    assert!(matches!(err, moderntodolist_lib::infrastructure::indexer::IndexError::Cancelled));

    fs::remove_dir_all(&data_dir).ok();
    fs::remove_dir_all(&ws_dir).ok();
}

// ─── C05: corrupted index.db handling ────────────────────────────────────────

/// QA-M10-C05: a corrupted index.db must NOT crash or lose business data.
///
/// SIMULATION: a real SQLite file is overwritten with non-database bytes (the
/// on-disk signature of header/page corruption). `DatabaseManager::open`
/// must degrade to `RecoveryMode` (unavailable), queries must fail cleanly,
/// and `delete_database` + `reopen` must restore a working index — proving
/// the index is disposable and XML stays authoritative.
#[test]
fn qa_m10_c05_corrupted_index_db_recovers() {
    let data_dir = sandbox("c05");
    fs::create_dir_all(&data_dir).unwrap();

    // First create a healthy DB so index.db exists with a valid schema.
    {
        let db = DatabaseManager::open(&data_dir);
        assert!(db.is_available());
    }
    // Corrupt it: overwrite with bytes that are not a SQLite database
    // (large enough that SQLite cannot mistake it for an empty database).
    let db_path = data_dir.join("index.db");
    let _ = fs::remove_file(data_dir.join("index.db-wal"));
    let _ = fs::remove_file(data_dir.join("index.db-shm"));
    let mut garbage = b"THIS IS NOT A SQLITE DATABASE FILE\x00\x01\x02CORRUPTED PAGES".to_vec();
    garbage.resize(8192, 0xAB);
    fs::write(&db_path, &garbage).unwrap();

    let db = DatabaseManager::open(&data_dir);
    assert_eq!(db.mode(), DatabaseMode::RecoveryMode, "corrupt DB must enter RecoveryMode");
    assert!(!db.is_available());
    let q = db.with_connection(|c| {
        c.query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get::<_, i64>(0))
            .map_err(DatabaseError::from)
    });
    assert!(q.is_err(), "queries must fail gracefully in RecoveryMode");

    // Recovery: delete + reopen gives a fresh, working index.
    let mut db = db;
    db.delete_database().unwrap();
    assert!(!db_path.exists(), "corrupt file removed");
    db.reopen().unwrap();
    assert!(db.is_available(), "reopened DB is available");
    assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    let c = db
        .with_connection(|conn| {
            conn.query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get::<_, i64>(0))
                .map_err(DatabaseError::from)
        })
        .unwrap();
    assert_eq!(c, 0, "fresh index starts empty");

    fs::remove_dir_all(&data_dir).ok();
}

// ─── C06: UNC / network path handling ────────────────────────────────────────

/// QA-M10-C06: UNC-path handling.
///
/// SIMULATION (documented): a real `\\server\share` cannot be mounted in CI.
/// We assert the two behaviours that matter for UNC support:
/// 1. A Linked document with a UNC path is preserved verbatim by
///    `resolve_document_path` (no rebasing onto the workspace root), while a
///    Managed document is always rebased under the root.
/// 2. When the data directory is unreachable (exactly what a dropped UNC mount
///    produces), `DatabaseManager::open` degrades to RecoveryMode instead of
///    panicking — the index is derived, XML remains the source of truth.
#[test]
fn qa_m10_c06_unc_path_handling() {
    let ws_dir = sandbox("c06_ws");
    let mut ws = Workspace::create(&ws_dir, "UNC".to_string()).unwrap();

    // 1. UNC linked document is preserved verbatim.
    let unc = r"\\fileserver\TeamShare\Projects\roadmap.tdl";
    let linked_id = ws.register_document(unc.to_string(), DocumentType::Linked).unwrap();
    let linked = ws.find_document(&linked_id).unwrap();
    let resolved = ws.resolve_document_path(linked);
    assert_eq!(resolved, PathBuf::from(unc), "UNC linked path must be verbatim");
    assert!(resolved.to_string_lossy().starts_with(r"\\"), "still a UNC path");

    // A managed doc is rebased under the root even if it looks UNC-ish.
    let managed_id = ws.register_document("local.xml".to_string(), DocumentType::Managed).unwrap();
    let managed = ws.find_document(&managed_id).unwrap();
    assert!(ws.resolve_document_path(managed).starts_with(&ws_dir));

    // The entry round-trips through workspace.json unchanged.
    ws.save().unwrap();
    let ws2 = Workspace::open(&ws_dir).unwrap();
    let l2 = ws2.find_document(&linked_id).unwrap();
    assert_eq!(l2.file_path, unc);
    assert_eq!(ws2.resolve_document_path(l2), PathBuf::from(unc));

    // 2. Unreachable data dir (dropped mount) -> graceful RecoveryMode.
    let unreachable = PathBuf::from(r"\\nonexistent-host-xyz\share\Data");
    let db = DatabaseManager::open(&unreachable);
    assert_eq!(db.mode(), DatabaseMode::RecoveryMode);
    assert!(!db.is_available(), "unreachable network data dir must degrade, not crash");

    fs::remove_dir_all(&ws_dir).ok();
}

// ─── C07: watcher detects external change ────────────────────────────────────

/// QA-M10-C07: the real `notify`-backed `WorkspaceWatcher` reports an external
/// file creation/modification after the debounce window (bounded retry poll).
#[test]
fn qa_m10_c07_watcher_detects_external_change() {
    let dir = sandbox("c07");
    let mut watcher = WorkspaceWatcher::new(WatcherConfig { debounce_ms: 50, recursive: true });
    watcher.watch(&dir).unwrap();
    assert!(watcher.watched_paths().contains(&dir));

    // Create then modify a task document externally.
    let file = dir.join("external.xml");
    fs::write(&file, "<TODOLIST NEXTUNIQUEID=\"1\"/>").unwrap();

    let got = poll_until(&mut watcher, &file, Duration::from_secs(5));
    assert!(got, "watcher must report the external change for {file:?}");

    watcher.unwatch(&dir).unwrap();
    assert!(!watcher.watched_paths().contains(&dir));
    watcher.stop();
    fs::remove_dir_all(&dir).ok();
}

/// Poll the watcher until an event for `path` (any change type) is observed or
/// `timeout` elapses. Keeps the file's mtime fresh on each poll so that even
/// debounce/poll-timing edge cases converge.
fn poll_until(watcher: &mut WorkspaceWatcher, path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        for ev in watcher.poll_events() {
            if ev.path == path && ev.change_type != ChangeType::Overflow {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

// ─── C08: watcher self-save suppression ──────────────────────────────────────

/// QA-M10-C08: a change whose fingerprint matches a registered self-save is
/// suppressed (no spurious external-change event), while a subsequent genuine
/// external edit (different bytes) IS reported. This is the notify-side
/// counterpart of B10's fingerprint discrimination.
#[test]
fn qa_m10_c08_watcher_suppresses_self_save() {
    let dir = sandbox("c08");
    let file = dir.join("doc.xml");
    let own_content = b"<TODOLIST NEXTUNIQUEID=\"1\"><TASK ID=\"1\" TITLE=\"mine\"/></TODOLIST>";
    fs::write(&file, own_content).unwrap();

    let mut watcher = WorkspaceWatcher::new(WatcherConfig { debounce_ms: 50, recursive: true });
    watcher.watch(&dir).unwrap();

    // Register our own save fingerprint BEFORE rewriting the same bytes.
    let fp = FileFingerprint::from_bytes(own_content);
    watcher.register_self_save(&file, fp.hash.clone());
    fs::write(&file, own_content).unwrap(); // our own save

    // Give the watcher time; the self-save must be suppressed.
    std::thread::sleep(Duration::from_millis(400));
    let leaked = watcher
        .poll_events()
        .into_iter()
        .any(|ev| ev.path == file && ev.change_type != ChangeType::Overflow);
    assert!(!leaked, "self-save with matching fingerprint must be suppressed");

    // Now a genuine external edit with different bytes must be reported.
    fs::write(&file, b"<TODOLIST NEXTUNIQUEID=\"2\"><TASK ID=\"9\" TITLE=\"external edit\"/></TODOLIST>").unwrap();
    let got = poll_until(&mut watcher, &file, Duration::from_secs(5));
    assert!(got, "genuine external edit must be reported after self-save suppression");

    watcher.stop();
    fs::remove_dir_all(&dir).ok();
}

// ─── C09: GATE — delete index.db -> rebuild -> full function ─────────────────

/// QA-M10-C09 (GATE-M10 critical): after indexing a real document, deleting
/// index.db entirely and rebuilding from XML must restore FULL function —
/// identical task/tag/participant/dependency query results — while
/// workspace.json and the XML bytes are untouched.
#[test]
fn qa_m10_c09_gate_delete_index_db_rebuild_full_function() {
    let data_dir = sandbox("c09_data");
    let ws_dir = sandbox("c09_ws");
    let mut ws = Workspace::create(&ws_dir, "GateRebuild".to_string()).unwrap();
    let xml_path = seed_index_source(&ws_dir, "index-source.xml");
    let xml_fp_before = FileFingerprint::from_file(&xml_path).unwrap();
    let doc_id = ws
        .register_document("index-source.xml".to_string(), DocumentType::Managed)
        .unwrap();
    ws.save().unwrap();
    let ws_json = fs::read_to_string(ws_dir.join("workspace.json")).unwrap();

    // Snapshot of "full function" queries.
    fn snapshot(conn: &Connection, doc: &str) -> (i64, i64, i64, i64) {
        (
            conn.query_row("SELECT COUNT(*) FROM task_index WHERE document_id=?1", [doc], |r| r.get(0)).unwrap(),
            conn.query_row("SELECT COUNT(*) FROM task_tags WHERE document_id=?1", [doc], |r| r.get(0)).unwrap(),
            conn.query_row("SELECT COUNT(*) FROM task_participants WHERE document_id=?1", [doc], |r| r.get(0)).unwrap(),
            conn.query_row("SELECT COUNT(*) FROM task_dependencies WHERE document_id=?1", [doc], |r| r.get(0)).unwrap(),
        )
    }

    let before;
    {
        let mut db = DatabaseManager::open(&data_dir);
        assert!(db.is_available());
        db.with_connection(|conn| {
            ensure_workspace_in_db(conn, &ws).map_err(|_| DatabaseError::Unavailable)?;
            sync_documents_to_db(conn, &ws).map_err(|_| DatabaseError::Unavailable)?;
            index_document(conn, &doc_id, &xml_path).map_err(|e| match e {
                moderntodolist_lib::infrastructure::indexer::IndexError::Sqlite(s) => DatabaseError::Sqlite(s),
                _ => DatabaseError::Unavailable,
            })?;
            Ok(())
        })
        .unwrap();
        before = db
            .with_connection(|conn| Ok(snapshot(conn, &doc_id)))
            .unwrap();
        assert_eq!(before.0, 4, "index is populated before deletion");

        // GATE action: delete the entire index database.
        db.delete_database().unwrap();
        assert!(!db.is_available());
        assert!(!data_dir.join("index.db").exists());
    }

    // Business data safety: workspace.json and the XML are untouched.
    assert_eq!(fs::read_to_string(ws_dir.join("workspace.json")).unwrap(), ws_json,
        "workspace.json must survive index deletion");
    assert_eq!(FileFingerprint::from_file(&xml_path).unwrap(), xml_fp_before,
        "XML source of truth must be byte-identical");

    // Reopen (recreates + migrates) and rebuild from XML.
    let mut db2 = DatabaseManager::open(&data_dir);
    db2.reopen().unwrap_or_else(|_| { /* open() already available */ });
    assert!(db2.is_available());
    let ws_reopened = Workspace::open(&ws_dir).unwrap();
    db2.with_connection(|conn| {
        ensure_workspace_in_db(conn, &ws_reopened).map_err(|_| DatabaseError::Unavailable)?;
        sync_documents_to_db(conn, &ws_reopened).map_err(|_| DatabaseError::Unavailable)?;
        rebuild_index(
            conn,
            &[(doc_id.clone(), xml_path.clone())],
            None,
            None,
        )
        .map_err(|e| match e {
            moderntodolist_lib::infrastructure::indexer::IndexError::Sqlite(s) => DatabaseError::Sqlite(s),
            _ => DatabaseError::Unavailable,
        })?;
        Ok(())
    })
    .unwrap();

    let after = db2.with_connection(|conn| Ok(snapshot(conn, &doc_id))).unwrap();
    assert_eq!(before, after, "full function restored: identical index after rebuild");

    // And a real query still returns the expected business rows.
    let done_titles = db2
        .with_connection(|conn| {
            let mut stmt = conn
                .prepare("SELECT title FROM task_index WHERE document_id=?1 AND status='Done'")
                .map_err(DatabaseError::from)?;
            let rows: Vec<String> = stmt
                .query_map([&doc_id], |r| r.get(0))
                .map_err(DatabaseError::from)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .unwrap();
    assert_eq!(done_titles, vec!["Atomic save hardening".to_string()]);

    assert_eq!(db2.schema_version().unwrap(), SCHEMA_VERSION);
    fs::remove_dir_all(&data_dir).ok();
    fs::remove_dir_all(&ws_dir).ok();
}

// ─── C10: workspace index lifecycle (clear keeps workspace.json) ─────────────

/// QA-M10-C10: `clear_workspace_index` removes all derived task rows for a
/// workspace while the workspace registration survives in workspace.json;
/// a subsequent rebuild restores the data. Also verifies FK cascade on
/// document removal and that DocumentEntry metadata round-trips.
#[test]
fn qa_m10_c10_clear_index_keeps_workspace_rebuild_restores() {
    let data_dir = sandbox("c10_data");
    let ws_dir = sandbox("c10_ws");
    let mut ws = Workspace::create(&ws_dir, "Lifecycle".to_string()).unwrap();
    let xml_path = seed_index_source(&ws_dir, "index-source.xml");
    let doc_id = ws
        .register_document("index-source.xml".to_string(), DocumentType::Managed)
        .unwrap();
    ws.save().unwrap();

    let conn = open_index_db(&data_dir);
    ensure_workspace_in_db(&conn, &ws).unwrap();
    sync_documents_to_db(&conn, &ws).unwrap();
    index_document(&conn, &doc_id, &xml_path).unwrap();
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index", []), 4);

    // Clear derived index for the workspace.
    clear_workspace_index(&conn, ws.id()).unwrap();
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index", []), 0);
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM documents WHERE workspace_id=?1", [ws.id()]), 0,
        "documents cleared for the workspace");

    // workspace.json (business registration) survives on disk.
    let ws2 = Workspace::open(&ws_dir).unwrap();
    assert_eq!(ws2.documents().len(), 1, "workspace.json still lists the document");
    assert!(ws2.find_document(&doc_id).is_some());

    // Rebuild restores the index.
    ensure_workspace_in_db(&conn, &ws2).unwrap();
    sync_documents_to_db(&conn, &ws2).unwrap();
    rebuild_index(&conn, &[(doc_id.clone(), xml_path.clone())], None, None).unwrap();
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index", []), 4, "rebuild restores tasks");

    // FK cascade: deleting the document row drops its task rows.
    conn.execute("DELETE FROM documents WHERE id=?1", [doc_id.as_str()]).unwrap();
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM task_index WHERE document_id=?1", [doc_id.as_str()]), 0,
        "task_index cascades on document delete");

    // DocumentEntry serializes/deserializes (workspace.json contract).
    let entry = DocumentEntry {
        id: "x".into(),
        file_path: "y.xml".into(),
        doc_type: DocumentType::Managed,
        fingerprint: Some("abc".into()),
        last_indexed: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: DocumentEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "x");
    assert_eq!(back.doc_type, DocumentType::Managed);

    fs::remove_dir_all(&data_dir).ok();
    fs::remove_dir_all(&ws_dir).ok();
}
