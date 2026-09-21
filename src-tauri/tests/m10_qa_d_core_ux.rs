//! M10 RC Regression Matrix D — Core Task UX and Canonical Identity
//! (INH-1127, spec section 5.5).
//!
//! 10 cases (D01..D10): create/read/update/delete tasks, nested hierarchy,
//! reorder, filter, multi-select, keyboard command dispatch, undo/redo and
//! autosave — all driven through the real domain engine
//! (`domain::command`, `domain::session`, `domain::id_allocator`,
//! `domain::task`, `domain::types`, `domain::mappers`,
//! `domain::persistence`) and the real index pipeline for filtering.
//!
//! Canonical identity invariant asserted throughout: a task's `TaskId`
//! (and therefore its `TaskKey`) never changes across create, update,
//! reorder, reparent, delete+undo, or save+reload cycles.
//!
//! Test document: `tests/fixtures/xml/rc/index-source.xml`
//! ```text
//! 1 "Epic: Portable release"  40%  p8  tags[release,p0]  Alice;Bob  by PM
//! ├─ 2 "Atomic save hardening" 100% p9 tag[data-safety]  Alice  dep->3
//! └─ 3 "Index rebuild"           0% p6 tag[release]      Charlie
//! 4 "Watcher debounce"          50% p4
//! ```

use moderntodolist_lib::domain::command::{
    AddTaskCommand, DeleteTaskCommand, FieldUpdateCommand, FieldValue, TaskField, UndoRedoManager,
};
use moderntodolist_lib::domain::fingerprint::FileFingerprint;
use moderntodolist_lib::domain::id_allocator::TaskIdAllocator;
use moderntodolist_lib::domain::mappers::{read_document_metadata, read_task, write_task};
use moderntodolist_lib::domain::persistence::{atomic_save, AutosaveCoordinator, SaveConfig};
use moderntodolist_lib::domain::session::{DocumentSession, SaveState};
use moderntodolist_lib::domain::task::{Task, TaskStatus, TaskTree};
use moderntodolist_lib::domain::types::{DocumentId, TaskId, TaskKey};
use moderntodolist_lib::domain::{parse_xml, serialize_xml, XmlElement, XmlNode};
use moderntodolist_lib::infrastructure::indexer::index_document;
use moderntodolist_lib::infrastructure::migration::run_migrations;
use rusqlite::Connection;

use std::fs;
use std::path::{Path, PathBuf};

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("xml")
}

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mtdl_rc_d_{}_{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The canonical RC test document (copied into a sandbox whenever mutated).
fn index_source_bytes() -> Vec<u8> {
    fs::read(fixtures_root().join("rc").join("index-source.xml")).unwrap()
}

/// Builds the TaskTree from a parsed document exactly the way the session
/// layer does (recursive TASK extraction via the public `read_task` mapper).
fn extract_tree(root: &XmlElement) -> TaskTree {
    fn rec(el: &XmlElement, tree: &mut TaskTree, is_root: bool) {
        for child in el.child_elements() {
            if child.tag == "TASK" {
                let task = read_task(child);
                let id = task.id.clone();
                tree.add_task(task);
                if is_root {
                    tree.add_root_id(id.clone());
                }
                rec(child, tree, false);
            } else {
                rec(child, tree, false);
            }
        }
    }
    let mut tree = TaskTree::new();
    rec(root, &mut tree, true);
    tree
}

/// Load the canonical fixture into (tree, doc).
fn load_fixture() -> (TaskTree, moderntodolist_lib::domain::XmlDocument) {
    let doc = parse_xml(&index_source_bytes()).unwrap();
    let tree = extract_tree(&doc.root);
    (tree, doc)
}

/// Serialize a TaskTree back to a full XmlDocument, preserving the original
/// root attributes and rebuilding every TASK through the public `write_task`
/// mapper (nested children written recursively).
fn serialize_tree(tree: &TaskTree, original_root: &XmlElement, next_unique_id: u64) -> Vec<u8> {
    fn write_recursive(tree: &TaskTree, id: &TaskId) -> XmlElement {
        let task = tree.get(id).expect("child id must resolve");
        let mut el = XmlElement::new("TASK");
        write_task(task, &mut el);
        for cid in &task.children {
            let child_el = write_recursive(tree, cid);
            el.children.push(XmlNode::Element(child_el));
        }
        el
    }
    let mut root = XmlElement::new(original_root.tag.clone());
    for attr in &original_root.attrs {
        root.set_attr(attr.name.clone(), attr.value.clone());
    }
    root.set_attr("NEXTUNIQUEID", next_unique_id.to_string());
    for rid in tree.root_ids() {
        root.children.push(XmlNode::Element(write_recursive(tree, rid)));
    }
    let doc = moderntodolist_lib::domain::XmlDocument::new(
        moderntodolist_lib::domain::encoding::XmlEncodingMeta::default_utf8(),
        root,
    );
    serialize_xml(&doc)
}

fn new_task(id: TaskId, title: &str) -> Task {
    let mut t = Task::new(id);
    t.title = title.to_string();
    t
}

fn titles<'a>(tasks: impl Iterator<Item = &'a Task>) -> Vec<String> {
    tasks.map(|t| t.title.clone()).collect()
}

