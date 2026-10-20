//! Command-layer integration tests for the M8 cross-document transfer IPC
//! (INH-1083). These drive the `*_core` functions behind the seven Tauri
//! commands against a real workspace + SQLite index — the same code path the
//! frontend exercises, minus the Tauri `State` plumbing.
//!
//! Coverage:
//! - list_transfer_targets: exclusion, writability flag.
//! - preview_transfer: counts, warnings, side-effect-free.
//! - copy: subtree lands in target, source untouched, index refreshed.
//! - move: source subtree removed only after target commit.
//! - undo: durable undo restores the target document.
//! - recover: no journals → empty; interrupted move → completed_with_duplicate.
//! - complete_interrupted_move: retained source copy removed on confirmation.
//! - failure paths: unknown document, same source/target.

use std::fs;
use std::path::PathBuf;

use moderntodolist_lib::domain::transfer::{
    execute_transfer, plan_transfer, ExternalDependencyPolicy, PlanOptions, TransferContext,
    TransferError, TransferHooks, TransferOperation, TransferPhase, TransferRecoveryAction,
};
use moderntodolist_lib::domain::types::TaskId;
use moderntodolist_lib::domain::workspace::{
    ensure_workspace_in_db, sync_documents_to_db, DocumentType, Workspace,
};
use moderntodolist_lib::domain::xml_parser::parse_xml;
use moderntodolist_lib::domain::xml_tree::{XmlElement, XmlNode};
use moderntodolist_lib::infrastructure::DatabaseError;
use moderntodolist_lib::transfer::{
    complete_interrupted_move_core, execute_transfer_core, list_transfer_targets_core,
    preview_transfer_core, recover_transfers_core, undo_transfer_core, TransferRequest,
};
use moderntodolist_lib::{AppState, WorkspaceState};

// ─────────────────────────── Harness ───────────────────────────

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate root should have a parent")
        .join("tests")
        .join("fixtures")
        .join("xml")
        .join("transfer")
}

struct Env {
    dir: PathBuf,
    app: AppState,
    ws_state: WorkspaceState,
    source_id: String,
    target_id: String,
    source: PathBuf,
    target: PathBuf,
}

