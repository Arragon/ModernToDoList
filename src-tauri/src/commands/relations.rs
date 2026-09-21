//! IPC bridge: M6 task-relation commands (participants, dependencies, progress
//! links, attachments).
//!
//! Reads carry only `document_id`/`task_key`, so they parse the saved document
//! (the relation index tables are only partially populated by `index_document`).
//! Writes carry a `session_id`, mutate the live session tree through an
//! [`UndoableCommand`] executed by the [`UndoRedoManager`], and then call
//! `record_mutation()`.
//!
//! All validation is delegated to the M6 domain modules — participant name
//! rules, dependency self/cycle rejection, progress-link URL scheme checking and
//! attachment path-traversal / managed-import integrity are NOT reimplemented
//! here; the commands below are thin, undoable wrappers over them.

use std::path::{Path, PathBuf};

use tauri::State;

use crate::commands::bridge::{
    attachment_dto, build_tree_from_bytes, dependency_dto, document_dir, load_document_tree,
    participant_dto, progress_link_dto, resolve_document_path, safe_task_id, AttachmentDto,
    AttachmentImportResult, DependencyDto, DependencyGraphDto, MutationAck, ParticipantDto,
    ProgressLinkDto,
};
use crate::commands::session::{AppState, SessionEntry};
use crate::commands::workspace::WorkspaceState;
use crate::domain::attachment::{self, AttachmentKind, AttachmentRef};
use crate::domain::command::{UndoRedoManager, UndoableCommand};
use crate::domain::dependency::{
    AddDependencyCommand, DependencyGraph, RemoveDependencyCommand, TaskRef,
};
use crate::domain::participant::{self, ParticipantRef};
use crate::domain::progress_link::{self, LinkProvider, ProgressLink};
use crate::domain::session::DocumentSession;
use crate::domain::task::{TaskFileLink, TaskTree};
use crate::domain::types::{DocumentId, TaskId};

// ── Undoable commands ─────────────────────────────────────────────────────────

/// Add/remove a participant, snapshotting the affected fields for undo. The
/// actual sync of `participants` <-> `allocated_to` is done by the domain
/// helpers; the `allocated_by` role maps to `Task::allocated_by`.
#[derive(Debug)]
pub struct ParticipantCommand {
    task_id: TaskId,
    name: String,
    role: String,
    add: bool,
    old_allocated_to: Vec<String>,
    old_participants: Vec<ParticipantRef>,
    old_allocated_by: Option<String>,
    desc: String,
}

impl ParticipantCommand {
    fn new(task_id: TaskId, name: String, role: String, add: bool) -> Self {
        let verb = if add { "Add" } else { "Remove" };
        let desc = format!("{} participant '{}' ({})", verb, name, role);
        Self {
            task_id,
            name,
            role,
            add,
            old_allocated_to: Vec::new(),
            old_participants: Vec::new(),
            old_allocated_by: None,
            desc,
        }
    }
}

impl UndoableCommand for ParticipantCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        if let Some(task) = tree.get_mut(&self.task_id) {
            self.old_allocated_to = task.allocated_to.clone();
            self.old_participants = task.participants.clone();
            self.old_allocated_by = task.allocated_by.clone();
            if self.role == "allocated_by" {
                if self.add {
                    task.allocated_by = Some(self.name.clone());
                } else if task.allocated_by.as_deref() == Some(self.name.as_str()) {
                    task.allocated_by = None;
                }
            } else if self.add {
                let _ = participant::add_participant(task, &self.name);
            } else {
                participant::remove_participant(task, &self.name);
            }
        }
        self.desc.clone()
    }
    fn undo(&mut self, tree: &mut TaskTree) {
        if let Some(task) = tree.get_mut(&self.task_id) {
            task.allocated_to = self.old_allocated_to.clone();
            task.participants = self.old_participants.clone();
            task.allocated_by = self.old_allocated_by.clone();
        }
    }
    fn redo(&mut self, tree: &mut TaskTree) {
        self.execute(tree);
    }
    fn description(&self) -> &str {
        &self.desc
    }
}

