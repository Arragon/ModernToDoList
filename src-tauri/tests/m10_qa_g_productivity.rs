//! M10 RC Regression Matrix G — Productivity
//! (QA-M10 G / INH-1130, spec section 5.5).
//!
//! 10 cases (G01..G10): global search (FTS5), the Chinese/CJK LIKE fallback,
//! smart views, saved views, quick add and the command palette — exercised
//! TOGETHER against the real public API: the M4 index pipeline, the real M6
//! index populators (attachments/progress links feed the M9 search extras
//! seam), the bridge `*_core` mutation path, the M3 atomic save and the M9
//! search/view/palette services.
//!
//! Test corpus (written into a sandbox, both documents registered in one
//! workspace so every cross-document path is live):
//!
//! ```text
//! doc-a.xml (6 tasks)                      doc-b.xml (3 tasks)
//! 1 Buy milk and eggs    p7 Alice #shopping  1 审查依赖关系 p6 张三 #评审
//!   + url attachment receipt.pdf               comment: 任务依赖图需要复查
//!   comment: …organic milk…                  2 Review transfer plan p8 Alice due 03-14
//! 2 购买清单：牛奶和鸡蛋 p5 due 03-13 #购物    3 Archive old lists p4 100% done 03-09
//!   comment: …带上购物清单…(one CJK run)
//! 3 Write release notes p8 start 01-15 Bob
//!   + github progress link
//!   comment: 汇总待办事项和任务统计 (one run)
//! 4 Overdue report p9 due 01-09
//! 5 Finished draft p3 100% due/completed 45444
//! 6 Someday maybe p2
//! ```
//!
//! NOTE (documented product bug E-F2, pinned in matrix E): `search::strip_html`
//! confuses char and byte indices, so HTML comments containing CJK panic the
//! search extractor. All comments in this corpus are PLAIN_TEXT (the TDL
//! default), which takes the as-is extraction path.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use rusqlite::Connection;

use moderntodolist_lib::bridge::build_tree_from_bytes;
use moderntodolist_lib::domain::attachment;
use moderntodolist_lib::domain::command::UndoRedoManager;
use moderntodolist_lib::domain::mappers::{read_task, write_task};
use moderntodolist_lib::domain::participant;
use moderntodolist_lib::domain::persistence::{atomic_save, SaveConfig};
use moderntodolist_lib::domain::progress_link;
use moderntodolist_lib::domain::quick_add::{build_task, parse_quick_add_with_now};
use moderntodolist_lib::domain::search::{
    command_registry, find_shortcut_conflicts, fuzzy_score, query_needs_cjk_fallback,
    resolve_ancestors, search_commands, MatchedField, SearchQuery, MAX_PAGE_SIZE,
};
use moderntodolist_lib::domain::session::{DocumentSession, SaveState};
use moderntodolist_lib::domain::smart_view::{
    self, by_tag_view, completed_view, naive_date_to_ole, overdue_view, today_view, upcoming_view,
    ViewTask,
};
use moderntodolist_lib::domain::task::TaskTree;
use moderntodolist_lib::domain::types::TaskId;
use moderntodolist_lib::domain::xml_tree::XmlNode;
use moderntodolist_lib::domain::{parse_xml, serialize_xml, XmlElement};
use moderntodolist_lib::infrastructure::saved_views::{
    self, PredicateField, PredicateNode, PredicateOperator, ViewPredicate,
};
use moderntodolist_lib::infrastructure::search_fts::{
    self, fts5_available, rebuild_search_index, IndexTableExtrasProvider,
};
use moderntodolist_lib::infrastructure::{indexer, migration, DatabaseError, DatabaseManager};
use moderntodolist_lib::relations::add_participant_core;
use moderntodolist_lib::task_edit::{
    delete_task_core, quick_add_core, update_task_field_core, QuickAddRequest,
};

// ── Corpus ───────────────────────────────────────────────────────────────────

fn doc_a_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<TDL PROJECTNAME="RC G Doc A" FILENAME="doc-a.xml" NEXTUNIQUEID="7">
    <TASK ID="1" TITLE="Buy milk and eggs" PRIORITY="7" ALLOCATEDTO="Alice" {att_attr}="[{att_json}]">
        <CATEGORY>shopping</CATEGORY>
        <COMMENTS>Get the organic milk from the corner store</COMMENTS>
        <FILEREFPATH>https://example.com/receipt.pdf</FILEREFPATH>
    </TASK>
    <TASK ID="2" TITLE="购买清单：牛奶和鸡蛋" PRIORITY="5" DUEDATE="45364">
        <CATEGORY>购物</CATEGORY>
        <COMMENTS>周末去超市购买带上购物清单顺便核对家里的储备</COMMENTS>
    </TASK>
    <TASK ID="3" TITLE="Write release notes" PRIORITY="8" PERCENTDONE="0" STARTDATE="45306" ALLOCATEDTO="Bob" {pl_attr}="[{pl_json}]">
        <COMMENTS>汇总待办事项和任务统计</COMMENTS>
    </TASK>
    <TASK ID="4" TITLE="Overdue report" PRIORITY="9" DUEDATE="45300"/>
    <TASK ID="5" TITLE="Finished draft" PRIORITY="3" PERCENTDONE="100" DUEDATE="45444" COMPLETIONDATE="45444"/>
    <TASK ID="6" TITLE="Someday maybe" PRIORITY="2"/>
</TDL>"#,
        att_attr = attachment::ATTACHMENTS_ATTR,
        att_json = r#"{"id":"att-a1","kind":"url","display_name":"receipt.pdf","path_or_url":"https://example.com/receipt.pdf","size":null,"hash":null}"#
            .replace('"', "&quot;"),
        pl_attr = progress_link::PROGRESS_LINKS_ATTR,
        pl_json = r#"{"id":"pl-a1","label":"Release PR","url":"https://github.com/org/repo/pull/42"}"#
            .replace('"', "&quot;"),
    )
}

