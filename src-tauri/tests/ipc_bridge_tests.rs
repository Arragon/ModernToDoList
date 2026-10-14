//! Integration tests for the M6/M9 IPC bridge (the 27 commands the frontend
//! invokes). They drive the pure `*_core` functions that back every
//! `#[tauri::command]` wrapper, against the public domain types
//! (`TaskTree` / `UndoRedoManager` / `DocumentSession`) — the exact code path a
//! command runs inside a session, minus the Tauri `State` plumbing.
//!
//! Coverage map (required by the bridge task):
//! - update_task_field: title / priority / percent_done / status / dates /
//!   comments / tags / completed_date (+ validation errors).
//! - undo reverses a field update.
//! - delete_task then undo restores identity (leaf + subtree).
//! - add / remove participant (+ separator/empty rejection, undo).
//! - add_dependency rejects a self-reference and a cycle (+ remove, unknown).
//! - add_progress_link rejects `javascript:` (+ obfuscated), accepts https,
//!   update/remove, undo.
//! - attachment add / remove (url + managed import w/ orphan tracking), undo.
//! - add_task / quick_add / set_task_tags.
//! - global_search page mapping.

use std::path::Path;

use moderntodolist_lib::domain::attachment;
use moderntodolist_lib::domain::command::UndoRedoManager;
use moderntodolist_lib::domain::session::DocumentSession;
use moderntodolist_lib::domain::task::{Task, TaskCategory, TaskStatus, TaskTree};
use moderntodolist_lib::domain::types::TaskId;
use moderntodolist_lib::bridge::{build_tree_from_bytes, load_document_tree};
use moderntodolist_lib::infrastructure::{DatabaseError, DatabaseManager};
use moderntodolist_lib::relations::{
    add_attachment_core, add_dependency_core, add_participant_core, add_progress_link_core,
    get_dependencies_core, get_participants_core, list_attachments_core, list_progress_links_core,
    remove_attachment_core, remove_dependency_core, remove_participant_core,
    remove_progress_link_core, update_progress_link_core,
};
use moderntodolist_lib::task_edit::{
    add_task_core, delete_task_core, page_to_response, quick_add_core, set_task_tags_core,
    update_task_field_core, AddTaskRequest, QuickAddRequest,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn session() -> DocumentSession {
    DocumentSession::new(None)
}

fn mgr() -> UndoRedoManager {
    UndoRedoManager::new()
}

/// Three sibling root tasks: 1 = "Task A", 2 = "Task B", 3 = "Task C".
fn sample_tree() -> TaskTree {
    let mut tree = TaskTree::new();
    for (id, title) in [("1", "Task A"), ("2", "Task B"), ("3", "Task C")] {
        let mut t = Task::new(TaskId::new(id));
        t.title = title.to_string();
        tree.add_task(t);
        tree.add_root_id(TaskId::new(id));
    }
    tree
}

fn task<'a>(tree: &'a TaskTree, id: &str) -> &'a Task {
    tree.get(&TaskId::new(id)).expect("task present")
}

fn cat_names(t: &Task) -> Vec<String> {
    t.categories.iter().map(|c| c.name.clone()).collect()
}

fn add_req(title: &str, parent: Option<&str>) -> AddTaskRequest {
    AddTaskRequest {
        session_id: 1,
        document_id: "doc-1".to_string(),
        task_key: None,
        title: title.to_string(),
        parent_key: parent.map(|p| p.to_string()),
        priority: 5,
        status: "Not Started".to_string(),
        due_date: None,
        start_date: None,
        tags: Vec::new(),
        participants: Vec::new(),
    }
}

// ── update_task_field ────────────────────────────────────────────────────────

#[test]
fn update_task_field_title_sets_value_and_reports() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let resp = update_task_field_core(&s, &mut tree, &mut u, "1", "title", "Renamed").unwrap();
    assert!(resp.success);
    assert_eq!(resp.task_key, "1");
    assert_eq!(resp.field, "title");
    assert_eq!(resp.new_value, "Renamed");
    assert_eq!(resp.revision, 2, "record_mutation bumps revision 1 -> 2");
    assert_eq!(task(&tree, "1").title, "Renamed");
    assert!(s.is_dirty(), "mutation marks the session dirty");
}