#[derive(Debug)]
enum LinkOp {
    Add(ProgressLink),
    Remove(String),
    Update { id: String, link: ProgressLink },
}

/// Progress-link add/update/remove. execute() delegates to the domain
/// `progress_link` helpers and snapshots the list for undo.
#[derive(Debug)]
pub struct ProgressLinkCommand {
    task_id: TaskId,
    op: LinkOp,
    old_links: Option<Vec<ProgressLink>>,
    desc: String,
}

impl UndoableCommand for ProgressLinkCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        if let Some(task) = tree.get_mut(&self.task_id) {
            self.old_links = Some(task.progress_links.clone());
            match &self.op {
                LinkOp::Add(link) => {
                    progress_link::add_progress_link(task, link.clone());
                }
                LinkOp::Remove(id) => {
                    progress_link::remove_progress_link(task, id);
                }
                LinkOp::Update { id, link } => {
                    progress_link::remove_progress_link(task, id);
                    progress_link::add_progress_link(task, link.clone());
                }
            }
        }
        self.desc.clone()
    }
    fn undo(&mut self, tree: &mut TaskTree) {
        if let Some(task) = tree.get_mut(&self.task_id) {
            if let Some(old) = self.old_links.clone() {
                task.progress_links = old;
            }
        }
    }
    fn redo(&mut self, tree: &mut TaskTree) {
        self.execute(tree);
    }
    fn description(&self) -> &str {
        &self.desc
    }
}

#[derive(Debug)]
enum AttOp {
    Add(AttachmentRef),
    Remove(String),
    Rename { id: String, display_name: String },
}

/// Attachment add/remove/rename. execute() delegates to the domain
/// `attachment` helpers (keeping the native `FILEREFPATH` mirror in sync) and
/// snapshots both vectors for undo.
#[derive(Debug)]
pub struct AttachmentCommand {
    task_id: TaskId,
    op: AttOp,
    old_attachments: Option<Vec<AttachmentRef>>,
    old_file_links: Option<Vec<TaskFileLink>>,
    desc: String,
}

impl UndoableCommand for AttachmentCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        if let Some(task) = tree.get_mut(&self.task_id) {
            self.old_attachments = Some(task.attachments.clone());
            self.old_file_links = Some(task.file_links.clone());
            match &self.op {
                AttOp::Add(att) => {
                    attachment::add_attachment(task, att.clone());
                }
                AttOp::Remove(id) => {
                    attachment::remove_attachment(task, id);
                }
                AttOp::Rename { id, display_name } => {
                    if let Some(a) = task.attachments.iter_mut().find(|a| &a.id == id) {
                        a.display_name = display_name.clone();
                    }
                }
            }
        }
        self.desc.clone()
    }
    fn undo(&mut self, tree: &mut TaskTree) {
        if let Some(task) = tree.get_mut(&self.task_id) {
            if let Some(old) = self.old_attachments.clone() {
                task.attachments = old;
            }
            if let Some(old) = self.old_file_links.clone() {
                task.file_links = old;
            }
        }
    }
    fn redo(&mut self, tree: &mut TaskTree) {
        self.execute(tree);
    }
    fn description(&self) -> &str {
        &self.desc
    }
}

/// Adds a cross-document (external) dependency. Local edges use the domain's
/// validated [`AddDependencyCommand`]; external edges cannot be cycle-checked
/// against a single-document graph (per the domain design) so they are appended
/// as a `TaskRef::External` and made undoable here.
#[derive(Debug)]
pub struct ExternalDependencyCommand {
    source: TaskId,
    target_doc: DocumentId,
    target_task: TaskId,
    dep_type: u8,
    executed: bool,
    desc: String,
}

