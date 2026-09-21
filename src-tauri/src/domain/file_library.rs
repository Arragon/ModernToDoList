//! File Library services (M8, RD-M8-032~045).
//!
//! Document-level operations on a [`Workspace`]:
//!
//! - Create a new Managed Document directly inside the workspace (no Save As).
//! - Rename / move / duplicate documents while preserving the stable
//!   workspace `DocumentId` (the entry `id` never changes on rename/move).
//! - Import as Managed (copy in) / Link External (reference in place) /
//!   Remove Reference (unregister only, file NEVER deleted).
//! - Reveal in Explorer.
//! - Path-reference repair after a move/rename: other registered documents
//!   that reference the moved document via `<FILEREFPATH>` are rewritten
//!   atomically.
//! - A receipt-based Undo policy for document operations
//!   (see `docs/multidoc/TRANSFER_DESIGN.md` §6).
//!
//! Deletion of managed documents is NOT done here: use [`super::trash`]
//! (delete-to-trash + restore + explicit permanent delete).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::persistence::{atomic_save, SaveConfig};
use super::session::SaveState;
use super::workspace::{DocumentEntry, DocumentType, Workspace};
use super::xml_parser::parse_xml;
use super::xml_serializer::serialize_xml;
use super::xml_tree::{XmlDocument, XmlElement, XmlNode};
use super::encoding::XmlEncodingMeta;

