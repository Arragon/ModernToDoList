//! IPC bridge: M8 cross-document transfer UX (RD-M8-024~031).
//!
//! Thin wrappers over `domain::transfer`: the transaction protocol
//! (target-first commit, guarded source delete, journal recovery, undo) is
//! implemented and tested in the domain layer; this module only resolves
//! document paths, guards against dirty sessions, re-indexes the touched
//! documents and maps structured errors to UI-classifiable strings.
//!
//! Error string contract consumed by the frontend (`transfer-store.ts`):
//! - `failed_before_target_commit: …` — target never committed; source intact
//! - `recovery_required: …` — target committed but source removal did not
//!   finish; BOTH documents may hold the subtree (duplicate-not-loss)
//! - `session_dirty: …` — refused: unsaved changes in source or target
//! - `failed: …` — everything else (validation, planning, I/O)

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{Emitter, State, Window};

use crate::commands::bridge::{resolve_document_path, safe_task_id, MutationAck};
use crate::commands::session::AppState;
use crate::commands::workspace::WorkspaceState;
use crate::domain::transfer::{
    self, AssetClassification, ExternalDependencyPolicy, PlanOptions, TransferContext,
    TransferError, TransferOperation, TransferOutcome, TransferProgress, TransferRecoveryReport,
    TransferTargetInfo, TransferWarning,
};
use crate::domain::transfer_journal::TransferJournal;
use crate::infrastructure::indexer;

/// Tauri event carrying [`TransferProgress`] while a transfer runs.
pub const TRANSFER_PROGRESS_EVENT: &str = "transfer://progress";

// ── DTOs ─────────────────────────────────────────────────────────────────────

/// Side-effect-free preview shown in the confirmation step of the picker.
#[derive(Debug, Clone, Serialize)]
pub struct TransferPreviewDto {
    pub operation: TransferOperation,
    /// Number of tasks in the subtree (root included).
    pub task_count: usize,
    /// External dependencies that would degrade to External refs (or block).
    pub external_dependency_count: usize,
    /// Managed asset files that would be physically copied.
    pub asset_copy_count: usize,
    pub warnings: Vec<TransferWarning>,
}

/// Shared parameters for preview/copy/move.
#[derive(Debug, Clone)]
pub struct TransferRequest {
    pub source_document_id: String,
    pub target_document_id: String,
    pub root_task_key: String,
}

// ── Error classification ─────────────────────────────────────────────────────

fn classify_error(e: &TransferError) -> String {
    match e {
        TransferError::TargetCommitFailed { .. }
        | TransferError::TargetValidationFailed { .. }
        | TransferError::AssetCopyFailed { .. }
        | TransferError::TargetConcurrentlyModified { .. } => {
            format!("failed_before_target_commit: {e}")
        }
        TransferError::SourceConcurrentlyModified { transaction_id, .. } => {
            format!("recovery_required:{transaction_id}:{e}")
        }
        TransferError::SourceCommitFailed { .. } | TransferError::Journal(_) => {
            format!("recovery_required::{e}")
        }
        _ => format!("failed: {e}"),
    }
}

// ── Session guards ───────────────────────────────────────────────────────────

/// Refuses a transfer while the source or target document has an open session
/// with unsaved changes: the transfer writes the files directly, and a later
/// save of the stale session would silently overwrite it.
fn guard_sessions<P: AsRef<Path>>(app: &AppState, paths: &[P]) -> Result<(), String> {
    let sessions = app.sessions.lock().map_err(|_| "Lock error")?;
    for entry in sessions.values() {
        if entry.session.is_dirty()
            && paths.iter().any(|p| p.as_ref() == entry.file_path.as_path())
        {
            return Err(format!(
                "session_dirty: '{}' has unsaved changes; save or discard them before transferring",
                entry.file_path.display()
            ));
        }
    }
    Ok(())
}

/// Drops open sessions for the given paths so no stale in-memory state can be
/// saved over the transferred files. The next edit lazily reopens from disk.
fn drop_sessions_for<P: AsRef<Path>>(app: &AppState, paths: &[P]) {
    if let Ok(mut sessions) = app.sessions.lock() {
        let stale: Vec<u64> = sessions
            .iter()
            .filter(|(_, e)| paths.iter().any(|p| p.as_ref() == e.file_path.as_path()))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            sessions.remove(&id);
        }
    }
}

// ── Index maintenance ────────────────────────────────────────────────────────