#[test]
fn update_task_field_priority_sets_and_clamps() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "priority", "7").unwrap();
    assert_eq!(task(&tree, "1").priority.value(), 7);
    let r = update_task_field_core(&s, &mut tree, &mut u, "1", "priority", "99").unwrap();
    assert_eq!(r.new_value, "10", "priority is clamped to the 0-10 scale");
    assert_eq!(task(&tree, "1").priority.value(), 10);
}

#[test]
fn update_task_field_percent_done_sets_value() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "percent_done", "42").unwrap();
    assert_eq!(task(&tree, "1").percent_done, 42);
    assert_eq!(task(&tree, "1").status(), TaskStatus::InProgress);
}

#[test]
fn update_task_field_status_completed_maps_to_percent_100() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "status", "Completed").unwrap();
    assert_eq!(task(&tree, "1").percent_done, 100);
    assert_eq!(task(&tree, "1").status(), TaskStatus::Done);
}

#[test]
fn update_task_field_status_not_started_maps_to_percent_0() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "percent_done", "100").unwrap();
    update_task_field_core(&s, &mut tree, &mut u, "1", "status", "Not Started").unwrap();
    assert_eq!(task(&tree, "1").percent_done, 0);
    assert_eq!(task(&tree, "1").status(), TaskStatus::NotStarted);
}

#[test]
fn update_task_field_status_in_progress_preserves_partial_percent() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    // Existing 1-99 is preserved.
    update_task_field_core(&s, &mut tree, &mut u, "1", "percent_done", "40").unwrap();
    update_task_field_core(&s, &mut tree, &mut u, "1", "status", "In Progress").unwrap();
    assert_eq!(task(&tree, "1").percent_done, 40);
    // From 0 it defaults to 50.
    update_task_field_core(&s, &mut tree, &mut u, "2", "status", "In Progress").unwrap();
    assert_eq!(task(&tree, "2").percent_done, 50);
}

#[test]
fn update_task_field_due_date_parses_iso_and_clears() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "due_date", "2024-03-15").unwrap();
    assert!(task(&tree, "1").due_date.is_some());
    update_task_field_core(&s, &mut tree, &mut u, "1", "due_date", "").unwrap();
    assert!(task(&tree, "1").due_date.is_none(), "empty value clears the date");
}

#[test]
fn update_task_field_start_date_sets_positive_ole() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "start_date", "2024-01-01").unwrap();
    let ole = task(&tree, "1").start_date.expect("start date set");
    assert!(ole > 0.0);
}

#[test]
fn update_task_field_rejects_invalid_date() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    assert!(update_task_field_core(&s, &mut tree, &mut u, "1", "due_date", "not-a-date").is_err());
}

#[test]
fn update_task_field_comments_sets_text() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "comments", "Hello world").unwrap();
    assert_eq!(task(&tree, "1").comments.as_ref().unwrap().content, "Hello world");
}

#[test]
fn update_task_field_tags_sets_categories() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "tags", "work, urgent ,home").unwrap();
    assert_eq!(cat_names(task(&tree, "1")), vec!["work", "urgent", "home"]);
}

#[test]
fn update_task_field_completed_date_sets_float_and_string() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "completed_date", "2024-05-01").unwrap();
    let t = task(&tree, "1");
    assert!(t.completion_date.is_some());
    assert_eq!(t.completion_date_string.as_deref(), Some("2024-05-01"));
}

#[test]
fn update_task_field_rejects_unknown_field_and_missing_task() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    assert!(update_task_field_core(&s, &mut tree, &mut u, "1", "bogus", "x").is_err());
    assert!(update_task_field_core(&s, &mut tree, &mut u, "999", "title", "x").is_err());
    assert!(update_task_field_core(&s, &mut tree, &mut u, "", "title", "x").is_err());
}

#[test]
fn undo_reverts_field_update_and_redo_reapplies() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    update_task_field_core(&s, &mut tree, &mut u, "1", "title", "Changed").unwrap();
    update_task_field_core(&s, &mut tree, &mut u, "1", "priority", "9").unwrap();
    assert_eq!(task(&tree, "1").title, "Changed");
    assert_eq!(task(&tree, "1").priority.value(), 9);

    assert!(u.undo(&mut tree).is_some());
    assert_eq!(task(&tree, "1").priority.value(), 5, "priority restored first");
    assert_eq!(task(&tree, "1").title, "Changed");
    assert!(u.undo(&mut tree).is_some());
    assert_eq!(task(&tree, "1").title, "Task A", "title restored");

    assert!(u.redo(&mut tree).is_some());
    assert_eq!(task(&tree, "1").title, "Changed");
}