/// Structured errors for file-library operations.
#[derive(Debug, Error)]
pub enum FileLibraryError {
    #[error("workspace error: {0}")]
    Workspace(#[from] super::workspace::WorkspaceError),

    #[error("document not found in workspace: {0}")]
    DocumentNotFound(String),

    #[error("file already exists: {0}")]
    AlreadyExists(PathBuf),

    #[error("file does not exist: {0}")]
    FileNotFound(PathBuf),

    #[error("path escapes the workspace root: {0}")]
    PathEscapesWorkspace(PathBuf),

    #[error("I/O error ({context}): {reason}")]
    Io { context: String, reason: String },

    #[error("XML error ({context}): {reason}")]
    Xml { context: String, reason: String },

    #[error("commit failed for {path}: {code:?}")]
    CommitFailed { path: PathBuf, code: String },

    #[error("unsupported on this platform: {0}")]
    UnsupportedPlatform(&'static str),

    #[error("undo receipt not found: {0}")]
    ReceiptNotFound(String),

    #[error("operation not undoable: {0}")]
    NotUndoable(String),

    #[error("trash error: {0}")]
    Trash(#[from] super::trash::TrashError),
}

pub type FileLibraryResult<T> = Result<T, FileLibraryError>;

fn io_err(context: impl Into<String>, e: impl std::fmt::Display) -> FileLibraryError {
    FileLibraryError::Io {
        context: context.into(),
        reason: e.to_string(),
    }
}

fn commit_bytes(path: &Path, bytes: &[u8]) -> FileLibraryResult<()> {
    let save = atomic_save(
        &SaveConfig {
            target_path: path.to_path_buf(),
            validate_temp: true,
        },
        bytes,
        |b| parse_xml(b).is_ok(),
    );
    if save.state != SaveState::Completed {
        return Err(FileLibraryError::CommitFailed {
            path: path.to_path_buf(),
            code: save
                .error
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".into()),
        });
    }
    Ok(())
}

// ─────────────────────── New managed document (RD-M8-032) ───────────────────────

/// Creates a NEW Managed Document directly inside the workspace.
///
/// Writes a valid minimal TDL document (UTF-8, `NEXTUNIQUEID="1"`) via the
/// atomic-save pipeline and registers it with a fresh stable `DocumentId`.
/// `rel_path` must stay inside the workspace root.
pub fn create_managed_document(
    ws: &mut Workspace,
    rel_path: &str,
    project_name: &str,
) -> FileLibraryResult<(String, DocumentOpReceipt)> {
    let rel = normalize_rel(rel_path)?;
    let abs = ws.root_path.join(&rel);
    ensure_inside_workspace(ws, &abs)?;
    if abs.exists() {
        return Err(FileLibraryError::AlreadyExists(abs));
    }
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| io_err(format!("create dir {}", parent.display()), e))?;
    }

    let bytes = new_document_bytes(project_name, abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    commit_bytes(&abs, &bytes)?;

    let doc_id = ws.register_document(rel_string(&rel), DocumentType::Managed)?;
    // Record the fingerprint so change detection has a baseline.
    if let Ok(fp) = super::fingerprint::FileFingerprint::from_file(&abs) {
        ws.update_fingerprint(&doc_id, &fp.hash)?;
    }
    ws.save()?;

    let receipt = DocumentOpReceipt {
        receipt_id: uuid::Uuid::new_v4().to_string(),
        op: DocumentOpKind::Create,
        doc_id: doc_id.clone(),
        before_path: String::new(),
        after_path: Some(rel_string(&rel)),
        before_entry: None,
        repaired_docs: Vec::new(),
        undone: false,
    };
    save_receipt(ws, &receipt)?;
    Ok((doc_id, receipt))
}

fn new_document_bytes(project_name: &str, filename: String) -> Vec<u8> {
    let mut root = XmlElement::new("TODOLIST");
    root.set_attr("PROJECTNAME", project_name);
    root.set_attr("FILENAME", filename);
    root.set_attr("NEXTUNIQUEID", "1");
    root.set_attr("FILEVERSION", "43");
    root.set_attr("APPVER", "9.0.14.0");
    root.set_attr("FILEFORMAT", "12");
    let meta = XmlEncodingMeta::default_utf8();
    let le = meta.line_ending.as_str().to_string();
    let mut doc = XmlDocument::new(meta, root);
    doc.root.children.push(XmlNode::Text(le));
    serialize_xml(&doc)
}

fn normalize_rel(rel: &str) -> FileLibraryResult<PathBuf> {
    let t = rel.trim().replace('/', "\\");
    let t = t.strip_prefix(".\\").unwrap_or(&t).to_string();
    if t.is_empty() {
        return Err(FileLibraryError::PathEscapesWorkspace(PathBuf::from(rel)));
    }
    let p = PathBuf::from(&t);
    if p.is_absolute() || t.split('\\').any(|c| c == "..") {
        return Err(FileLibraryError::PathEscapesWorkspace(p));
    }
    Ok(p)
}

fn rel_string(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\")
}

fn ensure_inside_workspace(ws: &Workspace, abs: &Path) -> FileLibraryResult<()> {
    // Lexical containment is enough for rejection of ".." (normalize_rel
    // already blocks it); canonicalization is applied to the nearest
    // EXISTING ancestor so not-yet-created paths are handled (symlink-safe).
    let root = std::fs::canonicalize(&ws.root_path)
        .map_err(|e| io_err("canonicalize workspace root", e))?;
    let mut ancestor = abs;
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    while !ancestor.exists() {
        match (ancestor.parent(), ancestor.file_name()) {
            (Some(p), Some(name)) => {
                tail.push(name);
                ancestor = p;
            }
            _ => return Err(FileLibraryError::FileNotFound(abs.to_path_buf())),
        }
    }
    let mut probe = std::fs::canonicalize(ancestor)
        .map_err(|e| io_err("canonicalize ancestor path", e))?;
    for name in tail.iter().rev() {
        probe.push(name);
    }
    if !probe.starts_with(&root) {
        return Err(FileLibraryError::PathEscapesWorkspace(abs.to_path_buf()));
    }
    Ok(())
}

// ─────────────────────── Rename / move / duplicate (RD-M8-033~035) ───────────────────────

/// Receipt describing a completed document operation, enabling Undo
/// (RD-M8-044). Persisted under `.moderntodo/file-library/receipts/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentOpReceipt {
    pub receipt_id: String,
    pub op: DocumentOpKind,
    pub doc_id: String,
    /// Path (relative for managed, absolute for linked) before the op.
    pub before_path: String,
    /// Path after the op (`None` for RemoveReference-like ops that only unregister).
    pub after_path: Option<String>,
    /// The full workspace entry as it was before the op (for re-registration).
    pub before_entry: Option<DocumentEntry>,
    /// Paths of documents whose references were repaired, with their
    /// pre-repair bytes backup paths (for undoing repairs).
    pub repaired_docs: Vec<RepairedDoc>,
    pub undone: bool,
}

/// The kinds of undoable document operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentOpKind {
    Create,
    Rename,
    Move,
    Duplicate,
    ImportManaged,
    RemoveReference,
}

/// One reference-repaired document plus its byte backup for undo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairedDoc {
    pub doc_id: String,
    pub path: PathBuf,
    pub backup_path: PathBuf,
}

fn receipts_dir(ws: &Workspace) -> PathBuf {
    ws.root_path
        .join(".moderntodo")
        .join("file-library")
        .join("receipts")
}

fn save_receipt(ws: &Workspace, receipt: &DocumentOpReceipt) -> FileLibraryResult<()> {
    let dir = receipts_dir(ws);
    std::fs::create_dir_all(&dir).map_err(|e| io_err("create receipts dir", e))?;
    let path = dir.join(format!("{}.json", receipt.receipt_id));
    let bytes = serde_json::to_vec_pretty(receipt)
        .map_err(|e| io_err("serialize receipt", e))?;
    std::fs::write(&path, bytes).map_err(|e| io_err("write receipt", e))?;
    Ok(())
}

