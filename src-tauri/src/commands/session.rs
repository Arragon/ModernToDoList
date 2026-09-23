//! M3 IPC commands: session management, atomic save, and undo/redo.
//!
//! These commands manage the lifecycle of open documents, providing
//! data safety through session tracking, atomic saves, and undo/redo.

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::commands::workspace::WorkspaceState;
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
    /// The document exactly as parsed at open time. Saving rebuilds from this so
    /// everything the task tree does not model — XML declaration and encoding,
    /// root attributes, comments, unknown elements, and each task's unknown
    /// attributes/children — survives a save instead of being discarded.
    pub(crate) source_doc: crate::domain::xml_tree::XmlDocument,
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
    ws: tauri::State<'_, WorkspaceState>,
) -> Result<OpenSessionResponse, String> {
    // Callers (and the stored index) may hand us a path relative to the open
    // workspace root; resolve it to an absolute path before reading.
    let raw = PathBuf::from(&path);
    let file_path = if raw.is_absolute() {
        raw
    } else {
        match ws.workspace.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(w) => w.root_path.join(raw),
                None => raw,
            },
            Err(_) => raw,
        }
    };

    // Read and parse the file
    let bytes = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read file '{}': {}", file_path.display(), e))?;

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
        source_doc: doc.clone(),
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
    let xml_bytes = serialize_task_tree(&entry.tree, &entry.source_doc);

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
/// Rebuilds the document bytes from the task tree, starting from the document as
/// it was parsed so that everything the tree does not model survives: the
/// original XML declaration and encoding, root attributes, comments, unknown
/// elements, and each task's unknown attributes and children.
///
/// This used to construct a fresh `<TODOLIST NEXTUNIQUEID="1">` and emit only
/// root-level tasks into brand-new empty `<TASK>` elements. Because
/// `mappers::write_task` preserves nested `<TASK>` children found on the element
/// it is given, feeding it an empty element meant **every nested task was
/// silently discarded on save**, and the file was re-encoded as UTF-8 regardless
/// of its original encoding.
fn serialize_task_tree(
    tree: &TaskTree,
    source: &crate::domain::xml_tree::XmlDocument,
) -> Vec<u8> {
    use crate::domain::mappers::write_task;
    use crate::domain::xml_tree::{XmlDocument, XmlElement, XmlNode};
    let _ = write_task; // used by build_task_node

    // Index the original TASK elements by ID so each can be updated in place.
    let mut originals: HashMap<String, XmlElement> = HashMap::new();
    collect_task_elements(&source.root, &mut originals);

    let mut root = source.root.clone();
    let rebuilt: Vec<XmlNode> = tree
        .root_ids()
        .iter()
        .filter_map(|id| build_task_node(tree, id, &mut originals))
        .collect();

    root.children = splice_tasks(root.children, rebuilt);

    serialize_xml(&XmlDocument::new(source.meta.clone(), root))
}

/// Replaces the TASK children of `children` with `tasks`, keeping every non-TASK
/// node (comments, unknown elements, whitespace) in its original position and
/// putting the tasks where the first TASK element appeared.
fn splice_tasks(
    children: Vec<crate::domain::xml_tree::XmlNode>,
    tasks: Vec<crate::domain::xml_tree::XmlNode>,
) -> Vec<crate::domain::xml_tree::XmlNode> {
    use crate::domain::xml_tree::XmlNode;
    let mut out: Vec<XmlNode> = Vec::with_capacity(children.len());
    let mut placed = false;
    for child in children {
        if matches!(&child, XmlNode::Element(e) if e.tag == "TASK") {
            if !placed {
                out.extend(tasks.iter().cloned());
                placed = true;
            }
            continue;
        }
        out.push(child);
    }
    if !placed {
        out.extend(tasks);
    }
    out
}

/// Indexes every `<TASK>` element in the subtree by its ID attribute.
fn collect_task_elements(
    elem: &crate::domain::xml_tree::XmlElement,
    out: &mut HashMap<String, crate::domain::xml_tree::XmlElement>,
) {
    use crate::domain::xml_tree::XmlNode;
    for child in &elem.children {
        if let XmlNode::Element(e) = child {
            if e.tag == "TASK" {
                if let Some(id) = e.get_attr("ID") {
                    out.insert(id.to_string(), e.clone());
                }
            }
            collect_task_elements(e, out);
        }
    }
}

/// Rebuilds one task element and recurses into its children, so additions,
/// deletions and reordering in the tree are all reflected in the output.
fn build_task_node(
    tree: &TaskTree,
    id: &crate::domain::TaskId,
    originals: &mut HashMap<String, crate::domain::xml_tree::XmlElement>,
) -> Option<crate::domain::xml_tree::XmlNode> {
    use crate::domain::mappers::write_task;
    use crate::domain::xml_tree::{XmlElement, XmlNode};

    let task = tree.get(id)?;
    let mut elem = originals
        .remove(id.as_str())
        .unwrap_or_else(|| XmlElement::new("TASK"));

    // Refreshes known attributes/elements while preserving unknown ones.
    write_task(task, &mut elem);

    // Nested tasks are rebuilt from the tree rather than inherited from the
    // stale element, so structural edits are honoured.
    let nested: Vec<XmlNode> = task
        .children
        .iter()
        .filter_map(|child_id| build_task_node(tree, child_id, originals))
        .collect();

    elem.children = splice_tasks(elem.children, nested);

    Some(XmlNode::Element(elem))
}

