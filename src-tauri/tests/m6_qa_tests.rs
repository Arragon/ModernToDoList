//! M6 QA Integration Tests — Task Relations.
//!
//! Covers QA-M6-001 … QA-M6-018 (Linear INH-1055 … INH-1059):
//! participants, dependencies, progress links, attachments, and the
//! M0–M5 cumulative regression with M6 fields populated.
//!
//! Core invariant under test: unknown elements, unknown attributes and
//! comments survive parse → domain → serialize round-trips unchanged.

use std::fs;
use std::path::{Path, PathBuf};

use moderntodolist_lib::domain::attachment;
use moderntodolist_lib::domain::command::UndoRedoManager;
use moderntodolist_lib::domain::dependency::{
    self, AddDependencyCommand, DependencyError, DependencyGraph, RemoveDependencyCommand, TaskRef,
};
use moderntodolist_lib::domain::mappers::{read_task, write_task};
use moderntodolist_lib::domain::participant;
use moderntodolist_lib::domain::progress_link::{self, LinkProvider, ProgressLink, UrlValidationError};
use moderntodolist_lib::domain::task::Task;
use moderntodolist_lib::domain::types::TaskId;
use moderntodolist_lib::domain::xml_tree::{XmlElement, XmlNode};
use moderntodolist_lib::domain::{parse_xml, serialize_xml, XmlDocument};

// ─── Helpers ─────────────────────────────────────────────────────────

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate root should have a parent")
        .join("tests")
        .join("fixtures")
        .join("xml")
}

fn relations_fixture(name: &str) -> PathBuf {
    fixtures_root().join("relations").join(name)
}