// ── delete_task ──────────────────────────────────────────────────────────────

#[test]
fn delete_task_then_undo_restores_identity() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let resp = delete_task_core(&s, &mut tree, &mut u, "2").unwrap();
    assert!(resp.success);
    assert_eq!(resp.task_key, "2");
    assert!(tree.get(&TaskId::new("2")).is_none());

    assert!(u.undo(&mut tree).is_some());
    let t = tree.get(&TaskId::new("2")).expect("task restored by undo");
    assert_eq!(t.id.as_str(), "2");
    assert_eq!(t.title, "Task B");
}

#[test]
fn delete_task_with_children_then_undo_restores_subtree() {
    let mut tree = TaskTree::new();
    let mut p = Task::new(TaskId::new("10"));
    p.title = "Parent".to_string();
    p.children.push(TaskId::new("11"));
    let mut c = Task::new(TaskId::new("11"));
    c.title = "Child".to_string();
    tree.add_task(p);
    tree.add_task(c);
    tree.add_root_id(TaskId::new("10"));

    let s = session();
    let mut u = mgr();
    delete_task_core(&s, &mut tree, &mut u, "10").unwrap();
    assert!(tree.get(&TaskId::new("10")).is_none());
    assert!(tree.get(&TaskId::new("11")).is_none());

    u.undo(&mut tree);
    assert_eq!(task(&tree, "10").title, "Parent");
    assert_eq!(task(&tree, "11").title, "Child");
}

#[test]
fn delete_task_missing_errors() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    assert!(delete_task_core(&s, &mut tree, &mut u, "999").is_err());
}

// ── add_task / quick_add / set_task_tags ─────────────────────────────────────

#[test]
fn add_task_assigns_id_and_inserts_under_parent() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let mut req = add_req("New task", Some("1"));
    req.priority = 7;
    req.due_date = Some("2024-06-01".to_string());
    req.tags = vec!["t1".to_string()];
    req.participants = vec!["Alice".to_string()];

    let resp = add_task_core(&s, &mut tree, &mut u, &req).unwrap();
    assert!(resp.success);
    assert_eq!(resp.task_key, "4", "id is allocated as max(existing)+1");
    let t = task(&tree, "4");
    assert_eq!(t.title, "New task");
    assert_eq!(t.priority.value(), 7);
    assert_eq!(cat_names(t), vec!["t1"]);
    assert_eq!(t.allocated_to, vec!["Alice".to_string()]);
    assert_eq!(t.participants.len(), 1);
    assert!(t.due_date.is_some());
    assert!(task(&tree, "1").children.contains(&TaskId::new("4")));
}

#[test]
fn add_task_rejects_duplicate_and_empty_title() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let mut dup = add_req("Dup", None);
    dup.task_key = Some("1".to_string());
    assert!(add_task_core(&s, &mut tree, &mut u, &dup).is_err());

    let empty = add_req("   ", None);
    assert!(add_task_core(&s, &mut tree, &mut u, &empty).is_err());

    let mut bad_parent = add_req("Orphan", Some("999"));
    bad_parent.parent_key = Some("999".to_string());
    assert!(add_task_core(&s, &mut tree, &mut u, &bad_parent).is_err());
}

#[test]
fn add_task_undo_removes_task() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let req = add_req("Temp", None);
    let resp = add_task_core(&s, &mut tree, &mut u, &req).unwrap();
    assert!(tree.get(&TaskId::new(&resp.task_key)).is_some());
    u.undo(&mut tree);
    assert!(tree.get(&TaskId::new(&resp.task_key)).is_none());
}

#[test]
fn quick_add_core_builds_and_inserts_at_root() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let req = QuickAddRequest {
        document_id: Some("doc-1".to_string()),
        parent_key: None,
        title: "Quick task".to_string(),
        tags: vec!["qt".to_string()],
        participants: vec!["Bob".to_string()],
        priority: 5,
        start_date: None,
        due_date: Some("2024-07-04".to_string()),
    };
    let resp = quick_add_core(&s, &mut tree, &mut u, &req).unwrap();
    assert!(resp.success);
    assert_eq!(resp.task_key, "4");
    let t = task(&tree, "4");
    assert_eq!(t.title, "Quick task");
    assert_eq!(t.allocated_to, vec!["Bob".to_string()]);
    assert_eq!(cat_names(t), vec!["qt"]);
    assert!(t.due_date.is_some());
    assert!(tree.root_ids().contains(&TaskId::new("4")));
}

