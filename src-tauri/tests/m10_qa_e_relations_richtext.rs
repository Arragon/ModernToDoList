//! M10 RC Regression Matrix E — Relations, Attachments and Rich Content
//! (QA-M10 E / INH-1128, spec section 5.5).
//!
//! 12 cases (E01..E12). These are RC-level matrices: every case exercises
//! M6 (participants / dependencies / progress links / attachments), M7
//! (comments-type preservation, sanitizer, managed image assets) and M9
//! (search seams) TOGETHER against the real public library API — the same
//! `*_core` bridge functions the IPC commands run, the real mappers, the
//! real index populators and the real search pipeline.
//!
//! Cross-cutting invariants asserted throughout:
//! * any relation mutated through the bridge survives the M2 XML save path
//!   losslessly (unknown attrs, unknown children and comments preserved);
//! * every derived index (participants, dependencies, links, attachments,
//!   search) is rebuildable from the saved XML alone and agrees with the
//!   domain read;
//! * dangerous URLs / hostile HTML are rejected at every choke point;
//! * one shared UndoRedoManager unwinds mixed relation edits exactly.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use moderntodolist_lib::bridge::build_tree_from_bytes;
use moderntodolist_lib::domain::attachment;
use moderntodolist_lib::domain::command::UndoRedoManager;
use moderntodolist_lib::domain::comments::{CommentsError, CommentsField, CommentsType, EditorMode, TaskComments};
use moderntodolist_lib::domain::dependency;
use moderntodolist_lib::domain::mappers::{read_task, write_task};
use moderntodolist_lib::domain::participant;
use moderntodolist_lib::domain::persistence::{atomic_save, SaveConfig};
use moderntodolist_lib::domain::progress_link;
use moderntodolist_lib::domain::rich_text::{GcOptions, ImageAssetStore, IngestSource, KeepReason, ReferenceSet};
use moderntodolist_lib::domain::sanitizer::{sanitize_for_core, sanitize_for_editor, sanitize_for_preview};
use moderntodolist_lib::domain::search::{self, MatchedField, SearchQuery};
use moderntodolist_lib::domain::session::{DocumentSession, SaveState};
use moderntodolist_lib::domain::task::{CommentType, Task, TaskComment, TaskTree};
use moderntodolist_lib::domain::types::TaskId;
use moderntodolist_lib::domain::xml_tree::{XmlElement, XmlNode};
use moderntodolist_lib::domain::{parse_xml, serialize_xml};
use moderntodolist_lib::infrastructure::indexer::index_document;
use moderntodolist_lib::infrastructure::migration::run_migrations;
use moderntodolist_lib::infrastructure::search_fts::{self, rebuild_search_index, IndexTableExtrasProvider};
use moderntodolist_lib::relations::{
    add_attachment_core, add_dependency_core, add_participant_core, add_progress_link_core,
    get_dependencies_core, get_participants_core, list_attachments_core, list_progress_links_core,
    remove_attachment_core, remove_dependency_core, remove_progress_link_core,
    update_progress_link_core,
};
use moderntodolist_lib::task_edit::update_task_field_core;

// ─── Harness ────────────────────────────────────────────────────────────────

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate root should have a parent")
        .join("tests")
        .join("fixtures")
        .join("xml")
}

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mtdl_rc_e_{}_{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Collects every TASK element in document order (depth-first).
fn collect_task_elems(elem: &XmlElement, out: &mut Vec<XmlElement>) {
    if elem.tag == "TASK" {
        out.push(elem.clone());
    }
    for child in elem.child_elements() {
        collect_task_elems(child, out);
    }
}

fn task_elems(doc: &moderntodolist_lib::domain::XmlDocument) -> Vec<XmlElement> {
    let mut v = Vec::new();
    collect_task_elems(&doc.root, &mut v);
    v
}

fn find_task_elem_mut<'a>(elem: &'a mut XmlElement, id: &str) -> Option<&'a mut XmlElement> {
    if elem.tag == "TASK" && elem.get_attr("ID") == Some(id) {
        return Some(elem);
    }
    for c in &mut elem.children {
        if let XmlNode::Element(child) = c {
            if let Some(found) = find_task_elem_mut(child, id) {
                return Some(found);
            }
        }
    }
    None
}

/// The save path used by the session layer: rewrite every TASK element of a
/// parsed document in place from the mutated tree (public `write_task`
/// mapper), preserving all surrounding nodes, then serialize.
fn commit_tree_to_bytes(original_bytes: &[u8], tree: &TaskTree) -> Vec<u8> {
    let mut doc = parse_xml(original_bytes).expect("original parses");
    let ids: Vec<String> = tree
        .iter()
        .map(|t| t.id.as_str().to_string())
        .collect();
    for id in ids {
        let task = tree.get(&TaskId::new(&id)).unwrap().clone();
        if let Some(el) = find_task_elem_mut(&mut doc.root, &id) {
            write_task(&task, el);
        }
    }
    serialize_xml(&doc)
}

/// parse → read_task/write_task in place → serialize (m6-style domain rewrite).
fn domain_rewrite_bytes(bytes: &[u8]) -> Vec<u8> {
    fn rewrite(elem: &mut XmlElement) {
        if elem.tag == "TASK" {
            let task = read_task(elem);
            write_task(&task, elem);
        }
        for child in &mut elem.children {
            if let XmlNode::Element(c) = child {
                rewrite(c);
            }
        }
    }
    let mut doc = parse_xml(bytes).expect("parse");
    rewrite(&mut doc.root);
    serialize_xml(&doc)
}

/// Real atomic save (M3 pipeline) of bytes to `path`.
fn save(path: &Path, bytes: &[u8]) {
    let result = atomic_save(
        &SaveConfig { target_path: path.to_path_buf(), validate_temp: true },
        bytes,
        |b| parse_xml(b).is_ok(),
    );
    assert_eq!(result.state, SaveState::Completed, "save failed: {:?}", result.error);
}

fn setup_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();
    conn
}

fn register_doc(conn: &Connection, ws: &str, doc_id: &str, file_path: &Path) {
    conn.execute(
        "INSERT INTO workspaces (id, name, root_path) VALUES (?1, ?1, '.')",
        [ws],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES (?1, ?2, ?3, 'managed')",
        rusqlite::params![doc_id, ws, file_path.to_string_lossy().as_ref()],
    )
    .unwrap();
}

fn search(conn: &Connection, q: &str) -> moderntodolist_lib::domain::search::SearchPage {
    search_fts::search(conn, &SearchQuery::new(q.to_string())).unwrap()
}

/// Bridges an M2 `TaskComment` (+ COMMENTSTYPE attr) into the M7 value object.
fn to_m7_comments(task: &Task) -> Option<TaskComments> {
    task.comments.as_ref().map(|c| TaskComments {
        comments_type: CommentsType::from_attr_value(task.comments_type.as_attr_value()),
        content: c.content.clone(),
    })
}

// ═══════════════ E01: bridge mutations persist through the XML save path ═══════════════