// ─── D01: create + read with canonical ID allocation ─────────────────────────

/// QA-M10-D01: creating a task through the real ID allocator + AddTaskCommand
/// yields a collision-free canonical TaskId (from NEXTUNIQUEID), readable back
/// from the tree, with a stable TaskKey identity.
#[test]
fn qa_m10_d01_create_and_read_task_canonical_id() {
    let (mut tree, doc) = load_fixture();
    let dm = read_document_metadata(&doc.root, doc.meta.clone());
    assert_eq!(dm.next_unique_id, 5, "fixture NEXTUNIQUEID");

    let mut alloc = TaskIdAllocator::new(&tree, dm.next_unique_id);
    for existing in ["1", "2", "3", "4"] {
        assert!(alloc.exists(&TaskId::new(existing)), "allocator knows fixture ids");
    }

    // Create through the real command pipeline.
    let new_id = alloc.allocate();
    assert_eq!(new_id.as_str(), "5", "canonical id allocated from NEXTUNIQUEID");
    let mut mgr = UndoRedoManager::new();
    mgr.execute(
        Box::new(AddTaskCommand {
            task: new_task(new_id.clone(), "Write RC matrix"),
            parent_id: Some(TaskId::new("1")),
            position: 2,
            executed: false,
        }),
        &mut tree,
    );

    // Read back.
    let created = tree.get(&new_id).expect("created task readable by canonical id");
    assert_eq!(created.title, "Write RC matrix");
    assert_eq!(tree.len(), 5);
    let children: Vec<&str> = tree.get(&TaskId::new("1")).unwrap().children.iter().map(|i| i.as_str()).collect();
    assert_eq!(children, ["2", "3", "5"], "inserted at requested position");

    // Canonical identity: TaskKey built from allocator id == key built from the
    // id stored in the tree, and the allocator advances NEXTUNIQUEID.
    let doc_id = DocumentId::new("index-source.xml");
    let key_alloc = TaskKey::new(doc_id.clone(), new_id.clone());
    let key_tree = TaskKey::new(doc_id.clone(), tree.get(&new_id).unwrap().id.clone());
    assert_eq!(key_alloc, key_tree);
    assert_eq!(key_alloc.to_string(), "index-source.xml:5");
    assert_eq!(alloc.next_unique_id(), 6);
    // Allocating again never collides with an existing id.
    let id6 = alloc.allocate();
    assert_eq!(id6.as_str(), "6");
    assert!(!tree.contains(&id6));
}

// ─── D02: update fields + read back ──────────────────────────────────────────