#[test]
fn quick_add_core_rejects_empty() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let req = QuickAddRequest {
        document_id: None,
        parent_key: None,
        title: "  ".to_string(),
        tags: Vec::new(),
        participants: Vec::new(),
        priority: 5,
        start_date: None,
        due_date: None,
    };
    assert!(quick_add_core(&s, &mut tree, &mut u, &req).is_err());
}

#[test]
fn set_task_tags_replaces_categories_and_undoes() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    tree.get_mut(&TaskId::new("1"))
        .unwrap()
        .categories
        .push(TaskCategory { name: "old".to_string() });

    let ack = set_task_tags_core(&s, &mut tree, &mut u, "1", vec!["a".to_string(), "b".to_string()]).unwrap();
    assert!(ack.success);
    assert_eq!(cat_names(task(&tree, "1")), vec!["a", "b"]);

    u.undo(&mut tree);
    assert_eq!(cat_names(task(&tree, "1")), vec!["old"]);
}

// ── Participants ─────────────────────────────────────────────────────────────

#[test]
fn add_then_remove_participant() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let ack = add_participant_core(&s, &mut tree, &mut u, "1", "Alice", "allocated_to").unwrap();
    assert!(ack.success);
    let rows = get_participants_core(&tree, "1", "doc-1").unwrap();
    assert!(rows.iter().any(|r| r.display_name == "Alice" && r.role == "allocated_to"));
    assert_eq!(task(&tree, "1").allocated_to, vec!["Alice".to_string()]);

    remove_participant_core(&s, &mut tree, &mut u, "1", "Alice", "allocated_to").unwrap();
    let rows2 = get_participants_core(&tree, "1", "doc-1").unwrap();
    assert!(!rows2.iter().any(|r| r.display_name == "Alice"));
    assert!(task(&tree, "1").allocated_to.is_empty());
}

#[test]
fn add_participant_allocated_by_role() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    add_participant_core(&s, &mut tree, &mut u, "1", "Lead", "allocated_by").unwrap();
    assert_eq!(task(&tree, "1").allocated_by.as_deref(), Some("Lead"));
    let rows = get_participants_core(&tree, "1", "doc-1").unwrap();
    assert!(rows.iter().any(|r| r.display_name == "Lead" && r.role == "allocated_by"));
}

#[test]
fn add_participant_rejects_separator_and_empty() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    assert!(add_participant_core(&s, &mut tree, &mut u, "1", "a;b", "allocated_to").is_err());
    assert!(add_participant_core(&s, &mut tree, &mut u, "1", "   ", "allocated_to").is_err());
    assert!(add_participant_core(&s, &mut tree, &mut u, "1", "bad\u{0}name", "allocated_to").is_err());
}

#[test]
fn add_participant_undo_reverts() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    add_participant_core(&s, &mut tree, &mut u, "1", "Alice", "allocated_to").unwrap();
    assert_eq!(task(&tree, "1").allocated_to, vec!["Alice".to_string()]);
    u.undo(&mut tree);
    assert!(task(&tree, "1").allocated_to.is_empty());
    assert!(task(&tree, "1").participants.is_empty());
}

// ── Dependencies ─────────────────────────────────────────────────────────────

#[test]
fn add_dependency_rejects_self_reference() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let err = add_dependency_core(&s, &mut tree, &mut u, "1", "doc-1", "1", None, 0).unwrap_err();
    assert!(err.to_lowercase().contains("itself"), "unexpected: {err}");
}

#[test]
fn add_dependency_rejects_cycle() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    add_dependency_core(&s, &mut tree, &mut u, "1", "doc-1", "2", None, 0).unwrap(); // 1 -> 2
    add_dependency_core(&s, &mut tree, &mut u, "2", "doc-1", "3", None, 0).unwrap(); // 2 -> 3
    let err = add_dependency_core(&s, &mut tree, &mut u, "3", "doc-1", "1", None, 0).unwrap_err();
    assert!(err.to_lowercase().contains("cycle"), "unexpected: {err}");
}

#[test]
fn add_dependency_rejects_unknown_target() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    assert!(add_dependency_core(&s, &mut tree, &mut u, "1", "doc-1", "999", None, 0).is_err());
}

