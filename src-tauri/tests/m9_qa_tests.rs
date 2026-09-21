//! M9 QA Integration Tests — Productivity
//!
//! QA-M9-001 ~ QA-M9-016 (INH-1105 ~ INH-1108).
//!
//! Coverage:
//! - QA-M9-001~005: global search correctness, Chinese fallback, incremental
//!   updates, pagination determinism, index rebuild (disposability).
//! - QA-M9-006~010: smart/saved view date boundaries, non-mutating
//!   persistence, SQL/memory evaluator agreement, restart survival.
//! - QA-M9-011~015: quick add safe fallback, command shortcut conflicts,
//!   task jump resolution, quick-add task building, canonical-filter
//!   regression.
//! - QA-M9-016: M0-M8 cumulative regression with productivity features
//!   enabled, including delete-index → rebuild → everything still works.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use rusqlite::Connection;

use moderntodolist_lib::domain::quick_add::{build_task, parse_quick_add_with_now};
use moderntodolist_lib::domain::search::{
    find_shortcut_conflicts, fuzzy_score, query_needs_cjk_fallback, resolve_ancestors,
    search_commands, MatchedField, SearchQuery, CommandDescriptor,
};
use moderntodolist_lib::domain::smart_view::{
    self, completed_view, naive_date_to_ole, overdue_view, today_view, upcoming_view, ViewTask,
};
use moderntodolist_lib::domain::types::TaskId;
use moderntodolist_lib::domain::xml_parser::parse_xml;
use moderntodolist_lib::domain::xml_serializer::serialize_xml;
use moderntodolist_lib::infrastructure::saved_views::{
    self, PredicateField, PredicateNode, PredicateOperator, ViewPredicate,
};
use moderntodolist_lib::infrastructure::search_fts::{
    self, fts5_available, rebuild_search_index, SearchBackend, SearchDocument, TaskSearchExtras,
};
use moderntodolist_lib::infrastructure::{indexer, migration};

// ── Fixture helpers ──────────────────────────────────────────────────────────

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mtdl_qa_m9_{}_{}", tag, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The M9 QA fixture document: English + Chinese titles, descriptions,
/// tags, participants, an attachment and a progress link.
fn write_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("m9_fixture.xml");
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="utf-8"?>
<TDL PROJECTNAME="M9 QA">
    <TASK ID="1" TITLE="Buy milk and eggs" PRIORITY="7" ALLOCATEDTO="Alice">
        <CATEGORY>shopping</CATEGORY>
        <COMMENTS>Get the organic milk from the corner store</COMMENTS>
    </TASK>
    <TASK ID="2" TITLE="购买清单：牛奶和鸡蛋" PRIORITY="5" DUEDATE="45364">
        <CATEGORY>购物</CATEGORY>
        <COMMENTS>周末去超市购买，顺便带上购物清单</COMMENTS>
    </TASK>
    <TASK ID="3" TITLE="Write release notes" PRIORITY="8" PERCENTDONE="0" STARTDATE="45306">
        <COMMENTS>包含 待办事项 汇总和 任务 统计</COMMENTS>
    </TASK>
    <TASK ID="4" TITLE="Overdue report" PRIORITY="9" DUEDATE="45300"/>
    <TASK ID="5" TITLE="Finished draft" PRIORITY="3" PERCENTDONE="100" DUEDATE="45444" COMPLETIONDATE="45444"/>
    <TASK ID="6" TITLE="Someday maybe" PRIORITY="2"/>
</TDL>"#,
    )
    .unwrap();
    path
}

/// In-memory DB with workspace/document registered, task index populated and
/// the search index built from the XML scan.
fn setup(tag: &str) -> (PathBuf, Connection) {
    let dir = temp_dir(tag);
    let xml_path = write_fixture(&dir);
    let conn = Connection::open_in_memory().unwrap();
    migration::run_migrations(&conn).unwrap();
    conn.execute(
        "INSERT INTO workspaces (id, name, root_path) VALUES ('ws-m9', 'M9 QA', '/tmp')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO documents (id, workspace_id, file_path) VALUES ('doc-m9', 'ws-m9', ?1)",
        [xml_path.to_string_lossy().as_ref()],
    )
    .unwrap();

    // Core index (M4 pipeline).
    indexer::index_document(&conn, "doc-m9", &xml_path).unwrap();

    // M6-owned side tables: seed directly to prove the search integration
    // seam picks them up without depending on M6 types.
    conn.execute(
        "INSERT INTO attachments_index (id, task_key, document_id, file_path, file_name) \
         VALUES ('att1', '1', 'doc-m9', 'x/receipt.pdf', 'receipt.pdf')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO progress_links_index (id, task_key, document_id, url, provider) \
         VALUES ('pl1', '3', 'doc-m9', 'https://github.com/pr/42', 'GitHub')",
        [],
    )
    .unwrap();

    // Search index (rebuildable from XML scan).
    let docs = vec![("doc-m9".to_string(), xml_path.clone())];
    rebuild_search_index(&conn, &docs, &search_fts::IndexTableExtrasProvider).unwrap();
    (dir, conn)
}