fn reindex_document(ws_state: &WorkspaceState, document_id: &str, path: &Path) {
    if let Ok(db) = ws_state.db.lock() {
        if db.is_available() {
            let _ = db.with_connection(|conn| {
                if let Err(e) = indexer::index_document(conn, document_id, path) {
                    log::warn!("Reindex after transfer failed for {}: {}", path.display(), e);
                }
                Ok(())
            });
        }
    }
    if let Ok(fp) = indexer::compute_fingerprint(path) {
        if let Ok(mut guard) = ws_state.workspace.lock() {
            if let Some(ws) = guard.as_mut() {
                let _ = ws.update_fingerprint(document_id, &fp);
                let _ = ws.save();
            }
        }
    }
}

/// Re-indexes every registered document. Used after undo/recovery where the
/// affected set is cheaper to recompute wholesale than to track precisely.
fn reindex_all_documents(ws_state: &WorkspaceState) {
    let doc_infos: Vec<(String, PathBuf)> = {
        let guard = match ws_state.workspace.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match guard.as_ref() {
            Some(ws) => ws
                .documents()
                .iter()
                .map(|doc| (doc.id.clone(), ws.resolve_document_path(doc)))
                .collect(),
            None => Vec::new(),
        }
    };
    for (doc_id, abs_path) in &doc_infos {
        if abs_path.exists() {
            reindex_document(ws_state, doc_id, abs_path);
        }
    }
}

fn workspace_root(ws_state: &WorkspaceState) -> Result<PathBuf, String> {
    let guard = ws_state.workspace.lock().map_err(|_| "Lock error")?;
    guard
        .as_ref()
        .map(|ws| ws.root_path.clone())
        .ok_or_else(|| "No workspace open".to_string())
}

fn resolve_paths(
    ws_state: &WorkspaceState,
    req: &TransferRequest,
) -> Result<(PathBuf, PathBuf), String> {
    let root = workspace_root(ws_state)?;
    let db = ws_state.db.lock().map_err(|_| "Lock error")?;
    // The index stores workspace-relative paths for managed documents; the
    // transfer engine writes files directly, so always hand it absolute paths.
    let absolutize = |p: PathBuf| if p.is_absolute() { p } else { root.join(p) };
    let source = absolutize(resolve_document_path(&db, &req.source_document_id)?);
    let target = absolutize(resolve_document_path(&db, &req.target_document_id)?);
    if source == target {
        return Err("failed: source and target are the same document".to_string());
    }
    Ok((source, target))
}

// ── Core logic (driven directly by integration tests) ────────────────────────

pub fn list_transfer_targets_core(
    ws_state: &WorkspaceState,
    exclude_document_id: Option<&str>,
) -> Result<Vec<TransferTargetInfo>, String> {
    let guard = ws_state.workspace.lock().map_err(|_| "Lock error")?;
    let ws = guard.as_ref().ok_or("No workspace open")?;
    Ok(transfer::list_transfer_targets(ws, exclude_document_id))
}

pub fn preview_transfer_core(
    ws_state: &WorkspaceState,
    req: &TransferRequest,
    operation: TransferOperation,
) -> Result<TransferPreviewDto, String> {
    let (source_path, target_path) = resolve_paths(ws_state, req)?;
    let task_id = safe_task_id(&req.root_task_key)?;
    let options = PlanOptions {
        operation,
        source_document_id: Some(req.source_document_id.clone()),
        target_document_id: Some(req.target_document_id.clone()),
        ..Default::default()
    };
    let plan = transfer::plan_transfer(&source_path, &target_path, &task_id, &options)
        .map_err(|e| classify_error(&e))?;
    let external_dependency_count = plan
        .warnings
        .iter()
        .filter(|w| matches!(w, TransferWarning::ExternalDependencyPreserved { .. }))
        .count();
    let asset_copy_count = plan
        .manifest
        .asset_refs
        .iter()
        .filter(|a| a.classification == AssetClassification::CopyRequired)
        .count();
    Ok(TransferPreviewDto {
        operation,
        task_count: plan.manifest.task_ids.len(),
        external_dependency_count,
        asset_copy_count,
        warnings: plan.warnings,
    })
}