fn parse_file(path: &Path) -> XmlDocument {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    parse_xml(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Collects TASK elements in document order (depth-first).
fn collect_task_elems(elem: &XmlElement, out: &mut Vec<XmlElement>) {
    if elem.tag == "TASK" {
        out.push(elem.clone());
    }
    for child in elem.child_elements() {
        collect_task_elems(child, out);
    }
}

fn task_elems(doc: &XmlDocument) -> Vec<XmlElement> {
    let mut v = Vec::new();
    collect_task_elems(&doc.root, &mut v);
    v
}

/// Finds a TASK element with the given ID for in-place mutation.
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

/// Domain-level rewrite of every TASK element in place (the same pipeline
/// the save path uses), preserving all surrounding nodes.
fn rewrite_tasks_in_place(elem: &mut XmlElement) {
    if elem.tag == "TASK" {
        let task = read_task(elem);
        write_task(&task, elem);
    }
    for child in &mut elem.children {
        if let XmlNode::Element(c) = child {
            rewrite_tasks_in_place(c);
        }
    }
}

/// parse → read_task/write_task (in place) → serialize → re-parse.
fn domain_round_trip(doc: &XmlDocument) -> (XmlDocument, String) {
    let mut doc2 = doc.clone();
    rewrite_tasks_in_place(&mut doc2.root);
    let bytes = serialize_xml(&doc2);
    let text = String::from_utf8_lossy(&bytes).to_string();
    let doc3 = parse_xml(&bytes).expect("re-parse of domain-rewritten document");
    (doc3, text)
}

fn setup_test_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    moderntodolist_lib::infrastructure::migration::run_migrations(&conn).unwrap();
    conn.execute(
        "INSERT INTO workspaces (id, name, root_path) VALUES ('ws-m6', 'M6', 'C:\\m6')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc-rel', 'ws-m6', 'rel.xml', 'managed')",
        [],
    )
    .unwrap();
    // Parent rows for the relation tables' foreign keys (tasks 1..3 of the
    // relations fixtures).
    for key in ["1", "2", "3"] {
        conn.execute(
            "INSERT INTO task_index (task_key, document_id) VALUES (?1, 'doc-rel')",
            [key],
        )
        .unwrap();
    }
    conn
}

fn make_tree(ids: &[&str]) -> moderntodolist_lib::domain::task::TaskTree {
    let mut tree = moderntodolist_lib::domain::task::TaskTree::new();
    for id in ids {
        let mut t = Task::new(TaskId::new(*id));
        t.title = format!("Task {id}");
        tree.add_task(t);
        tree.add_root_id(TaskId::new(*id));
    }
    tree
}

// ─── QA-M6-001/002: Participants ─────────────────────────────────────

#[test]
fn qa_m6_001_participant_roundtrip_preserves_order() {
    let doc = parse_file(&relations_fixture("participants.xml"));
    let elems = task_elems(&doc);
    let t1 = read_task(&elems[0]);

    // Typed participants mirror ALLOCATEDTO in exact source order and are
    // kept separate from the raw allocated_to strings.
    assert_eq!(t1.allocated_to, vec!["Zoe", "Alice", "Bob"]);
    let names: Vec<&str> = t1
        .participants
        .iter()
        .map(|p| p.display_name.as_str())
        .collect();
    assert_eq!(names, vec!["Zoe", "Alice", "Bob"]);
    assert_eq!(t1.allocated_by.as_deref(), Some("Lead"));

    // Domain round-trip preserves the native attribute byte-for-byte.
    let (doc3, _) = domain_round_trip(&doc);
    let t1b = read_task(&task_elems(&doc3)[0]);
    assert_eq!(t1b.allocated_to, t1.allocated_to);
    assert_eq!(t1b.participants, t1.participants);
    let attr = task_elems(&doc3)[0].get_attr("ALLOCATEDTO").unwrap().to_string();
    assert_eq!(attr, "Zoe; Alice; Bob");

    // Mutations keep both fields in sync and survive the round-trip.
    let mut t = t1.clone();
    assert!(participant::add_participant(&mut t, "Carol").unwrap());
    assert_eq!(t.allocated_to, vec!["Zoe", "Alice", "Bob", "Carol"]);
    assert_eq!(t.participants.len(), 4);
    assert!(participant::remove_participant(&mut t, "Alice"));
    assert_eq!(t.allocated_to, vec!["Zoe", "Bob", "Carol"]);
    assert_eq!(t.participants.len(), 3);

    // Index population (rebuildable derived state).
    let conn = setup_test_db();
    let n = participant::populate_task_participants(&conn, "doc-rel", "1", &t).unwrap();
    assert_eq!(n, 4, "3 allocated_to + 1 allocated_by");
    let zoe: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_participants WHERE participant='Zoe' AND role='allocated_to'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(zoe, 1);
}

#[test]
fn qa_m6_002_participant_unknown_data_preserved() {
    let doc = parse_file(&relations_fixture("participants.xml"));
    let elems = task_elems(&doc);
    let t1 = read_task(&elems[0]);

    // Unknown attribute and unknown element are captured by the domain.
    assert!(t1
        .unknown_attrs
        .iter()
        .any(|(k, v)| k == "CUSTOMATTR" && v == "keepme"));
    assert!(t1.unknown_children.iter().any(|c| c.contains("UNKNOWNEL")));

    // After the full domain round-trip they survive verbatim, along with
    // the comment inside the task.
    let (doc3, text) = domain_round_trip(&doc);
    let t1b = read_task(&task_elems(&doc3)[0]);
    assert!(t1b
        .unknown_attrs
        .iter()
        .any(|(k, v)| k == "CUSTOMATTR" && v == "keepme"));
    assert!(t1b.unknown_children.iter().any(|c| c.contains(r#"UNKNOWNEL"#)));
    assert!(t1b.unknown_children.iter().any(|c| c.contains(r#"FOO="bar""#)));
    assert!(
        text.contains("<!-- legacy comment must survive -->"),
        "task-level comment must survive the domain rewrite"
    );
    // The second task is untouched too.
    let t2 = read_task(&elems[1]);
    let t2b = read_task(&task_elems(&doc3)[1]);
    assert_eq!(t2, t2b);
}

// ─── QA-M6-003…008: Dependencies ─────────────────────────────────────

#[test]
fn qa_m6_003_dependency_roundtrip_preserves_unknown_attrs() {
    let doc = parse_file(&relations_fixture("dependency-external.xml"));
    let elems = task_elems(&doc);
    let t1 = read_task(&elems[0]);
    assert_eq!(t1.dependencies.len(), 2);

    // The enriched DEPENDENCY carries unknown attrs + unknown children.
    let raw = t1.dependencies[1]
        .raw_xml
        .clone()
        .expect("extra dependency data must be captured");
    assert!(raw.contains(r#"LEGACYATTR="keepme""#));
    assert!(raw.contains("<PLUGINNOTE>third-party data</PLUGINNOTE>"));
    // The plain native dependency stays minimal (no bloat).
    assert!(t1.dependencies[0].raw_xml.is_none());

    let (doc3, text) = domain_round_trip(&doc);
    let t1b = read_task(&task_elems(&doc3)[0]);
    assert_eq!(t1b.dependencies.len(), 2);
    assert_eq!(t1b.dependencies[0].task_id, "2");
    assert_eq!(t1b.dependencies[1].task_id, "7");
    assert_eq!(t1b.dependencies[1].dependency_type, 1);
    let raw2 = t1b.dependencies[1].raw_xml.clone().unwrap();
    assert!(raw2.contains(r#"LEGACYATTR="keepme""#));
    assert!(raw2.contains(r#"MTDL_DOCUMENTID="doc-other""#));
    assert!(raw2.contains("<PLUGINNOTE>third-party data</PLUGINNOTE>"));
    assert!(
        text.contains("<!-- dependency comment -->"),
        "comment between dependencies must survive"
    );
    assert_eq!(t1b, t1, "domain-level losslessness for dependencies");
}

#[test]
fn qa_m6_004_dependency_external_and_unresolved_refs() {
    let doc = parse_file(&relations_fixture("dependency-external.xml"));
    let elems = task_elems(&doc);
    let t1 = read_task(&elems[0]);
    let t3 = read_task(&elems[2]);

    // Local ref.
    assert_eq!(
        TaskRef::from_dependency(&t1.dependencies[0]),
        TaskRef::Local(TaskId::new("2"))
    );
    // External ref via DocumentId + TaskId.
    assert_eq!(
        TaskRef::from_dependency(&t1.dependencies[1]),
        TaskRef::External {
            document_id: moderntodolist_lib::domain::types::DocumentId::new("doc-other"),
            task_id: TaskId::new("7"),
        }
    );
    // Unresolved (empty TASKID) is preserved, never dropped.
    assert!(matches!(
        TaskRef::from_dependency(&t3.dependencies[0]),
        TaskRef::Unresolved(_)
    ));

    // External refs survive serialize → parse → read.
    let (doc3, _) = domain_round_trip(&doc);
    let t1b = read_task(&task_elems(&doc3)[0]);
    assert_eq!(
        TaskRef::from_dependency(&t1b.dependencies[1]),
        TaskRef::from_dependency(&t1.dependencies[1])
    );
    let t3b = read_task(&task_elems(&doc3)[2]);
    assert_eq!(t3b.dependencies.len(), 1, "unresolved ref preserved");

    // TaskRef → TaskDependency → TaskRef is stable.
    let ext = TaskRef::External {
        document_id: moderntodolist_lib::domain::types::DocumentId::new("d9"),
        task_id: TaskId::new("3"),
    };
    assert_eq!(TaskRef::from_dependency(&ext.to_dependency(2)), ext);
}

#[test]
fn qa_m6_005_dependency_cycle_rejected() {
    // Graph level: 1→2, 2→3 exist; 3→1 must be rejected with the path.
    let mut g = DependencyGraph::new();
    for id in ["1", "2", "3"] {
        g.add_node(&TaskId::new(id));
    }
    g.add_dependency(&TaskId::new("1"), &TaskId::new("2")).unwrap();
    g.add_dependency(&TaskId::new("2"), &TaskId::new("3")).unwrap();
    let err = g
        .add_dependency(&TaskId::new("3"), &TaskId::new("1"))
        .unwrap_err();
    match &err {
        DependencyError::WouldCreateCycle {
            source_task,
            target_task,
            ..
        } => {
            assert_eq!(source_task, "3");
            assert_eq!(target_task, "1");
        }
        other => panic!("expected cycle, got {other:?}"),
    }
    assert!(err.to_string().contains("1 -> 2 -> 3"));
    // Reverse lookup proves the reverse index is maintained.
    assert_eq!(g.dependents_of(&TaskId::new("2")), &[TaskId::new("1")]);
    assert_eq!(g.dependencies_of(&TaskId::new("2")), &[TaskId::new("3")]);

    // Command level: validated against the live tree.
    let mut tree = make_tree(&["1", "2", "3"]);
    let mut mgr = UndoRedoManager::new();
    let c1 = AddDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("2"), 0).unwrap();
    mgr.execute(Box::new(c1), &mut tree);
    let c2 = AddDependencyCommand::new(&tree, TaskId::new("2"), TaskId::new("3"), 0).unwrap();
    mgr.execute(Box::new(c2), &mut tree);
    assert!(matches!(
        AddDependencyCommand::new(&tree, TaskId::new("3"), TaskId::new("1"), 0),
        Err(DependencyError::WouldCreateCycle { .. })
    ));
    // Longer indirect cycle 3→x→1 style also caught (3→2→1 via new edge 3→2? no—)
    // direct 2-cycle:
    assert!(matches!(
        AddDependencyCommand::new(&tree, TaskId::new("2"), TaskId::new("1"), 0),
        Err(DependencyError::WouldCreateCycle { .. })
    ));
    // Tree state is unchanged by the rejected commands.
    assert_eq!(tree.get(&TaskId::new("3")).unwrap().dependencies.len(), 0);
}

#[test]
fn qa_m6_006_dependency_self_and_missing_rejected() {
    let tree = make_tree(&["1", "2"]);

    // Self-reference rejected at graph level...
    let mut g = DependencyGraph::from_tree(&tree);
    assert_eq!(
        g.add_dependency(&TaskId::new("1"), &TaskId::new("1")),
        Err(DependencyError::SelfReference("1".into()))
    );
    // ...and at command level.
    assert_eq!(
        AddDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("1"), 0).unwrap_err(),
        DependencyError::SelfReference("1".into())
    );
    // Source/target existence validated.
    assert!(matches!(
        AddDependencyCommand::new(&tree, TaskId::new("99"), TaskId::new("1"), 0),
        Err(DependencyError::TaskNotFound(ref s)) if s == "99"
    ));
    assert!(matches!(
        AddDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("99"), 0),
        Err(DependencyError::TaskNotFound(ref s)) if s == "99"
    ));
    // Removing a non-existent edge is an explicit error, not silent.
    assert!(matches!(
        RemoveDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("2")),
        Err(DependencyError::NoSuchDependency { .. })
    ));
    // Adding a duplicate edge is idempotent (no double entries in XML).
    let mut g2 = DependencyGraph::from_tree(&tree);
    assert!(g2.add_dependency(&TaskId::new("1"), &TaskId::new("2")).unwrap());
    assert!(!g2.add_dependency(&TaskId::new("1"), &TaskId::new("2")).unwrap());
    assert_eq!(g2.dependencies_of(&TaskId::new("1")).len(), 1);
}

#[test]
fn qa_m6_007_dependency_undo_redo_and_xml_persistence() {
    let mut tree = make_tree(&["1", "2"]);
    let mut mgr = UndoRedoManager::new();

    let cmd = AddDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("2"), 0).unwrap();
    mgr.execute(Box::new(cmd), &mut tree);
    assert_eq!(tree.get(&TaskId::new("1")).unwrap().dependencies.len(), 1);

    // The mutation is visible in serialized XML (save path).
    let xml_with = serialize_tree_task(&tree, "1");
    assert!(xml_with.contains("<DEPENDENCY"));
    assert!(xml_with.contains("<TASKID>2</TASKID>"));

    mgr.undo(&mut tree);
    assert!(tree.get(&TaskId::new("1")).unwrap().dependencies.is_empty());
    let xml_without = serialize_tree_task(&tree, "1");
    assert!(!xml_without.contains("<DEPENDENCY"));

    mgr.redo(&mut tree);
    assert_eq!(tree.get(&TaskId::new("1")).unwrap().dependencies.len(), 1);

    // Removal command restores the exact dependency (position + payload).
    let rm = RemoveDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("2")).unwrap();
    mgr.execute(Box::new(rm), &mut tree);
    assert!(tree.get(&TaskId::new("1")).unwrap().dependencies.is_empty());
    mgr.undo(&mut tree);
    let deps = &tree.get(&TaskId::new("1")).unwrap().dependencies;
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].task_id, "2");
    assert_eq!(deps[0].dependency_type, 0);
}

/// Serializes one task of the tree through the production write path.
fn serialize_tree_task(
    tree: &moderntodolist_lib::domain::task::TaskTree,
    id: &str,
) -> String {
    let task = tree.get(&TaskId::new(id)).unwrap();
    let mut elem = XmlElement::new("TASK");
    write_task(task, &mut elem);
    let mut root = XmlElement::new("TODOLIST");
    root.children.push(XmlNode::Element(elem));
    let doc = XmlDocument::new(Default::default(), root);
    String::from_utf8_lossy(&serialize_xml(&doc)).to_string()
}

#[test]
fn qa_m6_008_dependency_index_rebuild_forward_reverse() {
    let doc = parse_file(&relations_fixture("dependency-external.xml"));
    let elems = task_elems(&doc);
    let conn = setup_test_db();

    // Populate from an XML scan (rebuildable derived state).
    let mut total = 0;
    for e in &elems {
        let t = read_task(e);
        total +=
            dependency::populate_task_dependencies(&conn, "doc-rel", t.id.as_str(), &t).unwrap();
    }
    assert_eq!(total, 3, "2 deps on task 1 + 1 unresolved on task 3");

    // Forward lookup.
    let fwd: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT depends_on_key, dep_type FROM task_dependencies \
                 WHERE document_id='doc-rel' AND task_key='1' ORDER BY depends_on_key",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(fwd.len(), 2);
    assert!(fwd.iter().any(|(k, ty)| k == "2" && ty == "local"));
    assert!(fwd.iter().any(|(k, ty)| k == "doc-other:7" && ty == "external"));

    // Reverse lookup ("who is blocked by task 2?").
    let rev = dependency::reverse_lookup(&conn, "doc-rel", "2").unwrap();
    assert_eq!(rev, vec![("1".to_string(), "local".to_string())]);

    // Rebuild is idempotent — no duplicates after a second scan.
    for e in &elems {
        let t = read_task(e);
        dependency::populate_task_dependencies(&conn, "doc-rel", t.id.as_str(), &t).unwrap();
    }
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_dependencies WHERE document_id='doc-rel'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 3);
}

// ─── QA-M6-009/010: Progress links ───────────────────────────────────

#[test]
fn qa_m6_009_progress_link_persistence_and_index() {
    let doc = parse_file(&relations_fixture("progress-links.xml"));
    let elems = task_elems(&doc);
    let t1 = read_task(&elems[0]);

    assert_eq!(t1.progress_links.len(), 2);
    assert_eq!(t1.progress_links[0].label, "GitHub PR #12");
    assert_eq!(t1.progress_links[0].provider, LinkProvider::Github);
    assert_eq!(t1.progress_links[1].provider, LinkProvider::Generic);
    // The foreign TOOLATTR stays unknown and is never consumed.
    assert!(t1
        .unknown_attrs
        .iter()
        .any(|(k, v)| k == "TOOLATTR" && v == "legacy-value"));
    assert!(t1
        .unknown_attrs
        .iter()
        .all(|(k, _)| k != progress_link::PROGRESS_LINKS_ATTR));

    // Persistence: domain round-trip keeps links + unknown data + comment.
    let (doc3, text) = domain_round_trip(&doc);
    let t1b = read_task(&task_elems(&doc3)[0]);
    assert_eq!(t1b.progress_links, t1.progress_links);
    assert!(t1b
        .unknown_attrs
        .iter()
        .any(|(k, v)| k == "TOOLATTR" && v == "legacy-value"));
    assert!(text.contains("<!-- progress link comment -->"));
    assert!(text.contains(r#"UNKNOWNEL X="1""#));

    // Index population.
    let conn = setup_test_db();
    let n = progress_link::populate_progress_links_index(&conn, "doc-rel", "1", &t1.progress_links)
        .unwrap();
    assert_eq!(n, 2);
    let gh: String = conn
        .query_row(
            "SELECT provider FROM progress_links_index WHERE id='pl-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(gh, "github");

    // Add/edit/remove flows through the domain helpers.
    let mut t = t1.clone();
    let link = ProgressLink::new_auto_id("Jira", "https://team.atlassian.net/browse/X-2").unwrap();
    assert_eq!(link.provider, LinkProvider::Jira);
    assert!(progress_link::add_progress_link(&mut t, link.clone()));
    assert!(!progress_link::add_progress_link(&mut t, link.clone()));
    assert_eq!(t.progress_links.len(), 3);
    let removed = progress_link::remove_progress_link(&mut t, &link.id).unwrap();
    assert_eq!(removed.url, link.url);
    assert_eq!(t.progress_links.len(), 2);
}

#[test]
fn qa_m6_010_progress_link_url_security() {
    // Allowed.
    assert!(progress_link::validate_url("https://github.com/a/b").is_ok());
    assert!(progress_link::validate_url("http://internal.corp:8080/x").is_ok());
    assert!(progress_link::validate_url("HTTPS://EXAMPLE.COM/UPPER").is_ok());

    // Dangerous schemes — every one must be rejected.
    let attacks = [
        "javascript:alert(1)",
        "JavaScript:alert(document.cookie)",
        "JAVASCRIPT:alert(1)",
        "jAvAsCrIpT:alert(1)",
        "java\tscript:alert(1)",
        "java\nscript:alert(1)",
        "java\rscript:alert(1)",
        "\tjavascript:alert(1)\t",
        "javascript\u{0}:alert(1)",
        "javascript\u{7f}:x",
        "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
        "DATA:text/html,<script>alert(1)</script>",
        "data:,hello",
        "vbscript:msgbox(1)",
        "file:///C:/Windows/system32/cmd.exe",
        "ftp://example.com/payload",
        "about:blank",
        "blob:https://example.com/uuid",
        "http://exa mple.com",
        "https://example.com/a b",
    ];
    for attack in attacks {
        assert!(
            progress_link::validate_url(attack).is_err(),
            "MUST REJECT: {:?}",
            attack
        );
    }

    // Scheme look-alikes.
    assert_eq!(
        progress_link::validate_url("httpsecure://x.com"),
        Err(UrlValidationError::UnsupportedScheme("httpsecure".into()))
    );
    assert_eq!(
        progress_link::validate_url("//example.com/no-scheme"),
        Err(UrlValidationError::MissingScheme)
    );

    // Stored JSON containing a dangerous URL is refused on parse, so a
    // hand-edited XML attribute can never smuggle javascript: into a Task.
    assert!(progress_link::links_from_attr_value(
        r#"[{"id":"x","label":"l","url":"javascript:alert(1)"}]"#
    )
    .is_none());

    // ProgressLink::new enforces the same control.
    assert!(ProgressLink::new("id", "lbl", "javascript:alert(1)").is_err());
    assert!(ProgressLink::new("id", "lbl", "java\tscript:alert(1)").is_err());
    // URL attachments share the control.
    assert!(attachment::url_attachment("javascript:alert(1)", None).is_err());
}

// ─── QA-M6-011…017: Attachments ──────────────────────────────────────

fn temp_workspace(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::TempDir::new().unwrap_or_else(|e| panic!("{tag}: {e}"));
    let doc_dir = dir.path().join("docs");
    fs::create_dir_all(&doc_dir).unwrap();
    (dir, doc_dir)
}

#[test]
fn qa_m6_011_attachment_managed_import_lifecycle() {
    let (_tmp, doc_dir) = temp_workspace("lifecycle");
    let source = doc_dir.parent().unwrap().join("design doc v2.pdf");
    let payload = b"managed attachment payload \xff".to_vec();
    fs::write(&source, &payload).unwrap();
    let src_hash_before = attachment::blake3_file(&source).unwrap();

    // Transactional import.
    let att =
        attachment::import_managed_attachment(&source, &doc_dir, "doc-life", None).unwrap();
    assert_eq!(att.kind, attachment::AttachmentKind::ManagedFile);
    assert_eq!(att.display_name, "design doc v2.pdf");
    assert_eq!(att.size, Some(payload.len() as u64));
    assert_eq!(att.hash.as_deref(), Some(src_hash_before.as_str()));
    assert!(att.path_or_url.starts_with(".assets/doc-life/attachments/"));
    assert!(att.path_or_url.ends_with(".pdf"));

    // Asset root policy: file lives exactly in .assets/<doc>/attachments/.
    let final_path = attachment::resolve_path(&doc_dir, &att).unwrap();
    assert_eq!(
        final_path.parent().unwrap(),
        attachment::asset_root(&doc_dir, "doc-life")
    );
    assert!(final_path.exists());
    assert_eq!(fs::read(&final_path).unwrap(), payload);
    // Source never modified.
    assert_eq!(attachment::blake3_file(&source).unwrap(), src_hash_before);
    // No staging leftovers.
    let leftovers = fs::read_dir(final_path.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".staging-"))
        .count();
    assert_eq!(leftovers, 0);
    // Integrity verification.
    assert!(attachment::verify_integrity(&doc_dir, &att).unwrap());

    // XML persistence: attach → write → parse → read keeps everything.
    let mut task = Task::new(TaskId::new("1"));
    assert!(attachment::add_attachment(&mut task, att.clone()));
    assert_eq!(task.file_links.len(), 1);
    let xml = serialize_tree_task(
        &{
            let mut tree = make_tree(&[]);
            tree.add_task(task.clone());
            tree
        },
        "1",
    );
    assert!(xml.contains("<FILEREFPATH>"));
    assert!(xml.contains("MTDL_ATTACHMENTS"));
    let wrapped = format!("<TODOLIST>{}</TODOLIST>", xml);
    let doc = parse_xml(wrapped.as_bytes()).unwrap();
    let back = read_task(&task_elems(&doc)[0]);
    assert_eq!(back.attachments, vec![att]);
    assert_eq!(back.file_links.len(), 1);
}

#[test]
fn qa_m6_012_attachment_import_rollback_paths() {
    let (_tmp, doc_dir) = temp_workspace("rollback");
    let source = doc_dir.parent().unwrap().join("payload.bin");
    fs::write(&source, b"critical data").unwrap();
    let src_hash = attachment::blake3_file(&source).unwrap();

    // Rollback path 1: hash verification failure (corrupted transfer).
    let err =
        attachment::import_managed_attachment(&source, &doc_dir, "doc-rb", Some("00ff00ff"))
            .unwrap_err();
    assert!(matches!(
        err,
        attachment::AttachmentError::HashMismatch { .. }
    ));
    let root = attachment::asset_root(&doc_dir, "doc-rb");
    let entries: Vec<_> = fs::read_dir(&root).unwrap().collect();
    assert!(
        entries.is_empty(),
        "failed import must leave NO staging or partial file, found {}",
        entries.len()
    );
    // Source untouched.
    assert_eq!(attachment::blake3_file(&source).unwrap(), src_hash);

    // Rollback path 2: source is not a regular file.
    let err2 =
        attachment::import_managed_attachment(&doc_dir, &doc_dir, "doc-rb", None).unwrap_err();
    assert!(matches!(
        err2,
        attachment::AttachmentError::SourceNotFile(_)
    ));

    // Rollback path 3: missing source.
    let err3 = attachment::import_managed_attachment(
        &doc_dir.join("never-exists.bin"),
        &doc_dir,
        "doc-rb",
        None,
    )
    .unwrap_err();
    assert!(matches!(
        err3,
        attachment::AttachmentError::SourceNotFound(_)
    ));

    // Rollback path 4: asset root cannot be created (blocked by a file).
    let blocker_doc = doc_dir.parent().unwrap().join("blocked");
    fs::create_dir_all(&blocker_doc).unwrap();
    let src2 = blocker_doc.join("s.txt");
    fs::write(&src2, b"x").unwrap();
    let assets = blocker_doc.join(attachment::ASSETS_DIR);
    fs::write(&assets, b"not a directory").unwrap();
    let err4 =
        attachment::import_managed_attachment(&src2, &blocker_doc, "doc-b", None).unwrap_err();
    assert!(matches!(err4, attachment::AttachmentError::Io(_)));
    assert_eq!(fs::read(&src2).unwrap(), b"x");

    // A later successful import still works after all the failures.
    let ok = attachment::import_managed_attachment(&source, &doc_dir, "doc-rb", None).unwrap();
    assert!(attachment::resolve_path(&doc_dir, &ok).unwrap().exists());
}

#[test]
fn qa_m6_013_attachment_path_safety() {
    let root = Path::new("D:/lists");
    // Legal joins.
    assert!(attachment::safe_join(root, ".assets/doc/attachments/f.pdf").is_ok());
    assert!(attachment::safe_join(root, "./rel/f.txt").is_ok());
    // Traversal attacks.
    for bad in [
        "../secret.txt",
        "..\\secret.txt",
        ".assets/../../secret.txt",
        ".assets\\..\\..\\windows\\win.ini",
        "a/b/../../../c",
    ] {
        assert!(
            matches!(
                attachment::safe_join(root, bad),
                Err(attachment::AttachmentError::PathTraversal(_))
            ),
            "must reject traversal: {bad}"
        );
    }
    // Absolute / rooted paths.
    for bad in ["C:/x", "C:\\x", "/etc/passwd", "\\\\server\\share"] {
        assert!(
            matches!(
                attachment::safe_join(root, bad),
                Err(attachment::AttachmentError::AbsolutePath(_))
            ),
            "must reject absolute: {bad}"
        );
    }
    // Control characters.
    assert!(attachment::safe_join(root, "a\u{0}b.txt").is_err());

    // Collision-safe naming: unique, extension-preserving.
    let n1 = attachment::collision_safe_name("my report.final(1).docx");
    let n2 = attachment::collision_safe_name("my report.final(1).docx");
    assert_ne!(n1, n2);
    assert!(n1.ends_with(".docx"));
    assert!(!n1.contains(' '));

    // Managed refs always resolve INSIDE the asset root.
    let (_tmp, doc_dir) = temp_workspace("safety");
    let evil = attachment::AttachmentRef {
        id: "e".into(),
        kind: attachment::AttachmentKind::ManagedFile,
        display_name: "evil".into(),
        path_or_url: "../../../Windows/system32/x.dll".into(),
        size: None,
        hash: None,
    };
    assert!(matches!(
        attachment::resolve_path(&doc_dir, &evil),
        Err(attachment::AttachmentError::PathTraversal(_))
    ));
}

#[test]
fn qa_m6_014_attachment_linked_and_url_references() {
    let (_tmp, doc_dir) = temp_workspace("linked");
    let external = doc_dir.parent().unwrap().join("external.xlsx");
    fs::write(&external, b"spreadsheet bytes").unwrap();

    // Linked file: reference only — NO copy anywhere.
    let linked = attachment::link_local_file(external.to_str().unwrap()).unwrap();
    assert_eq!(linked.kind, attachment::AttachmentKind::LinkedFile);
    assert_eq!(linked.display_name, "external.xlsx");
    assert_eq!(linked.size, Some(17));
    assert!(linked.hash.is_some());
    let assets_parent = doc_dir.parent().unwrap();
    assert!(
        !assets_parent.join(attachment::ASSETS_DIR).exists(),
        "linking must not create the asset root or copy anything"
    );

    // Missing linked file: allowed, size/hash unknown (UI shows missing state).
    let missing = attachment::link_local_file("D:/gone/file.txt").unwrap();
    assert!(missing.size.is_none() && missing.hash.is_none());
    // Invalid references rejected.
    assert!(attachment::link_local_file("   ").is_err());
    assert!(attachment::link_local_file("bad\u{0}path.txt").is_err());

    // URL attachment: scheme validated, stored as native FileLink.
    let url = attachment::url_attachment("https://example.com/spec.pdf", Some("Spec")).unwrap();
    assert_eq!(url.kind, attachment::AttachmentKind::Url);
    assert_eq!(url.display_name, "Spec");
    assert!(attachment::url_attachment("javascript:alert(1)", None).is_err());
    assert!(attachment::url_attachment("data:text/html,x", None).is_err());
    assert!(attachment::url_attachment("file:///C:/x", None).is_err());

    // Both round-trip through the task XML.
    let mut task = Task::new(TaskId::new("4"));
    attachment::add_attachment(&mut task, linked.clone());
    attachment::add_attachment(&mut task, url.clone());
    assert_eq!(task.file_links.len(), 2);
    let mut elem = XmlElement::new("TASK");
    write_task(&task, &mut elem);
    let back = read_task(&elem);
    assert_eq!(back.attachments, vec![linked, url]);
    assert_eq!(back.file_links.len(), 2);
}

#[test]
fn qa_m6_015_attachments_index_rebuildable_from_xml() {
    let conn = setup_test_db();
    let xml_bytes = fs::read(relations_fixture("attachments-managed.xml")).unwrap();

    let n = attachment::rebuild_index_from_xml(&conn, "doc-rel", &xml_bytes).unwrap();
    // 2 typed (managed + url) + 1 legacy FILEREFPATH without metadata.
    assert_eq!(n, 3);

    let types: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT file_path, attachment_type FROM attachments_index \
                 WHERE document_id='doc-rel' ORDER BY attachment_type, file_path",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert!(types.iter().any(|(p, t)| t == "managed" && p.contains(".assets/doc-rel/attachments/")));
    assert!(types.iter().any(|(p, t)| t == "url" && p == "https://example.com/spec"));
    assert!(types.iter().any(|(p, t)| t == "linked" && p.contains("pre-m6-file.docx")));

    // Managed row carries BLAKE3 metadata from the XML attribute.
    let fp: String = conn
        .query_row(
            "SELECT fingerprint FROM attachments_index WHERE attachment_type='managed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fp, "aaaa");

    // Rebuild after wiping is identical (disposable derived state).
    conn.execute("DELETE FROM attachments_index", []).unwrap();
    let n2 = attachment::rebuild_index_from_xml(&conn, "doc-rel", &xml_bytes).unwrap();
    assert_eq!(n2, 3);
    // Deterministic synthesized ids: stable across rebuilds.
    let n3 = attachment::rebuild_index_from_xml(&conn, "doc-rel", &xml_bytes).unwrap();
    assert_eq!(n3, 3);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM attachments_index", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3, "rebuild must not duplicate rows");
}

#[test]
fn qa_m6_016_attachment_survives_document_relocation() {
    let (_tmp, doc_dir) = temp_workspace("relocate");
    let source = doc_dir.parent().unwrap().join("movable.bin");
    fs::write(&source, b"relocatable payload").unwrap();
    let att =
        attachment::import_managed_attachment(&source, &doc_dir, "doc-mv", None).unwrap();
    assert!(attachment::verify_integrity(&doc_dir, &att).unwrap());

    // Managed refs are document-relative: relocating the whole document
    // directory keeps them resolvable and intact (portable-app scenario:
    // drive letter / folder change).
    let new_dir = doc_dir.parent().unwrap().join("relocated-docs");
    fs::rename(&doc_dir, &new_dir).unwrap();
    let resolved = attachment::resolve_path(&new_dir, &att).unwrap();
    assert!(resolved.exists());
    assert!(resolved.starts_with(&new_dir));
    assert!(attachment::verify_integrity(&new_dir, &att).unwrap());
    // The old location is gone — nothing absolute was persisted.
    assert!(!attachment::resolve_path(&doc_dir, &att).unwrap().exists());
}

#[test]
fn qa_m6_017_attachment_removal_orphans_and_integrity() {
    let (_tmp, doc_dir) = temp_workspace("orphans");
    let source = doc_dir.parent().unwrap().join("spec.md");
    fs::write(&source, b"# spec").unwrap();

    let mut task = Task::new(TaskId::new("6"));
    let managed =
        attachment::import_managed_attachment(&source, &doc_dir, "doc-or", None).unwrap();
    let linked = attachment::link_local_file(source.to_str().unwrap()).unwrap();
    let url = attachment::url_attachment("https://example.com/x", None).unwrap();
    attachment::add_attachment(&mut task, managed.clone());
    attachment::add_attachment(&mut task, linked.clone());
    attachment::add_attachment(&mut task, url.clone());
    assert_eq!(task.file_links.len(), 3);

    // Linked removal: reference only — file untouched, no orphan record.
    let r = attachment::remove_attachment_tracked(&mut task, &doc_dir, "doc-or", &linked.id)
        .unwrap()
        .unwrap();
    assert_eq!(r.kind, attachment::AttachmentKind::LinkedFile);
    assert!(source.exists());
    assert!(attachment::read_orphans(&doc_dir, "doc-or").is_empty());

    // URL removal: reference only.
    let r = attachment::remove_attachment_tracked(&mut task, &doc_dir, "doc-or", &url.id)
        .unwrap()
        .unwrap();
    assert_eq!(r.kind, attachment::AttachmentKind::Url);
    assert!(attachment::read_orphans(&doc_dir, "doc-or").is_empty());

    // Managed removal: orphan CANDIDATE — file kept, manifest written.
    attachment::remove_attachment_tracked(&mut task, &doc_dir, "doc-or", &managed.id)
        .unwrap()
        .unwrap();
    assert_eq!(task.file_links.len(), 0);
    let orphans = attachment::read_orphans(&doc_dir, "doc-or");
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].id, managed.id);
    assert_eq!(orphans[0].hash, managed.hash);
    let managed_path = attachment::resolve_path(&doc_dir, &managed).unwrap();
    assert!(managed_path.exists(), "orphan candidate not deleted yet");
    assert!(attachment::orphans_manifest_path(&doc_dir, "doc-or").ends_with(
        format!(".assets{sep}doc-or{sep}orphans.manifest", sep = std::path::MAIN_SEPARATOR)
    ));

    // GC with integrity verification: a tampered file is KEPT.
    fs::write(&managed_path, b"tampered by user").unwrap();
    let report = attachment::gc_orphans(&doc_dir, "doc-or", true).unwrap();
    assert_eq!(report.integrity_failed, 1);
    assert_eq!(report.deleted, 0);
    assert!(managed_path.exists());
    assert_eq!(attachment::read_orphans(&doc_dir, "doc-or").len(), 1);

    // Restore the original bytes → hash matches → GC deletes it.
    fs::write(&managed_path, b"# spec").unwrap();
    let report = attachment::gc_orphans(&doc_dir, "doc-or", true).unwrap();
    assert_eq!(report.deleted, 1);
    assert!(!managed_path.exists());
    assert!(attachment::read_orphans(&doc_dir, "doc-or").is_empty());
    // Source file was never touched by any of this.
    assert_eq!(fs::read(&source).unwrap(), b"# spec");
}

// ─── QA-M6-018: Cumulative regression ────────────────────────────────

#[test]
fn qa_m6_018_cumulative_regression_m0_m5_with_m6_fields() {
    // 1. EVERY valid fixture (M0–M5 corpus + new M6 relations fixtures)
    //    survives the full domain rewrite pipeline losslessly: unknown
    //    attrs, unknown children, dependencies, comments, participants.
    let mut checked_files = 0;
    let mut checked_tasks = 0usize;
    for group in [
        "canonical",
        "comments",
        "attachments",
        "dependencies",
        "encoding",
        "real-world",
        "unknown",
        "relations",
    ] {
        let dir = fixtures_root().join(group);
        let mut files: Vec<_> = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "xml").unwrap_or(false))
            .collect();
        files.sort();
        for path in files {
            let doc1 = parse_file(&path);
            let tasks1: Vec<Task> = task_elems(&doc1).iter().map(read_task).collect();
            let (doc3, _text) = domain_round_trip(&doc1);
            let tasks3: Vec<Task> = task_elems(&doc3).iter().map(read_task).collect();
            assert_eq!(
                tasks1.len(),
                tasks3.len(),
                "{}: task count changed",
                path.display()
            );
            for (a, b) in tasks1.iter().zip(tasks3.iter()) {
                assert_eq!(a, b, "{}: task {} lost data", path.display(), a.id);
            }
            checked_files += 1;
            checked_tasks += tasks1.len();
        }
    }
    assert!(checked_files >= 20, "corpus should include all fixture groups");
    assert!(checked_tasks > 0);

    // 2. With M6 fields POPULATED on a legacy fixture, all pre-existing
    //    unknown data still survives (compatibility invariant under M6).
    let doc = parse_file(&fixtures_root().join("unknown").join("unknown-attribute.xml"));
    let mut doc2 = doc.clone();
    let target_id = task_elems(&doc2)[0]
        .get_attr("ID")
        .unwrap()
        .to_string();
    {
        let el = find_task_elem_mut(&mut doc2.root, &target_id)
            .expect("target task must exist in unknown-attribute.xml");
        let mut task = read_task(el);
        participant::add_participant(&mut task, " Regina ").unwrap();
        progress_link::add_progress_link(
            &mut task,
            ProgressLink::new_auto_id("CI", "https://github.com/o/r/actions/1").unwrap(),
        );
        let att =
            attachment::url_attachment("https://example.com/file", Some("F")).unwrap();
        attachment::add_attachment(&mut task, att);
        write_task(&task, el);
    }
    let bytes = serialize_xml(&doc2);
    let doc3 = parse_xml(&bytes).unwrap();
    let el3 = task_elems(&doc3);
    let t3 = read_task(&el3[0]);

    // M6 data present...
    assert_eq!(t3.allocated_to, vec!["Regina"]);
    assert_eq!(t3.participants.len(), 1);
    assert_eq!(t3.progress_links.len(), 1);
    assert_eq!(t3.progress_links[0].provider, LinkProvider::Github);
    assert_eq!(t3.attachments.len(), 1);
    assert_eq!(t3.file_links.len(), 1);
    // Attribute-level checks (encoding-safe: this fixture serializes as
    // UTF-16, so raw-text inspection would be lossy).
    assert!(el3[0]
        .get_attr(progress_link::PROGRESS_LINKS_ATTR)
        .is_some());
    assert!(el3[0].get_attr(attachment::ATTACHMENTS_ATTR).is_some());
    // ...and the legacy unknown attribute from the fixture is untouched.
    let doc1_tasks: Vec<Task> = task_elems(&doc).iter().map(read_task).collect();
    for (k, v) in &doc1_tasks[0].unknown_attrs {
        assert!(
            t3.unknown_attrs.iter().any(|(k2, v2)| k2 == k && v2 == v),
            "unknown attr {k}={v} lost when M6 fields populated"
        );
    }
    for child in &doc1_tasks[0].unknown_children {
        assert!(
            t3.unknown_children.contains(child),
            "unknown child lost when M6 fields populated: {child}"
        );
    }

    // 3. Documents WITHOUT M6 data gain no new attributes on rewrite
    //    (attribute-level check: encoding-safe for UTF-16 fixtures).
    let plain = parse_file(&fixtures_root().join("canonical").join("single-task.xml"));
    let (plain3, _) = domain_round_trip(&plain);
    for el in task_elems(&plain3) {
        assert!(el.get_attr(progress_link::PROGRESS_LINKS_ATTR).is_none());
        assert!(el.get_attr(attachment::ATTACHMENTS_ATTR).is_none());
    }
}