/// QA-M10-D02: updating fields through FieldUpdateCommand changes the derived
/// status, keeps the TaskId stable, and undo restores the exact old value.
#[test]
fn qa_m10_d02_update_task_fields() {
    let (mut tree, _doc) = load_fixture();
    let mut mgr = UndoRedoManager::new();
    let session = DocumentSession::new(None);
    let id2 = TaskId::new("2");

    // Title update.
    mgr.execute(
        Box::new(FieldUpdateCommand {
            task_id: id2.clone(),
            field: TaskField::Title,
            new_value: FieldValue::Text("Atomic save hardening (v2)".into()),
            old_value: None,
            desc: "Rename task".into(),
        }),
        &mut tree,
    );
    session.record_mutation();
    assert_eq!(tree.get(&id2).unwrap().title, "Atomic save hardening (v2)");
    assert_eq!(tree.get(&id2).unwrap().id, id2, "TaskId is immutable across updates");

    // Percent done update flips the derived status.
    assert_eq!(tree.get(&id2).unwrap().status(), TaskStatus::Done);
    mgr.execute(
        Box::new(FieldUpdateCommand {
            task_id: id2.clone(),
            field: TaskField::PercentDone,
            new_value: FieldValue::Integer(50),
            old_value: None,
            desc: "Set 50%".into(),
        }),
        &mut tree,
    );
    session.record_mutation();
    assert_eq!(tree.get(&id2).unwrap().status(), TaskStatus::InProgress);

    // Undo restores both (LIFO).
    assert!(mgr.undo(&mut tree).is_some());
    assert_eq!(tree.get(&id2).unwrap().percent_done, 100);
    assert_eq!(tree.get(&id2).unwrap().status(), TaskStatus::Done);
    assert!(mgr.undo(&mut tree).is_some());
    assert_eq!(tree.get(&id2).unwrap().title, "Atomic save hardening", "exact old value restored");

    // Priority clamping via the real domain type.
    mgr.execute(
        Box::new(FieldUpdateCommand {
            task_id: id2.clone(),
            field: TaskField::Priority,
            new_value: FieldValue::Integer(99),
            old_value: None,
            desc: "Priority".into(),
        }),
        &mut tree,
    );
    assert_eq!(tree.get(&id2).unwrap().priority.value(), 10, "priority clamped to 0-10");
    assert!(mgr.undo(&mut tree).is_some());
    assert_eq!(tree.get(&id2).unwrap().priority.value(), 9);
    assert!(session.is_dirty(), "mutations recorded on the session");
}

// ─── D03: delete subtree + undo restores identity ────────────────────────────

/// QA-M10-D03: deleting a task removes the whole subtree; undo restores every
/// task with its ORIGINAL TaskId, content and parent-child links
/// (duplicate-not-loss at the UX level: nothing is lost by a delete+undo
/// cycle).
///
/// Also documents finding F4 (minor): undo of a root-level delete re-appends
/// the task at the END of the root list instead of its original position
/// (no data loss; ordering only).
#[test]
fn qa_m10_d03_delete_subtree_undo_restores_identity() {
    let (mut tree, _doc) = load_fixture();
    let mut mgr = UndoRedoManager::new();

    let snapshot_ids: Vec<String> = tree.depth_first_ids().iter().map(|i| i.as_str().to_string()).collect();
    assert_eq!(snapshot_ids, ["1", "2", "3", "4"]);

    mgr.execute(
        Box::new(DeleteTaskCommand {
            task_id: TaskId::new("1"),
            saved_tasks: Vec::new(),
            saved_root_ids: Vec::new(),
            parent_id: None,
            executed: false,
        }),
        &mut tree,
    );
    // Whole subtree gone, sibling root untouched.
    assert_eq!(tree.len(), 1);
    assert!(!tree.contains(&TaskId::new("1")));
    assert!(!tree.contains(&TaskId::new("2")));
    assert!(!tree.contains(&TaskId::new("3")));
    assert!(tree.contains(&TaskId::new("4")));
    assert_eq!(tree.root_ids().len(), 1);

    // Undo restores the subtree with identical canonical ids and links.
    mgr.undo(&mut tree);
    assert_eq!(tree.len(), 4);
    let mut restored_ids: Vec<String> = tree.depth_first_ids().iter().map(|i| i.as_str().to_string()).collect();
    restored_ids.sort();
    assert_eq!(restored_ids, ["1", "2", "3", "4"], "identity preserved through delete+undo");
    let t1 = tree.get(&TaskId::new("1")).unwrap();
    assert_eq!(t1.title, "Epic: Portable release");
    let child_ids: Vec<&str> = t1.children.iter().map(|i| i.as_str()).collect();
    assert_eq!(child_ids, ["2", "3"], "child links restored in order");
    assert_eq!(tree.get(&TaskId::new("2")).unwrap().dependencies[0].task_id, "3",
        "relation data restored");

    // Finding F4 (minor bug): root ORDER after undo is [4, 1] — the restored
    // root task is appended instead of re-inserted at position 0, so the
    // depth-first traversal starts with task 4. No data loss; ordering only.
    let roots: Vec<&str> = tree.root_ids().iter().map(|i| i.as_str()).collect();
    assert_eq!(roots, ["4", "1"], "documents the current (buggy) ordering after undo");
    assert_eq!(
        tree.depth_first_ids().iter().map(|i| i.as_str().to_string()).collect::<Vec<_>>(),
        ["4", "1", "2", "3"],
        "F4: traversal order churns after delete+undo of a root task"
    );

    // Redo deletes again.
    assert!(mgr.redo(&mut tree).is_some());
    assert_eq!(tree.len(), 1);
}