/// Loads a persisted receipt.
pub fn load_receipt(ws: &Workspace, receipt_id: &str) -> FileLibraryResult<DocumentOpReceipt> {
    let path = receipts_dir(ws).join(format!("{}.json", receipt_id));
    let bytes = std::fs::read(&path)
        .map_err(|_| FileLibraryError::ReceiptNotFound(receipt_id.to_string()))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| FileLibraryError::ReceiptNotFound(receipt_id.to_string()))
}

fn find_entry<'a>(ws: &'a Workspace, doc_id: &str) -> FileLibraryResult<&'a DocumentEntry> {
    ws.find_document(doc_id)
        .ok_or_else(|| FileLibraryError::DocumentNotFound(doc_id.to_string()))
}

/// Renames or moves a MANAGED document to a new workspace-relative path.
///
/// The `DocumentId` stays stable. After the physical move, path references in
/// other registered documents are repaired (RD-M8-045).
pub fn rename_or_move_document(
    ws: &mut Workspace,
    doc_id: &str,
    new_rel_path: &str,
) -> FileLibraryResult<DocumentOpReceipt> {
    let entry = find_entry(ws, doc_id)?.clone();
    if entry.doc_type != DocumentType::Managed {
        return Err(FileLibraryError::NotUndoable(
            "rename/move is only supported for managed documents; relink external documents instead"
                .into(),
        ));
    }
    let new_rel = normalize_rel(new_rel_path)?;
    let old_abs = ws.root_path.join(&entry.file_path);
    let new_abs = ws.root_path.join(&new_rel);
    ensure_inside_workspace(ws, &new_abs)?;
    if !old_abs.is_file() {
        return Err(FileLibraryError::FileNotFound(old_abs));
    }
    if new_abs.exists() {
        return Err(FileLibraryError::AlreadyExists(new_abs));
    }
    if let Some(parent) = new_abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err("create dir for move", e))?;
    }
    std::fs::rename(&old_abs, &new_abs).map_err(|e| io_err("rename document file", e))?;

    let op = if old_abs.parent() == new_abs.parent() {
        DocumentOpKind::Rename
    } else {
        DocumentOpKind::Move
    };

    // Update the entry IN PLACE: the id (stable DocumentId) never changes.
    let repaired = repair_path_references(ws, doc_id, &entry.file_path, &rel_string(&new_rel))?;
    update_entry_path(ws, doc_id, &rel_string(&new_rel))?;
    ws.save()?;

    let receipt = DocumentOpReceipt {
        receipt_id: uuid::Uuid::new_v4().to_string(),
        op,
        doc_id: doc_id.to_string(),
        before_path: entry.file_path.clone(),
        after_path: Some(rel_string(&new_rel)),
        before_entry: Some(entry),
        repaired_docs: repaired,
        undone: false,
    };
    save_receipt(ws, &receipt)?;
    Ok(receipt)
}

fn update_entry_path(ws: &mut Workspace, doc_id: &str, new_path: &str) -> FileLibraryResult<()> {
    let idx = ws
        .metadata
        .documents
        .iter()
        .position(|d| d.id == doc_id)
        .ok_or_else(|| FileLibraryError::DocumentNotFound(doc_id.to_string()))?;
    ws.metadata.documents[idx].file_path = new_path.to_string();
    Ok(())
}