#[test]
fn add_dependency_ok_then_graph_and_remove() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    add_dependency_core(&s, &mut tree, &mut u, "1", "doc-1", "2", None, 0).unwrap();

    let g1 = get_dependencies_core(&tree, "1", "doc-1").unwrap();
    assert_eq!(g1.outgoing.len(), 1);
    assert_eq!(g1.outgoing[0].depends_on_key, "2");
    assert_eq!(g1.outgoing[0].ref_kind, "local");
    assert!(!g1.outgoing[0].circular);

    let g2 = get_dependencies_core(&tree, "2", "doc-1").unwrap();
    assert_eq!(g2.incoming.len(), 1);
    assert_eq!(g2.incoming[0].task_key, "1");

    remove_dependency_core(&s, &mut tree, &mut u, "1", "2").unwrap();
    assert!(get_dependencies_core(&tree, "1", "doc-1").unwrap().outgoing.is_empty());
    // Removing a non-existent edge errors.
    assert!(remove_dependency_core(&s, &mut tree, &mut u, "1", "2").is_err());
}

#[test]
fn add_dependency_undo_reverts_edge() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    add_dependency_core(&s, &mut tree, &mut u, "1", "doc-1", "2", None, 0).unwrap();
    assert_eq!(task(&tree, "1").dependencies.len(), 1);
    u.undo(&mut tree);
    assert!(task(&tree, "1").dependencies.is_empty());
}

// ── Progress links ───────────────────────────────────────────────────────────

#[test]
fn add_progress_link_rejects_javascript_scheme() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let err = add_progress_link_core(&s, &mut tree, &mut u, "1", "doc-1", "Evil", "javascript:alert(1)", None)
        .unwrap_err();
    assert!(err.to_lowercase().contains("scheme"), "unexpected: {err}");
    assert!(task(&tree, "1").progress_links.is_empty());
}

#[test]
fn add_progress_link_rejects_obfuscated_javascript() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    assert!(add_progress_link_core(&s, &mut tree, &mut u, "1", "doc-1", "x", "java\tscript:alert(1)", None).is_err());
    assert!(add_progress_link_core(&s, &mut tree, &mut u, "1", "doc-1", "x", "data:text/html,x", None).is_err());
    assert!(add_progress_link_core(&s, &mut tree, &mut u, "1", "doc-1", "x", "file:///C:/x", None).is_err());
}

#[test]
fn add_progress_link_accepts_https_update_and_remove() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let dto = add_progress_link_core(&s, &mut tree, &mut u, "1", "doc-1", "PR", "https://github.com/o/r/pull/1", None).unwrap();
    assert_eq!(dto.provider, "github");
    assert_eq!(dto.url, "https://github.com/o/r/pull/1");
    assert_eq!(list_progress_links_core(&tree, "1", "doc-1").unwrap().len(), 1);

    let upd = update_progress_link_core(&s, &mut tree, &mut u, &dto.id, "1", "doc-1", "PR2", "https://linear.app/x", None).unwrap();
    assert_eq!(upd.label, "PR2");
    assert_eq!(upd.provider, "linear");
    assert_eq!(upd.id, dto.id, "id is stable across update");
    assert_eq!(list_progress_links_core(&tree, "1", "doc-1").unwrap().len(), 1);

    remove_progress_link_core(&s, &mut tree, &mut u, &dto.id, "1").unwrap();
    assert!(list_progress_links_core(&tree, "1", "doc-1").unwrap().is_empty());
}

#[test]
fn add_progress_link_explicit_provider_override() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let dto = add_progress_link_core(&s, &mut tree, &mut u, "1", "doc-1", "Tracker", "https://example.com/i/9", Some("jira")).unwrap();
    assert_eq!(dto.provider, "jira");
}

#[test]
fn add_progress_link_undo_reverts() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    add_progress_link_core(&s, &mut tree, &mut u, "1", "doc-1", "L", "https://example.com", None).unwrap();
    assert_eq!(task(&tree, "1").progress_links.len(), 1);
    u.undo(&mut tree);
    assert!(task(&tree, "1").progress_links.is_empty());
}

#[test]
fn update_progress_link_rejects_bad_url_and_missing_id() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let dto = add_progress_link_core(&s, &mut tree, &mut u, "1", "doc-1", "L", "https://example.com", None).unwrap();
    assert!(update_progress_link_core(&s, &mut tree, &mut u, &dto.id, "1", "doc-1", "L", "javascript:x", None).is_err());
    assert!(update_progress_link_core(&s, &mut tree, &mut u, "nope", "1", "doc-1", "L", "https://example.com", None).is_err());
}