fn search(conn: &Connection, q: &str) -> moderntodolist_lib::domain::search::SearchPage {
    search_fts::search(conn, &SearchQuery::new(q.to_string())).unwrap()
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn load_tasks(conn: &Connection) -> Vec<ViewTask> {
    let mut stmt = conn
        .prepare(
            "SELECT task_key, document_id, title, status, percent_done, priority, \
                    start_date, due_date, completed_date FROM task_index \
             ORDER BY task_key",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            Ok(ViewTask {
                task_key: row.get::<_, String>(0)?,
                document_id: row.get::<_, String>(1)?,
                task_id: row.get::<_, String>(0)?,
                title: row.get::<_, String>(2)?,
                status: row.get::<_, String>(3)?,
                percent_done: row.get::<_, f64>(4)? as u8,
                priority: row.get::<_, i32>(5)? as u8,
                start_date: smart_view::parse_index_date(row.get::<_, Option<String>>(6)?.as_deref()),
                due_date: smart_view::parse_index_date(row.get::<_, Option<String>>(7)?.as_deref()),
                completed_date: smart_view::parse_index_date(row.get::<_, Option<String>>(8)?.as_deref()),
                tags: vec![],
                participants: vec![],
                flagged: false,
            })
        })
        .unwrap();
    rows.filter_map(|r| r.ok()).collect()
}

// ── QA-M9-001 ~ 005: Global search ──────────────────────────────────────────

#[test]
fn qa_m9_001_global_search_correctness() {
    let (_dir, conn) = setup("001");
    assert!(fts5_available(&conn), "FTS5 must be available in bundled rusqlite");

    // Title hit, with snippet and document context.
    let page = search(&conn, "milk");
    assert_eq!(page.backend, "fts5");
    assert!(page.total >= 1);
    let hit = page
        .results
        .iter()
        .find(|r| r.task_key.task_id.as_str() == "1")
        .expect("task 1 must match 'milk'");
    assert_eq!(hit.matched_field, MatchedField::Title);
    assert!(hit.snippet.contains("[[milk]]"), "snippet: {}", hit.snippet);
    assert_eq!(hit.document_context.document_id, "doc-m9");
    assert!(hit.score > 0.0);

    // Description hit (task 3's comments).
    let page = search(&conn, "release");
    assert!(page.results.iter().any(|r| r.task_key.task_id.as_str() == "3"));

    // Expanded searchable text: attachment display name.
    let page = search(&conn, "receipt.pdf");
    assert_eq!(page.results.len(), 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "1");
    assert_eq!(page.results[0].matched_field, MatchedField::Attachments);

    // Expanded searchable text: progress-link label/provider.
    let page = search(&conn, "GitHub");
    assert_eq!(page.results.len(), 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "3");
    assert_eq!(page.results[0].matched_field, MatchedField::Links);

    // Participant (ALLOCATEDTO indexed by M4 pipeline).
    let page = search(&conn, "Alice");
    assert!(page.results.iter().any(|r| r.task_key.task_id.as_str() == "1"));

    // No false positives.
    assert_eq!(search(&conn, "zzz_no_such_term").total, 0);
}