/// QA-M10-E01: five different relation mutations driven through the real
/// `*_core` bridge (participant, dependency, progress link, URL attachment,
/// comments field) all persist through write_task → serialize → atomic_save
/// → re-parse, while the fixture's unknown attribute, unknown child element
/// and XML comment survive verbatim.
#[test]
fn qa_m10_e01_bridge_relation_mutations_survive_xml_save_path() {
    let dir = sandbox("e01");
    let original = fs::read(fixtures_root().join("relations").join("participants.xml")).unwrap();
    let mut tree = build_tree_from_bytes(&original).unwrap();
    let s = DocumentSession::new(None);
    let mut u = UndoRedoManager::new();
    let doc_id = "doc-e01";

    add_participant_core(&s, &mut tree, &mut u, "1", "Carol", "allocated_to").unwrap();
    add_dependency_core(&s, &mut tree, &mut u, "1", doc_id, "2", None, 0).unwrap();
    add_progress_link_core(&s, &mut tree, &mut u, "1", doc_id, "CI Run", "https://github.com/o/r/actions/9", None).unwrap();
    let att = attachment::url_attachment("https://example.com/spec-e01", Some("Spec E01")).unwrap();
    add_attachment_core(&s, &mut tree, &mut u, "1", doc_id, &dir, att.clone()).unwrap();
    update_task_field_core(&s, &mut tree, &mut u, "1", "comments", "需要跟进 follow-up").unwrap();
    assert_eq!(u.undo_count(), 5, "one undoable command per bridge mutation");
    assert!(s.is_dirty(), "bridge mutations dirty the session");

    // Save through the real pipeline and reload from disk.
    let path = dir.join("participants.xml");
    let saved = commit_tree_to_bytes(&original, &tree);
    save(&path, &saved);
    let reloaded_bytes = fs::read(&path).unwrap();
    let doc = parse_xml(&reloaded_bytes).unwrap();
    let elems = task_elems(&doc);
    let t1 = read_task(&elems[0]);

    // Every relation survived the boundary.
    assert_eq!(t1.allocated_to, vec!["Zoe", "Alice", "Bob", "Carol"]);
    assert_eq!(t1.participants.len(), 4);
    assert_eq!(t1.dependencies.len(), 1);
    assert_eq!(t1.dependencies[0].task_id, "2");
    assert_eq!(t1.progress_links.len(), 1);
    assert_eq!(t1.progress_links[0].provider, progress_link::LinkProvider::Github);
    assert_eq!(t1.attachments, vec![att]);
    assert_eq!(t1.file_links.len(), 1);
    let comments = t1.comments.as_ref().expect("comments persisted");
    assert_eq!(comments.content, "需要跟进 follow-up");
    assert_eq!(comments.comment_type, CommentType::Plain, "COMMENTSTYPE unchanged");

    // Legacy data survived the same save.
    assert!(t1.unknown_attrs.iter().any(|(k, v)| k == "CUSTOMATTR" && v == "keepme"));
    assert!(t1.unknown_children.iter().any(|c| c.contains(r#"FOO="bar""#)));
    let text = String::from_utf8_lossy(&reloaded_bytes);
    assert!(text.contains("<!-- legacy comment must survive -->"));

    // The untouched second task is byte-identical at the domain level.
    let orig_doc = parse_xml(&original).unwrap();
    let orig_elems = task_elems(&orig_doc);
    assert_eq!(read_task(&elems[1]), read_task(&orig_elems[1]));

    // The read path (fresh parse of the saved file) reports the same relations
    // the bridge wrote — reads and writes agree through the DTO layer.
    let reloaded_tree = build_tree_from_bytes(&reloaded_bytes).unwrap();
    let parts = get_participants_core(&reloaded_tree, "1", doc_id).unwrap();
    assert_eq!(parts.len(), 5, "4 allocated_to + 1 allocated_by");
    assert!(parts.iter().any(|p| p.display_name == "Carol" && p.role == "allocated_to"));
    let graph = get_dependencies_core(&reloaded_tree, "1", doc_id).unwrap();
    assert_eq!(graph.outgoing.len(), 1);
    assert_eq!(graph.outgoing[0].depends_on_key, "2");
    assert_eq!(list_progress_links_core(&reloaded_tree, "1", doc_id).unwrap().len(), 1);
    assert_eq!(list_attachments_core(&reloaded_tree, "1", doc_id, &dir).unwrap().len(), 1);

    fs::remove_dir_all(&dir).ok();
}

// ═══════════════ E02: participant mutation flows into the search index ═══════════════

/// QA-M10-E02: a participant added through the bridge is saved to XML, and
/// the M4 index + M9 search pipeline (rebuilt from the saved file alone)
/// makes the new name searchable with `MatchedField::Participants` — the
/// M6 → M2 → M4 → M9 seam in one pass.
#[test]
fn qa_m10_e02_participant_mutation_reaches_search_index() {
    let dir = sandbox("e02");
    let original = fs::read(fixtures_root().join("relations").join("participants.xml")).unwrap();
    let mut tree = build_tree_from_bytes(&original).unwrap();
    let s = DocumentSession::new(None);
    let mut u = UndoRedoManager::new();

    assert!(!String::from_utf8_lossy(&original).contains("Dana"), "fixture must not contain the probe name");
    add_participant_core(&s, &mut tree, &mut u, "1", "Dana", "allocated_to").unwrap();

    let path = dir.join("participants.xml");
    save(&path, &commit_tree_to_bytes(&original, &tree));

    // The native attribute carries the new participant in source order.
    let doc = parse_xml(&fs::read(&path).unwrap()).unwrap();
    let attr = task_elems(&doc)[0].get_attr("ALLOCATEDTO").unwrap().to_string();
    assert_eq!(attr, "Zoe; Alice; Bob; Dana");

    // Index + search rebuilt purely from the saved XML.
    let conn = setup_db();
    register_doc(&conn, "ws-e02", "doc-e02", &path);
    assert_eq!(index_document(&conn, "doc-e02", &path).unwrap(), 2);
    let n = rebuild_search_index(
        &conn,
        &[("doc-e02".to_string(), path.clone())],
        &IndexTableExtrasProvider,
    )
    .unwrap();
    assert_eq!(n, 2);

    // The index table and the search store agree with the domain read.
    let dana: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_participants WHERE participant='Dana' AND role='allocated_to'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dana, 1);
    let page = search(&conn, "Dana");
    assert_eq!(page.backend, "fts5");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "1");
    assert_eq!(page.results[0].matched_field, MatchedField::Participants);
    assert_eq!(page.results[0].document_context.document_id, "doc-e02");

    // Pre-existing names still resolve; the legacy comment is still on disk.
    let zoe = search(&conn, "Zoe");
    assert_eq!(zoe.total, 1);
    assert!(String::from_utf8_lossy(&fs::read(&path).unwrap())
        .contains("<!-- legacy comment must survive -->"));

    fs::remove_dir_all(&dir).ok();
}

// ═══════════════ E03: dependency bridge, XML and index agree ═══════════════

/// QA-M10-E03: dependencies added/removed through the bridge stay consistent
/// across all three representations — the live tree, the saved XML and the
/// `task_dependencies` index (forward + reverse), with cycle and
/// self-reference rejection at the command layer.
#[test]
fn qa_m10_e03_dependency_bridge_xml_and_index_agree() {
    let dir = sandbox("e03");
    let xml = br#"<?xml version="1.0" encoding="utf-8"?>
<TODOLIST PROJECTNAME="E03" FILENAME="deps.xml" NEXTUNIQUEID="4">
<TASK ID="1" TITLE="First" REFID="0" COMMENTSTYPE="PLAIN_TEXT" PRIORITY="5" RISK="0" PERCENTDONE="0" POS="0" POSSTRING="1"/>
<TASK ID="2" TITLE="Second" REFID="0" COMMENTSTYPE="PLAIN_TEXT" PRIORITY="5" RISK="0" PERCENTDONE="0" POS="1" POSSTRING="2"/>
<TASK ID="3" TITLE="Third" REFID="0" COMMENTSTYPE="PLAIN_TEXT" PRIORITY="5" RISK="0" PERCENTDONE="0" POS="2" POSSTRING="3"/>
</TODOLIST>"#;
    let path = dir.join("deps.xml");
    fs::write(&path, xml).unwrap();
    let mut tree = build_tree_from_bytes(xml).unwrap();
    let s = DocumentSession::new(None);
    let mut u = UndoRedoManager::new();
    let doc_id = "doc-e03";

    // Safety gates at the bridge layer.
    let err = add_dependency_core(&s, &mut tree, &mut u, "1", doc_id, "1", None, 0).unwrap_err();
    assert!(err.to_lowercase().contains("itself"), "{err}");
    add_dependency_core(&s, &mut tree, &mut u, "1", doc_id, "2", None, 0).unwrap();
    add_dependency_core(&s, &mut tree, &mut u, "2", doc_id, "3", None, 0).unwrap();
    let err = add_dependency_core(&s, &mut tree, &mut u, "3", doc_id, "1", None, 0).unwrap_err();
    assert!(err.to_lowercase().contains("cycle"), "{err}");
    assert_eq!(tree.get(&TaskId::new("3")).unwrap().dependencies.len(), 0);

    // Blocked state is visible through the read DTO.
    let g2 = get_dependencies_core(&tree, "2", doc_id).unwrap();
    assert_eq!(g2.outgoing.len(), 1);
    assert_eq!(g2.incoming.len(), 1);
    assert_eq!(g2.incoming[0].task_key, "1");
    let g1 = get_dependencies_core(&tree, "1", doc_id).unwrap();
    assert!(!g1.blocked, "local target exists → not blocked");

    // Save, then rebuild the index from the saved XML only.
    save(&path, &commit_tree_to_bytes(xml, &tree));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("<DEPENDENCY>"));
    assert!(text.contains("<TASKID>2</TASKID>"));

    let conn = setup_db();
    register_doc(&conn, "ws-e03", doc_id, &path);
    assert_eq!(index_document(&conn, doc_id, &path).unwrap(), 3);
    let fwd: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT task_key, depends_on_key FROM task_dependencies WHERE document_id=?1 ORDER BY task_key")
            .unwrap();
        stmt.query_map([doc_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(fwd, vec![("1".to_string(), "2".to_string()), ("2".to_string(), "3".to_string())]);

    // The M6 populator upgrades the classification and stays idempotent.
    let reloaded = build_tree_from_bytes(&fs::read(&path).unwrap()).unwrap();
    for t in reloaded.iter() {
        dependency::populate_task_dependencies(&conn, doc_id, t.id.as_str(), t).unwrap();
    }
    let types: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT dep_type FROM task_dependencies WHERE document_id=?1")
            .unwrap();
        stmt.query_map([doc_id], |r| r.get::<_, String>(0)).unwrap().collect::<Result<_, _>>().unwrap()
    };
    assert_eq!(types, vec!["local".to_string()]);
    assert_eq!(
        dependency::reverse_lookup(&conn, doc_id, "3").unwrap(),
        vec![("2".to_string(), "local".to_string())]
    );

    // Removal through the bridge propagates to XML and index.
    remove_dependency_core(&s, &mut tree, &mut u, "1", "2").unwrap();
    save(&path, &commit_tree_to_bytes(xml, &tree));
    index_document(&conn, doc_id, &path).unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_dependencies WHERE document_id=?1", [doc_id], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 1, "only 2→3 remains");
    assert!(dependency::reverse_lookup(&conn, doc_id, "2").unwrap().is_empty());
    // Removing twice is an explicit error, not a silent no-op.
    assert!(remove_dependency_core(&s, &mut tree, &mut u, "1", "2").is_err());
    // Undo restores the edge (bridge-level reversibility).
    assert!(u.undo(&mut tree).is_some());
    assert_eq!(tree.get(&TaskId::new("1")).unwrap().dependencies.len(), 1);

    fs::remove_dir_all(&dir).ok();
}

// ═══════════════ E04: progress links — security, persistence, search ═══════════════

/// QA-M10-E04: progress-link mutations through the bridge enforce URL
/// security, persist via the Tier B attribute without disturbing legacy
/// unknown data, populate `progress_links_index` and become searchable
/// through the links field.
#[test]
fn qa_m10_e04_progress_link_security_persistence_and_search() {
    let dir = sandbox("e04");
    let original = fs::read(fixtures_root().join("relations").join("progress-links.xml")).unwrap();
    let mut tree = build_tree_from_bytes(&original).unwrap();
    let s = DocumentSession::new(None);
    let mut u = UndoRedoManager::new();
    let doc_id = "doc-e04";

    // Security gate at the bridge: javascript: is refused, tree unchanged.
    for evil in ["javascript:alert(1)", "java\tscript:alert(1)", "data:text/html,x", "file:///C:/x"] {
        let err = add_progress_link_core(&s, &mut tree, &mut u, "1", doc_id, "Evil", evil, None).unwrap_err();
        assert!(
            err.to_lowercase().contains("scheme") || err.to_lowercase().contains("url"),
            "unexpected error for {evil}: {err}"
        );
    }
    assert_eq!(tree.get(&TaskId::new("1")).unwrap().progress_links.len(), 2);

    // Add / update / remove through the bridge.
    let dto = add_progress_link_core(&s, &mut tree, &mut u, "1", doc_id, "Linear issue", "https://linear.app/team/proj/issues/9", None).unwrap();
    assert_eq!(dto.provider, "linear");
    let upd = update_progress_link_core(&s, &mut tree, &mut u, "pl-2", "1", doc_id, "Tracker v2", "https://example.com/issue/9", None).unwrap();
    assert_eq!(upd.id, "pl-2", "id stable across update");
    assert_eq!(upd.label, "Tracker v2");
    remove_progress_link_core(&s, &mut tree, &mut u, "pl-1", "1").unwrap();
    assert_eq!(list_progress_links_core(&tree, "1", doc_id).unwrap().len(), 2);

    // Persist and verify losslessness around the Tier B attribute.
    let path = dir.join("progress-links.xml");
    save(&path, &commit_tree_to_bytes(&original, &tree));
    let reloaded_bytes = fs::read(&path).unwrap();
    let doc = parse_xml(&reloaded_bytes).unwrap();
    let t1 = read_task(&task_elems(&doc)[0]);
    assert_eq!(t1.progress_links.len(), 2);
    assert!(t1.progress_links.iter().any(|l| l.id == "pl-2" && l.label == "Tracker v2"));
    assert!(t1.progress_links.iter().any(|l| l.provider == progress_link::LinkProvider::Linear));
    assert!(t1.unknown_attrs.iter().any(|(k, v)| k == "TOOLATTR" && v == "legacy-value"));
    assert!(t1.unknown_attrs.iter().all(|(k, _)| k != progress_link::PROGRESS_LINKS_ATTR));
    assert!(t1.unknown_children.iter().any(|c| c.contains(r#"UNKNOWNEL"#)));
    let text = String::from_utf8_lossy(&reloaded_bytes);
    assert!(text.contains("<!-- progress link comment -->"));

    // Index + search from the saved XML.
    let conn = setup_db();
    register_doc(&conn, "ws-e04", doc_id, &path);
    index_document(&conn, doc_id, &path).unwrap();
    let n = progress_link::populate_progress_links_index(&conn, doc_id, "1", &t1.progress_links).unwrap();
    assert_eq!(n, 2);
    rebuild_search_index(&conn, &[(doc_id.to_string(), path.clone())], &IndexTableExtrasProvider).unwrap();
    let page = search(&conn, "linear");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "1");
    assert_eq!(page.results[0].matched_field, MatchedField::Links);

    fs::remove_dir_all(&dir).ok();
}

// ═══════════════ E05: managed attachment lifecycle across the save boundary ═══════════════

/// QA-M10-E05: a managed attachment imported and attached through the bridge
/// survives atomic save + re-parse as typed data, is rediscovered by
/// `rebuild_index_from_xml`, is searchable by display name, keeps BLAKE3
/// integrity, and removal produces an orphan candidate that GC (with
/// integrity verification) collects.
#[test]
fn qa_m10_e05_managed_attachment_lifecycle_across_save() {
    let dir = sandbox("e05");
    let original = fs::read(fixtures_root().join("rc").join("index-source.xml")).unwrap();
    let mut tree = build_tree_from_bytes(&original).unwrap();
    let s = DocumentSession::new(None);
    let mut u = UndoRedoManager::new();
    let doc_id = "doc-e05";

    let source = dir.join("design-spec.pdf");
    let payload = b"managed payload for E05 \xff\x00".to_vec();
    fs::write(&source, &payload).unwrap();
    let att = attachment::import_managed_attachment(&source, &dir, doc_id, None).unwrap();
    assert_eq!(att.kind, attachment::AttachmentKind::ManagedFile);
    let stored = attachment::resolve_path(&dir, &att).unwrap();
    assert!(stored.starts_with(attachment::asset_root(&dir, doc_id)));
    assert!(attachment::verify_integrity(&dir, &att).unwrap());

    let res = add_attachment_core(&s, &mut tree, &mut u, "2", doc_id, &dir, att.clone()).unwrap();
    assert!(res.success);
    let dto = res.attachment.unwrap();
    assert_eq!(dto.kind, "managed");
    assert!(dto.exists && dto.status == "ok");
    assert_eq!(list_attachments_core(&tree, "2", doc_id, &dir).unwrap().len(), 1);

    // Save + reload: typed attachment data survives the XML boundary.
    let path = dir.join("index-source.xml");
    save(&path, &commit_tree_to_bytes(&original, &tree));
    let saved_bytes = fs::read(&path).unwrap();
    let doc = parse_xml(&saved_bytes).unwrap();
    let t2 = read_task(&task_elems(&doc).iter().find(|e| e.get_attr("ID") == Some("2")).unwrap());
    assert_eq!(t2.attachments, vec![att.clone()], "typed round-trip");
    assert_eq!(t2.file_links.len(), 1);

    // The attachments index is rebuildable from the saved XML alone (the
    // core task index must exist first — the relation tables are FK-bound
    // to task_index, exactly like the production rebuild path).
    let conn = setup_db();
    register_doc(&conn, "ws-e05", doc_id, &path);
    index_document(&conn, doc_id, &path).unwrap();
    let n = attachment::rebuild_index_from_xml(&conn, doc_id, &saved_bytes).unwrap();
    assert_eq!(n, 1);
    let fp: String = conn
        .query_row("SELECT fingerprint FROM attachments_index WHERE document_id=?1", [doc_id], |r| r.get(0))
        .unwrap();
    assert_eq!(fp, att.hash.clone().unwrap());

    // Searchable by display name through the M9 seam.
    rebuild_search_index(&conn, &[(doc_id.to_string(), path.clone())], &IndexTableExtrasProvider).unwrap();
    let page = search(&conn, "design");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "2");
    assert_eq!(page.results[0].matched_field, MatchedField::Attachments);

    // Removal through the bridge: reference-only delete + orphan candidate.
    remove_attachment_core(&s, &mut tree, &mut u, "2", doc_id, &dir, &att.id).unwrap();
    assert!(list_attachments_core(&tree, "2", doc_id, &dir).unwrap().is_empty());
    let orphans = attachment::read_orphans(&dir, doc_id);
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].id, att.id);
    assert!(stored.exists(), "orphan candidate is never deleted eagerly");

    // GC with integrity verification collects the untouched file.
    let report = attachment::gc_orphans(&dir, doc_id, true).unwrap();
    assert_eq!(report.deleted, 1);
    assert!(!stored.exists());
    assert!(attachment::read_orphans(&dir, doc_id).is_empty());

    // Undo of the removal restores the reference (the orphan marker staying
    // behind is documented bridge behaviour — the file itself is gone only
    // because GC ran; without GC the bytes would still be recoverable).
    assert!(u.undo(&mut tree).is_some());
    assert_eq!(tree.get(&TaskId::new("2")).unwrap().attachments, vec![att]);

    fs::remove_dir_all(&dir).ok();
}