fn doc_b_xml() -> String {
    r#"<?xml version="1.0" encoding="utf-8"?>
<TDL PROJECTNAME="RC G Doc B" FILENAME="doc-b.xml" NEXTUNIQUEID="4">
    <TASK ID="1" TITLE="审查依赖关系" PRIORITY="6" ALLOCATEDTO="张三">
        <CATEGORY>评审</CATEGORY>
        <COMMENTS>任务依赖图需要复查</COMMENTS>
    </TASK>
    <TASK ID="2" TITLE="Review transfer plan" PRIORITY="8" ALLOCATEDTO="Alice" DUEDATE="45365"/>
    <TASK ID="3" TITLE="Archive old lists" PRIORITY="4" PERCENTDONE="100" COMPLETIONDATE="45360"/>
</TDL>"#
        .to_string()
}

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mtdl_rc_g_{}_{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

struct Corpus {
    dir: PathBuf,
    doc_a: PathBuf,
    doc_b: PathBuf,
}

/// Writes both documents and returns their paths.
fn write_corpus(name: &str) -> Corpus {
    let dir = sandbox(name);
    let doc_a = dir.join("doc-a.xml");
    let doc_b = dir.join("doc-b.xml");
    fs::write(&doc_a, doc_a_xml()).unwrap();
    fs::write(&doc_b, doc_b_xml()).unwrap();
    Corpus { dir, doc_a, doc_b }
}

fn register(conn: &Connection, doc_id: &str, path: &Path) {
    conn.execute(
        "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES (?1,'ws-g',?2,'managed')",
        rusqlite::params![doc_id, path.to_string_lossy().as_ref()],
    )
    .unwrap();
}

/// The full production-derived-state pipeline over both documents:
/// core index (M4), M6 side tables rebuilt from XML through the REAL domain
/// populators, and the M9 search index.
fn index_everything(conn: &Connection, c: &Corpus) {
    conn.execute("INSERT OR IGNORE INTO workspaces (id, name, root_path) VALUES ('ws-g','G','.')" , [])
        .unwrap();
    register(conn, "doc-a", &c.doc_a);
    register(conn, "doc-b", &c.doc_b);
    index_document_both(conn, c);
    rebuild_side_tables(conn, c);
    rebuild_search_both(conn, c);
}

fn index_document_both(conn: &Connection, c: &Corpus) {
    indexer::index_document(conn, "doc-a", &c.doc_a).unwrap();
    indexer::index_document(conn, "doc-b", &c.doc_b).unwrap();
}

/// M6-owned side tables rebuilt from the saved XML through the real domain
/// functions (attachments scan + progress-link populator).
fn rebuild_side_tables(conn: &Connection, c: &Corpus) {
    attachment::rebuild_index_from_xml(conn, "doc-a", &fs::read(&c.doc_a).unwrap()).unwrap();
    attachment::rebuild_index_from_xml(conn, "doc-b", &fs::read(&c.doc_b).unwrap()).unwrap();
    for (doc_id, path) in [("doc-a", &c.doc_a), ("doc-b", &c.doc_b)] {
        let tree = build_tree_from_bytes(&fs::read(path).unwrap()).unwrap();
        for t in tree.iter() {
            progress_link::populate_progress_links_index(conn, doc_id, t.id.as_str(), &t.progress_links)
                .unwrap();
            participant::populate_task_participants(conn, doc_id, t.id.as_str(), t).unwrap();
        }
    }
}

fn rebuild_search_both(conn: &Connection, c: &Corpus) {
    rebuild_search_index(
        conn,
        &[
            ("doc-a".to_string(), c.doc_a.clone()),
            ("doc-b".to_string(), c.doc_b.clone()),
        ],
        &IndexTableExtrasProvider,
    )
    .unwrap();
}

fn setup(name: &str) -> (Corpus, Connection) {
    let c = write_corpus(name);
    let conn = Connection::open_in_memory().unwrap();
    migration::run_migrations(&conn).unwrap();
    index_everything(&conn, &c);
    (c, conn)
}

fn search(conn: &Connection, q: &str) -> moderntodolist_lib::domain::search::SearchPage {
    search_fts::search(conn, &SearchQuery::new(q.to_string())).unwrap()
}

fn keys(conn: &Connection, q: &str) -> Vec<(String, String)> {
    let page = search(conn, q);
    page.results
        .iter()
        .map(|r| (r.document_context.document_id.clone(), r.task_key.task_id.as_str().to_string()))
        .collect()
}

/// Loads view projections from the REAL index, joining the M6-owned tag and
/// participant tables (task keys are document-qualified per ViewTask docs).
fn load_view_tasks(conn: &Connection) -> Vec<ViewTask> {
    let mut stmt = conn
        .prepare(
            "SELECT task_key, document_id, title, status, percent_done, priority, \
                    start_date, due_date, completed_date FROM task_index \
             ORDER BY document_id, task_key",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)? as u8,
                row.get::<_, i32>(5)? as u8,
                smart_view::parse_index_date(row.get::<_, Option<String>>(6)?.as_deref()),
                smart_view::parse_index_date(row.get::<_, Option<String>>(7)?.as_deref()),
                smart_view::parse_index_date(row.get::<_, Option<String>>(8)?.as_deref()),
            ))
        })
        .unwrap();
    let mut tasks: Vec<ViewTask> = Vec::new();
    for row in rows {
        let (task_id, document_id, title, status, percent_done, priority, start_date, due_date, completed_date) =
            row.unwrap();
        let strings = |sql: &str| -> Vec<String> {
            let mut s = conn.prepare(sql).unwrap();
            s.query_map(
                rusqlite::params![document_id.as_str(), task_id.as_str()],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
        };
        tasks.push(ViewTask {
            task_key: format!("{document_id}:{task_id}"),
            document_id: document_id.clone(),
            task_id: task_id.clone(),
            title,
            status,
            percent_done,
            priority,
            start_date,
            due_date,
            completed_date,
            tags: strings("SELECT tag FROM task_tags WHERE document_id=?1 AND task_key=?2 ORDER BY tag"),
            participants: strings(
                "SELECT participant FROM task_participants \
                 WHERE document_id=?1 AND task_key=?2 AND role='allocated_to' ORDER BY participant",
            ),
            flagged: false,
        });
    }
    tasks
}