#[test]
fn qa_m9_002_chinese_fallback_where_fts5_returns_nothing() {
    let (_dir, conn) = setup("002");

    // Hard requirement: prove plain FTS5 does NOT find CJK substrings.
    let fts_hits: i64 = conn
        .query_row(
            "SELECT count(*) FROM task_search_fts WHERE task_search_fts MATCH '\"购买\"'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fts_hits, 0, "unicode61 cannot segment CJK — plain FTS5 finds nothing");

    // The LIKE fallback DOES find them.
    for term in ["购买清单", "待办事项", "购物"] {
        assert!(query_needs_cjk_fallback(term));
        let page = search(&conn, term);
        assert_eq!(page.backend, "like-fallback");
        assert!(page.total >= 1, "no fallback hits for {term}");
    }

    // "任务" appears in task 3's description.
    let page = search(&conn, "任务");
    assert!(page.results.iter().any(|r| r.task_key.task_id.as_str() == "3"));

    // Mixed Chinese/English query.
    let page = search(&conn, "release 待办事项");
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].task_key.task_id.as_str(), "3");

    // CJK snippets mark the match.
    let page = search(&conn, "牛奶");
    let hit = page
        .results
        .iter()
        .find(|r| r.task_key.task_id.as_str() == "2")
        .unwrap();
    assert!(hit.snippet.contains("[[牛奶]]"), "snippet: {}", hit.snippet);
}