/// Duplicates a document under a new path. The ORIGINAL keeps its stable
/// DocumentId; the duplicate is registered with a fresh one (its task-ID
/// space starts from the copied NEXTUNIQUEID, which is correct because the
/// duplicate is an independent document).
pub fn duplicate_document(
    ws: &mut Workspace,
    doc_id: &str,
    new_rel_path: &str,
) -> FileLibraryResult<(String, DocumentOpReceipt)> {
    let entry = find_entry(ws, doc_id)?.clone();
    let src_abs = ws.resolve_document_path(&entry);
    if !src_abs.is_file() {
        return Err(FileLibraryError::FileNotFound(src_abs));
    }
    let bytes = std::fs::read(&src_abs).map_err(|e| io_err("read document for duplicate", e))?;

    let (new_doc_id, new_abs) = if entry.doc_type == DocumentType::Managed {
        let rel = normalize_rel(new_rel_path)?;
        let abs = ws.root_path.join(&rel);
        ensure_inside_workspace(ws, &abs)?;
        if abs.exists() {
            return Err(FileLibraryError::AlreadyExists(abs));
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_err("create dir for duplicate", e))?;
        }
        let id = ws.register_document(rel_string(&rel), DocumentType::Managed)?;
        (id, abs)
    } else {
        let abs = PathBuf::from(new_rel_path);
        if abs.exists() {
            return Err(FileLibraryError::AlreadyExists(abs));
        }
        let id = ws
            .register_document(abs.to_string_lossy().into_owned(), DocumentType::Linked)?;
        (id, abs)
    };

    // Update the FILENAME root attribute of the duplicate to its new name so
    // the document stays self-describing (TDL convention).
    let mut final_bytes = bytes.clone();
    if let Ok(mut doc) = parse_xml(&bytes) {
        if let Some(name) = new_abs.file_name() {
            doc.root
                .set_attr("FILENAME", name.to_string_lossy().into_owned());
        }
        final_bytes = serialize_xml(&doc);
    }
    if let Err(e) = commit_bytes(&new_abs, &final_bytes) {
        // Roll back the registration; never leave a half-created duplicate.
        let _ = ws.unregister_document(&new_doc_id);
        return Err(e);
    }
    if let Ok(fp) = super::fingerprint::FileFingerprint::from_file(&new_abs) {
        ws.update_fingerprint(&new_doc_id, &fp.hash)?;
    }
    ws.save()?;

    let receipt = DocumentOpReceipt {
        receipt_id: uuid::Uuid::new_v4().to_string(),
        op: DocumentOpKind::Duplicate,
        doc_id: new_doc_id.clone(),
        before_path: String::new(),
        after_path: Some(new_rel_path.to_string()),
        before_entry: None,
        repaired_docs: Vec::new(),
        undone: false,
    };
    save_receipt(ws, &receipt)?;
    Ok((new_doc_id, receipt))
}

// ─────────────────────── Import / link / remove reference (RD-M8-036~038) ───────────────────────

/// Imports an external file as a MANAGED document: copies it into the
/// workspace (source file never modified) and registers it.
pub fn import_as_managed(
    ws: &mut Workspace,
    external_path: &Path,
    dest_rel_path: &str,
) -> FileLibraryResult<(String, DocumentOpReceipt)> {
    if !external_path.is_file() {
        return Err(FileLibraryError::FileNotFound(external_path.to_path_buf()));
    }
    let rel = normalize_rel(dest_rel_path)?;
    let abs = ws.root_path.join(&rel);
    ensure_inside_workspace(ws, &abs)?;
    if abs.exists() {
        return Err(FileLibraryError::AlreadyExists(abs));
    }
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err("create dir for import", e))?;
    }
    let bytes = std::fs::read(external_path).map_err(|e| io_err("read external file", e))?;
    // Verify it is a parseable task list before importing.
    parse_xml(&bytes).map_err(|e| FileLibraryError::Xml {
        context: format!("import {}", external_path.display()),
        reason: e.to_string(),
    })?;
    commit_bytes(&abs, &bytes)?;

    let doc_id = ws.register_document(rel_string(&rel), DocumentType::Managed)?;
    if let Ok(fp) = super::fingerprint::FileFingerprint::from_file(&abs) {
        ws.update_fingerprint(&doc_id, &fp.hash)?;
    }
    ws.save()?;

    let receipt = DocumentOpReceipt {
        receipt_id: uuid::Uuid::new_v4().to_string(),
        op: DocumentOpKind::ImportManaged,
        doc_id: doc_id.clone(),
        before_path: String::new(),
        after_path: Some(rel_string(&rel)),
        before_entry: None,
        repaired_docs: Vec::new(),
        undone: false,
    };
    save_receipt(ws, &receipt)?;
    Ok((doc_id, receipt))
}

/// Links an external file in place (no copy). The workspace stores the
/// absolute path; the file is never moved or modified.
pub fn link_external(ws: &mut Workspace, external_path: &Path) -> FileLibraryResult<String> {
    if !external_path.is_file() {
        return Err(FileLibraryError::FileNotFound(external_path.to_path_buf()));
    }
    // Absolute but WITHOUT the Windows `\\?\` verbatim prefix (canonicalize
    // would add it and leak it into workspace.json / the UI).
    let abs = std::path::absolute(external_path)
        .map_err(|e| io_err("absolutize external path", e))?;
    let bytes = std::fs::read(&abs).map_err(|e| io_err("read external file", e))?;
    parse_xml(&bytes).map_err(|e| FileLibraryError::Xml {
        context: format!("link {}", abs.display()),
        reason: e.to_string(),
    })?;
    let doc_id = ws.register_document(abs.to_string_lossy().into_owned(), DocumentType::Linked)?;
    if let Ok(fp) = super::fingerprint::FileFingerprint::from_file(&abs) {
        ws.update_fingerprint(&doc_id, &fp.hash)?;
    }
    ws.save()?;
    Ok(doc_id)
}