impl UndoableCommand for ExternalDependencyCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        if let Some(task) = tree.get_mut(&self.source) {
            let dep = TaskRef::External {
                document_id: self.target_doc.clone(),
                task_id: self.target_task.clone(),
            }
            .to_dependency(self.dep_type);
            if !task
                .dependencies
                .iter()
                .any(|d| d.task_id == dep.task_id && d.raw_xml == dep.raw_xml)
            {
                task.dependencies.push(dep);
            }
        }
        self.executed = true;
        self.desc.clone()
    }
    fn undo(&mut self, tree: &mut TaskTree) {
        if let Some(task) = tree.get_mut(&self.source) {
            task.dependencies.retain(|d| {
                !matches!(
                    TaskRef::from_dependency(d),
                    TaskRef::External { ref document_id, ref task_id }
                        if document_id == &self.target_doc && task_id == &self.target_task
                )
            });
        }
        self.executed = false;
    }
    fn redo(&mut self, tree: &mut TaskTree) {
        self.execute(tree);
    }
    fn description(&self) -> &str {
        &self.desc
    }
}

// ── Participants ──────────────────────────────────────────────────────────────

pub fn get_participants_core(
    tree: &TaskTree,
    task_key: &str,
    document_id: &str,
) -> Result<Vec<ParticipantDto>, String> {
    let tid = safe_task_id(task_key)?;
    let task = tree
        .get(&tid)
        .ok_or_else(|| format!("Task '{}' not found", task_key))?;
    Ok(participant::index_rows(document_id, task_key, task)
        .into_iter()
        .map(|r| participant_dto(&r.document_id, &r.task_key, &r.participant, &r.role))
        .collect())
}

#[tauri::command]
pub fn get_participants(
    task_key: String,
    document_id: String,
    state: State<'_, WorkspaceState>,
) -> Result<Vec<ParticipantDto>, String> {
    let db = state.db.lock().map_err(|_| "Lock error".to_string())?;
    let tree = load_document_tree(&db, &document_id)?;
    get_participants_core(&tree, &task_key, &document_id)
}

#[tauri::command]
pub fn list_participants(
    document_id: Option<String>,
    state: State<'_, WorkspaceState>,
) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|_| "Lock error".to_string())?;
    let mut names: Vec<String> = Vec::new();
    for (doc_id, tree) in load_target_trees(&db, document_id.as_deref())? {
        for task in tree.iter() {
            for row in participant::index_rows(&doc_id, task.id.as_str(), task) {
                if !names.iter().any(|n| *n == row.participant) {
                    names.push(row.participant);
                }
            }
        }
    }
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    Ok(names)
}

#[tauri::command]
pub fn list_task_participants(
    document_id: Option<String>,
    state: State<'_, WorkspaceState>,
) -> Result<Vec<ParticipantDto>, String> {
    let db = state.db.lock().map_err(|_| "Lock error".to_string())?;
    let mut out = Vec::new();
    for (doc_id, tree) in load_target_trees(&db, document_id.as_deref())? {
        for task in tree.iter() {
            for row in participant::index_rows(&doc_id, task.id.as_str(), task) {
                out.push(participant_dto(&row.document_id, &row.task_key, &row.participant, &row.role));
            }
        }
    }
    Ok(out)
}

pub fn add_participant_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
    display_name: &str,
    role: &str,
) -> Result<MutationAck, String> {
    let tid = safe_task_id(task_key)?;
    if tree.get(&tid).is_none() {
        return Err(format!("Task '{}' not found", task_key));
    }
    let role = if role.trim().is_empty() { "allocated_to".to_string() } else { role.trim().to_string() };
    if role == "allocated_by" {
        if display_name.trim().is_empty() {
            return Err("participant name is empty or whitespace-only".to_string());
        }
    } else {
        // Delegate validation to the domain (empty / control chars / ';').
        ParticipantRef::new(display_name).map_err(|e| e.to_string())?;
    }
    let cmd = ParticipantCommand::new(tid, display_name.trim().to_string(), role, true);
    undo.execute(Box::new(cmd), tree);
    let revision = session.record_mutation();
    Ok(MutationAck::ok(revision))
}

#[tauri::command]
pub fn add_participant(
    session_id: u64,
    task_key: String,
    document_id: String,
    display_name: String,
    role: String,
    state: State<'_, AppState>,
) -> Result<MutationAck, String> {
    let _ = document_id;
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    add_participant_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &task_key, &display_name, &role)
}