// ─── D04: nested hierarchy + cycle guard ─────────────────────────────────────

/// QA-M10-D04: nested hierarchy reads (children_of, depth-first order),
/// reparenting into the hierarchy, and the cycle guard that refuses to
/// reparent a task under itself or its own descendant.
#[test]
fn qa_m10_d04_nested_hierarchy_and_cycle_guard() {
    let (mut tree, _doc) = load_fixture();

    // Hierarchy reads.
    let kids = tree.children_of(&TaskId::new("1"));
    assert_eq!(titles(kids.into_iter()), ["Atomic save hardening", "Index rebuild"]);
    assert_eq!(
        tree.depth_first_ids().iter().map(|i| i.as_str().to_string()).collect::<Vec<_>>(),
        ["1", "2", "3", "4"]
    );
    assert!(tree.get(&TaskId::new("1")).unwrap().has_children());
    assert!(!tree.get(&TaskId::new("4")).unwrap().has_children());

    // Reparent root task 4 under task 3 (creates real nesting).
    assert!(tree.reparent_task(&TaskId::new("4"), Some(&TaskId::new("3")), 0));
    let roots: Vec<&str> = tree.root_ids().iter().map(|i| i.as_str()).collect();
    assert_eq!(roots, ["1"]);
    assert_eq!(
        tree.children_of(&TaskId::new("3")).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["4"]
    );
    assert_eq!(
        tree.depth_first_ids().iter().map(|i| i.as_str().to_string()).collect::<Vec<_>>(),
        ["1", "2", "3", "4"],
        "depth-first order stable, ids unchanged"
    );

    // Cycle guards: self-parent and descendant-parent are refused.
    assert!(!tree.reparent_task(&TaskId::new("1"), Some(&TaskId::new("1")), 0), "self reparent refused");
    assert!(!tree.reparent_task(&TaskId::new("1"), Some(&TaskId::new("4")), 0), "reparent under own descendant refused");
    assert!(!tree.reparent_task(&TaskId::new("3"), Some(&TaskId::new("4")), 0), "reparent under own child refused");
    // Structure intact after refused operations.
    assert_eq!(tree.root_ids().len(), 1);
    assert!(tree.contains(&TaskId::new("4")));
    assert_eq!(tree.len(), 4);
}

// ─── D05: reorder siblings ───────────────────────────────────────────────────

