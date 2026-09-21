//! IPC bridge: task mutation + M9 productivity commands.
//!
//! Thin Tauri wrappers over the domain primitives. Every mutation is routed
//! through an [`UndoableCommand`] executed by the session's [`UndoRedoManager`]
//! and followed by [`DocumentSession::record_mutation`] so the edit is undoable
//! and the session becomes dirty (autosave picks it up).
//!
//! The `*_core` functions hold the real logic and take only public domain types
//! (`&DocumentSession`, `&mut TaskTree`, `&mut UndoRedoManager`), so they are
//! exercised directly by `tests/ipc_bridge_tests.rs`; the `#[tauri::command]`
//! wrappers just resolve the session and delegate.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::bridge::{
    safe_task_id, MutationAck, SearchHitDto, SearchResponseDto,
};
use crate::commands::session::{AppState, SessionEntry};
use crate::commands::workspace::WorkspaceState;
use crate::domain::command::{
    AddTaskCommand, DeleteTaskCommand, FieldUpdateCommand, FieldValue, TaskField, UndoRedoManager,
    UndoableCommand,
};
use crate::domain::participant;
use crate::domain::quick_add::{build_task, QuickAddDraft};
use crate::domain::search::{SearchPage, SearchQuery};
use crate::domain::session::DocumentSession;
use crate::domain::smart_view::naive_date_to_ole;
use crate::domain::task::TaskTree;
use crate::domain::types::TaskId;
use crate::infrastructure::search_fts;

// ── Response / request DTOs (match src/ipc/types.ts) ──────────────────────────

/// Response of `update_task_field`.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateTaskFieldResponse {
    pub success: bool,
    pub task_key: String,
    pub field: String,
    pub new_value: String,
    pub revision: u64,
}

/// Response of `delete_task`.
#[derive(Debug, Clone, Serialize)]
pub struct DeleteTaskResponse {
    pub success: bool,
    pub task_key: String,
    pub revision: u64,
}

/// Request payload of `add_task`.
#[derive(Debug, Clone, Deserialize)]
pub struct AddTaskRequest {
    pub session_id: u64,
    pub document_id: String,
    pub task_key: Option<String>,
    pub title: String,
    pub parent_key: Option<String>,
    pub priority: i64,
    pub status: String,
    pub due_date: Option<String>,
    pub start_date: Option<String>,
    pub tags: Vec<String>,
    pub participants: Vec<String>,
}

/// Response of `add_task` / `quick_add_task`.
#[derive(Debug, Clone, Serialize)]
pub struct AddTaskResponse {
    pub success: bool,
    pub task_key: String,
    pub revision: u64,
}

/// Request payload of `quick_add_task`.
#[derive(Debug, Clone, Deserialize)]
pub struct QuickAddRequest {
    pub document_id: Option<String>,
    pub parent_key: Option<String>,
    pub title: String,
    pub tags: Vec<String>,
    pub participants: Vec<String>,
    pub priority: i64,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
}

// ── Custom undoable command: completion date ──────────────────────────────────

/// `TaskField` has no `CompletionDate` variant, so `completed_date` edits use
/// this command. Unlike [`FieldUpdateCommand`]'s start/due handling (which only
/// touches the OLE float), it keeps `COMPLETIONDATE` and `COMPLETIONDATESTRING`
/// in sync so the saved document is internally consistent.
#[derive(Debug)]
pub struct CompletionDateCommand {
    task_id: TaskId,
    new_date: Option<NaiveDate>,
    old_date: Option<f64>,
    old_string: Option<String>,
    desc: String,
}

impl CompletionDateCommand {
    /// Parses `value` (`YYYY-MM-DD`, empty clears the date).
    pub fn new(task_id: &TaskId, value: &str) -> Result<Self, String> {
        let new_date = parse_opt_date(Some(value))?;
        Ok(Self {
            task_id: task_id.clone(),
            new_date,
            old_date: None,
            old_string: None,
            desc: format!("Change completion date of task {}", task_id),
        })
    }
}