pub fn execute_transfer_core(
    app: &AppState,
    ws_state: &WorkspaceState,
    req: &TransferRequest,
    operation: TransferOperation,
    progress: impl FnMut(TransferProgress),
) -> Result<TransferOutcome, String> {
    let (source_path, target_path) = resolve_paths(ws_state, req)?;
    guard_sessions(app, &[&source_path, &target_path])?;

    let ctx = TransferContext::for_workspace_root(&workspace_root(ws_state)?);
    let options = PlanOptions {
        operation,
        external_dependency_policy: ExternalDependencyPolicy::PreserveAsExternal,
        source_document_id: Some(req.source_document_id.clone()),
        target_document_id: Some(req.target_document_id.clone()),
        ..Default::default()
    };
    let task_id = safe_task_id(&req.root_task_key)?;

    let result = match operation {
        TransferOperation::Copy => {
            transfer::copy_task(&source_path, &target_path, &task_id, &options, &ctx, progress)
        }
        TransferOperation::Move => {
            transfer::move_task(&source_path, &target_path, &task_id, &options, &ctx, progress)
        }
    }
    .map_err(|e| classify_error(&e));

    // Whatever the outcome, the files on disk are the truth: refresh the index
    // and drop stale sessions. This matters most for recovery_required errors,
    // where the target WAS committed but the error return would otherwise skip
    // reindexing and leave the UI showing pre-transfer content.
    reindex_document(ws_state, &req.source_document_id, &source_path);
    reindex_document(ws_state, &req.target_document_id, &target_path);
    drop_sessions_for(app, &[&source_path, &target_path]);
    result
}

pub fn undo_transfer_core(
    app: &AppState,
    ws_state: &WorkspaceState,
    transaction_id: &str,
) -> Result<MutationAck, String> {
    let ctx = TransferContext::for_workspace_root(&workspace_root(ws_state)?);
    let record = transfer::load_undo_record(&ctx, transaction_id)
        .map_err(|e| classify_error(&e))?;
    let paths = [&record.source_path, &record.target_path];
    guard_sessions(app, &paths)?;
    transfer::undo_transfer(&ctx, transaction_id).map_err(|e| classify_error(&e))?;
    reindex_all_documents(ws_state);
    drop_sessions_for(app, &paths);
    Ok(MutationAck::ok_with(
        0,
        format!("transfer {transaction_id} undone"),
    ))
}

pub fn recover_transfers_core(
    ws_state: &WorkspaceState,
) -> Result<Vec<TransferRecoveryReport>, String> {
    let root = workspace_root(ws_state)?;
    Ok(transfer::recover_interrupted_transfers(
        &TransferContext::for_workspace_root(&root).journal_dir,
    ))
}

pub fn complete_interrupted_move_core(
    app: &AppState,
    ws_state: &WorkspaceState,
    transaction_id: &str,
) -> Result<TransferRecoveryReport, String> {
    let root = workspace_root(ws_state)?;
    let journal_dir = TransferContext::for_workspace_root(&root).journal_dir;
    let journal_path = TransferJournal::path_for(&journal_dir, transaction_id);
    let paths: Vec<PathBuf> = TransferJournal::load(&journal_path)
        .map(|j| {
            let m = j.manifest();
            vec![m.source_doc.path.clone(), m.target_doc.path.clone()]
        })
        .unwrap_or_default();
    guard_sessions(app, &paths)?;
    let report = transfer::complete_interrupted_move(&journal_dir, transaction_id)
        .map_err(|e| classify_error(&e))?;
    reindex_all_documents(ws_state);
    drop_sessions_for(app, &paths);
    Ok(report)
}

// ── Tauri commands ───────────────────────────────────────────────────────────

/// Lists workspace documents that can receive a transfer (picker data).
#[tauri::command]
pub fn list_transfer_targets(
    state: State<'_, WorkspaceState>,
    exclude_document_id: Option<String>,
) -> Result<Vec<TransferTargetInfo>, String> {
    list_transfer_targets_core(&state, exclude_document_id.as_deref())
}

/// Side-effect-free plan preview for the confirmation step.
#[tauri::command]
pub fn preview_transfer(
    state: State<'_, WorkspaceState>,
    source_document_id: String,
    target_document_id: String,
    root_task_key: String,
    operation: String,
) -> Result<TransferPreviewDto, String> {
    let req = TransferRequest { source_document_id, target_document_id, root_task_key };
    preview_transfer_core(&state, &req, parse_operation(&operation)?)
}

/// Copies a task subtree into another document; the source is never modified.
#[tauri::command]
pub fn copy_task_between_documents(
    window: Window,
    app: State<'_, AppState>,
    state: State<'_, WorkspaceState>,
    source_document_id: String,
    target_document_id: String,
    root_task_key: String,
) -> Result<TransferOutcome, String> {
    let req = TransferRequest { source_document_id, target_document_id, root_task_key };
    execute_transfer_core(&app, &state, &req, TransferOperation::Copy, move |p| {
        let _ = window.emit(TRANSFER_PROGRESS_EVENT, &p);
    })
}