/// QA-M10-D05: reordering updates sibling order and POS values while every
/// TaskId stays stable (canonical identity across reorder).
#[test]
fn qa_m10_d05_reorder_siblings_and_pos_update() {
    let (mut tree, _doc) = load_fixture();

    // Reorder children of task 1: [2,3] -> [3,2].
    assert!(tree.reorder_task(&TaskId::new("3"), Some(&TaskId::new("1")), 0));
    let kids: Vec<&str> = tree.get(&TaskId::new("1")).unwrap().children.iter().map(|i| i.as_str()).collect();
    assert_eq!(kids, ["3", "2"]);
    assert_eq!(tree.get(&TaskId::new("3")).unwrap().pos, 0, "POS renumbered");
    assert_eq!(tree.get(&TaskId::new("2")).unwrap().pos, 1, "POS renumbered");

    // Reorder roots: [1,4] -> [4,1].
    assert!(tree.reorder_task(&TaskId::new("4"), None, 0));
    let roots: Vec<&str> = tree.root_ids().iter().map(|i| i.as_str()).collect();
    assert_eq!(roots, ["4", "1"]);
    assert_eq!(tree.get(&TaskId::new("4")).unwrap().pos, 0);
    assert_eq!(tree.get(&TaskId::new("1")).unwrap().pos, 1);

    // Out-of-range index clamps instead of panicking.
    assert!(tree.reorder_task(&TaskId::new("1"), None, 99));
    let roots: Vec<&str> = tree.root_ids().iter().map(|i| i.as_str()).collect();
    assert_eq!(roots, ["4", "1"]);

    // Unknown id / wrong parent are rejected, not panicked.
    assert!(!tree.reorder_task(&TaskId::new("999"), None, 0));
    assert!(!tree.reorder_task(&TaskId::new("4"), Some(&TaskId::new("1")), 0), "4 is not a child of 1");

    // Canonical identity: the same 4 ids, unchanged.
    let mut ids: Vec<String> = tree.iter().map(|t| t.id.as_str().to_string()).collect();
    ids.sort();
    assert_eq!(ids, ["1", "2", "3", "4"]);
}

// ─── D06: filter through the real index pipeline ─────────────────────────────

/// QA-M10-D06: task filtering (the backend of the UI filter bar) through the
/// real M4/M5 pipeline: `index_document` into SQLite, then canonical filter
/// queries by status, percent, priority, tag, participant and title.
#[test]
fn qa_m10_d06_filter_tasks_via_index() {
    let dir = sandbox("d06");
    let xml = dir.join("index-source.xml");
    fs::write(&xml, index_source_bytes()).unwrap();

    let conn = Connection::open(dir.join("index.db")).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();
    conn.execute("INSERT INTO workspaces (id, name, root_path) VALUES ('ws-d06','D06','.')", []).unwrap();
    conn.execute(
        "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc-d06','ws-d06','index-source.xml','managed')",
        [],
    ).unwrap();

    let n = index_document(&conn, "doc-d06", &xml).unwrap();
    assert_eq!(n, 4);

    let keys = |sql: &str, params: &[&str]| -> Vec<String> {
        let mut stmt = conn.prepare(sql).unwrap();
        let rows: Vec<String> = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        let mut rows = rows;
        rows.sort();
        rows
    };

    const BY_DOC: &str = " WHERE document_id='doc-d06'";
    // Status filter.
    assert_eq!(keys(&format!("SELECT task_key FROM task_index{BY_DOC} AND status='Done'"), &[]), ["2"]);
    assert_eq!(keys(&format!("SELECT task_key FROM task_index{BY_DOC} AND status='NotStarted'"), &[]), ["3"]);
    // Percent-done filter.
    assert_eq!(keys(&format!("SELECT task_key FROM task_index{BY_DOC} AND percent_done >= 50"), &[]), ["2", "4"]);
    // Priority filter.
    assert_eq!(keys(&format!("SELECT task_key FROM task_index{BY_DOC} AND priority >= 8"), &[]), ["1", "2"]);
    // Tag filter.
    assert_eq!(keys("SELECT task_key FROM task_tags WHERE tag='release'", &[]), ["1", "3"]);
    assert_eq!(keys("SELECT task_key FROM task_tags WHERE tag='data-safety'", &[]), ["2"]);
    // Participant filter.
    assert_eq!(keys("SELECT task_key FROM task_participants WHERE participant='Alice' AND role='allocated_to'", &[]), ["1", "2"]);
    assert_eq!(keys("SELECT task_key FROM task_participants WHERE participant='Charlie'", &[]), ["3"]);
    // Title substring filter (canonical search fallback).
    assert_eq!(keys(&format!("SELECT task_key FROM task_index{BY_DOC} AND title LIKE '%save%'"), &[]), ["2"]);
    // Dependency (blocked-by) filter.
    assert_eq!(keys("SELECT task_key FROM task_dependencies WHERE depends_on_key='3'", &[]), ["2"]);

    fs::remove_dir_all(&dir).ok();
}

// ─── D07: multi-select bulk update ───────────────────────────────────────────