#[test]
fn qa_m9_003_incremental_index_updates_on_mutation() {
    let (_dir, conn) = setup("003");
    assert_eq!(search(&conn, "milk").total, 1);

    // Mutate task 1 (title no longer mentions milk).
    let mut t = moderntodolist_lib::domain::task::Task::new(TaskId::new("1"));
    t.title = "Buy bread only".into();
    search_fts::on_task_mutated(&conn, "doc-m9", &t, &search_fts::IndexTableExtrasProvider).unwrap();
    assert_eq!(search(&conn, "milk").total, 0, "stale entry must be gone");
    assert_eq!(search(&conn, "bread").total, 1);

    // Delete task.
    search_fts::remove_task_search(&conn, "doc-m9", "1").unwrap();
    assert_eq!(search(&conn, "bread").total, 0);
    let fts_left: i64 = conn
        .query_row(
            "SELECT count(*) FROM task_search_fts WHERE task_id = '1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let plain_left: i64 = conn
        .query_row(
            "SELECT count(*) FROM task_search_plain WHERE task_id = '1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((fts_left, plain_left), (0, 0), "both stores updated together");
}

#[test]
fn qa_m9_004_pagination_bounded_and_deterministic() {
    let dir = temp_dir("004");
    let path = dir.join("many.xml");
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<TDL>\n");
    for i in 0..30 {
        xml.push_str(&format!("  <TASK ID=\"{i}\" TITLE=\"common keyword item {i}\"/>\n"));
    }
    xml.push_str("</TDL>");
    std::fs::write(&path, xml).unwrap();

    let conn = Connection::open_in_memory().unwrap();
    migration::run_migrations(&conn).unwrap();
    conn.execute(
        "INSERT INTO workspaces (id, name, root_path) VALUES ('ws', 'w', '/tmp')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO documents (id, workspace_id, file_path) VALUES ('doc', 'ws', ?1)",
        [path.to_string_lossy().as_ref()],
    )
    .unwrap();
    rebuild_search_index(
        &conn,
        &[("doc".to_string(), path)],
        &search_fts::IndexTableExtrasProvider,
    )
    .unwrap();

    let p1 = search_fts::search(&conn, &SearchQuery::new("common").with_page(10, 0)).unwrap();
    let p2 = search_fts::search(&conn, &SearchQuery::new("common").with_page(10, 10)).unwrap();
    assert_eq!(p1.total, 30);
    assert_eq!(p2.total, 30);
    assert_eq!(p1.results.len(), 10);
    assert!(!p1
        .results
        .iter()
        .any(|r| p2.results.iter().any(|o| o.task_key == r.task_key)),
        "pages must not overlap");

    // Deterministic ordering across runs.
    let again = search_fts::search(&conn, &SearchQuery::new("common").with_page(10, 0)).unwrap();
    assert_eq!(p1.results, again.results);

    // Hard bound on page size.
    let huge = search_fts::search(&conn, &SearchQuery::new("common").with_page(99999, 0)).unwrap();
    assert!(huge.results.len() <= moderntodolist_lib::domain::search::MAX_PAGE_SIZE);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn qa_m9_005_search_index_rebuildable_from_xml_scan() {
    let (dir, conn) = setup("005");
    let before = search(&conn, "milk");
    assert_eq!(before.total, 1);

    // Wipe the search stores completely — they are derived data.
    search_fts::clear_search_index(&conn).unwrap();
    assert_eq!(search(&conn, "milk").total, 0);
    assert_eq!(search(&conn, "购买清单").total, 0);

    // Rebuild purely from the XML scan; results must be identical.
    let docs = vec![("doc-m9".to_string(), dir.join("m9_fixture.xml"))];
    let n = rebuild_search_index(&conn, &docs, &search_fts::IndexTableExtrasProvider).unwrap();
    assert_eq!(n, 6);
    let after = search(&conn, "milk");
    assert_eq!(after.total, before.total);
    assert_eq!(after.results[0].task_key, before.results[0].task_key);
    assert!(search(&conn, "购买清单").total >= 1, "CJK fallback survives rebuild");
}

// ── QA-M9-006 ~ 010: Smart / saved views ────────────────────────────────────

#[test]
fn qa_m9_006_smart_view_date_boundaries_with_injected_now() {
    let (_dir, conn) = setup("006");
    let mut tasks = load_tasks(&conn);
    // Task 2 due 45364 = 2024-03-13.
    let today = d(2024, 3, 13);

    // Midnight/exact-day boundary: due today IS in Today; evaluated for
    // tomorrow it is NOT (it becomes Overdue).
    let t2 = tasks.iter_mut().find(|t| t.task_key == "2").unwrap();
    assert_eq!(t2.due_date, Some(today));
    let today_hits = today_view(&tasks, today);
    assert!(today_hits.iter().any(|t| t.task_key == "2"));
    // Task 3 started 45306 (2024-01-15) <= today → also Today.
    assert!(today_hits.iter().any(|t| t.task_key == "3"));

    let tomorrow_hits = today_view(&tasks, d(2024, 3, 14));
    assert!(!tomorrow_hits.iter().any(|t| t.task_key == "2"));

    // Overdue boundary: due < today, strictly.
    let overdue_today = overdue_view(&tasks, today);
    assert!(overdue_today.iter().any(|t| t.task_key == "4")); // due 45300 < today
    assert!(!overdue_today.iter().any(|t| t.task_key == "2")); // due == today

    // Completed: task 5 only, sorted by completed_date desc.
    let completed = completed_view(&tasks);
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].task_key, "5");

    // Flagged: priority >= 7 → tasks 1 (7), 3 (8), 4 (9).
    let flagged = smart_view::flagged_view(&tasks);
    let keys: Vec<&str> = flagged.iter().map(|t| t.task_key.as_str()).collect();
    assert_eq!(keys, vec!["4", "3", "1"]); // priority desc

    // Unscheduled: tasks 1 and 6 (no start, no due, unfinished).
    let unsched = smart_view::unscheduled_view(&tasks);
    let keys: Vec<&str> = unsched.iter().map(|t| t.task_key.as_str()).collect();
    assert!(keys.contains(&"1") && keys.contains(&"6"));
    assert!(!keys.contains(&"5"), "completed tasks are never unscheduled");
}

#[test]
fn qa_m9_007_upcoming_week_rollover() {
    // 2024-03-13 is Wednesday; week ends Sunday 2024-03-17.
    let today = d(2024, 3, 13);
    let mk = |key: &str, due: NaiveDate| ViewTask {
        task_key: key.into(),
        document_id: "doc".into(),
        task_id: key.into(),
        title: key.into(),
        status: "NotStarted".into(),
        percent_done: 0,
        priority: 5,
        start_date: None,
        due_date: Some(due),
        completed_date: None,
        tags: vec![],
        participants: vec![],
        flagged: false,
    };
    let tasks = vec![
        mk("today", today),
        mk("tomorrow", today + chrono::Duration::days(1)),
        mk("sunday", d(2024, 3, 17)),
        mk("monday", d(2024, 3, 18)),
    ];
    let groups = upcoming_view(&tasks, today);
    assert_eq!(groups.today[0].task_key, "today");
    assert_eq!(groups.tomorrow[0].task_key, "tomorrow");
    assert_eq!(groups.this_week[0].task_key, "sunday");
    assert_eq!(groups.later[0].task_key, "monday");

    // Sunday rollover: on 2024-03-17 the week ends. "monday" (03-18) is
    // Tomorrow from Sunday's perspective; nothing else fits This Week.
    let sunday = d(2024, 3, 17);
    let groups = upcoming_view(&tasks, sunday);
    assert!(groups.this_week.is_empty());
    assert_eq!(groups.today[0].task_key, "sunday");
    assert_eq!(groups.tomorrow[0].task_key, "monday");
    assert!(groups.later.is_empty());
    // Past-due tasks dropped out entirely (they belong to Overdue).
    assert_eq!(groups.total(), 2);
}

#[test]
fn qa_m9_008_smart_views_do_not_mutate_data() {
    let (_dir, conn) = setup("008");
    let tasks = load_tasks(&conn);
    let snapshot = tasks.clone();
    let count_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get(0))
        .unwrap();

    let today = d(2024, 3, 13);
    let _ = today_view(&tasks, today);
    let _ = upcoming_view(&tasks, today);
    let _ = overdue_view(&tasks, today);
    let _ = completed_view(&tasks);
    let _ = smart_view::flagged_view(&tasks);
    let _ = smart_view::by_tag_view(&tasks);

    assert_eq!(tasks, snapshot, "view evaluation must not mutate task data");
    let count_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_before, count_after, "index untouched by view evaluation");
}