/// Serializes a whole tree back to a TDL document (roots + nested children
/// through the public write_task mapper), the same shape the session save
/// path produces.
fn serialize_tree(tree: &TaskTree, next_unique_id: u64) -> Vec<u8> {
    fn rec(tree: &TaskTree, id: &TaskId) -> XmlElement {
        let task = tree.get(id).expect("child id must resolve");
        let mut el = XmlElement::new("TASK");
        write_task(task, &mut el);
        for cid in &task.children {
            let child = rec(tree, cid);
            el.children.push(XmlNode::Element(child));
        }
        el
    }
    let mut root = XmlElement::new("TDL");
    root.set_attr("NEXTUNIQUEID", next_unique_id.to_string());
    for rid in tree.root_ids() {
        let el = rec(tree, rid);
        root.children.push(XmlNode::Element(el));
    }
    let doc = moderntodolist_lib::domain::XmlDocument::new(
        moderntodolist_lib::domain::encoding::XmlEncodingMeta::default_utf8(),
        root,
    );
    serialize_xml(&doc)
}

fn save(path: &Path, bytes: &[u8]) {
    let result = atomic_save(
        &SaveConfig { target_path: path.to_path_buf(), validate_temp: true },
        bytes,
        |b| parse_xml(b).is_ok(),
    );
    assert_eq!(result.state, SaveState::Completed, "save failed: {:?}", result.error);
}