impl UndoableCommand for CompletionDateCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        if let Some(task) = tree.get_mut(&self.task_id) {
            self.old_date = task.completion_date;
            self.old_string = task.completion_date_string.clone();
            match self.new_date {
                Some(d) => {
                    task.completion_date = Some(naive_date_to_ole(d));
                    task.completion_date_string = Some(d.format("%Y-%m-%d").to_string());
                }
                None => {
                    task.completion_date = None;
                    task.completion_date_string = None;
                }
            }
        }
        self.desc.clone()
    }
    fn undo(&mut self, tree: &mut TaskTree) {
        if let Some(task) = tree.get_mut(&self.task_id) {
            task.completion_date = self.old_date;
            task.completion_date_string = self.old_string.clone();
        }
    }
    fn redo(&mut self, tree: &mut TaskTree) {
        self.execute(tree);
    }
    fn description(&self) -> &str {
        &self.desc
    }
}

// ── Field parsing helpers ─────────────────────────────────────────────────────

fn parse_int(value: &str) -> Option<i64> {
    let v = value.trim();
    if let Ok(n) = v.parse::<i64>() {
        return Some(n);
    }
    v.parse::<f64>().ok().map(|f| f.round() as i64)
}

fn clamp_u8(value: &str, min: u32, max: u32) -> Result<u8, String> {
    let n = parse_int(value).ok_or_else(|| format!("'{}' is not a valid number", value))?;
    Ok(n.clamp(min as i64, max as i64) as u8)
}

/// Parses an optional `YYYY-MM-DD` (also accepts an RFC3339 datetime or a raw
/// OLE float). Empty/None clears the date.
fn parse_opt_date(value: Option<&str>) -> Result<Option<NaiveDate>, String> {
    let v = match value.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(v) => v,
        None => return Ok(None),
    };
    if let Ok(d) = NaiveDate::parse_from_str(v, "%Y-%m-%d") {
        return Ok(Some(d));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(v) {
        return Ok(Some(dt.date_naive()));
    }
    Err(format!("'{}' is not a valid date (expected YYYY-MM-DD)", v))
}

/// Builds the `FieldValue` for a start/due date plus its display string.
fn date_field_value(value: &str) -> Result<(FieldValue, String), String> {
    match parse_opt_date(Some(value))? {
        None => Ok((FieldValue::Float(None), String::new())),
        Some(d) => Ok((
            FieldValue::Float(Some(naive_date_to_ole(d))),
            d.format("%Y-%m-%d").to_string(),
        )),
    }
}

