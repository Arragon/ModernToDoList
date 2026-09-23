//! IPC bridge layer: shared DTOs, document/tree resolution helpers and the
//! attachment open/reveal commands.
//!
//! The 27 commands the frontend invokes but that had no backend are split
//! across three modules:
//! - [`crate::commands::task_edit`] — field edits, add/delete, tags, quick add,
//!   global search.
//! - [`crate::commands::relations`] — participants, dependencies, progress
//!   links, attachments.
//! - this module — the DTOs both of the above serialize to (matching
//!   `src/ipc/types.ts` byte-for-byte), the read helpers that resolve a
//!   `document_id` to a parsed [`TaskTree`], and the two shell-integration
//!   commands (`open_attachment` / `reveal_attachment`).
//!
//! Design notes
//! ------------
//! *Mutations* run against the live [`SessionEntry`] tree through the
//! `UndoRedoManager` so Ctrl+Z reverses them and `record_mutation()` marks the
//! session dirty. *Reads* carry only `document_id`/`task_key` (no session id),
//! so they resolve the document's file path from the workspace index and parse
//! the saved XML. This keeps reads independent of the (partially populated)
//! relation index tables — `index_document` never fills `progress_links_index`
//! or `attachments_index`, so parsing the source document is the only complete
//! source. The frontend overlays unsaved edits from its own cache.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;
use tauri_plugin_shell::ShellExt;

use crate::commands::session::AppState;
use crate::commands::workspace::WorkspaceState;
use crate::domain::attachment::{self, AttachmentKind, AttachmentRef};
use crate::domain::dependency::{DependencyGraph, TaskRef};
use crate::domain::mappers::read_task;
use crate::domain::progress_link::ProgressLink;
use crate::domain::task::{TaskDependency, TaskTree};
use crate::domain::types::TaskId;
use crate::domain::xml_parser::parse_xml;
use crate::domain::xml_tree::XmlElement;
use crate::infrastructure::DatabaseManager;

// ── Shared response DTOs (must match src/ipc/types.ts exactly) ────────────────

/// Generic acknowledgement for relation mutations that carry no payload.
#[derive(Debug, Clone, Serialize)]
pub struct MutationAck {
    pub success: bool,
    pub revision: u64,
    pub message: Option<String>,
}

impl MutationAck {
    /// A successful acknowledgement at `revision`.
    pub fn ok(revision: u64) -> Self {
        Self { success: true, revision, message: None }
    }
    /// A successful acknowledgement carrying an informational message.
    pub fn ok_with(revision: u64, message: impl Into<String>) -> Self {
        Self { success: true, revision, message: Some(message.into()) }
    }
}

/// One participant row (M6).
#[derive(Debug, Clone, Serialize)]
pub struct ParticipantDto {
    pub task_key: String,
    pub document_id: String,
    pub display_name: String,
    pub role: String,
}

/// One dependency edge (M6).
#[derive(Debug, Clone, Serialize)]
pub struct DependencyDto {
    pub task_key: String,
    pub document_id: String,
    pub depends_on_key: String,
    pub depends_on_document_id: Option<String>,
    pub dep_type: u8,
    pub ref_kind: String,
    pub circular: bool,
    pub raw_ref: Option<String>,
}

/// Outgoing + incoming dependency edges for one task (M6).
#[derive(Debug, Clone, Serialize)]
pub struct DependencyGraphDto {
    pub outgoing: Vec<DependencyDto>,
    pub incoming: Vec<DependencyDto>,
    pub blocked: bool,
}

/// One progress link (M6).
#[derive(Debug, Clone, Serialize)]
pub struct ProgressLinkDto {
    pub id: String,
    pub task_key: String,
    pub document_id: String,
    pub label: String,
    pub url: String,
    pub provider: String,
}

/// One attachment (M6).
#[derive(Debug, Clone, Serialize)]
pub struct AttachmentDto {
    pub id: String,
    pub task_key: String,
    pub document_id: String,
    pub kind: String,
    pub display_name: String,
    pub path_or_url: String,
    pub size: Option<u64>,
    pub hash: Option<String>,
    pub exists: bool,
    pub status: String,
}