fn next_id(tree: &TaskTree) -> u64 {
    tree.iter()
        .filter_map(|t| t.id.as_str().parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

// ═══════════════ G01: global search across two documents through real M6 seams ═══════════════

/// QA-M10-G01: FTS5 search over a two-document workspace where the expanded
/// searchable text comes from the REAL M6 populators (attachment index scan
/// + progress-link index), not hand-seeded SQL: titles, descriptions,
/// attachment display names, link provider/URL and participants are all
/// findable with correct document context, matched field and snippets.
#[test]
fn qa_m10_g01_global_search_two_docs_through_real_m6_seams() {
    let (_c, conn) = setup("g01");
    assert!(fts5_available(&conn), "bundled rusqlite must provide FTS5");

    // Title hit with snippet + document context.
    let page = search(&conn, "milk");
    assert_eq!(page.backend, "fts5");
    let hit = page
        .results
        .iter()
        .find(|r| r.task_key.task_id.as_str() == "1" && r.document_context.document_id == "doc-a")
        .expect("doc-a task 1 must match 'milk'");
    assert_eq!(hit.matched_field, MatchedField::Title);
    assert!(hit.snippet.contains("[[milk]]"), "snippet: {}", hit.snippet);
    assert_eq!(hit.document_context.document_name, "doc-a.xml");
    assert!(hit.score > 0.0);

    // Attachment display name — populated by attachment::rebuild_index_from_xml.
    let page = search(&conn, "receipt.pdf");
    assert_eq!(page.results.len(), 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "1");
    assert_eq!(page.results[0].matched_field, MatchedField::Attachments);

    // Progress-link provider/URL — populated by populate_progress_links_index.
    let page = search(&conn, "github");
    assert_eq!(page.results.len(), 1);
    assert_eq!(page.results[0].document_context.document_id, "doc-a");
    assert_eq!(page.results[0].task_key.task_id.as_str(), "3");
    assert_eq!(page.results[0].matched_field, MatchedField::Links);

    // Participants resolve across BOTH documents.
    let mut alice = keys(&conn, "Alice");
    alice.sort();
    assert_eq!(alice, vec![("doc-a".into(), "1".into()), ("doc-b".into(), "2".into())]);

    // Description hit and document filter.
    assert_eq!(keys(&conn, "corner"), vec![("doc-a".into(), "1".into())]);
    let only_b = search_fts::search(&conn, &SearchQuery::new("Alice").with_document("doc-b")).unwrap();
    assert_eq!(only_b.total, 1);
    assert_eq!(only_b.results[0].document_context.document_id, "doc-b");

    // No false positives.
    assert_eq!(search(&conn, "zzz_no_such_term").total, 0);
}

// ═══════════════ G02: Chinese search — LIKE fallback where FTS5 is blind ═══════════════

/// QA-M10-G02: the CJK requirement, proven against the real stores: plain
/// FTS5 MATCH returns ZERO rows for 任务 / 待办事项 / 购物清单 (unicode61
/// cannot segment Han runs), while the production search path degrades to
/// the LIKE fallback and returns the correct cross-document hits, marked
/// snippets and rebuild resilience.
#[test]
fn qa_m10_g02_chinese_search_fallback_where_fts5_is_blind() {
    let (_c, conn) = setup("g02");

    // Hard requirement: plain FTS5 finds NONE of these substrings.
    for term in ["任务", "待办事项", "购物清单"] {
        let fts_hits: i64 = conn
            .query_row(
                &format!("SELECT count(*) FROM task_search_fts WHERE task_search_fts MATCH '\"{term}\"'"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_hits, 0, "unicode61 must not segment '{term}' — plain FTS5 has to be blind here");
        assert!(query_needs_cjk_fallback(term));
    }

    // The production path falls back to LIKE and DOES find them.
    let page = search(&conn, "任务");
    assert_eq!(page.backend, "like-fallback");
    let mut hits = keys(&conn, "任务");
    hits.sort();
    assert_eq!(
        hits,
        vec![("doc-a".into(), "3".into()), ("doc-b".into(), "1".into())],
        "任务 appears in both documents' descriptions"
    );
    assert_eq!(keys(&conn, "待办事项"), vec![("doc-a".into(), "3".into())]);
    assert_eq!(keys(&conn, "购物清单"), vec![("doc-a".into(), "2".into())]);

    // Mixed CJK + Latin query: AND semantics across fields.
    let page = search(&conn, "release 待办事项");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "3");

    // CJK title substring + marked snippet.
    assert_eq!(keys(&conn, "依赖"), vec![("doc-b".into(), "1".into())]);
    let page = search(&conn, "牛奶");
    let hit = page.results.iter().find(|r| r.task_key.task_id.as_str() == "2").unwrap();
    assert!(hit.snippet.contains("[[牛奶]]"), "snippet: {}", hit.snippet);

    // The fallback survives a full index rebuild from the XML scan.
    search_fts::clear_search_index(&conn).unwrap();
    assert_eq!(search(&conn, "任务").total, 0);
    let c = Corpus {
        dir: PathBuf::new(),
        doc_a: doc_path(&conn, "doc-a"),
        doc_b: doc_path(&conn, "doc-b"),
    };
    rebuild_search_both(&conn, &c);
    assert!(search(&conn, "任务").total >= 2, "CJK fallback survives rebuild");
    assert_eq!(keys(&conn, "购物清单"), vec![("doc-a".into(), "2".into())]);
}

fn doc_path(conn: &Connection, doc_id: &str) -> PathBuf {
    PathBuf::from(
        conn.query_row("SELECT file_path FROM documents WHERE id=?1", [doc_id], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
    )
}

// ═══════════════ G03: smart views from the real index, non-mutating ═══════════════

/// QA-M10-G03: every smart view evaluated from the REAL index rows (with M6
/// tags/participants joined in): date boundaries with an injected "now",
/// grouping views, and the guarantee that view evaluation mutates nothing.
#[test]
fn qa_m10_g03_smart_views_boundaries_groupings_nonmutating() {
    let (_c, conn) = setup("g03");
    let tasks = load_view_tasks(&conn);
    assert_eq!(tasks.len(), 9);
    let snapshot = tasks.clone();
    let count_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get(0))
        .unwrap();

    // 2024-03-13 (Wednesday): doc-a:2 due today; doc-a:3 started 2024-01-15.
    let today = d(2024, 3, 13);
    let t2 = tasks.iter().find(|t| t.task_key == "doc-a:2").unwrap();
    assert_eq!(t2.due_date, Some(today), "OLE 45364 must decode to 2024-03-13");

    let today_hits = today_view(&tasks, today);
    let tk: Vec<&str> = today_hits.iter().map(|t| t.task_key.as_str()).collect();
    assert!(tk.contains(&"doc-a:2") && tk.contains(&"doc-a:3"), "today: {tk:?}");
    assert!(!tk.contains(&"doc-b:2"), "due tomorrow is not today");
    // Evaluated one day later, the boundary moves.
    assert!(!today_view(&tasks, d(2024, 3, 14)).iter().any(|t| t.task_key == "doc-a:2"));

    // Upcoming buckets.
    let groups = upcoming_view(&tasks, today);
    assert_eq!(groups.today.iter().map(|t| &t.task_key).collect::<Vec<_>>(), vec!["doc-a:2"]);
    assert_eq!(groups.tomorrow.iter().map(|t| &t.task_key).collect::<Vec<_>>(), vec!["doc-b:2"]);
    assert_eq!(groups.total(), 2, "past-due and completed tasks stay out of Upcoming");

    // Overdue: strictly due < today and active.
    let od: Vec<&str> = overdue_view(&tasks, today).iter().map(|t| t.task_key.as_str()).collect();
    assert_eq!(od, vec!["doc-a:4"]);

    // Completed across documents, sorted by completion date desc.
    let comp: Vec<String> = completed_view(&tasks).into_iter().map(|t| t.task_key).collect();
    assert_eq!(comp, vec!["doc-a:5", "doc-b:3"]);

    // Flagged: priority >= 7, priority desc then key.
    let fl: Vec<String> = smart_view::flagged_view(&tasks).into_iter().map(|t| t.task_key).collect();
    assert_eq!(fl, vec!["doc-a:4", "doc-a:3", "doc-b:2", "doc-a:1"]);

    // Unscheduled: active without start/due dates.
    let un: Vec<&str> = smart_view::unscheduled_view(&tasks).iter().map(|t| t.task_key.as_str()).collect();
    assert_eq!(un, vec!["doc-a:1", "doc-a:6", "doc-b:1"]);

    // Grouping views use the REAL M6 participant/tag tables.
    let by_p = smart_view::by_participant_view(&tasks);
    let alice = by_p.iter().find(|g| g.name == "Alice").unwrap();
    assert_eq!(alice.tasks.iter().map(|t| &t.task_key).collect::<Vec<_>>(), vec!["doc-a:1", "doc-b:2"]);
    assert!(by_p.iter().find(|g| g.name == "张三").unwrap().tasks.iter().any(|t| t.task_key == "doc-b:1"));
    assert!(by_p.iter().any(|g| g.name == smart_view::UNASSIGNED_GROUP));
    let by_t = by_tag_view(&tasks);
    assert_eq!(by_t.iter().find(|g| g.name == "shopping").unwrap().tasks[0].task_key, "doc-a:1");
    assert_eq!(by_t.iter().find(|g| g.name == "购物").unwrap().tasks[0].task_key, "doc-a:2");
    assert_eq!(by_t.iter().find(|g| g.name == "评审").unwrap().tasks[0].task_key, "doc-b:1");

    // Non-mutating: identical projections and untouched index.
    assert_eq!(tasks, snapshot, "view evaluation must not mutate task data");
    let count_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_before, count_after);
}

// ═══════════════ G04: saved views persist, survive restart, SQL ≡ memory ═══════════════

/// QA-M10-G04: saved views over the file-backed index database: versioned
/// predicate serialization, SQL evaluation agreeing with the in-memory
/// evaluator on every row, restart survival with Chinese names, rename /
/// reorder / delete, parameterized SQL (no inlined values) and read-only
/// evaluation.
#[test]
fn qa_m10_g04_saved_views_persist_restart_sql_memory_agree() {
    let c = write_corpus("g04");
    let db_path = c.dir.join("views.db");
    let node = PredicateNode::all_of(vec![
        PredicateNode::Leaf(ViewPredicate::new(
            PredicateField::Priority,
            PredicateOperator::GreaterOrEqual,
            serde_json::json!(7),
        )),
        PredicateNode::Leaf(ViewPredicate::new(
            PredicateField::Status,
            PredicateOperator::NotEquals,
            serde_json::json!("Done"),
        )),
    ]);

    let view_id;
    {
        let conn = Connection::open(&db_path).unwrap();
        migration::run_migrations(&conn).unwrap();
        index_everything(&conn, &c);

        let view = saved_views::create_view(&conn, "ws-g", "重要未完成", &node).unwrap();
        view_id = view.id.clone();
        let rows = saved_views::evaluate(&conn, "ws-g", &node).unwrap();
        let got: Vec<(String, String)> = rows.iter().map(|r| (r.document_id.clone(), r.task_key.clone())).collect();
        assert_eq!(
            got,
            vec![
                ("doc-a".into(), "1".into()),
                ("doc-a".into(), "3".into()),
                ("doc-a".into(), "4".into()),
                ("doc-b".into(), "2".into())
            ]
        );

        // The in-memory evaluator agrees with SQL on EVERY indexed row.
        let all = saved_views::evaluate(&conn, "ws-g", &PredicateNode::And(vec![])).unwrap();
        assert_eq!(all.len(), 9);
        for row in &all {
            assert_eq!(
                node.matches(row),
                got.contains(&(row.document_id.clone(), row.task_key.clone())),
                "memory/SQL disagreement on {}:{}",
                row.document_id,
                row.task_key
            );
        }

        // Evaluation is read-only.
        let title: String = conn
            .query_row("SELECT title FROM task_index WHERE document_id='doc-a' AND task_key='1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "Buy milk and eggs");
    }

    // Simulated restart: a brand-new connection on the same file.
    {
        let conn = Connection::open(&db_path).unwrap();
        let views = saved_views::list_views(&conn, "ws-g").unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].name, "重要未完成", "Chinese name survives restart");
        assert_eq!(views[0].predicates, node);
        assert_eq!(saved_views::evaluate(&conn, "ws-g", &views[0].predicates).unwrap().len(), 4);

        // Versioned payload round-trip + forward-compat guard.
        let json = saved_views::serialize_predicates(&node).unwrap();
        assert!(json.contains("\"version\":1"));
        assert_eq!(saved_views::deserialize_predicates(&json).unwrap(), node);
        assert!(saved_views::deserialize_predicates(
            r#"{"version":42,"root":{"field":"title","operator":"equals","value":"x"}}"#
        )
        .is_err());

        // Rename / reorder / delete.
        let v2 = saved_views::create_view(&conn, "ws-g", "全部", &PredicateNode::And(vec![])).unwrap();
        saved_views::rename_view(&conn, &view_id, "重命名视图").unwrap();
        assert_eq!(saved_views::get_view(&conn, &view_id).unwrap().unwrap().name, "重命名视图");
        saved_views::reorder_views(&conn, &[v2.id.clone(), view_id.clone()]).unwrap();
        let order: Vec<String> = saved_views::list_views(&conn, "ws-g").unwrap().into_iter().map(|v| v.id).collect();
        assert_eq!(order, vec![v2.id.clone(), view_id.clone()]);
        saved_views::delete_view(&conn, &v2.id).unwrap();
        assert_eq!(saved_views::list_views(&conn, "ws-g").unwrap().len(), 1);

        // Compiled SQL is parameterized: values are bound, never inlined.
        let filter = PredicateNode::Leaf(ViewPredicate::new(
            PredicateField::Title,
            PredicateOperator::Contains,
            serde_json::json!("milk; DROP TABLE task_index"),
        ));
        let (sql, params) = saved_views::compile_sql(&filter).unwrap();
        assert!(!params.is_empty());
        assert!(!sql.contains("DROP TABLE"), "values must be bound, not inlined: {sql}");
    }
    fs::remove_dir_all(&c.dir).ok();
}