// ═══════════════ E06: hostile HTML sanitize → commit → save → search ═══════════════

/// QA-M10-E06: the M7 choke points neutralize the hostile fixture HTML, the
/// sanitized commit persists with COMMENTSTYPE="HTML" unchanged through the
/// M2 save path, the M9 search extractor indexes only the safe text, and the
/// sanitized document is searchable (including through the CJK fallback).
#[test]
fn qa_m10_e06_hostile_html_sanitized_commit_roundtrip_and_search() {
    let dir = sandbox("e06");
    let original = fs::read(fixtures_root().join("richtext").join("html-hostile.xml")).unwrap();
    let doc = parse_xml(&original).unwrap();
    let mut task = read_task(&task_elems(&doc)[0]);
    let hostile = task.comments.as_ref().expect("fixture has comments").content.clone();
    assert!(hostile.contains("<script>"), "fixture really is hostile");

    // Core→Editor feed, an edit, and the Editor→Core commit: every choke
    // point sanitizes independently.
    let mut field = CommentsField::new(to_m7_comments(&task));
    assert_eq!(field.view().mode, EditorMode::RichText);
    let fed = sanitize_for_editor(&hostile).unwrap();
    assert!(fed.changed() && fed.threat_count() > 0);
    let edited = format!("{}<p>Reviewed and approved</p>", fed.html);
    let committed = sanitize_for_core(&edited).unwrap().html;
    field.save(&committed, CommentsType::PlainText).expect("html save keeps its type");
    let value = field.value().unwrap();
    assert_eq!(value.comments_type, CommentsType::Html);
    // Markup-level danger must be gone. The sanitizer deliberately keeps the
    // `<scr<script>ipt>` tokenizer-probe residue as inert ESCAPED text
    // ("ipt&gt;alert(6)") — documented behaviour asserted in m7/sanitizer
    // unit tests — so the probe here is for live markup, not inert words.
    for needle in ["<script", "onclick", "onerror", "javascript:", "<iframe", "<svg", "<object", "behavior:"] {
        assert!(!value.content.contains(needle), "commit still carries {needle}: {}", value.content);
    }
    assert!(value.content.contains("Benign paragraph"));
    assert!(value.content.contains("Reviewed and approved"));

    // Write back through the M2 mapper and save.
    task.comments = Some(TaskComment {
        comment_type: CommentType::Html,
        content: value.content.clone(),
    });
    let mut doc2 = parse_xml(&original).unwrap();
    {
        let el = find_task_elem_mut(&mut doc2.root, "1").unwrap();
        write_task(&task, el);
    }
    let saved_bytes = serialize_xml(&doc2);
    let path = dir.join("html-hostile.xml");
    save(&path, &saved_bytes);
    let on_disk = fs::read(&path).unwrap();
    let text = String::from_utf8_lossy(&on_disk).to_string();
    assert!(text.contains(r#"COMMENTSTYPE="HTML""#), "type attribute unchanged");
    assert!(!text.to_lowercase().contains("<script"), "no script reached the file");
    assert!(!text.contains("onerror"));

    // Re-parse: COMMENTSTYPE + sanitized content stable at the domain level.
    let reread = read_task(&task_elems(&parse_xml(&on_disk).unwrap())[0]);
    assert_eq!(reread.comments_type, CommentType::Html);
    assert_eq!(reread.comments.as_ref().unwrap().content, value.content);

    // The M9 extractor indexes only the safe text (ASCII HTML path).
    let plain = search::task_description_text(&reread);
    assert!(plain.contains("Benign paragraph") && plain.contains("Reviewed and approved"));
    assert!(!plain.contains("<script") && !plain.contains("onclick"));

    // End-to-end search over the sanitized, saved document.
    let conn = setup_db();
    register_doc(&conn, "ws-e06", "doc-e06", &path);
    index_document(&conn, "doc-e06", &path).unwrap();
    rebuild_search_index(&conn, &[("doc-e06".to_string(), path.clone())], &IndexTableExtrasProvider).unwrap();
    let page = search(&conn, "Benign");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].matched_field, MatchedField::Description);
    assert_eq!(search(&conn, "onclick").total, 0, "sanitized payload is not searchable");

    // ── REGRESSION: product bug E-F2, now FIXED ──
    // `search::strip_html` used to confuse CHAR indices with BYTE indices
    // (src-tauri/src/domain/search.rs): `tag_end = i` indexed the `Vec<char>`
    // but was then used to slice `html`/`lower` by BYTE. Any HTML comment with
    // a multi-byte (e.g. CJK) character before a tag's closing `>` panicked
    // `task_description_text` — and with it `SearchDocument::from_task` /
    // `rebuild_search_index` / the whole search-index build for documents
    // carrying CJK rich text. The tag is now reconstructed in char space, so
    // extraction is correct for any Unicode content. This assertion was a
    // tripwire pinned to the buggy behaviour; it now requires correct output.
    let mut cjk_html = Task::new(TaskId::new("9"));
    cjk_html.comments_type = CommentType::Html;
    cjk_html.comments = Some(TaskComment {
        comment_type: CommentType::Html,
        content: "<p>评审记录</p>".to_string(),
    });
    assert_eq!(
        search::task_description_text(&cjk_html),
        "评审记录",
        "E-F2 regression: CJK HTML must extract cleanly, not panic"
    );
    // The original panic trigger: a comment carrying CJK before a tag's `>`.
    let mut cjk_comment_html = Task::new(TaskId::new("10"));
    cjk_comment_html.comments_type = CommentType::Html;
    cjk_comment_html.comments = Some(TaskComment {
        comment_type: CommentType::Html,
        content: "<!-- 评审备注 --><p>待办事项</p>".to_string(),
    });
    assert_eq!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            search::task_description_text(&cjk_comment_html)
        }))
        .expect("E-F2 regression: a CJK HTML comment must not panic strip_html"),
        "待办事项"
    );
    // PLAIN_TEXT CJK comments take the as-is path and are unaffected.
    let mut cjk_plain = Task::new(TaskId::new("9"));
    cjk_plain.comments_type = CommentType::Plain;
    cjk_plain.comments = Some(TaskComment {
        comment_type: CommentType::Plain,
        content: "评审记录".to_string(),
    });
    assert_eq!(search::task_description_text(&cjk_plain), "评审记录");

    fs::remove_dir_all(&dir).ok();
}