pub fn remove_participant_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
    display_name: &str,
    role: &str,
) -> Result<MutationAck, String> {
    let tid = safe_task_id(task_key)?;
    if tree.get(&tid).is_none() {
        return Err(format!("Task '{}' not found", task_key));
    }
    let role = if role.trim().is_empty() { "allocated_to".to_string() } else { role.trim().to_string() };
    let cmd = ParticipantCommand::new(tid, display_name.trim().to_string(), role, false);
    undo.execute(Box::new(cmd), tree);
    let revision = session.record_mutation();
    Ok(MutationAck::ok(revision))
}

#[tauri::command]
pub fn remove_participant(
    session_id: u64,
    task_key: String,
    document_id: String,
    display_name: String,
    role: Option<String>,
    state: State<'_, AppState>,
) -> Result<MutationAck, String> {
    let _ = document_id;
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    remove_participant_core(
        &entry.session,
        &mut entry.tree,
        &mut entry.undo_manager,
        &task_key,
        &display_name,
        role.as_deref().unwrap_or("allocated_to"),
    )
}

// ── Dependencies ──────────────────────────────────────────────────────────────

pub fn get_dependencies_core(
    tree: &TaskTree,
    task_key: &str,
    document_id: &str,
) -> Result<DependencyGraphDto, String> {
    let tid = safe_task_id(task_key)?;
    let task = tree
        .get(&tid)
        .ok_or_else(|| format!("Task '{}' not found", task_key))?;
    let graph = DependencyGraph::from_tree(tree);

    let outgoing: Vec<DependencyDto> = task
        .dependencies
        .iter()
        .map(|dep| dependency_dto(document_id, task_key, dep, &graph))
        .collect();

    let mut incoming: Vec<DependencyDto> = Vec::new();
    for other in tree.iter() {
        if other.id == tid {
            continue;
        }
        for dep in &other.dependencies {
            let targets_this = match TaskRef::from_dependency(dep) {
                TaskRef::Local(id) => id == tid,
                TaskRef::External { task_id, .. } => task_id == tid,
                TaskRef::Unresolved(_) => false,
            };
            if targets_this {
                incoming.push(dependency_dto(document_id, other.id.as_str(), dep, &graph));
            }
        }
    }

    let blocked = outgoing.iter().any(|d| {
        d.circular
            || d.ref_kind == "unresolved"
            || (d.ref_kind == "local"
                && safe_task_id(&d.depends_on_key)
                    .ok()
                    .and_then(|t| tree.get(&t).is_none().into())
                    .unwrap_or(false))
    });

    Ok(DependencyGraphDto { outgoing, incoming, blocked })
}

#[tauri::command]
pub fn get_dependencies(
    task_key: String,
    document_id: String,
    state: State<'_, WorkspaceState>,
) -> Result<DependencyGraphDto, String> {
    let db = state.db.lock().map_err(|_| "Lock error".to_string())?;
    let tree = load_document_tree(&db, &document_id)?;
    get_dependencies_core(&tree, &task_key, &document_id)
}

#[tauri::command]
pub fn list_dependencies(
    document_id: Option<String>,
    state: State<'_, WorkspaceState>,
) -> Result<Vec<DependencyDto>, String> {
    let db = state.db.lock().map_err(|_| "Lock error".to_string())?;
    let mut out = Vec::new();
    for (doc_id, tree) in load_target_trees(&db, document_id.as_deref())? {
        let graph = DependencyGraph::from_tree(&tree);
        for task in tree.iter() {
            for dep in &task.dependencies {
                out.push(dependency_dto(&doc_id, task.id.as_str(), dep, &graph));
            }
        }
    }
    Ok(out)
}