// ═══════════════ G05: quick add reaches every productivity surface ═══════════════

/// QA-M10-G05: the quick-add grammar parsed with an injected "now", inserted
/// through the real bridge command, saved through the M3 atomic pipeline and
/// re-indexed — the new tasks (English AND Chinese) become searchable, show
/// up in the Today smart view, match a saved-view predicate, carry their
/// display-date strings into the XML, and are removed again by undo.
#[test]
fn qa_m10_g05_quick_add_flows_through_save_index_search_and_views() {
    let c = write_corpus("g05");
    let conn = Connection::open_in_memory().unwrap();
    migration::run_migrations(&conn).unwrap();
    index_everything(&conn, &c);

    // Grammar parse (frontend side of the seam) with an injected now.
    let now = d(2024, 3, 10);
    let draft = parse_quick_add_with_now("Buy oat milk #shopping @Alice !high due:2024-03-13", now);
    assert_eq!(draft.title, "Buy oat milk");
    assert_eq!(draft.tags, vec!["shopping"]);
    assert_eq!(draft.participants, vec!["Alice"]);
    assert_eq!(draft.priority, Some(7));
    assert_eq!(draft.due_date, Some(d(2024, 3, 13)));
    let cjk = parse_quick_add_with_now("买燕麦奶 #购物 @张三", now);
    assert_eq!(cjk.title, "买燕麦奶");

    // Insert through the real bridge command against the live tree.
    let mut tree = build_tree_from_bytes(&fs::read(&c.doc_a).unwrap()).unwrap();
    let s = DocumentSession::new(None);
    let mut u = UndoRedoManager::new();
    let resp = quick_add_core(
        &s,
        &mut tree,
        &mut u,
        &QuickAddRequest {
            document_id: Some("doc-a".into()),
            parent_key: None,
            title: draft.title.clone(),
            tags: draft.tags.clone(),
            participants: draft.participants.clone(),
            priority: draft.priority.unwrap_or(5) as i64,
            start_date: None,
            due_date: Some("2024-03-13".into()),
        },
    )
    .unwrap();
    assert_eq!(resp.task_key, "7", "id allocated as max(existing)+1");
    let resp2 = quick_add_core(
        &s,
        &mut tree,
        &mut u,
        &QuickAddRequest {
            document_id: Some("doc-a".into()),
            parent_key: None,
            title: cjk.title.clone(),
            tags: cjk.tags.clone(),
            participants: cjk.participants.clone(),
            priority: 5,
            start_date: None,
            due_date: None,
        },
    )
    .unwrap();
    assert_eq!(resp2.task_key, "8");
    let t7 = tree.get(&TaskId::new("7")).unwrap();
    assert_eq!(t7.due_date, Some(naive_date_to_ole(d(2024, 3, 13))));
    assert_eq!(t7.allocated_to, vec!["Alice".to_string()]);
    assert_eq!(t7.categories[0].name, "shopping");

    // Save through the M3 pipeline, then rebuild the derived state from XML.
    save(&c.doc_a, &serialize_tree(&tree, next_id(&tree)));
    index_document_both(&conn, &c);
    rebuild_side_tables(&conn, &c);
    rebuild_search_both(&conn, &c);

    // Searchable: English via FTS5, Chinese via the LIKE fallback.
    let page = search(&conn, "oat");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "7");
    let page = search(&conn, "燕麦");
    assert_eq!(page.backend, "like-fallback");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "8");
    // The participant added by quick add is searchable too.
    assert!(keys(&conn, "Alice").contains(&("doc-a".into(), "7".into())));

    // The saved XML carries the display-date string (TDL dual-date contract).
    let saved = parse_xml(&fs::read(&c.doc_a).unwrap()).unwrap();
    let mut t7_el: Option<&XmlElement> = None;
    fn find<'a>(el: &'a XmlElement, t: &mut Option<&'a XmlElement>) {
        if el.tag == "TASK" && el.get_attr("ID") == Some("7") {
            *t = Some(el);
        }
        for c in el.child_elements() {
            find(c, t);
        }
    }
    find(&saved.root, &mut t7_el);
    let t7_saved = read_task(t7_el.unwrap());
    assert_eq!(t7_saved.due_date_string.as_deref(), Some("2024-03-13"));

    // Today smart view (due == today) and a saved-view tag predicate both
    // pick the quick-added task up.
    let tasks = load_view_tasks(&conn);
    assert!(today_view(&tasks, d(2024, 3, 13)).iter().any(|t| t.task_key == "doc-a:7"));
    let tag_filter = PredicateNode::all_of(vec![
        PredicateNode::Leaf(ViewPredicate::new(
            PredicateField::Tag,
            PredicateOperator::Equals,
            serde_json::json!("shopping"),
        )),
    ]);
    let rows = saved_views::evaluate(&conn, "ws-g", &tag_filter).unwrap();
    assert!(rows.iter().any(|r| r.document_id == "doc-a" && r.task_key == "7"));
    assert!(rows.iter().any(|r| r.document_id == "doc-a" && r.task_key == "1"));

    // Undo removes both quick-added tasks (bridge-level reversibility).
    assert!(u.undo(&mut tree).is_some());
    assert!(u.undo(&mut tree).is_some());
    assert_eq!(tree.len(), 6);
    assert!(!tree.contains(&TaskId::new("7")) && !tree.contains(&TaskId::new("8")));
}