// ── Attachments ──────────────────────────────────────────────────────────────

#[test]
fn add_url_attachment_then_remove_and_undo() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    let doc_dir = Path::new(".");

    let att = attachment::url_attachment("https://example.com/spec.pdf", Some("Spec")).unwrap();
    let id = att.id.clone();
    let res = add_attachment_core(&s, &mut tree, &mut u, "1", "doc-1", doc_dir, att).unwrap();
    assert!(res.success);
    let dto = res.attachment.unwrap();
    assert_eq!(dto.kind, "url");
    assert_eq!(dto.display_name, "Spec");
    assert!(dto.exists);
    assert_eq!(dto.status, "ok");
    assert_eq!(list_attachments_core(&tree, "1", "doc-1", doc_dir).unwrap().len(), 1);

    remove_attachment_core(&s, &mut tree, &mut u, "1", "doc-1", doc_dir, &id).unwrap();
    assert!(list_attachments_core(&tree, "1", "doc-1", doc_dir).unwrap().is_empty());

    u.undo(&mut tree);
    assert_eq!(list_attachments_core(&tree, "1", "doc-1", doc_dir).unwrap().len(), 1);
}

#[test]
fn url_attachment_rejects_non_http_scheme() {
    // The bridge delegates URL validation to the domain (http/https only).
    assert!(attachment::url_attachment("javascript:alert(1)", None).is_err());
    assert!(attachment::url_attachment("file:///C:/x", None).is_err());
}

#[test]
fn add_managed_attachment_imports_file_then_remove_marks_orphan() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc_dir = dir.path();
    let source = doc_dir.join("source.txt");
    std::fs::write(&source, b"managed payload").unwrap();

    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();

    let att = attachment::import_managed_attachment(&source, doc_dir, "doc-1", None).unwrap();
    let stored = attachment::resolve_path(doc_dir, &att).unwrap();
    assert!(stored.exists(), "managed file copied into the asset root");
    let id = att.id.clone();

    let res = add_attachment_core(&s, &mut tree, &mut u, "1", "doc-1", doc_dir, att).unwrap();
    let dto = res.attachment.unwrap();
    assert_eq!(dto.kind, "managed");
    assert!(dto.exists);
    assert_eq!(dto.status, "ok");
    assert!(dto.hash.is_some());

    remove_attachment_core(&s, &mut tree, &mut u, "1", "doc-1", doc_dir, &id).unwrap();
    let orphans = attachment::read_orphans(doc_dir, "doc-1");
    assert_eq!(orphans.len(), 1, "managed removal records an orphan candidate");
    assert!(stored.exists(), "file is kept until gc_orphans runs");
}

#[test]
fn remove_attachment_missing_errors() {
    let s = session();
    let mut tree = sample_tree();
    let mut u = mgr();
    assert!(remove_attachment_core(&s, &mut tree, &mut u, "1", "doc-1", Path::new("."), "nope").is_err());
}

// ── global_search mapping ────────────────────────────────────────────────────

#[test]
fn page_to_response_maps_hits_and_truncation() {
    use moderntodolist_lib::domain::search::{
        DocumentContext, MatchedField, SearchPage, SearchResult,
    };
    use moderntodolist_lib::domain::types::{DocumentId, TaskKey};

    let page = SearchPage {
        results: vec![SearchResult {
            task_key: TaskKey::new(DocumentId::new("doc-1"), TaskId::new("7")),
            title: "Hit".to_string(),
            document_context: DocumentContext {
                document_id: "doc-1".to_string(),
                document_name: "file.xml".to_string(),
            },
            matched_field: MatchedField::Title,
            snippet: "…Hit…".to_string(),
            score: 1.5,
        }],
        total: 3,
        limit: 1,
        offset: 0,
        backend: "fts5".to_string(),
    };
    let dto = page_to_response(&page);
    assert_eq!(dto.hits.len(), 1);
    assert_eq!(dto.hits[0].task_key, "7");
    assert_eq!(dto.hits[0].document_id, "doc-1");
    assert_eq!(dto.hits[0].document_path.as_deref(), Some("file.xml"));
    assert_eq!(dto.hits[0].matched_field, "title");
    assert_eq!(dto.total, 3);
    assert!(dto.truncated, "total (3) exceeds returned hits (1)");
}