/// Result of importing/adding an attachment (M6).
#[derive(Debug, Clone, Serialize)]
pub struct AttachmentImportResult {
    pub success: bool,
    pub attachment: Option<AttachmentDto>,
    pub message: Option<String>,
}

/// One global-search hit (M9).
#[derive(Debug, Clone, Serialize)]
pub struct SearchHitDto {
    pub task_key: String,
    pub document_id: String,
    pub title: String,
    pub document_path: Option<String>,
    pub matched_field: String,
    pub snippet: String,
    pub score: f64,
}

/// Global-search response page (M9).
#[derive(Debug, Clone, Serialize)]
pub struct SearchResponseDto {
    pub hits: Vec<SearchHitDto>,
    pub total: usize,
    pub truncated: bool,
}

// ── Small helpers ─────────────────────────────────────────────────────────────

/// Builds a [`TaskId`] from a raw key without panicking on empty input
/// (`TaskId::new` asserts non-empty; IPC input must never panic the backend).
pub fn safe_task_id(key: &str) -> Result<TaskId, String> {
    if key.trim().is_empty() {
        return Err("task_key must not be empty".to_string());
    }
    Ok(TaskId::new(key))
}

/// Parses XML bytes into a flat [`TaskTree`] (task map + root ids + children).
///
/// Mirrors the session bootstrap in `commands::session::extract_tasks_for_session`
/// but treats any `TASK` whose parent element is not itself a `TASK` as a root,
/// so it works for both `<TODOLIST>` and `<TDL>` document roots.
pub fn build_tree_from_bytes(bytes: &[u8]) -> Result<TaskTree, String> {
    let doc = parse_xml(bytes).map_err(|e| format!("Failed to parse XML: {}", e))?;
    let mut tree = TaskTree::new();
    collect_into_tree(&doc.root, &mut tree, false);
    Ok(tree)
}

fn collect_into_tree(elem: &XmlElement, tree: &mut TaskTree, parent_is_task: bool) {
    for child in elem.child_elements() {
        if child.tag == "TASK" {
            let task = read_task(child);
            let id = task.id.clone();
            tree.add_task(task);
            if !parent_is_task {
                tree.add_root_id(id);
            }
            collect_into_tree(child, tree, true);
        } else {
            collect_into_tree(child, tree, parent_is_task);
        }
    }
}