// ═══════════════ G06: command palette registry, fuzzy search and task jump ═══════════════

/// QA-M10-G06: the palette's backend contracts — a canonical registry with
/// unique ids and no shortcut conflicts, fuzzy matching over commands and
/// task titles, and jump resolution that returns the true ancestor chain
/// from real fixture documents (nested transfer subtree + rc index source).
#[test]
fn qa_m10_g06_command_palette_registry_fuzzy_and_task_jump() {
    // Registry integrity.
    let registry = command_registry();
    assert!(!registry.is_empty());
    let mut ids: Vec<&str> = registry.iter().map(|c| c.id).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n, "command ids must be unique");
    assert!(registry.iter().all(|c| !c.label.trim().is_empty() && !c.category.trim().is_empty()));
    assert!(find_shortcut_conflicts(&registry).is_empty(), "canonical registry must be conflict-free");
    assert!(registry.iter().any(|c| c.id == "search.open" && c.shortcut == Some("Ctrl+K")));

    // Fuzzy command search (palette ranking).
    assert_eq!(search_commands("undo", 5)[0].command.id, "edit.undo");
    assert_eq!(search_commands("today", 5)[0].command.id, "view.today");
    assert!(search_commands("zzzzqqq", 5).is_empty());

    // Fuzzy over task titles (subsequence, negative case).
    assert!(fuzzy_score("wrn", "Write release notes").is_some());
    assert!(fuzzy_score("xyz", "Write release notes").is_none());

    // Jump resolution against REAL nested documents.
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("xml");
    let transfer_src = fs::read(fx.join("transfer").join("source-project.xml")).unwrap();
    assert_eq!(resolve_ancestors(&transfer_src, "7"), Some(vec!["5".to_string(), "6".to_string()]));
    assert_eq!(resolve_ancestors(&transfer_src, "6"), Some(vec!["5".to_string()]));
    assert_eq!(resolve_ancestors(&transfer_src, "5"), Some(vec![]));
    assert_eq!(resolve_ancestors(&transfer_src, "999"), None, "unknown task → explicit not-found");
    let rc_src = fs::read(fx.join("rc").join("index-source.xml")).unwrap();
    assert_eq!(resolve_ancestors(&rc_src, "3"), Some(vec!["1".to_string()]));
    assert_eq!(resolve_ancestors(&rc_src, "4"), Some(vec![]));
}

// ═══════════════ G07: pagination bounded, deterministic, mixed EN+CJK ═══════════════