/// QA-M10-D07: multi-select bulk edit — one command per selected task through
/// the shared UndoRedoManager; undo unwinds each selected task individually
/// and restores exact previous values; ids stay canonical throughout.
#[test]
fn qa_m10_d07_multi_select_bulk_update_undo() {
    let (mut tree, _doc) = load_fixture();
    let mut mgr = UndoRedoManager::new();

    // "Select" tasks 2, 3 and 4 and bulk-set percent done to 0.
    let selection = [TaskId::new("2"), TaskId::new("3"), TaskId::new("4")];
    let before: Vec<u8> = selection.iter().map(|id| tree.get(id).unwrap().percent_done).collect();
    assert_eq!(before, [100, 0, 50]);

    for id in &selection {
        mgr.execute(
            Box::new(FieldUpdateCommand {
                task_id: id.clone(),
                field: TaskField::PercentDone,
                new_value: FieldValue::Integer(0),
                old_value: None,
                desc: format!("Reset {id}"),
            }),
            &mut tree,
        );
    }
    assert_eq!(mgr.undo_count(), 3, "one undo entry per selected task");
    for id in &selection {
        assert_eq!(tree.get(id).unwrap().percent_done, 0);
        assert_eq!(tree.get(id).unwrap().status(), TaskStatus::NotStarted);
    }
    // Unselected task untouched.
    assert_eq!(tree.get(&TaskId::new("1")).unwrap().percent_done, 40);

    // Undo all three: exact previous values restored.
    for _ in 0..3 {
        assert!(mgr.undo(&mut tree).is_some());
    }
    let after: Vec<u8> = selection.iter().map(|id| tree.get(id).unwrap().percent_done).collect();
    assert_eq!(after, before, "bulk undo restores each original value");
    assert_eq!(tree.get(&TaskId::new("2")).unwrap().status(), TaskStatus::Done);

    // Redo all three.
    for _ in 0..3 {
        assert!(mgr.redo(&mut tree).is_some());
    }
    for id in &selection {
        assert_eq!(tree.get(id).unwrap().percent_done, 0);
    }
    // Canonical ids unchanged.
    let mut ids: Vec<String> = tree.iter().map(|t| t.id.as_str().to_string()).collect();
    ids.sort();
    assert_eq!(ids, ["1", "2", "3", "4"]);
}

// ─── D08: keyboard command dispatch (undo/redo stack discipline) ─────────────

/// QA-M10-D08: the command-dispatch semantics behind the keyboard shortcuts
/// (Ctrl+Z / Ctrl+Y): strict LIFO undo, redo replay, redo-stack invalidation
/// on a new mutation, and safe no-ops on empty stacks.
#[test]
fn qa_m10_d08_keyboard_command_dispatch_undo_redo() {
    let (mut tree, _doc) = load_fixture();
    let mut mgr = UndoRedoManager::new();

    // Empty-stack dispatch is a safe no-op.
    assert!(mgr.undo(&mut tree).is_none());
    assert!(mgr.redo(&mut tree).is_none());
    assert!(!mgr.can_undo() && !mgr.can_redo());

    // Dispatch three title edits A, B, C on task 3 (as three keystroke commands).
    for v in ["A", "B", "C"] {
        mgr.execute(
            Box::new(FieldUpdateCommand {
                task_id: TaskId::new("3"),
                field: TaskField::Title,
                new_value: FieldValue::Text(v.into()),
                old_value: None,
                desc: format!("title={v}"),
            }),
            &mut tree,
        );
    }
    assert_eq!(tree.get(&TaskId::new("3")).unwrap().title, "C");
    assert_eq!(mgr.undo_count(), 3);

    // Ctrl+Z twice: C -> B -> A (strict LIFO).
    assert_eq!(mgr.undo(&mut tree).as_deref(), Some("title=C"));
    assert_eq!(tree.get(&TaskId::new("3")).unwrap().title, "B");
    assert_eq!(mgr.undo(&mut tree).as_deref(), Some("title=B"));
    assert_eq!(tree.get(&TaskId::new("3")).unwrap().title, "A");
    assert_eq!(mgr.undo_count(), 1);
    assert_eq!(mgr.redo_count(), 2);

    // Ctrl+Y once: replays B.
    assert_eq!(mgr.redo(&mut tree).as_deref(), Some("title=B"));
    assert_eq!(tree.get(&TaskId::new("3")).unwrap().title, "B");

    // A NEW mutation invalidates the redo branch (standard editor semantics).
    mgr.execute(
        Box::new(FieldUpdateCommand {
            task_id: TaskId::new("3"),
            field: TaskField::Title,
            new_value: FieldValue::Text("D".into()),
            old_value: None,
            desc: "title=D".into(),
        }),
        &mut tree,
    );
    assert!(!mgr.can_redo(), "new command clears the redo stack");
    assert_eq!(tree.get(&TaskId::new("3")).unwrap().title, "D");

    // Full unwind reaches the pristine fixture value.
    while mgr.can_undo() {
        assert!(mgr.undo(&mut tree).is_some());
    }
    assert_eq!(tree.get(&TaskId::new("3")).unwrap().title, "Index rebuild");
    assert!(!mgr.can_undo());
}