/// Removes a document reference from the workspace. The FILE IS NEVER
/// DELETED by this operation (managed files stay on disk; use the trash for
/// deletion).
pub fn remove_reference(ws: &mut Workspace, doc_id: &str) -> FileLibraryResult<DocumentOpReceipt> {
    let entry = find_entry(ws, doc_id)?.clone();
    ws.unregister_document(doc_id)?;
    ws.save()?;
    let receipt = DocumentOpReceipt {
        receipt_id: uuid::Uuid::new_v4().to_string(),
        op: DocumentOpKind::RemoveReference,
        doc_id: doc_id.to_string(),
        before_path: entry.file_path.clone(),
        after_path: None,
        before_entry: Some(entry),
        repaired_docs: Vec::new(),
        undone: false,
    };
    save_receipt(ws, &receipt)?;
    Ok(receipt)
}

// ─────────────────────── Path-reference repair (RD-M8-045) ───────────────────────

/// Rewrites `<FILEREFPATH>` references in ALL OTHER registered documents that
/// point at `old_rel_path` so they resolve to `new_rel_path`.
///
/// Each modified document is committed through the atomic-save pipeline; its
/// pre-repair bytes are backed up so the repair can be undone. Matching is on
/// normalized path strings (case-insensitive on Windows), comparing both the
/// raw text and the text resolved relative to the referencing document.
fn repair_path_references(
    ws: &Workspace,
    moved_doc_id: &str,
    old_rel_path: &str,
    new_rel_path: &str,
) -> FileLibraryResult<Vec<RepairedDoc>> {
    let mut repaired = Vec::new();
    let ws_root = ws.root_path.clone();
    let old_abs = normalize_path_lower(&ws_root.join(old_rel_path).to_string_lossy());
    let backup_dir = ws_root
        .join(".moderntodo")
        .join("file-library")
        .join("repair-backups");

    for doc in ws.documents() {
        if doc.id == moved_doc_id {
            continue;
        }
        let path = ws.resolve_document_path(doc);
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(mut xml) = parse_xml(&bytes) else { continue };
        let doc_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut changed = false;
        rewrite_filerefpaths(&mut xml.root, &doc_dir, &old_abs, new_rel_path, &ws_root, &mut changed);
        if !changed {
            continue;
        }
        std::fs::create_dir_all(&backup_dir)
            .map_err(|e| io_err("create repair backup dir", e))?;
        let backup = backup_dir.join(format!(
            "{}-{}.xml",
            doc.id,
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&backup, &bytes).map_err(|e| io_err("write repair backup", e))?;
        let new_bytes = serialize_xml(&xml);
        commit_bytes(&path, &new_bytes)?;
        repaired.push(RepairedDoc {
            doc_id: doc.id.clone(),
            path,
            backup_path: backup,
        });
    }
    Ok(repaired)
}

fn normalize_path_lower(s: &str) -> String {
    if cfg!(windows) {
        s.replace('/', "\\").to_lowercase()
    } else {
        s.replace('/', "\\")
    }
}

fn rewrite_filerefpaths(
    elem: &mut XmlElement,
    doc_dir: &Path,
    old_abs_lower: &str,
    new_rel_to_ws: &str,
    ws_root: &Path,
    changed: &mut bool,
) {
    for node in elem.children.iter_mut() {
        if let XmlNode::Element(e) = node {
            if e.tag == "FILEREFPATH" {
                let text = e.text_content();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let candidate = if Path::new(trimmed).is_absolute() {
                        PathBuf::from(trimmed)
                    } else {
                        let norm = trimmed
                            .strip_prefix(".\\")
                            .or_else(|| trimmed.strip_prefix("./"))
                            .unwrap_or(trimmed);
                        doc_dir.join(norm)
                    };
                    if normalize_path_lower(&candidate.to_string_lossy()) == old_abs_lower {
                        // Rewrite as a path relative to the referencing document.
                        let new_abs = ws_root.join(new_rel_to_ws);
                        let new_text = relative_or_abs(&doc_dir, &new_abs);
                        e.set_text_content(new_text);
                        *changed = true;
                    }
                }
            } else {
                rewrite_filerefpaths(e, doc_dir, old_abs_lower, new_rel_to_ws, ws_root, changed);
            }
        }
    }
}

fn relative_or_abs(from_dir: &Path, to: &Path) -> String {
    // Simple lexical relativization when both share the same root; otherwise
    // fall back to the absolute path.
    let from = normalize_path_lower(&from_dir.to_string_lossy());
    let target = normalize_path_lower(&to.to_string_lossy());
    if target.starts_with(&from) {
        let rest = &target[from.len()..];
        let rest = rest.strip_prefix('\\').unwrap_or(rest);
        if !rest.is_empty() && !rest.contains("..") {
            return format!(".\\{}", restore_case(to, from_dir));
        }
    }
    to.to_string_lossy().into_owned()
}