/// QA-M10-G07: result pages are bounded, non-overlapping, deterministically
/// ordered across repeated queries (both the FTS5 and the CJK LIKE-fallback
/// backend), report the true total independent of the page, and clamp to
/// MAX_PAGE_SIZE.
#[test]
fn qa_m10_g07_pagination_bounded_deterministic_mixed_corpus() {
    let dir = sandbox("g07");
    let path = dir.join("many.xml");
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<TDL>\n");
    for i in 0..40 {
        xml.push_str(&format!("  <TASK ID=\"{i}\" TITLE=\"common keyword item {i}\"/>\n"));
    }
    for i in 0..5 {
        xml.push_str(&format!("  <TASK ID=\"c{i}\" TITLE=\"共同关键词项目{i}\"/>\n"));
    }
    xml.push_str("</TDL>");
    fs::write(&path, xml).unwrap();

    let conn = Connection::open_in_memory().unwrap();
    migration::run_migrations(&conn).unwrap();
    conn.execute("INSERT INTO workspaces (id, name, root_path) VALUES ('ws-g','w','.')", []).unwrap();
    register(&conn, "doc-many", &path);
    rebuild_search_index(&conn, &[("doc-many".to_string(), path.clone())], &IndexTableExtrasProvider).unwrap();

    // FTS5 backend.
    let p1 = search_fts::search(&conn, &SearchQuery::new("common").with_page(10, 0)).unwrap();
    let p2 = search_fts::search(&conn, &SearchQuery::new("common").with_page(10, 10)).unwrap();
    assert_eq!(p1.total, 40);
    assert_eq!(p2.total, 40, "total is page-independent");
    assert_eq!(p1.results.len(), 10);
    assert!(!p1.results.iter().any(|r| p2.results.iter().any(|o| o.task_key == r.task_key)), "pages must not overlap");
    let again = search_fts::search(&conn, &SearchQuery::new("common").with_page(10, 0)).unwrap();
    assert_eq!(p1.results, again.results, "ordering is deterministic");
    let huge = search_fts::search(&conn, &SearchQuery::new("common").with_page(99999, 0)).unwrap();
    assert!(huge.results.len() <= MAX_PAGE_SIZE, "hard page-size bound");
    let past = search_fts::search(&conn, &SearchQuery::new("common").with_page(10, 40)).unwrap();
    assert!(past.results.is_empty() && past.total == 40, "offset past the end is empty, not an error");

    // CJK LIKE-fallback backend: same contract.
    let c1 = search_fts::search(&conn, &SearchQuery::new("共同关键词").with_page(3, 0)).unwrap();
    let c2 = search_fts::search(&conn, &SearchQuery::new("共同关键词").with_page(3, 3)).unwrap();
    assert_eq!(c1.backend, "like-fallback");
    assert_eq!(c1.total, 5);
    assert_eq!(c1.results.len(), 3);
    assert_eq!(c2.results.len(), 2);
    assert!(!c1.results.iter().any(|r| c2.results.iter().any(|o| o.task_key == r.task_key)));
    let c_again = search_fts::search(&conn, &SearchQuery::new("共同关键词").with_page(3, 0)).unwrap();
    assert_eq!(c1.results, c_again.results, "fallback ordering is deterministic");

    fs::remove_dir_all(&dir).ok();
}

// ═══════════════ G08: incremental index updates track bridge mutations ═══════════════

/// QA-M10-G08: mutations driven through the real bridge cores stay visible
/// in search via incremental updates (title edit, participant add, task
/// delete touch BOTH search stores consistently), and a full rebuild from
/// the saved XML afterwards produces identical results — incremental and
/// rebuild paths agree.
#[test]
fn qa_m10_g08_incremental_updates_agree_with_full_rebuild() {
    let c = setup("g08").0;
    let conn = Connection::open_in_memory().unwrap();
    migration::run_migrations(&conn).unwrap();
    index_everything(&conn, &c);
    let provider = IndexTableExtrasProvider;

    assert_eq!(search(&conn, "eggs").total, 1);
    let mut tree = build_tree_from_bytes(&fs::read(&c.doc_a).unwrap()).unwrap();
    let s = DocumentSession::new(None);
    let mut u = UndoRedoManager::new();

    // Title edit through the bridge → incremental search update.
    update_task_field_core(&s, &mut tree, &mut u, "1", "title", "Buy bread only").unwrap();
    search_fts::on_task_mutated(&conn, "doc-a", tree.get(&TaskId::new("1")).unwrap(), &provider).unwrap();
    assert_eq!(search(&conn, "eggs").total, 0, "stale title term must be gone");
    let page = search(&conn, "bread");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].matched_field, MatchedField::Title);

    // Participant add through the bridge → M6 table + incremental update.
    add_participant_core(&s, &mut tree, &mut u, "1", "Dana", "allocated_to").unwrap();
    participant::populate_task_participants(&conn, "doc-a", "1", tree.get(&TaskId::new("1")).unwrap()).unwrap();
    search_fts::on_task_mutated(&conn, "doc-a", tree.get(&TaskId::new("1")).unwrap(), &provider).unwrap();
    let page = search(&conn, "Dana");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].matched_field, MatchedField::Participants);

    // Delete through the bridge → both search stores drop the task.
    delete_task_core(&s, &mut tree, &mut u, "6").unwrap();
    search_fts::remove_task_search(&conn, "doc-a", "6").unwrap();
    assert_eq!(search(&conn, "Someday").total, 0);
    let fts_left: i64 = conn
        .query_row("SELECT count(*) FROM task_search_fts WHERE document_id='doc-a' AND task_id='6'", [], |r| r.get(0))
        .unwrap();
    let plain_left: i64 = conn
        .query_row("SELECT count(*) FROM task_search_plain WHERE document_id='doc-a' AND task_id='6'", [], |r| r.get(0))
        .unwrap();
    assert_eq!((fts_left, plain_left), (0, 0), "both stores updated together");

    // Persist the mutated tree, then rebuild everything from the XML scan:
    // the incremental state and the rebuild state must agree.
    save(&c.doc_a, &serialize_tree(&tree, next_id(&tree)));
    let probes = ["bread", "Dana", "eggs", "Someday", "milk"];
    let before: Vec<(String, usize)> = probes.iter().map(|q| (q.to_string(), search(&conn, q).total)).collect();
    index_document_both(&conn, &c);
    rebuild_side_tables(&conn, &c);
    rebuild_search_both(&conn, &c);
    let after: Vec<(String, usize)> = probes.iter().map(|q| (q.to_string(), search(&conn, q).total)).collect();
    assert_eq!(before, after, "incremental updates ≡ full rebuild from XML");
    assert_eq!(search(&conn, "eggs").total, 0);
    assert_eq!(search(&conn, "bread").total, 1);
}

// ═══════════════ G09: cross-document views and search context ═══════════════