pub fn add_dependency_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
    document_id: &str,
    depends_on_key: &str,
    depends_on_document_id: Option<&str>,
    dep_type: u8,
) -> Result<MutationAck, String> {
    let source = safe_task_id(task_key)?;
    if tree.get(&source).is_none() {
        return Err(format!("Task '{}' not found", task_key));
    }
    let target = safe_task_id(depends_on_key)?;

    let external = depends_on_document_id
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != document_id);

    if let Some(ext_doc) = external {
        let cmd = ExternalDependencyCommand {
            source,
            target_doc: DocumentId::new(ext_doc),
            target_task: target,
            dep_type,
            executed: false,
            desc: format!("Add external dependency {} -> {}:{}", task_key, ext_doc, depends_on_key),
        };
        undo.execute(Box::new(cmd), tree);
    } else {
        // Local edge: the domain's checked constructor rejects unknown tasks,
        // self-references and cycles.
        let cmd = AddDependencyCommand::new(tree, source, target, dep_type).map_err(|e| e.to_string())?;
        undo.execute(Box::new(cmd), tree);
    }
    let revision = session.record_mutation();
    Ok(MutationAck::ok(revision))
}

#[tauri::command]
pub fn add_dependency(
    session_id: u64,
    task_key: String,
    document_id: String,
    depends_on_key: String,
    depends_on_document_id: Option<String>,
    dep_type: u8,
    state: State<'_, AppState>,
) -> Result<MutationAck, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    add_dependency_core(
        &entry.session,
        &mut entry.tree,
        &mut entry.undo_manager,
        &task_key,
        &document_id,
        &depends_on_key,
        depends_on_document_id.as_deref(),
        dep_type,
    )
}

pub fn remove_dependency_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
    depends_on_key: &str,
) -> Result<MutationAck, String> {
    let source = safe_task_id(task_key)?;
    let target = safe_task_id(depends_on_key)?;
    let cmd = RemoveDependencyCommand::new(tree, source, target).map_err(|e| e.to_string())?;
    undo.execute(Box::new(cmd), tree);
    let revision = session.record_mutation();
    Ok(MutationAck::ok(revision))
}

#[tauri::command]
pub fn remove_dependency(
    session_id: u64,
    task_key: String,
    document_id: String,
    depends_on_key: String,
    state: State<'_, AppState>,
) -> Result<MutationAck, String> {
    let _ = document_id;
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    remove_dependency_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &task_key, &depends_on_key)
}

// ── Progress links ────────────────────────────────────────────────────────────

pub fn list_progress_links_core(
    tree: &TaskTree,
    task_key: &str,
    document_id: &str,
) -> Result<Vec<ProgressLinkDto>, String> {
    let tid = safe_task_id(task_key)?;
    let task = tree
        .get(&tid)
        .ok_or_else(|| format!("Task '{}' not found", task_key))?;
    Ok(task
        .progress_links
        .iter()
        .map(|l| progress_link_dto(document_id, task_key, l))
        .collect())
}

#[tauri::command]
pub fn list_progress_links(
    task_key: String,
    document_id: String,
    state: State<'_, WorkspaceState>,
) -> Result<Vec<ProgressLinkDto>, String> {
    let db = state.db.lock().map_err(|_| "Lock error".to_string())?;
    let tree = load_document_tree(&db, &document_id)?;
    list_progress_links_core(&tree, &task_key, &document_id)
}

/// Builds + validates a link (URL scheme is enforced by the domain), applying an
/// explicit provider override when supplied.
fn build_link(id: Option<String>, label: &str, url: &str, provider: Option<&str>) -> Result<ProgressLink, String> {
    let mut link = match id {
        Some(id) => ProgressLink::new(id, label, url).map_err(|e| e.to_string())?,
        None => ProgressLink::new_auto_id(label, url).map_err(|e| e.to_string())?,
    };
    if let Some(p) = provider.map(str::trim).filter(|s| !s.is_empty()) {
        link.provider = LinkProvider::parse(p);
    }
    Ok(link)
}