// ─── D09: undo/redo across command types with full-tree identity ─────────────

/// QA-M10-D09: mixed operations (field update, create, delete) unwind to a
/// tree that is EXACTLY equal to the original snapshot (whole-tree identity),
/// and replay returns to the modified state — with all TaskIds canonical.
#[test]
fn qa_m10_d09_undo_redo_full_tree_identity() {
    let (mut tree, doc) = load_fixture();
    let original = tree.clone();
    let mut mgr = UndoRedoManager::new();

    // 1) Field update on task 4.
    mgr.execute(
        Box::new(FieldUpdateCommand {
            task_id: TaskId::new("4"),
            field: TaskField::Title,
            new_value: FieldValue::Text("Watcher debounce v2".into()),
            old_value: None,
            desc: "rename 4".into(),
        }),
        &mut tree,
    );
    // 2) Create task 5 under task 3.
    let dm = read_document_metadata(&doc.root, doc.meta.clone());
    let mut alloc = TaskIdAllocator::new(&original, dm.next_unique_id);
    let id5 = alloc.allocate();
    mgr.execute(
        Box::new(AddTaskCommand {
            task: new_task(id5.clone(), "Fresh task"),
            parent_id: Some(TaskId::new("3")),
            position: 0,
            executed: false,
        }),
        &mut tree,
    );
    // 3) Delete root task 4 (last root -> delete+undo is order-stable).
    mgr.execute(
        Box::new(DeleteTaskCommand {
            task_id: TaskId::new("4"),
            saved_tasks: Vec::new(),
            saved_root_ids: Vec::new(),
            parent_id: None,
            executed: false,
        }),
        &mut tree,
    );
    let modified = tree.clone();
    assert_eq!(modified.len(), 4, "1,2,3,5 remain after deleting 4");
    assert!(!modified.contains(&TaskId::new("4")));

    // Unwind everything (LIFO) -> exact original tree.
    for _ in 0..3 {
        assert!(mgr.undo(&mut tree).is_some());
    }
    assert_eq!(tree, original, "full unwind must restore the exact original tree");

    // Replay everything -> exact modified tree.
    for _ in 0..3 {
        assert!(mgr.redo(&mut tree).is_some());
    }
    assert_eq!(tree, modified, "replay must restore the exact modified tree");

    // Canonical identity: the created task kept its allocated id through the
    // whole undo/redo cycle.
    assert_eq!(tree.get(&id5).unwrap().id, id5);
    assert_eq!(id5.as_str(), "5");
}

// ─── D10: autosave lifecycle + persisted canonical identity ─────────────────