#[test]
fn qa_m9_009_saved_views_persist_and_never_mutate_tasks() {
    let dir = temp_dir("009");
    let db_path = dir.join("views.db");
    let node = PredicateNode::all_of(vec![
        PredicateNode::Leaf(ViewPredicate::new(
            PredicateField::Priority,
            PredicateOperator::GreaterOrEqual,
            serde_json::json!(7),
        )),
        PredicateNode::Leaf(ViewPredicate::new(
            PredicateField::Status,
            PredicateOperator::Equals,
            serde_json::json!("NotStarted"),
        )),
    ]);

    let view_id;
    {
        let conn = Connection::open(&db_path).unwrap();
        migration::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES ('ws', 'w', '/tmp')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path) VALUES ('dx', 'ws', 'x.tdl')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done) \
             VALUES ('k1', 'dx', 'High open', 8, 'NotStarted', 0)",
            [],
        )
        .unwrap();
        let view = saved_views::create_view(&conn, "ws", "重要未完成", &node).unwrap();
        view_id = view.id.clone();
        let rows = saved_views::evaluate(&conn, "ws", &node).unwrap();
        assert_eq!(rows.len(), 1);
    }
    // Simulated restart.
    {
        let conn = Connection::open(&db_path).unwrap();
        let views = saved_views::list_views(&conn, "ws").unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].name, "重要未完成");
        assert_eq!(views[0].predicates, node);

        // Evaluation is read-only: task rows unchanged afterwards.
        let rows = saved_views::evaluate(&conn, "ws", &views[0].predicates).unwrap();
        assert_eq!(rows.len(), 1);
        let title: String = conn
            .query_row("SELECT title FROM task_index WHERE task_key='k1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(title, "High open");

        saved_views::rename_view(&conn, &view_id, "Renamed 重命名").unwrap();
        assert_eq!(saved_views::get_view(&conn, &view_id).unwrap().unwrap().name, "Renamed 重命名");
        saved_views::delete_view(&conn, &view_id).unwrap();
        assert!(saved_views::list_views(&conn, "ws").unwrap().is_empty());
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn qa_m9_010_saved_view_date_boundaries_sql_matches_memory() {
    let (_dir, conn) = setup("010");
    // due <= 2024-03-13: tasks 2 (45364) and 4 (45300); NOT 5 (45444).
    let node = PredicateNode::Leaf(ViewPredicate::new(
        PredicateField::DueDate,
        PredicateOperator::LessOrEqual,
        serde_json::json!("2024-03-13"),
    ));
    let rows = saved_views::evaluate(&conn, "ws-m9", &node).unwrap();
    let keys: Vec<&str> = rows.iter().map(|r| r.task_key.as_str()).collect();
    assert!(keys.contains(&"2") && keys.contains(&"4"));
    assert!(!keys.contains(&"5"));

    // In-memory evaluator agrees with SQL on every indexed row.
    let all = saved_views::evaluate(&conn, "ws-m9", &PredicateNode::And(vec![])).unwrap();
    for row in &all {
        assert_eq!(
            node.matches(row),
            keys.contains(&row.task_key.as_str()),
            "memory/SQL disagreement on {}",
            row.task_key
        );
    }

    // Versioned payload round-trip.
    let json = saved_views::serialize_predicates(&node).unwrap();
    assert!(json.contains("\"version\":1"));
    assert_eq!(saved_views::deserialize_predicates(&json).unwrap(), node);
    assert!(saved_views::deserialize_predicates(
        r#"{"version":42,"root":{"field":"title","operator":"equals","value":"x"}}"#
    )
    .is_err());
}

// ── QA-M9-011 ~ 015: Quick add / palette / jump / filters ───────────────────

#[test]
fn qa_m9_011_quick_add_safe_fallback() {
    let now = d(2024, 1, 10);
    // Happy path.
    let draft = parse_quick_add_with_now("Buy milk #shopping @Alice !high due:2024-01-15", now);
    assert_eq!(draft.title, "Buy milk");
    assert_eq!(draft.tags, vec!["shopping"]);
    assert_eq!(draft.participants, vec!["Alice"]);
    assert_eq!(draft.priority, Some(7));
    assert_eq!(draft.due_date, Some(d(2024, 1, 15)));

    // Email containing @ must NOT be misread as a participant.
    let draft = parse_quick_add_with_now("Mail john@example.com the report #work", now);
    assert_eq!(draft.title, "Mail john@example.com the report");
    assert!(draft.participants.is_empty());
    assert_eq!(draft.tags, vec!["work"]);

    // Malformed tokens remain in the title — never lose user text.
    let draft = parse_quick_add_with_now("Weird # @ ! due:nope !nope", now);
    assert_eq!(draft.title, "Weird # @ ! due:nope !nope");
    assert!(draft.tags.is_empty() && draft.due_date.is_none() && draft.priority.is_none());

    // Duplicate tags dedup, first occurrence wins.
    let draft = parse_quick_add_with_now("T #a #a #A", now);
    assert_eq!(draft.tags, vec!["a"]);

    // Empty input.
    assert!(parse_quick_add_with_now("  ", now).is_empty());

    // CJK title.
    let draft = parse_quick_add_with_now("买牛奶 #购物 @张三", now);
    assert_eq!(draft.title, "买牛奶");
    assert_eq!(draft.tags, vec!["购物"]);
    assert_eq!(draft.participants, vec!["张三"]);
}

#[test]
fn qa_m9_012_command_registry_no_conflicts_and_detection_works() {
    // Canonical registry: unique ids, no duplicate shortcuts.
    let registry = moderntodolist_lib::domain::search::command_registry();
    assert!(find_shortcut_conflicts(&registry).is_empty());
    assert!(registry.iter().any(|c| c.id == "search.open" && c.shortcut == Some("Ctrl+K")));

    // Synthetic conflict is detected case-insensitively.
    let reg = vec![
        CommandDescriptor { id: "x.one", label: "One", icon: "i", shortcut: Some("Ctrl+K"), category: "search" },
        CommandDescriptor { id: "x.two", label: "Two", icon: "i", shortcut: Some("ctrl+k"), category: "search" },
    ];
    let conflicts = find_shortcut_conflicts(&reg);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].command_ids, vec!["x.one", "x.two"]);
}