pub fn add_progress_link_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
    document_id: &str,
    label: &str,
    url: &str,
    provider: Option<&str>,
) -> Result<ProgressLinkDto, String> {
    let tid = safe_task_id(task_key)?;
    if tree.get(&tid).is_none() {
        return Err(format!("Task '{}' not found", task_key));
    }
    let link = build_link(None, label, url, provider)?;
    let dto = progress_link_dto(document_id, task_key, &link);
    let cmd = ProgressLinkCommand {
        task_id: tid,
        op: LinkOp::Add(link),
        old_links: None,
        desc: format!("Add progress link to task {}", task_key),
    };
    undo.execute(Box::new(cmd), tree);
    let _ = session.record_mutation();
    Ok(dto)
}

#[tauri::command]
pub fn add_progress_link(
    session_id: u64,
    task_key: String,
    document_id: String,
    label: String,
    url: String,
    provider: Option<String>,
    state: State<'_, AppState>,
) -> Result<ProgressLinkDto, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    add_progress_link_core(
        &entry.session,
        &mut entry.tree,
        &mut entry.undo_manager,
        &task_key,
        &document_id,
        &label,
        &url,
        provider.as_deref(),
    )
}

pub fn update_progress_link_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    link_id: &str,
    task_key: &str,
    document_id: &str,
    label: &str,
    url: &str,
    provider: Option<&str>,
) -> Result<ProgressLinkDto, String> {
    let tid = safe_task_id(task_key)?;
    let task = tree
        .get(&tid)
        .ok_or_else(|| format!("Task '{}' not found", task_key))?;
    if !task.progress_links.iter().any(|l| l.id == link_id) {
        return Err(format!("Progress link '{}' not found", link_id));
    }
    // Validate the new URL, preserving the stable id.
    let link = build_link(Some(link_id.to_string()), label, url, provider)?;
    let dto = progress_link_dto(document_id, task_key, &link);
    let cmd = ProgressLinkCommand {
        task_id: tid,
        op: LinkOp::Update { id: link_id.to_string(), link },
        old_links: None,
        desc: format!("Update progress link '{}' of task {}", link_id, task_key),
    };
    undo.execute(Box::new(cmd), tree);
    let _ = session.record_mutation();
    Ok(dto)
}

#[tauri::command]
pub fn update_progress_link(
    session_id: u64,
    link_id: String,
    task_key: String,
    document_id: String,
    label: String,
    url: String,
    provider: Option<String>,
    state: State<'_, AppState>,
) -> Result<ProgressLinkDto, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    update_progress_link_core(
        &entry.session,
        &mut entry.tree,
        &mut entry.undo_manager,
        &link_id,
        &task_key,
        &document_id,
        &label,
        &url,
        provider.as_deref(),
    )
}

pub fn remove_progress_link_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    link_id: &str,
    task_key: &str,
) -> Result<MutationAck, String> {
    let tid = safe_task_id(task_key)?;
    let task = tree
        .get(&tid)
        .ok_or_else(|| format!("Task '{}' not found", task_key))?;
    if !task.progress_links.iter().any(|l| l.id == link_id) {
        return Err(format!("Progress link '{}' not found", link_id));
    }
    let cmd = ProgressLinkCommand {
        task_id: tid,
        op: LinkOp::Remove(link_id.to_string()),
        old_links: None,
        desc: format!("Remove progress link '{}' from task {}", link_id, task_key),
    };
    undo.execute(Box::new(cmd), tree);
    let revision = session.record_mutation();
    Ok(MutationAck::ok(revision))
}

#[tauri::command]
pub fn remove_progress_link(
    session_id: u64,
    link_id: String,
    task_key: String,
    document_id: String,
    state: State<'_, AppState>,
) -> Result<MutationAck, String> {
    let _ = document_id;
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    remove_progress_link_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &link_id, &task_key)
}

// ── Attachments ───────────────────────────────────────────────────────────────

pub fn list_attachments_core(
    tree: &TaskTree,
    task_key: &str,
    document_id: &str,
    doc_dir: &Path,
) -> Result<Vec<AttachmentDto>, String> {
    let tid = safe_task_id(task_key)?;
    let task = tree
        .get(&tid)
        .ok_or_else(|| format!("Task '{}' not found", task_key))?;
    Ok(attachment::scan_task_attachments(task_key, task)
        .iter()
        .map(|a| attachment_dto(document_id, task_key, doc_dir, a))
        .collect())
}