// ═══════════════ E07: comments-type preservation matrix over the fixtures ═══════════════

/// QA-M10-E07: every richtext fixture survives the full domain rewrite with
/// its COMMENTSTYPE attribute byte-identical; RTF and unknown types refuse
/// edits and conversions through the M7 value object; a task without
/// comments never materializes one; viewing never converts.
#[test]
fn qa_m10_e07_comments_type_matrix_across_xml_roundtrip() {
    let dir = fixtures_root().join("richtext");
    let mut files: Vec<_> = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    files.sort();
    assert_eq!(files.len(), 5, "richtext fixture corpus");

    for path in &files {
        let bytes = fs::read(path).unwrap();
        let doc = parse_xml(&bytes).unwrap();
        let before: Vec<(String, String, Task)> = task_elems(&doc)
            .iter()
            .map(|e| {
                (
                    e.get_attr("ID").unwrap().to_string(),
                    e.get_attr("COMMENTSTYPE").unwrap_or("").to_string(),
                    read_task(e),
                )
            })
            .collect();

        let rewritten = domain_rewrite_bytes(&bytes);
        let doc2 = parse_xml(&rewritten).unwrap();
        let after_elems = task_elems(&doc2);
        assert_eq!(after_elems.len(), before.len(), "{}: task count", path.display());
        for (i, (id, attr, task)) in before.iter().enumerate() {
            // COMMENTSTYPE is byte-identical when present. When the source
            // omitted the attribute entirely, write_task normalizes it to
            // the TDL default "PLAIN_TEXT" (semantic no-op, asserted below).
            let attr_after = after_elems[i].get_attr("COMMENTSTYPE").unwrap_or("");
            assert!(
                attr_after == attr || (attr.is_empty() && attr_after == "PLAIN_TEXT"),
                "{}: task {id} COMMENTSTYPE changed {attr:?} -> {attr_after:?}",
                path.display()
            );
            if attr.is_empty() {
                assert_eq!(task.comments_type, CommentType::Plain, "default type must be plain");
            }
            // Full domain equality: comments content + type + everything else.
            assert_eq!(&after_elems[i].get_attr("ID").unwrap().to_string(), id);
            let after = read_task(&after_elems[i]);
            assert_eq!(&after, task, "{}: task {id} lost data", path.display());
        }

        // M7 read-only discipline on the same payload.
        for (_id, _attr, task) in &before {
            let Some(tc) = to_m7_comments(task) else {
                // No comments: viewing/saving empty must never materialize.
                let mut empty = CommentsField::new(None);
                assert!(!empty.view().has_comments);
                empty.save("", CommentsType::Html).expect("no-op save");
                assert_eq!(empty.value(), None);
                continue;
            };
            let mut field = CommentsField::new(Some(tc.clone()));
            let projection = field.view();
            assert_eq!(projection.comments_type, tc.comments_type);
            match tc.comments_type {
                CommentsType::Rtf | CommentsType::Unknown(_) => {
                    assert_eq!(projection.mode, EditorMode::ReadOnly);
                    assert!(!projection.editable);
                    assert!(matches!(field.save("tampered", CommentsType::Html), Err(CommentsError::ReadOnly(_))));
                    assert!(matches!(field.convert_to_rich_text(true), Err(CommentsError::ReadOnly(_))));
                    assert_eq!(field.value(), Some(&tc), "read-only payload untouched");
                    // Opaque types never leak into the search index.
                    assert_eq!(moderntodolist_lib::domain::rich_text::extract_plain_text(&tc), "");
                    assert!(moderntodolist_lib::domain::rich_text::opaque_placeholder(&tc).is_some());
                }
                CommentsType::PlainText => {
                    assert_eq!(projection.mode, EditorMode::PlainText);
                    // A wrong default type on save must be ignored.
                    field.save(&format!("{}\nappended", tc.content), CommentsType::Html).unwrap();
                    assert_eq!(field.value().unwrap().comments_type, CommentsType::PlainText);
                    assert_eq!(field.metrics().type_changes, 0);
                }
                CommentsType::Html => {
                    assert_eq!(projection.mode, EditorMode::RichText);
                    // HTML saves always produce sanitized HTML.
                    field.save(&format!("{}<script>alert(1)</script>", tc.content), CommentsType::Html).unwrap();
                    assert_eq!(field.value().unwrap().comments_type, CommentsType::Html);
                    assert!(!field.value().unwrap().content.contains("<script"));
                }
            }
        }
    }

    // Specific corpus markers: CJK plain text stays verbatim; the lowercase
    // "html" spelling and its unknown attribute survive; the RTF payload is
    // byte-stable.
    let cjk = fs::read(dir.join("plain-text-cjk.xml")).unwrap();
    let cjk2 = domain_rewrite_bytes(&cjk);
    let t1 = read_task(&task_elems(&parse_xml(&cjk2).unwrap())[0]);
    assert!(t1.comments.as_ref().unwrap().content.contains("第一行中文内容"));
    assert!(t1.comments.as_ref().unwrap().content.contains("<不是标记>"));
    let opaque = fs::read(dir.join("opaque-commentstypes.xml")).unwrap();
    let opaque2 = domain_rewrite_bytes(&opaque);
    let elems = task_elems(&parse_xml(&opaque2).unwrap());
    assert_eq!(elems[2].get_attr("COMMENTSTYPE"), Some("html"), "lowercase spelling preserved");
    let t3 = read_task(&elems[2]);
    assert!(t3.unknown_attrs.iter().any(|(k, v)| k == "M7UNKNOWNATTR" && v == "keep-me"));
    let rtf = read_task(&elems[0]);
    assert!(rtf.comments.as_ref().unwrap().content.contains("\\rtf1"));
}