/// Real workspace + index DB with the two transfer fixtures registered.
fn setup_env(name: &str) -> Env {
    let dir = std::env::temp_dir().join(format!(
        "mtdl_m8ipc_{}_{}_{}",
        name,
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).unwrap();
    let fx = fixtures_root();
    let source = dir.join("source-project.xml");
    let target = dir.join("target-project.xml");
    fs::copy(fx.join("source-project.xml"), &source).unwrap();
    fs::copy(fx.join("target-project.xml"), &target).unwrap();

    let mut ws = Workspace::create(&dir, format!("m8-ipc-{name}")).unwrap();
    let source_id = ws
        .register_document("source-project.xml".to_string(), DocumentType::Managed)
        .unwrap();
    let target_id = ws
        .register_document("target-project.xml".to_string(), DocumentType::Managed)
        .unwrap();
    ws.save().unwrap();

    let data_dir = dir.join("Data");
    fs::create_dir_all(&data_dir).unwrap();
    let ws_state = WorkspaceState::new(&data_dir);
    {
        let db = ws_state.db.lock().unwrap();
        assert!(db.is_available(), "index DB must open in the test harness");
        db.with_connection(|conn| {
            ensure_workspace_in_db(conn, &ws).map_err(|e| {
                DatabaseError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
            })?;
            sync_documents_to_db(conn, &ws).map_err(|e| {
                DatabaseError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
            })?;
            Ok(())
        })
        .unwrap();
    }
    *ws_state.workspace.lock().unwrap() = Some(ws);

    Env {
        dir,
        app: AppState::new(),
        ws_state,
        source_id,
        target_id,
        source,
        target,
    }
}

impl Env {
    fn req(&self, root_task_key: &str) -> TransferRequest {
        TransferRequest {
            source_document_id: self.source_id.clone(),
            target_document_id: self.target_id.clone(),
            root_task_key: root_task_key.to_string(),
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

fn task_titles(path: &std::path::Path) -> Vec<String> {
    let doc = parse_xml(&fs::read(path).unwrap()).unwrap();
    fn walk(el: &XmlElement, out: &mut Vec<String>) {
        if el.tag == "TASK" {
            out.push(el.get_attr("TITLE").unwrap_or("").to_string());
        }
        for child in &el.children {
            if let XmlNode::Element(child_el) = child {
                walk(child_el, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.root, &mut out);
    out
}

// ── list_transfer_targets ────────────────────────────────────────────────────

#[test]
fn list_targets_excludes_source_and_marks_writable() {
    let env = setup_env("targets");
    let targets =
        list_transfer_targets_core(&env.ws_state, Some(&env.source_id)).unwrap();
    assert_eq!(targets.len(), 1, "source excluded, only target remains");
    assert_eq!(targets[0].document_id, env.target_id);
    assert!(targets[0].writable, "fixture target is writable");
    assert!(targets[0].path.is_absolute(), "picker needs absolute paths");

    let all = list_transfer_targets_core(&env.ws_state, None).unwrap();
    assert_eq!(all.len(), 2);
}

// ── preview_transfer ─────────────────────────────────────────────────────────

#[test]
fn preview_reports_counts_and_is_side_effect_free() {
    let env = setup_env("preview");
    let src_before = fs::read(&env.source).unwrap();
    let tgt_before = fs::read(&env.target).unwrap();

    // Task 5 = "Release Epic" with children 6 and 7.
    let preview =
        preview_transfer_core(&env.ws_state, &env.req("5"), TransferOperation::Copy).unwrap();
    assert_eq!(preview.task_count, 3);
    assert_eq!(preview.operation, TransferOperation::Copy);

    assert_eq!(fs::read(&env.source).unwrap(), src_before, "preview must not write");
    assert_eq!(fs::read(&env.target).unwrap(), tgt_before, "preview must not write");
}

// ── copy ─────────────────────────────────────────────────────────────────────

#[test]
fn copy_subtree_keeps_source_and_reindexes() {
    let env = setup_env("copy");
    let outcome =
        execute_transfer_core(&env.app, &env.ws_state, &env.req("5"), TransferOperation::Copy, |_| {})
            .unwrap();

    assert_eq!(outcome.operation, TransferOperation::Copy);
    assert_eq!(outcome.target_task_ids.len(), 3);
    assert!(!outcome.transaction_id.is_empty());

    // Source untouched (all five original tasks still present).
    let src_titles = task_titles(&env.source);
    for title in ["Stay Behind", "Release Epic", "Build Installer", "Sign Binaries", "Unrelated Root"] {
        assert!(src_titles.iter().any(|t| t == title), "source lost {title}");
    }
    // Target gained the subtree (3 original + 3 copied).
    assert_eq!(task_titles(&env.target).len(), 6);

    // The index was refreshed: the copied tasks are queryable by document.
    let db = env.ws_state.db.lock().unwrap();
    let count: i64 = db
        .with_connection(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM task_index WHERE document_id = ?1",
                [&env.target_id],
                |r| r.get(0),
            )
            .map_err(DatabaseError::Sqlite)
        })
        .unwrap();
    assert_eq!(count, 6, "target document reindexed after copy");
}

// ── move ─────────────────────────────────────────────────────────────────────

#[test]
fn move_subtree_removes_source_after_commit() {
    let env = setup_env("move");
    let outcome =
        execute_transfer_core(&env.app, &env.ws_state, &env.req("5"), TransferOperation::Move, |_| {})
            .unwrap();

    assert_eq!(outcome.operation, TransferOperation::Move);
    assert_eq!(outcome.target_task_ids.len(), 3);

    let src_titles = task_titles(&env.source);
    assert_eq!(src_titles.len(), 2, "subtree removed from source");
    assert!(src_titles.iter().any(|t| t == "Stay Behind"));
    assert!(src_titles.iter().any(|t| t == "Unrelated Root"));
    assert_eq!(task_titles(&env.target).len(), 6);
}

// ── undo ─────────────────────────────────────────────────────────────────────

#[test]
fn undo_transfer_restores_target_document() {
    let env = setup_env("undo");

    let outcome =
        execute_transfer_core(&env.app, &env.ws_state, &env.req("5"), TransferOperation::Copy, |_| {})
            .unwrap();
    assert_eq!(task_titles(&env.target).len(), 6);

    let ack = undo_transfer_core(&env.app, &env.ws_state, &outcome.transaction_id).unwrap();
    assert!(ack.success, "undo failed: {:?}", ack.message);
    let titles = task_titles(&env.target);
    assert_eq!(titles.len(), 3, "copied subtree removed");
    for title in ["Target Task One", "Target Task Two", "Target Nested"] {
        assert!(titles.iter().any(|t| t == title), "target lost {title}");
    }
    // Note: byte-level restoration is intentionally NOT asserted — undo keeps
    // the id allocator's NEXTUNIQUEID advanced, which is the correct semantic.
}

// ── recovery ─────────────────────────────────────────────────────────────────

#[test]
fn recover_without_journals_is_empty() {
    let env = setup_env("recover_none");
    let reports = recover_transfers_core(&env.ws_state).unwrap();
    assert!(reports.is_empty());
}

/// Crashes a move right before CommitSource through the production hook seam,
/// then drives the command-layer recovery. The journal directory is the same
/// one `execute_transfer_core` uses (workspace-root derived).
fn crash_move_before_commit_source(env: &Env, txn: &str) {
    let ctx = TransferContext::for_workspace_root(&env.dir);
    let options = PlanOptions {
        operation: TransferOperation::Move,
        external_dependency_policy: ExternalDependencyPolicy::default(),
        transaction_id: Some(txn.to_string()),
        source_document_id: Some(env.source_id.clone()),
        target_document_id: Some(env.target_id.clone()),
    };
    let plan = plan_transfer(&env.source, &env.target, &TaskId::new("5"), &options).unwrap();
    let err = execute_transfer(
        plan,
        &ctx,
        &TransferHooks { fail_before: Some(TransferPhase::CommitSource) },
        |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(err, TransferError::SimulatedKill { .. }),
        "expected SimulatedKill, got {err:?}"
    );
}

#[test]
fn recover_interrupted_move_reports_duplicate() {
    let env = setup_env("recover_dup");
    crash_move_before_commit_source(&env, "txn-ipc-crash");

    let reports = recover_transfers_core(&env.ws_state).unwrap();
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!(report.transaction_id.as_deref(), Some("txn-ipc-crash"));
    assert_eq!(report.action, TransferRecoveryAction::CompletedWithDuplicate);
    assert!(!report.duplicate_task_ids.is_empty());

    // The overriding invariant: no task lost — both copies still exist.
    assert!(task_titles(&env.source).iter().any(|t| t == "Release Epic"));
    assert!(task_titles(&env.target).iter().any(|t| t == "Release Epic"));
}

#[test]
fn complete_interrupted_move_removes_source_copy() {
    let env = setup_env("complete_move");
    crash_move_before_commit_source(&env, "txn-ipc-complete");

    let report =
        complete_interrupted_move_core(&env.app, &env.ws_state, "txn-ipc-complete").unwrap();
    assert_eq!(report.action, TransferRecoveryAction::Completed);

    // Source copy removed; target keeps the subtree. No loss, no duplicate.
    assert!(!task_titles(&env.source).iter().any(|t| t == "Release Epic"));
    assert!(task_titles(&env.target).iter().any(|t| t == "Release Epic"));

    // Journal consumed: a second recovery scan is clean.
    assert!(recover_transfers_core(&env.ws_state).unwrap().is_empty());
}

// ── failure paths ────────────────────────────────────────────────────────────

#[test]
fn unknown_document_id_is_rejected() {
    let env = setup_env("unknown_doc");
    let req = TransferRequest {
        source_document_id: "no-such-doc".to_string(),
        target_document_id: env.target_id.clone(),
        root_task_key: "5".to_string(),
    };
    let err =
        execute_transfer_core(&env.app, &env.ws_state, &req, TransferOperation::Copy, |_| {})
            .unwrap_err();
    assert!(err.contains("not registered"), "unexpected error: {err}");
}

#[test]
fn same_source_and_target_is_rejected() {
    let env = setup_env("same_doc");
    let req = TransferRequest {
        source_document_id: env.source_id.clone(),
        target_document_id: env.source_id.clone(),
        root_task_key: "5".to_string(),
    };
    let err =
        execute_transfer_core(&env.app, &env.ws_state, &req, TransferOperation::Copy, |_| {})
            .unwrap_err();
    assert!(err.starts_with("failed:"), "unexpected error: {err}");
}

#[test]
fn unknown_task_key_is_rejected_before_any_write() {
    let env = setup_env("unknown_task");
    let src_before = fs::read(&env.source).unwrap();
    let tgt_before = fs::read(&env.target).unwrap();
    let err =
        execute_transfer_core(&env.app, &env.ws_state, &env.req("999"), TransferOperation::Copy, |_| {})
            .unwrap_err();
    assert!(err.starts_with("failed:"), "unexpected error: {err}");
    assert_eq!(fs::read(&env.source).unwrap(), src_before);
    assert_eq!(fs::read(&env.target).unwrap(), tgt_before);
}