/// QA-M10-D10: the autosave loop — mutation makes the session dirty and arms
/// the AutosaveCoordinator; the real `atomic_save` persists the serialized
/// tree; after save the session is clean and the coordinator disarmed;
/// reloading the saved file yields the SAME canonical TaskIds, hierarchy and
/// edited values (identity survives the persistence boundary). Disabling
/// autosave stops the trigger.
#[test]
fn qa_m10_d10_autosave_and_persisted_identity() {
    let dir = sandbox("d10");
    let target = dir.join("working.xml");
    fs::write(&target, index_source_bytes()).unwrap();

    let (mut tree, doc) = load_fixture();
    let dm = read_document_metadata(&doc.root, doc.meta.clone());
    let mut alloc = TaskIdAllocator::new(&tree, dm.next_unique_id);

    let session = DocumentSession::new(Some(FileFingerprint::from_file(&target).unwrap()));
    let mut autosave = AutosaveCoordinator::new(30);
    assert!(!autosave.should_save(), "nothing pending initially");
    assert_eq!(autosave.interval_ms(), 30);

    // A user edit arms the autosave and dirties the session.
    let mut mgr = UndoRedoManager::new();
    mgr.execute(
        Box::new(FieldUpdateCommand {
            task_id: TaskId::new("3"),
            field: TaskField::Title,
            new_value: FieldValue::Text("Index rebuild — edited 编辑".into()),
            old_value: None,
            desc: "edit title".into(),
        }),
        &mut tree,
    );
    let id5 = alloc.allocate();
    mgr.execute(
        Box::new(AddTaskCommand {
            task: new_task(id5.clone(), "Autosaved new task"),
            parent_id: Some(TaskId::new("1")),
            position: 2,
            executed: false,
        }),
        &mut tree,
    );
    session.record_mutation();
    session.record_mutation();
    autosave.on_mutation();
    assert!(session.is_dirty());
    assert!(autosave.should_save(), "mutation arms the autosave");

    // Real atomic save of the serialized tree.
    assert!(session.begin_save());
    let bytes = serialize_tree(&tree, &doc.root, alloc.next_unique_id());
    let config = SaveConfig { target_path: target.clone(), validate_temp: true };
    let result = atomic_save(&config, &bytes, |b| parse_xml(b).is_ok());
    assert_eq!(result.state, SaveState::Completed, "save must succeed: {:?}", result.error);
    session.record_save();
    session.set_fingerprint(result.fingerprint.clone().unwrap());
    session.end_save();
    autosave.on_save_complete();

    assert!(!session.is_dirty(), "clean after autosave");
    assert!(!autosave.should_save(), "autosave disarmed after completion");
    assert!(FileFingerprint::from_file(&target).unwrap().matches_bytes(&bytes));

    // Reload the persisted file: canonical identity across the save boundary.
    let reloaded_doc = parse_xml(&fs::read(&target).unwrap()).unwrap();
    let reloaded = extract_tree(&reloaded_doc.root);
    assert_eq!(reloaded.len(), 5);
    let mut ids: Vec<String> = reloaded.iter().map(|t| t.id.as_str().to_string()).collect();
    ids.sort();
    assert_eq!(ids, ["1", "2", "3", "4", "5"], "canonical TaskIds stable through save/reload");
    let roots: Vec<&str> = reloaded.root_ids().iter().map(|i| i.as_str()).collect();
    assert_eq!(roots, ["1", "4"], "root hierarchy preserved");
    assert_eq!(reloaded.get(&TaskId::new("3")).unwrap().title, "Index rebuild — edited 编辑",
        "edited value persisted (UTF-8 intact)");
    let kids: Vec<&str> = reloaded.get(&TaskId::new("1")).unwrap().children.iter().map(|i| i.as_str()).collect();
    assert_eq!(kids, ["2", "3", "5"], "created task persisted under its parent");
    // Relations persisted through write_task.
    assert_eq!(reloaded.get(&TaskId::new("2")).unwrap().dependencies[0].task_id, "3");
    let cats: Vec<&str> = reloaded.get(&TaskId::new("1")).unwrap().categories.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cats, ["release", "p0"]);
    let alloc_to = &reloaded.get(&TaskId::new("1")).unwrap().allocated_to;
    assert_eq!(alloc_to, &vec!["Alice".to_string(), "Bob".to_string()]);
    // NEXTUNIQUEID advanced in the persisted document (validator-clean).
    let reloaded_dm = read_document_metadata(&reloaded_doc.root, reloaded_doc.meta.clone());
    assert_eq!(reloaded_dm.next_unique_id, alloc.next_unique_id());
    assert!(moderntodolist_lib::domain::validator::validate_document(&reloaded_doc).is_empty());

    // Disabling autosave stops the trigger even on mutation.
    autosave.set_enabled(false);
    autosave.on_mutation();
    assert!(!autosave.should_save(), "disabled autosave never fires");

    assert!(Path::new(&target).exists());
    fs::remove_dir_all(&dir).ok();
}