fn restore_case(abs: &Path, base: &Path) -> String {
    // Re-derive the original-case relative tail from the absolute path.
    match abs.strip_prefix(base) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => abs.to_string_lossy().into_owned(),
    }
}

// ─────────────────────── Reveal in Explorer (RD-M8-043) ───────────────────────

/// Builds the platform command that reveals `path` in the file manager.
/// Returning the command (instead of spawning it here) keeps the domain layer
/// pure; the IPC layer can spawn it directly.
pub fn reveal_command(path: &Path) -> FileLibraryResult<std::process::Command> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("explorer");
        cmd.arg(format!("/select,{}", path.to_string_lossy()));
        Ok(cmd)
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        cmd.arg("-R").arg(path);
        Ok(cmd)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // No universal "select" on Linux: open the parent directory.
        let parent = path.parent().unwrap_or(path).to_path_buf();
        let mut cmd = std::process::Command::new("xdg-open");
        cmd.arg(parent);
        Ok(cmd)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        let _ = path;
        Err(FileLibraryError::UnsupportedPlatform(
            "reveal in file manager is not supported on this platform",
        ))
    }
}

/// Reveals `path` in the platform file manager (spawns the command).
///
/// On Windows `explorer` frequently exits non-zero even on success, so the
/// exit code is intentionally ignored; only spawn failures are errors.
pub fn reveal_in_explorer(path: &Path) -> FileLibraryResult<()> {
    if !path.exists() {
        return Err(FileLibraryError::FileNotFound(path.to_path_buf()));
    }
    let mut cmd = reveal_command(path)?;
    cmd.spawn()
        .map_err(|e| io_err("spawn file manager", e))?;
    Ok(())
}

// ─────────────────────── Document-op Undo policy (RD-M8-044) ───────────────────────

/// Undoes a document operation using its receipt:
///
/// | Op              | Undo action                                             |
/// |-----------------|---------------------------------------------------------|
/// | Create          | unregister + move file to trash (never silent delete)   |
/// | Rename / Move   | move file back + restore repaired references + path     |
/// | Duplicate       | unregister + move the duplicate file to trash           |
/// | ImportManaged   | unregister + move the imported copy to trash            |
/// | RemoveReference | re-register the original entry (stable DocumentId)      |
pub fn undo_document_op(
    ws: &mut Workspace,
    receipt_id: &str,
) -> FileLibraryResult<DocumentOpReceipt> {
    let mut receipt = load_receipt(ws, receipt_id)?;
    if receipt.undone {
        return Err(FileLibraryError::NotUndoable(
            "receipt already undone".into(),
        ));
    }

    // Restore repaired reference documents first (byte-exact backups).
    for rep in &receipt.repaired_docs {
        let backup = std::fs::read(&rep.backup_path).map_err(|e| {
            io_err(format!("read repair backup {}", rep.backup_path.display()), e)
        })?;
        commit_bytes(&rep.path, &backup)?;
    }

    match receipt.op {
        DocumentOpKind::Create | DocumentOpKind::Duplicate | DocumentOpKind::ImportManaged => {
            let abs = if receipt.op == DocumentOpKind::ImportManaged
                || receipt.op == DocumentOpKind::Duplicate
            {
                resolve_after_path(ws, &receipt)
            } else {
                ws.root_path.join(&receipt.after_path.clone().unwrap_or_default())
            };
            // Unregister (ignore "not found": duplicate/import receipts store
            // the NEW doc id which is registered).
            let _ = ws.unregister_document(&receipt.doc_id);
            if abs.is_file() {
                // Delete-to-trash: never silently destroy bytes.
                super::trash::move_file_to_trash(&ws.root_path, &abs, Some(&receipt.doc_id))?;
            }
        }
        DocumentOpKind::Rename | DocumentOpKind::Move => {
            let new_abs = ws.root_path.join(receipt.after_path.clone().unwrap_or_default());
            let old_abs = ws.root_path.join(&receipt.before_path);
            if new_abs.is_file() {
                if let Some(parent) = old_abs.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| io_err("recreate original dir", e))?;
                }
                std::fs::rename(&new_abs, &old_abs)
                    .map_err(|e| io_err("undo rename: move file back", e))?;
            }
            update_entry_path(ws, &receipt.doc_id, &receipt.before_path)?;
        }
        DocumentOpKind::RemoveReference => {
            if let Some(entry) = receipt.before_entry.clone() {
                // Re-register with the SAME stable DocumentId.
                re_register_entry(ws, entry)?;
            }
        }
    }

    ws.save()?;
    receipt.undone = true;
    save_receipt(ws, &receipt)?;
    Ok(receipt)
}