#[tauri::command]
pub fn list_attachments(
    task_key: String,
    document_id: String,
    state: State<'_, WorkspaceState>,
) -> Result<Vec<AttachmentDto>, String> {
    let db = state.db.lock().map_err(|_| "Lock error".to_string())?;
    let file_path = resolve_document_path(&db, &document_id)?;
    let doc_dir = document_dir(&file_path);
    let bytes = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read '{}': {}", file_path.display(), e))?;
    let tree = build_tree_from_bytes(&bytes)?;
    list_attachments_core(&tree, &task_key, &document_id, &doc_dir)
}

/// Core logic for the three "add attachment" commands: appends an already
/// built + validated [`AttachmentRef`] to the task through the undo manager and
/// returns the import result. A plain function (no closures) so the disjoint
/// session/tree/undo borrows resolve at each call site.
pub fn add_attachment_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
    document_id: &str,
    doc_dir: &Path,
    att: AttachmentRef,
) -> Result<AttachmentImportResult, String> {
    let tid = safe_task_id(task_key)?;
    let dto = attachment_dto(document_id, task_key, doc_dir, &att);
    let cmd = AttachmentCommand {
        task_id: tid,
        op: AttOp::Add(att),
        old_attachments: None,
        old_file_links: None,
        desc: format!("Add attachment to task {}", task_key),
    };
    undo.execute(Box::new(cmd), tree);
    let _ = session.record_mutation();
    Ok(AttachmentImportResult { success: true, attachment: Some(dto), message: None })
}

fn apply_display_name(mut att: AttachmentRef, display_name: Option<&str>) -> AttachmentRef {
    if let Some(name) = display_name.map(str::trim).filter(|s| !s.is_empty()) {
        att.display_name = name.to_string();
    }
    att
}

/// Resolves the session entry's document directory, ensuring the task exists.
fn attachment_doc_dir(entry: &SessionEntry, task_key: &str) -> Result<PathBuf, String> {
    let tid = safe_task_id(task_key)?;
    if entry.tree.get(&tid).is_none() {
        return Err(format!("Task '{}' not found", task_key));
    }
    Ok(document_dir(&entry.file_path))
}

#[tauri::command]
pub fn add_managed_attachment(
    session_id: u64,
    task_key: String,
    document_id: String,
    source_path: String,
    display_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<AttachmentImportResult, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    let doc_dir = attachment_doc_dir(entry, &task_key)?;
    let att = attachment::import_managed_attachment(Path::new(&source_path), &doc_dir, &document_id, None)
        .map_err(|e| e.to_string())?;
    let att = apply_display_name(att, display_name.as_deref());
    add_attachment_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &task_key, &document_id, &doc_dir, att)
}

#[tauri::command]
pub fn link_local_attachment(
    session_id: u64,
    task_key: String,
    document_id: String,
    source_path: String,
    display_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<AttachmentImportResult, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    let doc_dir = attachment_doc_dir(entry, &task_key)?;
    let att = attachment::link_local_file(&source_path).map_err(|e| e.to_string())?;
    let att = apply_display_name(att, display_name.as_deref());
    add_attachment_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &task_key, &document_id, &doc_dir, att)
}

#[tauri::command]
pub fn add_url_attachment(
    session_id: u64,
    task_key: String,
    document_id: String,
    url: String,
    display_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<AttachmentImportResult, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    let doc_dir = attachment_doc_dir(entry, &task_key)?;
    // URL scheme (http/https only) is validated by the domain.
    let att = attachment::url_attachment(&url, display_name.as_deref()).map_err(|e| e.to_string())?;
    add_attachment_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &task_key, &document_id, &doc_dir, att)
}

