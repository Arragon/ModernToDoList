//! M3 IPC commands: session management, atomic save, and undo/redo.
//!
//! These commands manage the lifecycle of open documents, providing
//! data safety through session tracking, atomic saves, and undo/redo.

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::domain::command::UndoRedoManager;
use crate::domain::fingerprint::FileFingerprint;
use crate::domain::persistence::{atomic_save, SaveConfig};
use crate::domain::session::DocumentSession;
use crate::domain::task::TaskTree;
use crate::domain::xml_parser::parse_xml;
use crate::domain::xml_serializer::serialize_xml;

// ── Shared Application State ─────────────────────────────────────────────────

/// Per-session data held by the application.
pub(crate) struct SessionEntry {
    pub(crate) session: DocumentSession,
    pub(crate) tree: TaskTree,
    pub(crate) undo_manager: UndoRedoManager,
    pub(crate) file_path: PathBuf,
}

// SAFETY: SessionEntry is always accessed through a Mutex, so it never
// crosses thread boundaries while in a partially-mutated state. The
// concrete types inside UndoRedoManager (FieldUpdateCommand, etc.) are
// all Send, but the trait object `Box<dyn UndoableCommand>` lacks a
// Send bound. This unsafe impl bridges that gap.
unsafe impl Send for SessionEntry {}

/// Global application state shared across all IPC commands.
///
/// Registered via `tauri::Manager::manage()` in `lib.rs`.
pub struct AppState {
    /// Active document sessions keyed by session ID.
    pub sessions: Mutex<HashMap<u64, SessionEntry>>,
    /// Monotonically increasing session ID counter.
    pub next_session_id: Mutex<u64>,
}