/// Maps a UI status string to the derived `percent_done`.
///
/// `Task` has no status field — status is derived from `percent_done`
/// (0 = NotStarted, 1-99 = InProgress, 100 = Done). `InProgress` preserves an
/// existing 1-99 value so a slider edit is not clobbered.
fn status_to_percent_done(value: &str, current: u8) -> Result<u8, String> {
    let norm: String = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    match norm.as_str() {
        "done" | "completed" | "complete" | "finished" | "closed" => Ok(100),
        "" | "notstarted" | "todo" | "new" | "none" | "open" | "backlog" => Ok(0),
        "inprogress" | "active" | "started" | "doing" | "wip" => {
            Ok(if (1..=99).contains(&current) { current } else { 50 })
        }
        _ => {
            if let Ok(n) = norm.parse::<u32>() {
                if n <= 100 {
                    return Ok(n as u8);
                }
            }
            Err(format!("Unsupported status '{}'", value))
        }
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split([',', ';', '\n'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn clamp_priority(p: i64) -> u8 {
    p.clamp(0, 10) as u8
}

/// Allocates a fresh numeric task id (max existing numeric id + 1).
fn allocate_task_id(tree: &TaskTree) -> TaskId {
    let mut max = 0u64;
    for t in tree.iter() {
        if let Some(n) = t.id.as_u64() {
            if n > max {
                max = n;
            }
        }
    }
    TaskId::from_u64(max + 1)
}

// ── update_task_field ─────────────────────────────────────────────────────────

/// Core logic of `update_task_field` (no Tauri state).
pub fn update_task_field_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
    field: &str,
    value: &str,
) -> Result<UpdateTaskFieldResponse, String> {
    let tid = safe_task_id(task_key)?;
    let task = tree
        .get(&tid)
        .ok_or_else(|| format!("Task '{}' not found", task_key))?;
    let field_norm = field.trim().to_ascii_lowercase();

    // completion date is not a TaskField variant -> dedicated command.
    if field_norm == "completed_date" || field_norm == "completion_date" {
        let cmd = CompletionDateCommand::new(&tid, value)?;
        let new_value = parse_opt_date(Some(value))?
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        undo.execute(Box::new(cmd), tree);
        let revision = session.record_mutation();
        return Ok(UpdateTaskFieldResponse {
            success: true,
            task_key: task_key.to_string(),
            field: field_norm,
            new_value,
            revision,
        });
    }

    let current_percent = task.percent_done;
    let (task_field, field_value, new_value) = match field_norm.as_str() {
        "title" => (TaskField::Title, FieldValue::Text(value.to_string()), value.to_string()),
        "priority" => {
            let n = clamp_u8(value, 0, 10)?;
            (TaskField::Priority, FieldValue::Integer(n as u32), n.to_string())
        }
        "percent_done" | "percentdone" | "percent" => {
            let n = clamp_u8(value, 0, 100)?;
            (TaskField::PercentDone, FieldValue::Integer(n as u32), n.to_string())
        }
        "risk" => {
            let n = clamp_u8(value, 0, 10)?;
            (TaskField::Risk, FieldValue::Integer(n as u32), n.to_string())
        }
        "status" => {
            let n = status_to_percent_done(value, current_percent)?;
            (TaskField::PercentDone, FieldValue::Integer(n as u32), n.to_string())
        }
        "start_date" | "startdate" => {
            let (fv, s) = date_field_value(value)?;
            (TaskField::StartDate, fv, s)
        }
        "due_date" | "duedate" => {
            let (fv, s) = date_field_value(value)?;
            (TaskField::DueDate, fv, s)
        }
        "comments" | "comment" | "description" => {
            (TaskField::Comments, FieldValue::Text(value.to_string()), value.to_string())
        }
        "tags" | "categories" => {
            let list = split_list(value);
            let shown = list.join(", ");
            (TaskField::Categories, FieldValue::StringList(list), shown)
        }
        "allocated_to" | "allocatedto" | "participants" => {
            let list = split_list(value);
            let shown = list.join(", ");
            (TaskField::AllocatedTo, FieldValue::StringList(list), shown)
        }
        other => return Err(format!("Unsupported field '{}'", other)),
    };

    let cmd = FieldUpdateCommand {
        task_id: tid,
        field: task_field,
        new_value: field_value,
        old_value: None,
        desc: format!("Change {} of task {}", field_norm, task_key),
    };
    undo.execute(Box::new(cmd), tree);

    // AllocatedTo edits must re-sync the typed participant mirror.
    if field_norm == "allocated_to" || field_norm == "allocatedto" || field_norm == "participants" {
        if let Some(t) = tree.get_mut(&safe_task_id(task_key)?) {
            participant::refresh_from_allocated(t);
        }
    }

    let revision = session.record_mutation();
    Ok(UpdateTaskFieldResponse {
        success: true,
        task_key: task_key.to_string(),
        field: field_norm,
        new_value,
        revision,
    })
}

#[tauri::command]
pub fn update_task_field(
    session_id: u64,
    task_key: String,
    field: String,
    value: String,
    state: State<'_, AppState>,
) -> Result<UpdateTaskFieldResponse, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    update_task_field_core(
        &entry.session,
        &mut entry.tree,
        &mut entry.undo_manager,
        &task_key,
        &field,
        &value,
    )
}

// ── add_task / quick_add_task ─────────────────────────────────────────────────

/// Core logic shared by `add_task` and `quick_add_task`.
pub fn add_task_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    request: &AddTaskRequest,
) -> Result<AddTaskResponse, String> {
    let title = request.title.trim();
    if title.is_empty() {
        return Err("Task title must not be empty".to_string());
    }

    let task_id = match request.task_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(key) => {
            let tid = safe_task_id(key)?;
            if tree.contains(&tid) {
                return Err(format!("Task '{}' already exists", key));
            }
            tid
        }
        None => allocate_task_id(tree),
    };

    let draft = QuickAddDraft {
        title: title.to_string(),
        tags: request.tags.clone(),
        participants: request.participants.clone(),
        priority: Some(clamp_priority(request.priority)),
        due_date: parse_opt_date(request.due_date.as_deref())?,
        start_date: parse_opt_date(request.start_date.as_deref())?,
        rejected_tokens: Vec::new(),
    };
    let mut task = build_task(&draft, task_id.clone());
    task.title = title.to_string();
    task.percent_done = status_to_percent_done(&request.status, 0)?;
    // Keep the typed participant mirror in sync with allocated_to.
    task.participants = participant::refs_from_names(&task.allocated_to);

    let parent_id = match request.parent_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(pk) => {
            let pid = safe_task_id(pk)?;
            if tree.get(&pid).is_none() {
                return Err(format!("Parent task '{}' not found", pk));
            }
            Some(pid)
        }
        None => None,
    };
    let position = match &parent_id {
        Some(pid) => tree.get(pid).map(|p| p.children.len()).unwrap_or(0),
        None => tree.root_ids().len(),
    };

    let key_out = task_id.as_str().to_string();
    let cmd = AddTaskCommand { task, parent_id, position, executed: false };
    undo.execute(Box::new(cmd), tree);
    let revision = session.record_mutation();
    Ok(AddTaskResponse { success: true, task_key: key_out, revision })
}