#[cfg(test)]
mod serialize_task_tree_tests {
    //! Regression coverage for the session save path.
    //!
    //! `serialize_task_tree` used to build a fresh `<TODOLIST NEXTUNIQUEID="1">`
    //! and emit only root-level tasks into empty `<TASK>` elements. Since
    //! `write_task` preserves the nested `<TASK>` children already present on the
    //! element it is given, an empty element meant every subtask vanished on
    //! save, along with the root attributes, comments and original encoding.
    //! These tests are in-module because the function is private to `commands`.

    use super::*;
    use crate::domain::xml_parser::parse_xml;

    const NESTED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<TODOLIST NEXTUNIQUEID="42" VERSION="2.0" PROJECT="评审计划">
<!-- 项目级备注：必须存活 -->
<TASK ID="1" TITLE="根任务" PERCENTDONE="0">
<TASK ID="2" TITLE="子任务" PERCENTDONE="50">
<TASK ID="3" TITLE="孙任务" PRIORITY="7"/>
</TASK>
<UNKNOWNPLUGIN DATA="keep-me"/>
</TASK>
<TASK ID="4" TITLE="第二个根任务"/>
</TODOLIST>"#;

    fn tree_and_doc(xml: &str) -> (TaskTree, crate::domain::xml_tree::XmlDocument) {
        let doc = parse_xml(xml.as_bytes()).expect("fixture must parse");
        let mut tree = TaskTree::new();
        extract_tasks_for_session(&doc.root, &mut tree);
        (tree, doc)
    }

    #[test]
    fn save_preserves_every_nested_task() {
        let (tree, doc) = tree_and_doc(NESTED);
        assert_eq!(tree.len(), 4, "fixture has 4 tasks at three depths");

        let out = serialize_task_tree(&tree, &doc);
        let reparsed = parse_xml(&out).expect("saved output must reparse");

        let mut after = TaskTree::new();
        extract_tasks_for_session(&reparsed.root, &mut after);
        assert_eq!(
            after.len(),
            tree.len(),
            "save must not drop nested tasks (was: only root level survived)"
        );
        for id in ["1", "2", "3", "4"] {
            assert!(
                after.get(&crate::domain::TaskId::new(id)).is_some(),
                "task {id} must survive the save"
            );
        }
    }

    #[test]
    fn save_preserves_nesting_depth_and_order() {
        let (tree, doc) = tree_and_doc(NESTED);
        let out = serialize_task_tree(&tree, &doc);
        let reparsed = parse_xml(&out).expect("saved output must reparse");

        let mut after = TaskTree::new();
        extract_tasks_for_session(&reparsed.root, &mut after);

        let root = after.get(&crate::domain::TaskId::new("1")).expect("task 1");
        assert_eq!(root.children.len(), 1, "task 1 keeps its single child");
        let child = after.get(&root.children[0]).expect("child");
        assert_eq!(child.children.len(), 1, "task 2 keeps its single child");
        assert_eq!(
            after.root_ids().len(),
            tree.root_ids().len(),
            "root task count and order are preserved"
        );
    }

    #[test]
    fn save_preserves_root_attributes_comments_and_unknown_elements() {
        let (tree, doc) = tree_and_doc(NESTED);
        let out = serialize_task_tree(&tree, &doc);
        let text = String::from_utf8_lossy(&out);

        let reparsed = parse_xml(&out).expect("saved output must reparse");
        assert_eq!(
            reparsed.root.get_attr("NEXTUNIQUEID"),
            Some("42"),
            "root NEXTUNIQUEID must not be reset to 1"
        );
        assert_eq!(reparsed.root.get_attr("PROJECT"), Some("评审计划"));
        assert!(
            text.contains("项目级备注：必须存活"),
            "root-level comment must survive the save"
        );
        assert!(
            text.contains("UNKNOWNPLUGIN"),
            "unknown plugin element must survive the save"
        );
    }

    #[test]
    fn save_preserves_the_original_encoding_metadata() {
        let (tree, doc) = tree_and_doc(NESTED);
        let out = serialize_task_tree(&tree, &doc);
        let reparsed = parse_xml(&out).expect("saved output must reparse");
        assert_eq!(
            reparsed.meta.encoding, doc.meta.encoding,
            "the source encoding must be reused, not forced to UTF-8"
        );
    }

    #[test]
    fn edited_field_is_written_without_losing_siblings() {
        let (mut tree, doc) = tree_and_doc(NESTED);
        let id = crate::domain::TaskId::new("2");
        tree.get_mut(&id).expect("task 2").title = "改名后的子任务".to_string();

        let out = serialize_task_tree(&tree, &doc);
        let reparsed = parse_xml(&out).expect("saved output must reparse");

        let mut after = TaskTree::new();
        extract_tasks_for_session(&reparsed.root, &mut after);
        assert_eq!(after.len(), 4, "editing one task must not drop the others");
        assert_eq!(after.get(&id).expect("task 2").title, "改名后的子任务");
        assert!(
            after.get(&crate::domain::TaskId::new("3")).is_some(),
            "the edited task's own child must still be present"
        );
    }
}