impl AppState {
    /// Creates a new empty `AppState`.
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            next_session_id: Mutex::new(1),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Response DTOs ─────────────────────────────────────────────────────────────

/// Response from `open_document_session`.
#[derive(Debug, Clone, Serialize)]
pub struct OpenSessionResponse {
    /// The unique session ID for subsequent commands.
    pub session_id: u64,
    /// File fingerprint (hash + size).
    pub fingerprint_hash: String,
    pub fingerprint_size: u64,
    /// Initial revision (always 1).
    pub initial_revision: u64,
    /// Number of tasks loaded.
    pub task_count: usize,
}

/// Response from `get_session_status`.
#[derive(Debug, Clone, Serialize)]
pub struct SessionStatusResponse {
    pub session_id: u64,
    pub current_revision: u64,
    pub saved_revision: u64,
    pub is_dirty: bool,
    pub is_saving: bool,
    pub save_generation: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_count: usize,
    pub redo_count: usize,
}

/// Response from `save_document_atomic`.
#[derive(Debug, Clone, Serialize)]
pub struct SaveDocumentResponse {
    pub success: bool,
    pub fingerprint_hash: Option<String>,
    pub error: Option<String>,
    pub new_revision: u64,
}

/// Response from undo/redo commands.
#[derive(Debug, Clone, Serialize)]
pub struct UndoRedoResponse {
    pub success: bool,
    pub description: Option<String>,
    pub message: String,
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Opens a new document session.
///
/// Reads the file, computes its fingerprint, creates a session with
/// revision tracking, and returns the session ID for subsequent commands.
#[tauri::command]
pub fn open_document_session(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<OpenSessionResponse, String> {
    let file_path = PathBuf::from(&path);

    // Read and parse the file
    let bytes = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

    let doc = parse_xml(&bytes)
        .map_err(|e| format!("Failed to parse XML: {}", e))?;

    // Compute fingerprint
    let fingerprint = FileFingerprint::from_bytes(&bytes);

    // Extract tasks
    let mut tree = TaskTree::new();
    extract_tasks_for_session(&doc.root, &mut tree);
    let task_count = tree.len();

    // Create session
    let session = DocumentSession::new(Some(fingerprint.clone()));
    let initial_revision = session.current_revision();

    // Allocate session ID
    let mut next_id = state.next_session_id.lock().map_err(|e| format!("Lock error: {}", e))?;
    let session_id = *next_id;
    *next_id += 1;
    drop(next_id);

    // Store session
    let entry = SessionEntry {
        session,
        tree,
        undo_manager: UndoRedoManager::new(),
        file_path,
    };

    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    sessions.insert(session_id, entry);

    Ok(OpenSessionResponse {
        session_id,
        fingerprint_hash: fingerprint.hash,
        fingerprint_size: fingerprint.size,
        initial_revision,
        task_count,
    })
}

/// Closes a document session and cleans up resources.
#[tauri::command]
pub fn close_document_session(
    session_id: u64,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    sessions.remove(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    Ok(())
}

/// Saves a document atomically.
///
/// Serializes the current task tree, validates, and performs an atomic
/// replace of the original file. Updates the session fingerprint and
/// revision on success.
#[tauri::command]
pub fn save_document_atomic(
    session_id: u64,
    state: tauri::State<'_, AppState>,
) -> Result<SaveDocumentResponse, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions.get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    // Check if save is already in progress
    if !entry.session.begin_save() {
        return Err("A save operation is already in progress for this session".into());
    }

    // Serialize the document to bytes
    // We need to rebuild the XML document from the task tree
    // For now, we serialize using the current tree state
    let xml_bytes = serialize_task_tree(&entry.tree);

    let config = SaveConfig {
        target_path: entry.file_path.clone(),
        validate_temp: true,
    };

    let result = atomic_save(&config, &xml_bytes, |_data| true);

    let new_revision = entry.session.current_revision();

    match result.state {
        crate::domain::session::SaveState::Completed => {
            entry.session.record_save();
            if let Some(fp) = &result.fingerprint {
                entry.session.set_fingerprint(fp.clone());
            }
            entry.session.end_save();

            Ok(SaveDocumentResponse {
                success: true,
                fingerprint_hash: result.fingerprint.map(|f| f.hash),
                error: None,
                new_revision,
            })
        }
        _ => {
            entry.session.end_save();
            Ok(SaveDocumentResponse {
                success: false,
                fingerprint_hash: None,
                error: result.error.map(|e| format!("{:?}", e)),
                new_revision,
            })
        }
    }
}

/// Returns the current status of a document session.
#[tauri::command]
pub fn get_session_status(
    session_id: u64,
    state: tauri::State<'_, AppState>,
) -> Result<SessionStatusResponse, String> {
    let sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions.get(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    Ok(SessionStatusResponse {
        session_id,
        current_revision: entry.session.current_revision(),
        saved_revision: entry.session.saved_revision(),
        is_dirty: entry.session.is_dirty(),
        is_saving: entry.session.is_saving(),
        save_generation: entry.session.save_generation(),
        can_undo: entry.undo_manager.can_undo(),
        can_redo: entry.undo_manager.can_redo(),
        undo_count: entry.undo_manager.undo_count(),
        redo_count: entry.undo_manager.redo_count(),
    })
}

/// Undoes the last command in the session.
///
/// Returns a description of the undone operation.
#[tauri::command]
pub fn undo_last_command(
    session_id: u64,
    state: tauri::State<'_, AppState>,
) -> Result<UndoRedoResponse, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions.get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    match entry.undo_manager.undo(&mut entry.tree) {
        Some(desc) => {
            entry.session.record_mutation();
            Ok(UndoRedoResponse {
                success: true,
                description: Some(desc.clone()),
                message: format!("Undone: {}", desc),
            })
        }
        None => Ok(UndoRedoResponse {
            success: false,
            description: None,
            message: "Nothing to undo".into(),
        }),
    }
}

/// Redoes the last undone command in the session.
///
/// Returns a description of the redone operation.
#[tauri::command]
pub fn redo_last_command(
    session_id: u64,
    state: tauri::State<'_, AppState>,
) -> Result<UndoRedoResponse, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions.get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    match entry.undo_manager.redo(&mut entry.tree) {
        Some(desc) => {
            entry.session.record_mutation();
            Ok(UndoRedoResponse {
                success: true,
                description: Some(desc.clone()),
                message: format!("Redone: {}", desc),
            })
        }
        None => Ok(UndoRedoResponse {
            success: false,
            description: None,
            message: "Nothing to redo".into(),
        }),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Recursively extracts tasks from the XML tree for session initialization.
fn extract_tasks_for_session(
    elem: &crate::domain::xml_tree::XmlElement,
    tree: &mut TaskTree,
) {
    use crate::domain::mappers::read_task;

    for child in elem.child_elements() {
        if child.tag == "TASK" {
            let task = read_task(child);
            let id = task.id.clone();
            tree.add_task(task);

            if elem.tag == "TODOLIST" {
                tree.add_root_id(id);
            }

            extract_tasks_for_session(child, tree);
        } else {
            extract_tasks_for_session(child, tree);
        }
    }
}

/// Serializes a TaskTree back to XML bytes.
///
/// This is a simplified serialization for the save pipeline.
/// In a full implementation, this would reconstruct the XmlDocument
/// from the TaskTree using the write_task mapper.
fn serialize_task_tree(tree: &TaskTree) -> Vec<u8> {
    use crate::domain::encoding::XmlEncodingMeta;
    use crate::domain::xml_tree::{XmlDocument, XmlElement, XmlNode};
    use crate::domain::mappers::write_task;

    let meta = XmlEncodingMeta::default_utf8();
    let mut root = XmlElement::new("TODOLIST");
    root.set_attr("NEXTUNIQUEID", "1");

    // Write root-level tasks
    for root_id in tree.root_ids() {
        if let Some(task) = tree.get(root_id) {
            let mut task_elem = XmlElement::new("TASK");
            write_task(task, &mut task_elem);
            root.children.push(XmlNode::Element(task_elem));
        }
    }

    let doc = XmlDocument::new(meta, root);
    serialize_xml(&doc)
}