// ── Read path: XML -> TaskTree -> DTO ────────────────────────────────────────

#[test]
fn build_tree_from_bytes_parses_hierarchy() {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<TODOLIST NEXTUNIQUEID="3">
  <TASK ID="1" TITLE="Parent">
    <TASK ID="2" TITLE="Child"/>
  </TASK>
</TODOLIST>"#;
    let tree = build_tree_from_bytes(xml.as_bytes()).unwrap();
    assert_eq!(tree.len(), 2);
    assert_eq!(tree.root_ids().len(), 1);
    assert_eq!(tree.root_ids()[0].as_str(), "1");
    assert!(task(&tree, "1").children.contains(&TaskId::new("2")));
    assert_eq!(task(&tree, "2").title, "Child");
}

#[test]
fn build_tree_from_bytes_handles_tdl_root() {
    // The indexer fixtures use a <TDL> root; roots must still be detected.
    let xml = r#"<?xml version="1.0" encoding="utf-8"?><TDL><TASK ID="9" TITLE="Solo"/></TDL>"#;
    let tree = build_tree_from_bytes(xml.as_bytes()).unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree.root_ids().len(), 1);
}

#[test]
fn list_attachments_synthesizes_from_filerefpath() {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<TODOLIST>
  <TASK ID="1" TITLE="T">
    <FILEREFPATH>https://example.com/a</FILEREFPATH>
    <FILEREFPATH>.\files\b.pdf</FILEREFPATH>
  </TASK>
</TODOLIST>"#;
    let tree = build_tree_from_bytes(xml.as_bytes()).unwrap();
    let atts = list_attachments_core(&tree, "1", "doc-1", Path::new(".")).unwrap();
    assert_eq!(atts.len(), 2);
    assert!(atts.iter().any(|a| a.kind == "url" && a.exists));
    assert!(atts.iter().any(|a| a.kind == "linked"));
}

#[test]
fn read_path_resolves_document_then_lists_relations() {
    // Full read plumbing: workspace DB (documents row) -> file path -> parse ->
    // DTOs. Exercises resolve_document_path + load_document_tree + the cores.
    let data = tempfile::TempDir::new().unwrap();
    let db = DatabaseManager::open(data.path());
    assert!(db.is_available());

    let xml_path = data.path().join("doc.xml");
    std::fs::write(
        &xml_path,
        r#"<?xml version="1.0" encoding="utf-8"?>
<TODOLIST NEXTUNIQUEID="3">
  <TASK ID="1" TITLE="Task One" ALLOCATEDTO="Alice; Bob" PRIORITY="7">
    <CATEGORY>work</CATEGORY>
    <DEPENDENCY><TASKID>2</TASKID><DEPENDENCYTYPE>0</DEPENDENCYTYPE></DEPENDENCY>
  </TASK>
  <TASK ID="2" TITLE="Task Two"/>
</TODOLIST>"#,
    )
    .unwrap();

    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES ('ws', 'W', ?1)",
            [data.path().to_string_lossy().to_string()],
        )
        .map_err(DatabaseError::Sqlite)?;
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc-1', 'ws', ?1, 'managed')",
            [xml_path.to_string_lossy().to_string()],
        )
        .map_err(DatabaseError::Sqlite)?;
        Ok(())
    })
    .unwrap();

    let tree = load_document_tree(&db, "doc-1").unwrap();
    assert_eq!(tree.len(), 2);

    let parts = get_participants_core(&tree, "1", "doc-1").unwrap();
    assert!(parts.iter().any(|p| p.display_name == "Alice" && p.role == "allocated_to"));
    assert!(parts.iter().any(|p| p.display_name == "Bob"));

    let graph = get_dependencies_core(&tree, "1", "doc-1").unwrap();
    assert_eq!(graph.outgoing.len(), 1);
    assert_eq!(graph.outgoing[0].depends_on_key, "2");
    assert_eq!(graph.outgoing[0].ref_kind, "local");
    // Task 2 has task 1 depending on it -> one incoming edge.
    let g2 = get_dependencies_core(&tree, "2", "doc-1").unwrap();
    assert_eq!(g2.incoming.len(), 1);

    // Unknown document id degrades to an error (frontend is fail-soft).
    assert!(load_document_tree(&db, "nope").is_err());
}