#[tauri::command]
pub fn update_attachment(
    session_id: u64,
    attachment_id: String,
    task_key: String,
    document_id: String,
    display_name: String,
    state: State<'_, AppState>,
) -> Result<AttachmentDto, String> {
    let name = display_name.trim();
    if name.is_empty() {
        return Err("display_name must not be empty".to_string());
    }
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    let doc_dir = attachment_doc_dir(entry, &task_key)?;
    let tid = safe_task_id(&task_key)?;
    let existing = {
        let task = entry
            .tree
            .get(&tid)
            .ok_or_else(|| format!("Task '{}' not found", task_key))?;
        task.attachments
            .iter()
            .find(|a| a.id == attachment_id)
            .cloned()
            .ok_or_else(|| format!("Attachment '{}' not found on task '{}'", attachment_id, task_key))?
    };
    let cmd = AttachmentCommand {
        task_id: tid,
        op: AttOp::Rename { id: attachment_id.clone(), display_name: name.to_string() },
        old_attachments: None,
        old_file_links: None,
        desc: format!("Rename attachment '{}' of task {}", attachment_id, task_key),
    };
    entry.undo_manager.execute(Box::new(cmd), &mut entry.tree);
    let _ = entry.session.record_mutation();
    let mut updated = existing;
    updated.display_name = name.to_string();
    Ok(attachment_dto(&document_id, &task_key, &doc_dir, &updated))
}

/// Core logic of `remove_attachment`: removes the reference (undoably) and, for
/// managed files, marks the copied file as an orphan candidate (RD-M6-046).
///
/// NOTE: an undo of this removal restores the reference but leaves the orphan
/// marker. No bridge command runs `gc_orphans`, and removal never deletes the
/// file, so undo is safe within a session.
pub fn remove_attachment_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
    document_id: &str,
    doc_dir: &Path,
    attachment_id: &str,
) -> Result<MutationAck, String> {
    let tid = safe_task_id(task_key)?;
    // Capture the ref (for orphan tracking) before the undoable removal.
    let removed = {
        let task = tree
            .get(&tid)
            .ok_or_else(|| format!("Task '{}' not found", task_key))?;
        task.attachments.iter().find(|a| a.id == attachment_id).cloned()
    };
    let removed = removed
        .ok_or_else(|| format!("Attachment '{}' not found on task '{}'", attachment_id, task_key))?;

    let cmd = AttachmentCommand {
        task_id: tid,
        op: AttOp::Remove(attachment_id.to_string()),
        old_attachments: None,
        old_file_links: None,
        desc: format!("Remove attachment '{}' from task {}", attachment_id, task_key),
    };
    undo.execute(Box::new(cmd), tree);
    let revision = session.record_mutation();

    if removed.kind == AttachmentKind::ManagedFile {
        let _ = attachment::mark_orphan(doc_dir, document_id, &removed);
    }
    Ok(MutationAck::ok(revision))
}

#[tauri::command]
pub fn remove_attachment(
    session_id: u64,
    attachment_id: String,
    task_key: String,
    document_id: String,
    state: State<'_, AppState>,
) -> Result<MutationAck, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    let doc_dir = document_dir(&entry.file_path);
    remove_attachment_core(
        &entry.session,
        &mut entry.tree,
        &mut entry.undo_manager,
        &task_key,
        &document_id,
        &doc_dir,
        &attachment_id,
    )
}

// ── Read helpers ──────────────────────────────────────────────────────────────

/// Loads the trees for either one document (`Some(id)`) or every registered
/// document (`None`), skipping any that fail to read/parse.
fn load_target_trees(
    db: &crate::infrastructure::DatabaseManager,
    document_id: Option<&str>,
) -> Result<Vec<(String, TaskTree)>, String> {
    let docs: Vec<(String, PathBuf)> = match document_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => vec![(id.to_string(), resolve_document_path(db, id)?)],
        None => crate::commands::bridge::list_all_documents(db),
    };
    let mut out = Vec::new();
    for (doc_id, path) in docs {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if let Ok(tree) = build_tree_from_bytes(&bytes) {
            out.push((doc_id, tree));
        }
    }
    Ok(out)
}