/// Moves a task subtree into another document (target-first commit, guarded
/// source delete). The UI must keep showing the source row until this returns.
#[tauri::command]
pub fn move_task_between_documents(
    window: Window,
    app: State<'_, AppState>,
    state: State<'_, WorkspaceState>,
    source_document_id: String,
    target_document_id: String,
    root_task_key: String,
) -> Result<TransferOutcome, String> {
    let req = TransferRequest { source_document_id, target_document_id, root_task_key };
    execute_transfer_core(&app, &state, &req, TransferOperation::Move, move |p| {
        let _ = window.emit(TRANSFER_PROGRESS_EVENT, &p);
    })
}

/// Undoes a completed transfer using its durable undo artifacts.
#[tauri::command]
pub fn undo_transfer(
    app: State<'_, AppState>,
    state: State<'_, WorkspaceState>,
    transaction_id: String,
) -> Result<MutationAck, String> {
    undo_transfer_core(&app, &state, &transaction_id)
}

/// Scans the transfer journal for interrupted transactions (startup / after
/// workspace open). Never deletes source data; duplicates are surfaced.
#[tauri::command]
pub fn recover_transfers(
    state: State<'_, WorkspaceState>,
) -> Result<Vec<TransferRecoveryReport>, String> {
    recover_transfers_core(&state)
}

/// Completes an interrupted move whose target committed but source retained:
/// removes the source subtree now that the user confirmed the duplicate.
#[tauri::command]
pub fn complete_interrupted_move(
    app: State<'_, AppState>,
    state: State<'_, WorkspaceState>,
    transaction_id: String,
) -> Result<TransferRecoveryReport, String> {
    complete_interrupted_move_core(&app, &state, &transaction_id)
}

fn parse_operation(raw: &str) -> Result<TransferOperation, String> {
    match raw {
        "copy" => Ok(TransferOperation::Copy),
        "move" => Ok(TransferOperation::Move),
        other => Err(format!("failed: unknown transfer operation '{other}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::session::SessionEntry;
    use crate::domain::command::UndoRedoManager;
    use crate::domain::session::DocumentSession;
    use crate::domain::task::TaskTree;
    use crate::domain::xml_parser::parse_xml;

    fn entry(file_path: PathBuf, dirty: bool) -> SessionEntry {
        let session = DocumentSession::new(None);
        // A fresh session starts dirty (revision 1, saved 0); record_save
        // settles it, record_mutation makes it dirty again.
        if dirty {
            session.record_mutation();
        } else {
            session.record_save();
        }
        let source_doc = parse_xml(br#"<?xml version="1.0"?><TODOLIST />"#).unwrap();
        SessionEntry {
            session,
            tree: TaskTree::new(),
            undo_manager: UndoRedoManager::new(),
            file_path,
            source_doc,
        }
    }

    #[test]
    fn dirty_session_blocks_transfer_with_session_dirty_prefix() {
        let app = AppState::new();
        let dirty_path = PathBuf::from("C:/ws/dirty.tdl");
        let clean_path = PathBuf::from("C:/ws/clean.tdl");
        app.sessions
            .lock()
            .unwrap()
            .insert(1, entry(dirty_path.clone(), true));
        app.sessions
            .lock()
            .unwrap()
            .insert(2, entry(clean_path.clone(), false));

        let err = guard_sessions(&app, &[&dirty_path]).unwrap_err();
        assert!(err.starts_with("session_dirty:"), "unexpected: {err}");

        // A clean session on the target does not block.
        assert!(guard_sessions(&app, &[&clean_path]).is_ok());
        // Unrelated paths are unaffected.
        let other = PathBuf::from("C:/ws/other.tdl");
        assert!(guard_sessions(&app, &[&other]).is_ok());
    }

    #[test]
    fn drop_sessions_removes_only_matching_paths() {
        let app = AppState::new();
        let keep_path = PathBuf::from("C:/ws/keep.tdl");
        let drop_path = PathBuf::from("C:/ws/drop.tdl");
        app.sessions
            .lock()
            .unwrap()
            .insert(1, entry(keep_path.clone(), false));
        app.sessions
            .lock()
            .unwrap()
            .insert(2, entry(drop_path.clone(), false));

        drop_sessions_for(&app, &[&drop_path]);

        let sessions = app.sessions.lock().unwrap();
        assert!(sessions.contains_key(&1), "unrelated session kept");
        assert!(!sessions.contains_key(&2), "transferred session dropped");
    }
}