#[test]
fn qa_m9_013_task_jump_resolution_and_palette_fuzzy() {
    let (dir, _conn) = setup("013");
    let bytes = std::fs::read(dir.join("m9_fixture.xml")).unwrap();
    // Jump to a top-level task → empty ancestor chain.
    assert_eq!(resolve_ancestors(&bytes, "1"), Some(vec![]));
    // Unknown task → None (caller shows "not found").
    assert_eq!(resolve_ancestors(&bytes, "999"), None);

    // Palette fuzzy over commands.
    let hits = search_commands("undo", 5);
    assert_eq!(hits[0].command.id, "edit.undo");
    let hits = search_commands("today", 5);
    assert_eq!(hits[0].command.id, "view.today");
    // Fuzzy over task titles (subsequence).
    assert!(fuzzy_score("wrn", "Write release notes").is_some());
}

#[test]
fn qa_m9_014_quick_add_builds_valid_domain_task() {
    let draft = parse_quick_add_with_now("Ship release #m9 @QA !high due:2024-02-01", d(2024, 1, 10));
    let task = build_task(&draft, TaskId::new("100"));
    assert_eq!(task.id.as_str(), "100");
    assert_eq!(task.title, "Ship release");
    assert_eq!(task.priority.value(), 7);
    assert_eq!(task.categories[0].name, "m9");
    assert_eq!(task.allocated_to, vec!["QA".to_string()]);
    assert_eq!(task.due_date, Some(naive_date_to_ole(d(2024, 2, 1))));

    // The built task survives the XML round-trip through the M2 pipeline.
    let mut elem = moderntodolist_lib::domain::xml_tree::XmlElement::new("TASK");
    moderntodolist_lib::domain::mappers::write_task(&task, &mut elem);
    let reread = moderntodolist_lib::domain::mappers::read_task(&elem);
    assert_eq!(reread.title, task.title);
    assert_eq!(reread.due_date, task.due_date);
    assert_eq!(reread.allocated_to, task.allocated_to);
}