// ═══════════════ E08: managed image asset + rich text in the XML ═══════════════

/// A real 1x1 transparent PNG.
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
    0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
    0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
    0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// QA-M10-E08: a pasted base64 image is externalized into the managed asset
/// store (BLAKE3-verified, UUID-named), the rich text references it by
/// relative path, the saved XML never contains base64, the reference
/// round-trips through parse/read, and GC keeps the asset while the XML
/// references it.
#[test]
fn qa_m10_e08_managed_image_no_base64_in_saved_xml() {
    let tmp = tempfile::TempDir::new().unwrap();
    let ws_root = tmp.path();
    let store = ImageAssetStore::new(ws_root, "doc-e08", 1).unwrap();

    // Paste with an inline base64 PNG → externalized, content-addressed.
    let payload = moderntodolist_lib::domain::rich_text::base64_encode(PNG_1X1);
    let pasted = format!("<p>截图</p><img alt=\"截图\" src=\"data:image/png;base64,{payload}\">");
    let report = store.externalize_pasted_html(&pasted).unwrap();
    assert_eq!(report.ingested.len(), 1);
    let asset = store
        .ingest_bytes(PNG_1X1, "screenshot.png", IngestSource::Clipboard)
        .unwrap();
    assert!(asset.deduplicated, "same bytes dedupe onto one managed file");
    assert_eq!(asset.file_name, report.ingested[0].file_name);
    assert!(store.verify(&asset.file_name).unwrap(), "BLAKE3 integrity");

    // A remote tracker image never survives the commit choke point.
    let with_remote = format!("{}<img src=\"https://evil.example/t.png\">", report.html);
    let committed = sanitize_for_core(&with_remote).unwrap().html;
    assert!(!committed.contains("evil.example"));
    assert!(committed.contains(&asset.reference));

    // Commit through the M7 value object into an M2 task, then save.
    let mut field = CommentsField::new(None);
    field.save(&committed, CommentsType::Html).unwrap();
    let value = field.value().unwrap();
    let mut task = Task::new(TaskId::new("1"));
    task.title = "Image task".into();
    task.comments_type = CommentType::Html;
    task.comments = Some(TaskComment {
        comment_type: CommentType::Html,
        content: value.content.clone(),
    });
    let mut el = XmlElement::new("TASK");
    write_task(&task, &mut el);
    let mut root = XmlElement::new("TODOLIST");
    root.children.push(XmlNode::Element(el));
    let doc = moderntodolist_lib::domain::XmlDocument::new(
        moderntodolist_lib::domain::encoding::XmlEncodingMeta::default_utf8(),
        root,
    );
    let bytes = serialize_xml(&doc);
    let text = String::from_utf8_lossy(&bytes).to_string();

    // The XML carries the relative managed reference and NO base64.
    assert!(!text.contains("base64"), "base64 leaked into XML: {text}");
    assert!(!text.contains(&payload));
    assert!(text.contains(&asset.reference));
    assert!(text.contains(r#"COMMENTSTYPE="HTML""#));
    assert!(asset.reference.starts_with("../.assets/doc-e08/images/"));

    // Round-trip: re-parse → identical comments payload.
    let back = read_task(&task_elems(&parse_xml(&bytes).unwrap())[0]);
    assert_eq!(back.comments.as_ref().unwrap().content, value.content);
    assert_eq!(back.comments_type, CommentType::Html);

    // Preview rendering keeps the managed image and stays inert.
    let preview = sanitize_for_preview(&value.content).unwrap();
    assert!(preview.html.contains("<img"));
    assert!(preview.html.contains(&asset.reference));

    // GC: while the saved XML references the asset it is never collected.
    let mut refs = ReferenceSet::new();
    refs.push_xml(text.clone());
    let gc = store
        .run_gc(
            &refs,
            &GcOptions {
                user_confirmed: true,
                dry_run: false,
                grace_period_ms: 0,
                require_orphan_mark: false,
                now_ms: u64::MAX,
            },
        )
        .unwrap();
    assert!(gc.deleted.is_empty());
    assert_eq!(
        gc.decisions.iter().find(|d| d.file_name == asset.file_name).unwrap().keep_reason,
        Some(KeepReason::ReferencedInXml)
    );
    assert!(asset.abs_path.exists());
}

// ═══════════════ E09: every relation index rebuildable from one saved XML ═══════════════

/// QA-M10-E09: a document carrying participants, tags, local + external
/// dependencies, progress links, attachments, comments AND legacy unknown
/// data drives every derived store — task_index, task_participants,
/// task_dependencies, progress_links_index, attachments_index and the search
/// index — rebuilt from the saved XML alone, idempotently and consistently
/// with the domain read.
#[test]
fn qa_m10_e09_all_relation_indexes_rebuildable_from_saved_xml() {
    let dir = sandbox("e09");
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<TODOLIST PROJECTNAME="E09 Combined" FILENAME="combined.xml" NEXTUNIQUEID="4">
<TASK ID="1" TITLE="Combined relations task" REFID="0" COMMENTSTYPE="PLAIN_TEXT" PRIORITY="8" RISK="2" PERCENTDONE="40" POS="0" POSSTRING="1" ALLOCATEDTO="Alice; Bob" ALLOCATEDBY="PM" CUSTOMATTR="keepme" {pl_attr}="[{pl_json}]" {att_attr}="[{att_json}]">
<!-- combined comment -->
<CATEGORY>release</CATEGORY>
<CATEGORY>rc</CATEGORY>
<DEPENDENCY><TASKID>2</TASKID><DEPENDENCYTYPE>0</DEPENDENCYTYPE></DEPENDENCY>
<FILEREFPATH>https://example.com/zebra-report.pdf</FILEREFPATH>
<COMMENTS>zebra striped description text</COMMENTS>
<UNKNOWNEL A="1"/>
</TASK>
<TASK ID="2" TITLE="Second task" REFID="0" COMMENTSTYPE="PLAIN_TEXT" PRIORITY="5" RISK="0" PERCENTDONE="0" POS="1" POSSTRING="2" ALLOCATEDTO="Carol"/>
<TASK ID="3" TITLE="External dependent" REFID="0" COMMENTSTYPE="PLAIN_TEXT" PRIORITY="5" RISK="0" PERCENTDONE="0" POS="2" POSSTRING="3">
<DEPENDENCY MTDL_DOCUMENTID="doc-other"><TASKID>9</TASKID><DEPENDENCYTYPE>1</DEPENDENCYTYPE></DEPENDENCY>
</TASK>
</TODOLIST>"#,
        pl_attr = progress_link::PROGRESS_LINKS_ATTR,
        pl_json = r#"{"id":"pl-9","label":"Build pipeline","url":"https://github.com/org/repo/actions/9"}"#
            .replace('"', "&quot;"),
        att_attr = attachment::ATTACHMENTS_ATTR,
        att_json = r#"{"id":"att-9","kind":"url","display_name":"zebra-report.pdf","path_or_url":"https://example.com/zebra-report.pdf","size":null,"hash":null}"#
            .replace('"', "&quot;"),
    );
    let path = dir.join("combined.xml");
    fs::write(&path, xml.as_bytes()).unwrap();

    // Domain read consumes the Tier B attributes into typed fields.
    let doc = parse_xml(xml.as_bytes()).unwrap();
    let elems = task_elems(&doc);
    let t1 = read_task(&elems[0]);
    assert_eq!(t1.progress_links.len(), 1);
    assert_eq!(t1.progress_links[0].provider, progress_link::LinkProvider::Github);
    assert_eq!(t1.attachments.len(), 1);
    assert!(t1.unknown_attrs.iter().any(|(k, v)| k == "CUSTOMATTR" && v == "keepme"));
    assert!(t1.unknown_attrs.iter().all(|(k, _)| k != progress_link::PROGRESS_LINKS_ATTR));

    // Save (identity: the file already holds exactly these bytes) and build
    // every derived store from the saved XML.
    save(&path, xml.as_bytes());
    let conn = setup_db();
    let doc_id = "doc-e09";
    register_doc(&conn, "ws-e09", doc_id, &path);
    assert_eq!(index_document(&conn, doc_id, &path).unwrap(), 3);

    let rebuild_all = |conn: &Connection, path: &Path| {
        let bytes = fs::read(path).unwrap();
        let tree = build_tree_from_bytes(&bytes).unwrap();
        let mut p = 0usize;
        let mut d = 0usize;
        let mut l = 0usize;
        for t in tree.iter() {
            p += participant::populate_task_participants(conn, doc_id, t.id.as_str(), t).unwrap();
            d += dependency::populate_task_dependencies(conn, doc_id, t.id.as_str(), t).unwrap();
            l += progress_link::populate_progress_links_index(conn, doc_id, t.id.as_str(), &t.progress_links).unwrap();
        }
        let a = attachment::rebuild_index_from_xml(conn, doc_id, &bytes).unwrap();
        let s = rebuild_search_index(conn, &[(doc_id.to_string(), path.to_path_buf())], &IndexTableExtrasProvider).unwrap();
        (p, d, l, a, s)
    };

    let counts = rebuild_all(&conn, &path);
    assert_eq!(counts, (4, 2, 1, 1, 3), "participants / deps / links / attachments / search rows");

    // Idempotency: a second rebuild changes nothing.
    assert_eq!(rebuild_all(&conn, &path), counts);
    for table in ["task_participants", "task_dependencies", "progress_links_index", "attachments_index"] {
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table} WHERE document_id='{doc_id}'"), [], |r| r.get(0))
            .unwrap();
        let expected = match table {
            "task_participants" => 4,
            "task_dependencies" => 2,
            "progress_links_index" => 1,
            _ => 1,
        };
        assert_eq!(n, expected, "{table} row count drifted");
    }

    // Index contents agree with the domain read.
    let ext: String = conn
        .query_row("SELECT depends_on_key FROM task_dependencies WHERE document_id=?1 AND task_key='3'", [doc_id], |r| r.get(0))
        .unwrap();
    assert_eq!(ext, "doc-other:9", "external ref keyed by document");
    let rows = participant::index_rows(doc_id, "1", &t1);
    assert_eq!(rows.len(), 3, "Alice + Bob + PM");
    assert_eq!(
        dependency::reverse_lookup(&conn, doc_id, "2").unwrap(),
        vec![("1".to_string(), "local".to_string())]
    );

    // Search agrees with every side table.
    assert_eq!(search(&conn, "zebra").results.iter().map(|r| r.task_key.task_id.as_str()).collect::<Vec<_>>(), vec!["1"]);
    assert_eq!(search(&conn, "Alice").total, 1);
    let gh = search(&conn, "github");
    assert_eq!(gh.total, 1);
    assert_eq!(gh.results[0].matched_field, MatchedField::Links);
    let tag = search(&conn, "release");
    assert_eq!(tag.total, 1);
    assert_eq!(tag.results[0].matched_field, MatchedField::Tags);

    fs::remove_dir_all(&dir).ok();
}