/// Resolves a `document_id` to its registered file path via the workspace index.
pub fn resolve_document_path(db: &DatabaseManager, document_id: &str) -> Result<PathBuf, String> {
    if !db.is_available() {
        return Err("Index database unavailable; cannot resolve document path".to_string());
    }
    db.with_connection(|conn| {
        conn.query_row(
            "SELECT d.file_path, w.root_path FROM documents d \
             JOIN workspaces w ON w.id = d.workspace_id WHERE d.id = ?1",
            [document_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .map(|(file_path, root_path)| {
            // Documents are stored relative to the workspace root for
            // portability; join the root back on so callers get a usable path.
            let p = PathBuf::from(&file_path);
            if p.is_absolute() {
                p
            } else {
                PathBuf::from(&root_path).join(p)
            }
        })
        .map_err(|_| {
            crate::infrastructure::DatabaseError::Sqlite(rusqlite::Error::QueryReturnedNoRows)
        })
    })
    .map_err(|_| format!("Document '{}' is not registered in the workspace index", document_id))
}

/// Parses the saved document identified by `document_id` into a [`TaskTree`].
pub fn load_document_tree(db: &DatabaseManager, document_id: &str) -> Result<TaskTree, String> {
    let file_path = resolve_document_path(db, document_id)?;
    let bytes = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read '{}': {}", file_path.display(), e))?;
    build_tree_from_bytes(&bytes)
}

/// Lists every registered document as `(document_id, file_path)`.
pub fn list_all_documents(db: &DatabaseManager) -> Vec<(String, PathBuf)> {
    if !db.is_available() {
        return Vec::new();
    }
    db.with_connection(|conn| {
        let mut stmt = conn.prepare("SELECT id, file_path FROM documents ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, PathBuf::from(row.get::<_, String>(1)?)))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
    })
    .unwrap_or_default()
}

/// The document directory (parent of the document file) used to resolve
/// document-relative managed attachment paths.
pub fn document_dir(file_path: &Path) -> PathBuf {
    file_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

// ── DTO conversions ───────────────────────────────────────────────────────────

/// Builds a [`ParticipantDto`].
pub fn participant_dto(document_id: &str, task_key: &str, name: &str, role: &str) -> ParticipantDto {
    ParticipantDto {
        task_key: task_key.to_string(),
        document_id: document_id.to_string(),
        display_name: name.to_string(),
        role: role.to_string(),
    }
}

/// Builds a [`ProgressLinkDto`] from a domain [`ProgressLink`].
pub fn progress_link_dto(
    document_id: &str,
    task_key: &str,
    link: &ProgressLink,
) -> ProgressLinkDto {
    ProgressLinkDto {
        id: link.id.clone(),
        task_key: task_key.to_string(),
        document_id: document_id.to_string(),
        label: link.label.clone(),
        url: link.url.clone(),
        provider: link.provider.as_str().to_string(),
    }
}

/// Builds an [`AttachmentDto`], computing on-disk `exists`/`status` for
/// managed/linked files (URL attachments are always present).
pub fn attachment_dto(
    document_id: &str,
    task_key: &str,
    doc_dir: &Path,
    att: &AttachmentRef,
) -> AttachmentDto {
    let (exists, status) = match att.kind {
        AttachmentKind::Url => (true, "ok".to_string()),
        AttachmentKind::ManagedFile | AttachmentKind::LinkedFile => {
            match attachment::resolve_path(doc_dir, att) {
                Ok(path) if path.exists() => {
                    if att.hash.is_some() {
                        (true, "ok".to_string())
                    } else {
                        (true, "unverified".to_string())
                    }
                }
                _ => (false, "missing".to_string()),
            }
        }
    };
    AttachmentDto {
        id: att.id.clone(),
        task_key: task_key.to_string(),
        document_id: document_id.to_string(),
        kind: att.kind.as_str().to_string(),
        display_name: att.display_name.clone(),
        path_or_url: att.path_or_url.clone(),
        size: att.size,
        hash: att.hash.clone(),
        exists,
        status,
    }
}

/// Builds a [`DependencyDto`] for one native `<DEPENDENCY>` of `source_task_key`,
/// marking `circular` when the target can already reach the source.
pub fn dependency_dto(
    document_id: &str,
    source_task_key: &str,
    dep: &TaskDependency,
    graph: &DependencyGraph,
) -> DependencyDto {
    let task_ref = TaskRef::from_dependency(dep);
    let (depends_on_key, depends_on_document_id, raw_ref) = match &task_ref {
        TaskRef::Local(id) => (id.as_str().to_string(), None, None),
        TaskRef::External { document_id: doc, task_id } => (
            task_id.as_str().to_string(),
            Some(doc.as_str().to_string()),
            Some(doc.as_str().to_string()),
        ),
        TaskRef::Unresolved(raw) => (raw.clone(), None, Some(raw.clone())),
    };
    let circular = match &task_ref {
        TaskRef::Local(target) if !source_task_key.trim().is_empty() => {
            graph.find_path(target, &TaskId::new(source_task_key)).is_some()
        }
        _ => false,
    };
    DependencyDto {
        task_key: source_task_key.to_string(),
        document_id: document_id.to_string(),
        depends_on_key,
        depends_on_document_id,
        dep_type: dep.dependency_type,
        ref_kind: task_ref.kind_name().to_string(),
        circular,
        raw_ref,
    }
}

// ── Attachment open / reveal (shell integration) ──────────────────────────────

/// Locates an attachment by id, preferring an open session's live tree (so a
/// just-imported, not-yet-saved managed file can be opened) and falling back to
/// parsing the saved document. Returns `(doc_dir, attachment)`.
fn locate_attachment(
    ws: &State<'_, WorkspaceState>,
    sess: &State<'_, AppState>,
    document_id: &str,
    task_key: &str,
    attachment_id: &str,
) -> Result<(PathBuf, AttachmentRef), String> {
    let file_path = {
        let db = ws.db.lock().map_err(|_| "Lock error".to_string())?;
        resolve_document_path(&db, document_id)?
    };
    let doc_dir = document_dir(&file_path);
    let tid = safe_task_id(task_key)?;

    // 1. Prefer an open session (reflects unsaved imports).
    if let Ok(map) = sess.sessions.lock() {
        for entry in map.values() {
            if &entry.file_path == &file_path {
                if let Some(task) = entry.tree.get(&tid) {
                    if let Some(att) = attachment::scan_task_attachments(task_key, task)
                        .into_iter()
                        .find(|a| a.id == attachment_id)
                    {
                        return Ok((doc_dir, att));
                    }
                }
                break;
            }
        }
    }

    // 2. Fall back to the saved document.
    let bytes = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read '{}': {}", file_path.display(), e))?;
    let tree = build_tree_from_bytes(&bytes)?;
    let task = tree
        .get(&tid)
        .ok_or_else(|| format!("Task '{}' not found in document", task_key))?;
    let att = attachment::scan_task_attachments(task_key, task)
        .into_iter()
        .find(|a| a.id == attachment_id)
        .ok_or_else(|| format!("Attachment '{}' not found on task '{}'", attachment_id, task_key))?;
    Ok((doc_dir, att))
}

/// Resolves the OS-openable target for an attachment: the URL for url kinds, or
/// the absolute filesystem path (which must exist) otherwise.
fn resolve_open_target(doc_dir: &Path, att: &AttachmentRef) -> Result<String, String> {
    match att.kind {
        AttachmentKind::Url => Ok(att.path_or_url.clone()),
        _ => {
            let path = attachment::resolve_path(doc_dir, att).map_err(|e| e.to_string())?;
            if !path.exists() {
                return Err(format!(
                    "Attachment file not found on disk: {}",
                    path.display()
                ));
            }
            Ok(path.to_string_lossy().to_string())
        }
    }
}

/// Opens an attachment with the OS default handler (browser for URLs, the
/// associated application for files) via `tauri-plugin-shell`.
#[tauri::command]
pub fn open_attachment(
    app: tauri::AppHandle,
    ws: State<'_, WorkspaceState>,
    sess: State<'_, AppState>,
    attachment_id: String,
    task_key: String,
    document_id: String,
) -> Result<(), String> {
    let (doc_dir, att) = locate_attachment(&ws, &sess, &document_id, &task_key, &attachment_id)?;
    let target = resolve_open_target(&doc_dir, &att)?;
    // `Shell::open` is deprecated in favour of tauri-plugin-opener (not bundled
    // here). Called from Rust it bypasses the JS URL-only scope and delegates to
    // the `open` crate (ShellExecuteW on Windows), so it handles files and URLs.
    #[allow(deprecated)]
    app.shell()
        .open(target, None)
        .map_err(|e| format!("Failed to open attachment: {}", e))
}

/// Reveals a file attachment in the OS file manager (Windows Explorer with the
/// file selected). URL attachments have no filesystem location.
#[tauri::command]
pub fn reveal_attachment(
    ws: State<'_, WorkspaceState>,
    sess: State<'_, AppState>,
    attachment_id: String,
    task_key: String,
    document_id: String,
) -> Result<(), String> {
    let (doc_dir, att) = locate_attachment(&ws, &sess, &document_id, &task_key, &attachment_id)?;
    if att.kind == AttachmentKind::Url {
        return Err("URL attachments have no filesystem location to reveal".to_string());
    }
    let path = attachment::resolve_path(&doc_dir, &att).map_err(|e| e.to_string())?;
    if !path.exists() {
        return Err(format!(
            "Attachment file not found on disk: {}",
            path.display()
        ));
    }
    reveal_in_file_manager(&path)
}

/// `tauri-plugin-shell`'s `open` cannot select a file in Explorer and the
/// capability set only grants `shell:allow-open` (no `execute`), so the true
/// "reveal" is done by launching `explorer /select,<path>` directly. Explorer
/// exits non-zero even on success, so only a spawn failure is an error.
#[cfg(target_os = "windows")]
fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to reveal in Explorer: {}", e))
}

#[cfg(not(target_os = "windows"))]
fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    // Best-effort cross-platform fallback: open the containing directory.
    let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.to_path_buf());
    std::process::Command::new("xdg-open")
        .arg(&parent)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to reveal attachment: {}", e))
}