#[tauri::command]
pub fn add_task(
    request: AddTaskRequest,
    state: State<'_, AppState>,
) -> Result<AddTaskResponse, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&request.session_id)
        .ok_or_else(|| format!("Session {} not found", request.session_id))?;
    add_task_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &request)
}

/// Core logic of `quick_add_task`: delegates to [`add_task_core`] after mapping
/// the structured request (the frontend parses the quick-add grammar first via
/// `parse_quick_add`, then submits the fields here).
pub fn quick_add_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    request: &QuickAddRequest,
) -> Result<AddTaskResponse, String> {
    if request.title.trim().is_empty() {
        return Err("Quick add input is empty".to_string());
    }
    let add = AddTaskRequest {
        session_id: 0,
        document_id: request.document_id.clone().unwrap_or_default(),
        task_key: None,
        title: request.title.clone(),
        parent_key: request.parent_key.clone(),
        priority: request.priority,
        status: "Not Started".to_string(),
        due_date: request.due_date.clone(),
        start_date: request.start_date.clone(),
        tags: request.tags.clone(),
        participants: request.participants.clone(),
    };
    add_task_core(session, tree, undo, &add)
}

/// Chooses the session a quick-add targets: the one whose document matches
/// `target_path`, else the sole open session.
fn resolve_session_id(
    sessions: &HashMap<u64, SessionEntry>,
    target_path: Option<&PathBuf>,
) -> Result<u64, String> {
    if let Some(p) = target_path {
        if let Some((&id, _)) = sessions.iter().find(|(_, e)| &e.file_path == p) {
            return Ok(id);
        }
    }
    let mut it = sessions.keys();
    match (it.next(), it.next()) {
        (Some(&only), None) => Ok(only),
        (None, _) => Err("No open document session to add the task to; open the document first".to_string()),
        _ => Err("Could not resolve the quick-add target document session; open or select the document first".to_string()),
    }
}

#[tauri::command]
pub fn quick_add_task(
    request: QuickAddRequest,
    ws: State<'_, WorkspaceState>,
    sess: State<'_, AppState>,
) -> Result<AddTaskResponse, String> {
    // Resolve the target document path first (db lock released before sessions).
    let target_path = match request.document_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(doc_id) => {
            let db = ws.db.lock().map_err(|_| "Lock error".to_string())?;
            crate::commands::bridge::resolve_document_path(&db, doc_id).ok()
        }
        None => None,
    };

    let mut sessions = sess.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let session_id = resolve_session_id(&sessions, target_path.as_ref())?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    quick_add_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &request)
}

// ── delete_task ───────────────────────────────────────────────────────────────