// ═══════════════ E10: external + unresolved dependency refs end-to-end ═══════════════

/// QA-M10-E10: cross-document (external) and unresolved dependency refs are
/// classified by the bridge read DTOs, survive the XML round-trip with their
/// raw payload (MTDL_DOCUMENTID + legacy attrs + plugin children), keep the
/// document intact for unresolved refs, and land in the index under
/// document-qualified keys.
#[test]
fn qa_m10_e10_external_and_unresolved_refs_end_to_end() {
    let original = fs::read(fixtures_root().join("relations").join("dependency-external.xml")).unwrap();
    let tree = build_tree_from_bytes(&original).unwrap();
    let doc_id = "doc-e10";

    // Bridge classification.
    let g1 = get_dependencies_core(&tree, "1", doc_id).unwrap();
    assert_eq!(g1.outgoing.len(), 2);
    let local = &g1.outgoing[0];
    assert_eq!(local.ref_kind, "local");
    assert_eq!(local.depends_on_key, "2");
    assert_eq!(local.depends_on_document_id, None);
    let external = &g1.outgoing[1];
    assert_eq!(external.ref_kind, "external");
    assert_eq!(external.depends_on_key, "7");
    assert_eq!(external.depends_on_document_id.as_deref(), Some("doc-other"));
    assert_eq!(external.dep_type, 1);
    assert!(!g1.blocked);
    let g2 = get_dependencies_core(&tree, "2", doc_id).unwrap();
    assert_eq!(g2.incoming.len(), 1, "reverse edge visible on the target");
    let g3 = get_dependencies_core(&tree, "3", doc_id).unwrap();
    assert_eq!(g3.outgoing[0].ref_kind, "unresolved");
    assert!(g3.blocked, "unresolved ref degrades to blocked");

    // XML round-trip keeps the raw enriched payload byte-stable.
    let rewritten = domain_rewrite_bytes(&original);
    let doc2 = parse_xml(&rewritten).unwrap();
    let elems = task_elems(&doc2);
    let t1 = read_task(&elems[0]);
    let raw = t1.dependencies[1].raw_xml.clone().expect("raw payload captured");
    assert!(raw.contains(r#"MTDL_DOCUMENTID="doc-other""#));
    assert!(raw.contains(r#"LEGACYATTR="keepme""#));
    assert!(raw.contains("<PLUGINNOTE>third-party data</PLUGINNOTE>"));
    let t3 = read_task(&elems[2]);
    assert_eq!(t3.dependencies.len(), 1, "unresolved ref preserved, never dropped");
    assert_eq!(t3.dependencies[0].task_id, "");
    let text = String::from_utf8_lossy(&rewritten);
    assert!(text.contains("<!-- dependency comment -->"));
    // Second rewrite is a fixed point.
    assert_eq!(domain_rewrite_bytes(&rewritten), rewritten);

    // Index: document-qualified keys and reverse lookup. task_index is
    // FK-bound to documents and the relation tables are FK-bound to
    // task_index, so register the document and seed its three task rows
    // first (same pattern as the M6 QA suite).
    let conn = setup_db();
    register_doc(&conn, "ws-e10", doc_id, Path::new("dependency-external.xml"));
    for key in ["1", "2", "3"] {
        conn.execute(
            "INSERT INTO task_index (task_key, document_id) VALUES (?1, ?2)",
            rusqlite::params![key, doc_id],
        )
        .unwrap();
    }
    let mut total = 0;
    for e in &elems {
        let t = read_task(e);
        total += dependency::populate_task_dependencies(&conn, doc_id, t.id.as_str(), &t).unwrap();
    }
    assert_eq!(total, 3);
    let keys: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT depends_on_key, dep_type FROM task_dependencies WHERE document_id=?1 AND task_key='1' ORDER BY depends_on_key")
            .unwrap();
        stmt.query_map([doc_id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
    };
    assert!(keys.iter().any(|(k, t)| k == "2" && t == "local"));
    assert!(keys.iter().any(|(k, t)| k == "doc-other:7" && t == "external"));
    assert_eq!(
        dependency::reverse_lookup(&conn, doc_id, "2").unwrap(),
        vec![("1".to_string(), "local".to_string())]
    );
}

// ═══════════════ E11: one undo stack across all relation types ═══════════════

/// QA-M10-E11: mixed relation edits (title, participant, dependency,
/// progress link, attachment, comments) share ONE UndoRedoManager; a full
/// unwind restores a tree that is exactly equal to the original snapshot and
/// serializes to the same bytes; a full redo returns the mutated state.
#[test]
fn qa_m10_e11_unified_undo_across_relation_types() {
    let xml = br#"<?xml version="1.0" encoding="utf-8"?>
<TODOLIST PROJECTNAME="E11" FILENAME="undo.xml" NEXTUNIQUEID="4">
<TASK ID="1" TITLE="Alpha" REFID="0" COMMENTSTYPE="PLAIN_TEXT" PRIORITY="5" RISK="0" PERCENTDONE="0" POS="0" POSSTRING="1"/>
<TASK ID="2" TITLE="Beta" REFID="0" COMMENTSTYPE="PLAIN_TEXT" PRIORITY="5" RISK="0" PERCENTDONE="0" POS="1" POSSTRING="2"/>
</TODOLIST>"#;
    let dir = sandbox("e11");
    let mut tree = build_tree_from_bytes(xml).unwrap();
    let original = tree.clone();
    let s = DocumentSession::new(None);
    let mut u = UndoRedoManager::new();
    let doc_id = "doc-e11";

    update_task_field_core(&s, &mut tree, &mut u, "1", "title", "Alpha renamed").unwrap();
    add_participant_core(&s, &mut tree, &mut u, "1", "Alice", "allocated_to").unwrap();
    add_dependency_core(&s, &mut tree, &mut u, "1", doc_id, "2", None, 0).unwrap();
    add_progress_link_core(&s, &mut tree, &mut u, "1", doc_id, "CI", "https://github.com/o/r/actions/1", None).unwrap();
    let att = attachment::url_attachment("https://example.com/e11", Some("E11")).unwrap();
    add_attachment_core(&s, &mut tree, &mut u, "1", doc_id, &dir, att).unwrap();
    update_task_field_core(&s, &mut tree, &mut u, "1", "comments", "mixed edits").unwrap();
    assert_eq!(u.undo_count(), 6);
    let mutated = tree.clone();
    assert_eq!(tree.get(&TaskId::new("1")).unwrap().title, "Alpha renamed");
    assert_eq!(tree.get(&TaskId::new("1")).unwrap().dependencies.len(), 1);

    // Full unwind → the original tree, with ONE documented deviation
    // (finding E-F1, minor): `TaskField::Comments` cannot represent "no
    // comments" — `get_field` captures `unwrap_or_default()` (command.rs
    // L260) and `set_field` materializes `Some(TaskComment)` (command.rs
    // L294-303) — so undoing the FIRST comments edit leaves an empty
    // `Some(TaskComment{content:""})` instead of `None`. No user content is
    // lost; the only effect is that a post-undo save would add an empty
    // `<COMMENTS></COMMENTS>` element to a document that had none.
    let mut expected = original.clone();
    expected.get_mut(&TaskId::new("1")).unwrap().comments = Some(TaskComment {
        comment_type: CommentType::Plain,
        content: String::new(),
    });
    for _ in 0..6 {
        assert!(u.undo(&mut tree).is_some());
    }
    assert_eq!(tree, expected, "mixed relation edits must unwind exactly (modulo E-F1)");
    assert_eq!(
        commit_tree_to_bytes(xml, &tree),
        commit_tree_to_bytes(xml, &expected),
        "byte-identical document after full unwind"
    );
    let t1 = tree.get(&TaskId::new("1")).unwrap();
    assert_eq!(t1.title, "Alpha");
    assert!(t1.allocated_to.is_empty() && t1.participants.is_empty());
    assert!(t1.dependencies.is_empty() && t1.progress_links.is_empty());
    assert!(t1.attachments.is_empty() && t1.file_links.is_empty());
    assert_eq!(t1.comments.as_ref().map(|c| c.content.as_str()), Some(""), "E-F1: empty, never stale content");

    // Full redo → exact mutated tree.
    for _ in 0..6 {
        assert!(u.redo(&mut tree).is_some());
    }
    assert_eq!(tree, mutated, "redo replays every relation edit");

    fs::remove_dir_all(&dir).ok();
}

// ═══════════════ E12: cumulative corpus regression ═══════════════

/// QA-M10-E12: the whole relations + richtext + rc fixture corpus survives
/// the domain rewrite losslessly at the task level, byte-stably from the
/// first save onwards, with validator-clean output; documents without M6/M7
/// data never gain extension attributes.
#[test]
fn qa_m10_e12_relations_richtext_corpus_cumulative_regression() {
    let root = fixtures_root();
    let mut files: Vec<PathBuf> = Vec::new();
    for group in ["relations", "richtext", "rc"] {
        let dir = root.join(group);
        let mut group_files: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "xml").unwrap_or(false))
            .collect();
        group_files.sort();
        files.extend(group_files);
    }
    assert!(files.len() >= 14, "corpus should cover relations+richtext+rc: {}", files.len());

    for path in &files {
        let bytes = fs::read(path).unwrap();
        let doc1 = parse_xml(&bytes).unwrap_or_else(|e| panic!("{}: parse: {e}", path.display()));
        let tasks1: Vec<Task> = task_elems(&doc1).iter().map(read_task).collect();

        let s1 = domain_rewrite_bytes(&bytes);
        let doc2 = parse_xml(&s1).unwrap_or_else(|e| panic!("{}: re-parse: {e}", path.display()));
        let tasks2: Vec<Task> = task_elems(&doc2).iter().map(read_task).collect();
        assert_eq!(tasks1, tasks2, "{}: domain rewrite lost data", path.display());
        assert!(
            moderntodolist_lib::domain::validator::validate_document(&doc2).is_empty(),
            "{}: rewritten document fails validation",
            path.display()
        );

        // Byte stability from the first save onwards (the first save may
        // normalize BOM/declaration — a documented, accepted deviation).
        let s2 = domain_rewrite_bytes(&s1);
        let s3 = domain_rewrite_bytes(&s2);
        assert_eq!(s2, s3, "{}: rewrite is not a fixed point", path.display());
    }

    // M6 markers survive in the rewritten corpus files that carry them.
    let participants = domain_rewrite_bytes(&fs::read(root.join("relations").join("participants.xml")).unwrap());
    let text = String::from_utf8_lossy(&participants);
    assert!(text.contains("<!-- legacy comment must survive -->"));
    assert!(text.contains(r#"CUSTOMATTR="keepme""#));

    // Documents WITHOUT M6/M7 data never gain extension attributes.
    for rel in ["canonical/single-task.xml", "rc/index-source.xml"] {
        let bytes = fs::read(root.join(rel)).unwrap();
        let rewritten = domain_rewrite_bytes(&bytes);
        let doc = parse_xml(&rewritten).unwrap();
        for el in task_elems(&doc) {
            assert!(el.get_attr(progress_link::PROGRESS_LINKS_ATTR).is_none(), "{rel} gained progress-link attr");
            assert!(el.get_attr(attachment::ATTACHMENTS_ATTR).is_none(), "{rel} gained attachment attr");
        }
    }
}