fn resolve_after_path(ws: &Workspace, receipt: &DocumentOpReceipt) -> PathBuf {
    match receipt.after_path.as_deref() {
        Some(p) if Path::new(p).is_absolute() => PathBuf::from(p),
        Some(p) => ws.root_path.join(p),
        None => PathBuf::new(),
    }
}

/// Re-inserts a full entry (preserving its DocumentId) into the workspace.
///
/// `Workspace` keeps a private in-memory id index that direct metadata pushes
/// do not update, so the workspace is reloaded in place (save + open) to
/// rebuild the index through the public API only.
pub fn re_register_entry(ws: &mut Workspace, entry: DocumentEntry) -> FileLibraryResult<()> {
    if ws.find_document(&entry.id).is_some() {
        return Ok(());
    }
    ws.metadata.documents.push(entry);
    reload_workspace_inplace(ws)
}

/// Persists the workspace metadata and reloads it in place so the internal
/// document index reflects direct `metadata.documents` mutations.
pub fn reload_workspace_inplace(ws: &mut Workspace) -> FileLibraryResult<()> {
    ws.save()?;
    let reloaded = Workspace::open(&ws.root_path)?;
    *ws = reloaded;
    Ok(())
}

// ─────────────────────── Tests ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_ws(name: &str) -> (PathBuf, Workspace) {
        let dir = std::env::temp_dir().join(format!(
            "mtdl_fl_{}_{}_{}",
            name,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ws = Workspace::create(&dir, "FL".into()).unwrap();
        (dir, ws)
    }

    fn valid_doc_bytes(name: &str) -> Vec<u8> {
        new_document_bytes(name, format!("{}.xml", name))
    }

    #[test]
    fn create_managed_document_writes_and_registers() {
        let (dir, mut ws) = tmp_ws("create");
        let (id, _receipt) = create_managed_document(&mut ws, "projects\\new.xml", "New Project").unwrap();
        let abs = dir.join("projects").join("new.xml");
        assert!(abs.is_file());
        let doc = ws.find_document(&id).unwrap();
        assert_eq!(doc.doc_type, DocumentType::Managed);
        // Parseable + NEXTUNIQUEID=1
        let bytes = std::fs::read(&abs).unwrap();
        let parsed = parse_xml(&bytes).unwrap();
        assert_eq!(parsed.root.get_attr("NEXTUNIQUEID"), Some("1"));
        assert_eq!(parsed.root.get_attr("PROJECTNAME"), Some("New Project"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_rejects_escape_and_existing() {
        let (dir, mut ws) = tmp_ws("create_bad");
        assert!(matches!(
            create_managed_document(&mut ws, "..\\evil.xml", "x"),
            Err(FileLibraryError::PathEscapesWorkspace(_))
        ));
        let _ = create_managed_document(&mut ws, "a.xml", "x").unwrap();
        assert!(matches!(
            create_managed_document(&mut ws, "a.xml", "x"),
            Err(FileLibraryError::AlreadyExists(_))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_keeps_document_id_stable_and_repairs_refs() {
        let (dir, mut ws) = tmp_ws("rename");
        // Document B references document A by relative path.
        let (a_id, _) = create_managed_document(&mut ws, "a.xml", "A").unwrap();
        std::fs::write(dir.join("b.xml"), valid_doc_bytes("B")).unwrap();
        let b_id = ws.register_document("b.xml".into(), DocumentType::Managed).unwrap();
        // Give B a FILEREFPATH pointing at A.
        let b_bytes = std::fs::read(dir.join("b.xml")).unwrap();
        let mut b_doc = parse_xml(&b_bytes).unwrap();
        let mut task = XmlElement::new("TASK");
        task.set_attr("ID", "1");
        let mut fr = XmlElement::new("FILEREFPATH");
        fr.set_text_content(".\\a.xml");
        task.children.push(XmlNode::Element(fr));
        b_doc.root.children.push(XmlNode::Element(task));
        std::fs::write(dir.join("b.xml"), serialize_xml(&b_doc)).unwrap();

        let receipt = rename_or_move_document(&mut ws, &a_id, "sub\\renamed.xml").unwrap();
        // Stable DocumentId
        assert_eq!(receipt.doc_id, a_id);
        assert!(ws.find_document(&a_id).is_some());
        assert!(!dir.join("a.xml").exists());
        assert!(dir.join("sub").join("renamed.xml").is_file());
        // Reference repaired in B
        let b_after = std::fs::read_to_string(dir.join("b.xml")).unwrap();
        assert!(b_after.contains("renamed.xml"), "B should reference new path: {}", b_after);
        assert!(!b_after.contains(".\\a.xml"));
        assert_eq!(receipt.repaired_docs.len(), 1);
        assert_eq!(receipt.repaired_docs[0].doc_id, b_id);

        // Undo restores everything.
        undo_document_op(&mut ws, &receipt.receipt_id).unwrap();
        assert!(dir.join("a.xml").is_file());
        assert!(!dir.join("sub").join("renamed.xml").exists());
        let b_restored = std::fs::read_to_string(dir.join("b.xml")).unwrap();
        assert!(b_restored.contains(".\\a.xml"));
        assert_eq!(ws.find_document(&a_id).unwrap().file_path, "a.xml");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_creates_independent_doc_with_new_id() {
        let (dir, mut ws) = tmp_ws("dup");
        let (a_id, _) = create_managed_document(&mut ws, "a.xml", "A").unwrap();
        let (dup_id, receipt) = duplicate_document(&mut ws, &a_id, "a-copy.xml").unwrap();
        assert_ne!(dup_id, a_id);
        assert!(dir.join("a-copy.xml").is_file());
        let bytes = std::fs::read(dir.join("a-copy.xml")).unwrap();
        let doc = parse_xml(&bytes).unwrap();
        assert_eq!(doc.root.get_attr("FILENAME").unwrap(), "a-copy.xml");

        undo_document_op(&mut ws, &receipt.receipt_id).unwrap();
        assert!(!dir.join("a-copy.xml").exists()); // moved to trash
        assert!(ws.find_document(&dup_id).is_none());
        assert!(ws.find_document(&a_id).is_some()); // original untouched
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn import_managed_copies_and_links_reference_in_place() {
        let (dir, mut ws) = tmp_ws("import");
        let ext_dir = std::env::temp_dir().join(format!("mtdl_ext_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&ext_dir).unwrap();
        let ext = ext_dir.join("external.xml");
        std::fs::write(&ext, valid_doc_bytes("Ext")).unwrap();

        let (imp_id, imp_receipt) = import_as_managed(&mut ws, &ext, "imported.xml").unwrap();
        assert!(dir.join("imported.xml").is_file());
        assert!(ext.is_file()); // source never modified
        assert_eq!(ws.find_document(&imp_id).unwrap().doc_type, DocumentType::Managed);

        let link_id = link_external(&mut ws, &ext).unwrap();
        let linked = ws.find_document(&link_id).unwrap();
        assert_eq!(linked.doc_type, DocumentType::Linked);
        assert!(Path::new(&linked.file_path).is_absolute());

        // Remove reference: file MUST survive.
        let rm_receipt = remove_reference(&mut ws, &link_id).unwrap();
        assert!(ws.find_document(&link_id).is_none());
        assert!(ext.is_file());
        // Undo re-registers with the SAME DocumentId.
        undo_document_op(&mut ws, &rm_receipt.receipt_id).unwrap();
        assert!(ws.find_document(&link_id).is_some());

        undo_document_op(&mut ws, &imp_receipt.receipt_id).unwrap();
        assert!(!dir.join("imported.xml").exists());
        assert!(ext.is_file());
        std::fs::remove_dir_all(&ext_dir).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn import_rejects_non_xml() {
        let (dir, mut ws) = tmp_ws("import_bad");
        let ext = dir.join("notes.txt");
        std::fs::write(&ext, b"not xml at all <<<").unwrap();
        assert!(matches!(
            import_as_managed(&mut ws, &ext, "notes.xml"),
            Err(FileLibraryError::Xml { .. })
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reveal_command_shape() {
        let dir = std::env::temp_dir();
        let cmd = reveal_command(&dir).unwrap();
        if cfg!(target_os = "windows") {
            assert_eq!(cmd.get_program().to_string_lossy(), "explorer");
        }
    }

    #[test]
    fn undo_create_moves_file_to_trash_not_delete() {
        let (dir, mut ws) = tmp_ws("undo_create");
        let (id, receipt) = create_managed_document(&mut ws, "temp.xml", "T").unwrap();
        assert!(dir.join("temp.xml").is_file());

        undo_document_op(&mut ws, &receipt.receipt_id).unwrap();
        // File is NOT hard-deleted: it must be recoverable from the trash.
        assert!(!dir.join("temp.xml").exists());
        assert!(ws.find_document(&id).is_none());
        let trashed = super::super::trash::list_trash(&dir).unwrap();
        assert_eq!(trashed.len(), 1);
        assert_eq!(
            trashed[0].original_path,
            dir.join("temp.xml"),
            "created doc should be in trash with its original path"
        );
        // Second undo rejected.
        assert!(matches!(
            undo_document_op(&mut ws, &receipt.receipt_id),
            Err(FileLibraryError::NotUndoable(_))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }
}