#[test]
fn qa_m9_015_canonical_filter_regression() {
    let (_dir, conn) = setup("015");
    // Canonical filter: AND of leaf conditions, OR composition inside.
    let filter = PredicateNode::all_of(vec![
        PredicateNode::Leaf(ViewPredicate::new(
            PredicateField::Status,
            PredicateOperator::NotEquals,
            serde_json::json!("Done"),
        )),
        PredicateNode::any_of(vec![
            PredicateNode::Leaf(ViewPredicate::new(
                PredicateField::Tag,
                PredicateOperator::Equals,
                serde_json::json!("shopping"),
            )),
            PredicateNode::Leaf(ViewPredicate::new(
                PredicateField::Priority,
                PredicateOperator::GreaterOrEqual,
                serde_json::json!(8),
            )),
        ]),
    ]);
    let rows = saved_views::evaluate(&conn, "ws-m9", &filter).unwrap();
    let keys: Vec<&str> = rows.iter().map(|r| r.task_key.as_str()).collect();
    // Task 1 (shopping tag, NotStarted), tasks 3 & 4 (priority >= 8, NotStarted).
    // Task 5 excluded (status Done).
    assert!(keys.contains(&"1") && keys.contains(&"3") && keys.contains(&"4"));
    assert!(!keys.contains(&"5"));

    // Persist → reload → evaluate gives the same canonical result.
    let view = saved_views::create_view(&conn, "ws-m9", "canonical", &filter).unwrap();
    let reloaded = saved_views::get_view(&conn, &view.id).unwrap().unwrap();
    assert_eq!(reloaded.predicates, filter);
    let rows2 = saved_views::evaluate(&conn, "ws-m9", &reloaded.predicates).unwrap();
    assert_eq!(rows, rows2);

    // Compiled SQL is parameterized (no inline user text → no injection).
    let (sql, params) = saved_views::compile_sql(&filter).unwrap();
    assert!(!params.is_empty());
    assert!(!sql.contains("shopping"), "values must be bound, not inlined: {sql}");
}

// ── QA-M9-016: Cumulative M0-M8 regression with productivity enabled ────────