/// QA-M10-G09: saved-view evaluation, smart views and search results all
/// carry correct per-document context in one workspace: participant
/// predicates span documents, DocumentId predicates scope to one, and every
/// search hit names its document.
#[test]
fn qa_m10_g09_cross_document_views_and_search_context() {
    let (_c, conn) = setup("g09");

    // Participant predicate spans both documents.
    let alice = PredicateNode::Leaf(ViewPredicate::new(
        PredicateField::Participant,
        PredicateOperator::Equals,
        serde_json::json!("Alice"),
    ));
    let rows = saved_views::evaluate(&conn, "ws-g", &alice).unwrap();
    let got: Vec<(String, String)> = rows.iter().map(|r| (r.document_id.clone(), r.task_key.clone())).collect();
    assert_eq!(got, vec![("doc-a".into(), "1".into()), ("doc-b".into(), "2".into())]);
    assert!(rows.iter().all(|r| r.participants.contains(&"Alice".to_string())));

    // CJK participant name works through the same path.
    let zhang = PredicateNode::Leaf(ViewPredicate::new(
        PredicateField::Participant,
        PredicateOperator::Equals,
        serde_json::json!("张三"),
    ));
    let rows = saved_views::evaluate(&conn, "ws-g", &zhang).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].document_id.as_str(), rows[0].task_key.as_str()), ("doc-b", "1"));

    // DocumentId predicate scopes to a single document.
    let only_b = PredicateNode::Leaf(ViewPredicate::new(
        PredicateField::DocumentId,
        PredicateOperator::Equals,
        serde_json::json!("doc-b"),
    ));
    assert_eq!(saved_views::evaluate(&conn, "ws-g", &only_b).unwrap().len(), 3);

    // Smart views group across documents with the right context.
    let tasks = load_view_tasks(&conn);
    let alice_group = smart_view::by_participant_view(&tasks)
        .into_iter()
        .find(|g| g.name == "Alice")
        .unwrap();
    let docs: Vec<&str> = alice_group.tasks.iter().map(|t| t.document_id.as_str()).collect();
    assert_eq!(docs, vec!["doc-a", "doc-b"]);
    let comp: Vec<String> = completed_view(&tasks).into_iter().map(|t| t.task_key).collect();
    assert_eq!(comp, vec!["doc-a:5", "doc-b:3"], "completed sorted across documents");

    // Search hits name their document.
    let page = search(&conn, "transfer");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].document_context.document_id, "doc-b");
    assert_eq!(page.results[0].document_context.document_name, "doc-b.xml");
    assert_eq!(keys(&conn, "依赖"), vec![("doc-b".into(), "1".into())]);
}

// ═══════════════ G10: the whole productivity stack survives index deletion ═══════════════

/// QA-M10-G10: RC disposability proof on the FILE-backed DatabaseManager —
/// quick-add a task through the bridge, save it, verify every productivity
/// surface works, delete the index database outright, rebuild from the XML
/// scan alone (core index + M6 side tables + search) and verify identical
/// behaviour, including the quick-added task.
#[test]
fn qa_m10_g10_index_disposable_full_productivity_rebuild() {
    let c = write_corpus("g10");
    let data_dir = c.dir.join("Data");
    fs::create_dir_all(&data_dir).unwrap();

    // Quick-add through the bridge and persist (so the rebuild must find it).
    let mut tree = build_tree_from_bytes(&fs::read(&c.doc_a).unwrap()).unwrap();
    let s = DocumentSession::new(None);
    let mut u = UndoRedoManager::new();
    quick_add_core(
        &s,
        &mut tree,
        &mut u,
        &QuickAddRequest {
            document_id: Some("doc-a".into()),
            parent_key: None,
            title: "Persisted quick task".into(),
            tags: vec!["rc".into()],
            participants: vec!["Erin".into()],
            priority: 6,
            start_date: None,
            due_date: None,
        },
    )
    .unwrap();
    save(&c.doc_a, &serialize_tree(&tree, next_id(&tree)));

    let mut db = DatabaseManager::open(&data_dir);
    assert!(db.is_available());

    let build = |db: &DatabaseManager, c: &Corpus| {
        db.with_connection(|conn| {
            conn.execute("INSERT OR IGNORE INTO workspaces (id, name, root_path) VALUES ('ws-g','G','.')", [])
                .map_err(DatabaseError::Sqlite)?;
            conn.execute("DELETE FROM documents WHERE workspace_id='ws-g'", [])
                .map_err(DatabaseError::Sqlite)?;
            register(conn, "doc-a", &c.doc_a);
            register(conn, "doc-b", &c.doc_b);
            index_document_both(conn, c);
            rebuild_side_tables(conn, c);
            rebuild_search_both(conn, c);
            Ok(())
        })
        .unwrap();
    };
    build(&db, &c);

    // One closure pinning EVERY productivity surface; it must hold before
    // the wipe and identically after the rebuild.
    let assert_all_work = |conn: &Connection| {
        // M5: query rows (6 + 1 quick-added + 3).
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 10);
        // M9 search: English FTS5 + CJK fallback.
        assert_eq!(search(conn, "milk").total, 1);
        assert_eq!(search(conn, "Persisted quick").total, 1, "quick-added task is searchable");
        let cjk = search(conn, "购物清单");
        assert_eq!(cjk.backend, "like-fallback");
        assert_eq!(cjk.total, 1);
        // M9 smart views.
        let tasks = load_view_tasks(conn);
        assert_eq!(tasks.len(), 10);
        assert!(!today_view(&tasks, d(2024, 3, 13)).is_empty());
        assert_eq!(completed_view(&tasks).len(), 2);
        // M9 saved views.
        let node = PredicateNode::Leaf(ViewPredicate::new(
            PredicateField::Tag,
            PredicateOperator::Equals,
            serde_json::json!("rc"),
        ));
        let rows = saved_views::evaluate(conn, "ws-g", &node).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task_key, "7");
        // M9 quick add + palette.
        let draft = parse_quick_add_with_now("Another #t @p !high", d(2024, 3, 13));
        assert_eq!(build_task(&draft, TaskId::new("900")).title, "Another");
        assert_eq!(search_commands("undo", 5)[0].command.id, "edit.undo");
        let bytes = fs::read(doc_path(conn, "doc-a")).unwrap();
        assert_eq!(resolve_ancestors(&bytes, "7"), Some(vec![]));
    };

    db.with_connection(|conn| {
        assert_all_work(conn);
        Ok(())
    })
    .unwrap();

    // DISPOSABILITY: delete the whole index database, rebuild from the XML
    // scan alone — every surface must behave identically.
    db.delete_database().unwrap();
    assert!(!db.is_available());
    assert!(!data_dir.join("index.db").exists());
    db.reopen().unwrap();
    assert!(db.is_available());
    db.with_connection(|conn| {
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "fresh database starts empty");
        Ok(())
    })
    .unwrap();

    build(&db, &c);
    db.with_connection(|conn| {
        assert_all_work(conn);
        Ok(())
    })
    .unwrap();

    fs::remove_dir_all(&c.dir).ok();
}