/// Core logic of `delete_task`.
pub fn delete_task_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
) -> Result<DeleteTaskResponse, String> {
    let tid = safe_task_id(task_key)?;
    if tree.get(&tid).is_none() {
        return Err(format!("Task '{}' not found", task_key));
    }
    let cmd = DeleteTaskCommand {
        task_id: tid,
        saved_tasks: Vec::new(),
        saved_root_ids: Vec::new(),
        parent_id: None,
        executed: false,
    };
    undo.execute(Box::new(cmd), tree);
    let revision = session.record_mutation();
    Ok(DeleteTaskResponse { success: true, task_key: task_key.to_string(), revision })
}

#[tauri::command]
pub fn delete_task(
    session_id: u64,
    task_key: String,
    state: State<'_, AppState>,
) -> Result<DeleteTaskResponse, String> {
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    delete_task_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &task_key)
}

// ── set_task_tags ─────────────────────────────────────────────────────────────

/// Core logic of `set_task_tags` (replaces the task's categories).
pub fn set_task_tags_core(
    session: &DocumentSession,
    tree: &mut TaskTree,
    undo: &mut UndoRedoManager,
    task_key: &str,
    tags: Vec<String>,
) -> Result<MutationAck, String> {
    let tid = safe_task_id(task_key)?;
    if tree.get(&tid).is_none() {
        return Err(format!("Task '{}' not found", task_key));
    }
    let clean: Vec<String> = tags
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let cmd = FieldUpdateCommand {
        task_id: tid,
        field: TaskField::Categories,
        new_value: FieldValue::StringList(clean),
        old_value: None,
        desc: format!("Set tags of task {}", task_key),
    };
    undo.execute(Box::new(cmd), tree);
    let revision = session.record_mutation();
    Ok(MutationAck::ok(revision))
}

#[tauri::command]
pub fn set_task_tags(
    session_id: u64,
    task_key: String,
    document_id: String,
    tags: Vec<String>,
    state: State<'_, AppState>,
) -> Result<MutationAck, String> {
    let _ = document_id; // part of the IPC contract; the mutation targets the session tree.
    let mut sessions = state.sessions.lock().map_err(|e| format!("Lock error: {}", e))?;
    let entry = sessions
        .get_mut(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    set_task_tags_core(&entry.session, &mut entry.tree, &mut entry.undo_manager, &task_key, tags)
}

// ── global_search (M9) ────────────────────────────────────────────────────────

fn to_db_error(e: search_fts::SearchError) -> crate::infrastructure::DatabaseError {
    match e {
        search_fts::SearchError::Sqlite(s) => crate::infrastructure::DatabaseError::Sqlite(s),
        other => crate::infrastructure::DatabaseError::Sqlite(rusqlite::Error::ToSqlConversionFailure(
            Box::new(other),
        )),
    }
}

/// Maps a domain [`SearchPage`] to the frontend's [`SearchResponseDto`].
pub fn page_to_response(page: &SearchPage) -> SearchResponseDto {
    let hits = page
        .results
        .iter()
        .map(|r| SearchHitDto {
            task_key: r.task_key.task_id.as_str().to_string(),
            document_id: r.document_context.document_id.clone(),
            title: r.title.clone(),
            document_path: Some(r.document_context.document_name.clone()),
            matched_field: r.matched_field.as_str().to_string(),
            snippet: r.snippet.clone(),
            score: r.score,
        })
        .collect();
    SearchResponseDto {
        truncated: page.total > page.results.len(),
        total: page.total,
        hits,
    }
}

#[tauri::command]
pub fn global_search(
    query: String,
    document_id: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    state: State<'_, WorkspaceState>,
) -> Result<SearchResponseDto, String> {
    let db = state.db.lock().map_err(|_| "Lock error".to_string())?;
    if !db.is_available() {
        return Ok(SearchResponseDto { hits: Vec::new(), total: 0, truncated: false });
    }
    let mut q = SearchQuery::new(query).with_page(limit.unwrap_or(50), offset.unwrap_or(0));
    if let Some(doc) = document_id {
        q = q.with_document(doc);
    }
    db.with_connection(|conn| {
        search_fts::ensure_search_schema(conn);
        let page = search_fts::search(conn, &q).map_err(to_db_error)?;
        Ok(page_to_response(&page))
    })
    .map_err(|e| e.to_string())
}