#[test]
fn qa_m9_016_cumulative_regression_index_stays_disposable() {
    let dir = temp_dir("016");
    let data_dir = dir.join("Data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let xml_path = write_fixture(&dir);

    // M2: XML lossless round-trip.
    let bytes = std::fs::read(&xml_path).unwrap();
    let doc = parse_xml(&bytes).unwrap();
    let reserialized = serialize_xml(&doc);
    let doc2 = parse_xml(&reserialized).unwrap();
    assert_eq!(doc.root.tag, doc2.root.tag);
    assert_eq!(doc.root.children.len(), doc2.root.children.len());

    // M4/M5: file-backed DatabaseManager + index + task query rows.
    let mut db = moderntodolist_lib::infrastructure::DatabaseManager::open(&data_dir);
    assert!(db.is_available());
    assert_eq!(db.schema_version().unwrap(), moderntodolist_lib::infrastructure::schema::SCHEMA_VERSION);

    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES ('ws-c', 'cum', '/tmp')",
            [],
        )
        .map_err(moderntodolist_lib::infrastructure::DatabaseError::Sqlite)?;
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path) VALUES ('doc-c', 'ws-c', ?1)",
            [xml_path.to_string_lossy().as_ref()],
        )
        .map_err(moderntodolist_lib::infrastructure::DatabaseError::Sqlite)?;
        indexer::index_document(conn, "doc-c", &xml_path).map_err(|e| {
            moderntodolist_lib::infrastructure::DatabaseError::Sqlite(
                rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
            )
        })?;
        rebuild_search_index(
            conn,
            &[("doc-c".to_string(), xml_path.clone())],
            &search_fts::IndexTableExtrasProvider,
        )
        .map_err(|e| {
            moderntodolist_lib::infrastructure::DatabaseError::Sqlite(
                rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
            )
        })?;
        Ok(())
    })
    .unwrap();

    // Everything works before the wipe.
    let assert_all_work = |conn: &Connection| {
        // M5: task query
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 6);
        // M9: search (EN + CJK)
        assert_eq!(search_fts::search(conn, &SearchQuery::new("milk")).unwrap().total, 1);
        assert!(search_fts::search(conn, &SearchQuery::new("购买清单")).unwrap().total >= 1);
        // M9: smart views
        let tasks = load_tasks(conn);
        assert!(!completed_view(&tasks).is_empty());
        assert!(!overdue_view(&tasks, d(2024, 3, 13)).is_empty());
        // M9: saved views
        let node = PredicateNode::Leaf(ViewPredicate::new(
            PredicateField::Title,
            PredicateOperator::Contains,
            serde_json::json!("release"),
        ));
        let rows = saved_views::evaluate(conn, "ws-c", &node).unwrap();
        assert_eq!(rows.len(), 1);
        // M9: quick add
        let draft = parse_quick_add_with_now("New thing #t @p !high", d(2024, 3, 13));
        assert_eq!(build_task(&draft, TaskId::new("900")).title, "New thing");
        // M9: palette + jump
        assert_eq!(search_commands("undo", 5)[0].command.id, "edit.undo");
        let bytes = std::fs::read(
            conn.query_row("SELECT file_path FROM documents WHERE id='doc-c'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(resolve_ancestors(&bytes, "5"), Some(vec![]));
    };

    db.with_connection(|conn| {
        assert_all_work(conn);
        Ok(())
    })
    .unwrap();

    // DISPOSABILITY: delete the entire index database, rebuild from XML scan,
    // and every function above must work identically.
    db.delete_database().unwrap();
    assert!(!db.is_available());
    assert!(!data_dir.join("index.db").exists());

    db.reopen().unwrap();
    assert!(db.is_available());
    db.with_connection(|conn| {
        // Fresh database: no data at all.
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES ('ws-c', 'cum', '/tmp')",
            [],
        )
        .map_err(moderntodolist_lib::infrastructure::DatabaseError::Sqlite)?;
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path) VALUES ('doc-c', 'ws-c', ?1)",
            [xml_path.to_string_lossy().as_ref()],
        )
        .map_err(moderntodolist_lib::infrastructure::DatabaseError::Sqlite)?;
        indexer::index_document(conn, "doc-c", &xml_path).map_err(|e| {
            moderntodolist_lib::infrastructure::DatabaseError::Sqlite(
                rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
            )
        })?;
        assert_eq!(
            search_fts::ensure_search_schema(conn),
            SearchBackend::Fts5
        );
        rebuild_search_index(
            conn,
            &[("doc-c".to_string(), xml_path.clone())],
            &search_fts::IndexTableExtrasProvider,
        )
        .map_err(|e| {
            moderntodolist_lib::infrastructure::DatabaseError::Sqlite(
                rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
            )
        })?;
        // All productivity + core functions work again after full rebuild.
        assert_all_work(conn);
        Ok(())
    })
    .unwrap();

    std::fs::remove_dir_all(&dir).ok();
}

// Keep the SearchDocument import meaningful for downstream wiring examples:
// incremental updates from the M6 pipeline build SearchDocument payloads.
#[test]
fn qa_m9_extras_seam_search_document_from_task() {
    let mut task = moderntodolist_lib::domain::task::Task::new(TaskId::new("7"));
    task.title = "Seam check".into();
    let extras = TaskSearchExtras {
        tags: vec!["t1".into()],
        participants: vec!["p1".into()],
        attachment_names: vec!["file one.png".into()],
        progress_link_labels: vec!["Linear INH-1".into()],
    };
    let doc = SearchDocument::from_task("doc-x", &task, &extras);
    assert_eq!(doc.title, "Seam check");
    assert_eq!(doc.tags, "t1");
    assert_eq!(doc.attachments, "file one.png");
    assert_eq!(doc.links, "Linear INH-1");
}
