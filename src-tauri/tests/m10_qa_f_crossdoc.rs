//! M10 RC Regression Matrix F — Cross-Document Operations
//! (QA-M10 F / INH-1129, spec section 5.5).
//!
//! 10 cases (F01..F10): copy/move subtree, dependency remap, crash recovery
//! and transfer undo — driven through the real public transfer pipeline
//! (`domain::transfer`, `domain::transfer_journal`) plus the M4 index, the
//! M9 search pipeline, the M2 lossless mapper layer and the M8 file
//! library/trash, all in the same scenario.
//!
//! THE OVERRIDING INVARIANT (GATE-M8 P0, re-asserted at RC level): for every
//! crash point and every lifecycle step, the union of source and target
//! documents after recovery contains EVERY original task. An interrupted
//! transfer leaves a duplicate, never a loss.
//!
//! Crash injection uses the production `TransferHooks` seam — the same
//! phase-boundary kill switch the M8 QA matrix used; the pipeline code path
//! is otherwise the real one (planning, staging, atomic commits, journaling).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use moderntodolist_lib::domain::mappers::read_task;
use moderntodolist_lib::domain::transfer::{
    complete_interrupted_move, copy_task, execute_transfer, find_task_element, move_task,
    plan_transfer, recover_interrupted_transfers, undo_transfer, ExternalDependencyPolicy,
    PlanOptions, TransferContext, TransferError, TransferHooks, TransferOperation, TransferPhase,
    TransferProgress, TransferRecoveryAction,
};
use moderntodolist_lib::domain::transfer_journal::TransferJournal;
use moderntodolist_lib::domain::types::TaskId;
use moderntodolist_lib::domain::xml_parser::parse_xml;
use moderntodolist_lib::domain::xml_tree::XmlElement;
use moderntodolist_lib::domain::{file_library, trash, workspace};
use moderntodolist_lib::domain::search::SearchQuery;
use moderntodolist_lib::infrastructure::indexer::index_document;
use moderntodolist_lib::infrastructure::migration::run_migrations;
use moderntodolist_lib::infrastructure::search_fts::{self, rebuild_search_index, IndexTableExtrasProvider};

// ─────────────────────────── Harness ───────────────────────────

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate root should have a parent")
        .join("tests")
        .join("fixtures")
        .join("xml")
        .join("transfer")
}

struct Env {
    dir: PathBuf,
    source: PathBuf,
    target: PathBuf,
    ctx: TransferContext,
}

/// Copies the transfer fixtures into a fresh temp workspace layout.
///
/// Fixture inventory (source): "Stay Behind"(1, depends on 5),
/// "Release Epic"(5, unknown attrs/children + METADATA), "Build
/// Installer"(6, internal dep on 5, external dep on 1, asset refs),
/// "Sign Binaries"(7, internal dep on 6), "Unrelated Root"(2).
/// Fixture inventory (target): "Target Task One"(1), "Target Task Two"(2),
/// "Target Nested"(3).
fn setup_env(name: &str) -> Env {
    let dir = std::env::temp_dir().join(format!(
        "mtdl_rc_f_{}_{}_{}",
        name,
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).unwrap();
    let fx = fixtures_root();
    let source = dir.join("source-project.xml");
    let target = dir.join("target-project.xml");
    fs::copy(fx.join("source-project.xml"), &source).unwrap();
    fs::copy(fx.join("target-project.xml"), &target).unwrap();
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::copy(fx.join("assets").join("spec.txt"), dir.join("assets").join("spec.txt")).unwrap();
    let ctx = TransferContext::for_workspace_root(&dir);
    Env { dir, source, target, ctx }
}

fn task_titles(path: &Path) -> HashSet<String> {
    let mut titles = HashSet::new();
    if let Ok(bytes) = fs::read(path) {
        if let Ok(doc) = parse_xml(&bytes) {
            collect_titles(&doc.root, &mut titles);
        }
    }
    titles
}

fn collect_titles(elem: &XmlElement, out: &mut HashSet<String>) {
    for child in elem.child_elements() {
        if child.tag == "TASK" {
            if let Some(t) = child.get_attr("TITLE") {
                out.insert(t.to_string());
            }
            collect_titles(child, out);
        } else {
            collect_titles(child, out);
        }
    }
}

fn task_ids(path: &Path) -> Vec<String> {
    let bytes = fs::read(path).unwrap();
    let doc = parse_xml(&bytes).unwrap();
    let mut ids = Vec::new();
    collect_ids(&doc.root, &mut ids);
    ids
}

fn collect_ids(elem: &XmlElement, out: &mut Vec<String>) {
    for child in elem.child_elements() {
        if child.tag == "TASK" {
            if let Some(id) = child.get_attr("ID") {
                out.push(id.to_string());
            }
            collect_ids(child, out);
        } else {
            collect_ids(child, out);
        }
    }
}

/// THE OVERRIDING INVARIANT: every original task title of both documents
/// must still exist in the union of both documents.
fn assert_no_task_loss(env: &Env, before_source: &HashSet<String>, before_target: &HashSet<String>) {
    let union: HashSet<String> = task_titles(&env.source).union(&task_titles(&env.target)).cloned().collect();
    for title in before_source.iter().chain(before_target.iter()) {
        assert!(union.contains(title), "TASK LOST: {title:?} (union={union:?})");
    }
}

fn fixture_titles() -> (HashSet<String>, HashSet<String>) {
    let fx = fixtures_root();
    (
        task_titles(&fx.join("source-project.xml")),
        task_titles(&fx.join("target-project.xml")),
    )
}

fn plan_opts(op: TransferOperation, txn: &str) -> PlanOptions {
    PlanOptions {
        operation: op,
        external_dependency_policy: ExternalDependencyPolicy::default(),
        transaction_id: Some(txn.to_string()),
        source_document_id: None,
        target_document_id: None,
    }
}

/// Plans + executes with a kill hook at an exact phase boundary.
fn run_killed(env: &Env, op: TransferOperation, txn: &str, phase: TransferPhase) {
    let plan = plan_transfer(&env.source, &env.target, &TaskId::new("5"), &plan_opts(op, txn)).unwrap();
    let err = execute_transfer(plan, &env.ctx, &TransferHooks { fail_before: Some(phase) }, |_| {}).unwrap_err();
    assert!(
        matches!(err, TransferError::SimulatedKill { phase: p } if p == phase),
        "expected SimulatedKill({phase:?}), got {err:?}"
    );
}

fn dep_task_ids(target_elem: &XmlElement) -> Vec<(String, bool)> {
    target_elem
        .children_by_tag("DEPENDENCY")
        .map(|d| {
            (
                d.first_child_by_tag("TASKID").map(|e| e.text_content()).unwrap_or_default(),
                d.first_child_by_tag("FILENAME").is_some(),
            )
        })
        .collect()
}

// ═══════════════ F01: copy subtree — source untouched, target remapped ═══════════════

/// QA-M10-F01: a cross-document COPY plans a collision-free id map, remaps
/// internal dependencies, degrades external ones to marked External refs,
/// and never touches the source document (byte-identical).
#[test]
fn qa_m10_f01_copy_subtree_remaps_ids_and_never_touches_source() {
    let env = setup_env("f01");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_before = fs::read(&env.source).unwrap();

    // Planning is side-effect free and exposes the durable contract.
    let plan = plan_transfer(&env.source, &env.target, &TaskId::new("5"), &plan_opts(TransferOperation::Copy, "txn-f01")).unwrap();
    let map: Vec<(String, String)> = plan.manifest.id_map.iter().map(|(s, t)| (s.as_str().to_string(), t.as_str().to_string())).collect();
    assert_eq!(map, vec![("5".into(), "4".into()), ("6".into(), "5".into()), ("7".into(), "6".into())]);
    assert_eq!(plan.manifest.mapped_id(&TaskId::new("6")).unwrap().as_str(), "5");
    assert_eq!(plan.manifest.task_ids.len(), 3);
    assert!(fs::read(&env.source).unwrap() == src_before && fs::read(&env.target).unwrap().len() > 0, "planning wrote nothing");

    let outcome = execute_transfer(plan, &env.ctx, &TransferHooks::default(), |_| {}).unwrap();
    assert_eq!(outcome.operation, TransferOperation::Copy);
    assert_eq!(outcome.target_root_task_id.as_str(), "4");
    assert!(outcome.undo_available);

    // Source byte-identical; target holds the remapped subtree.
    assert_eq!(fs::read(&env.source).unwrap(), src_before, "copy must never touch the source");
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3", "4", "5", "6"]);

    // Internal dependency remap (6→5 became 5→4; 7→6 became 6→5) and the
    // external dep (6→1) preserved with a FILENAME marker naming the source.
    let doc = parse_xml(&fs::read(&env.target).unwrap()).unwrap();
    let installer = find_task_element(&doc.root, "5").unwrap();
    assert_eq!(dep_task_ids(installer), vec![("4".to_string(), false), ("1".to_string(), true)]);
    let signer = find_task_element(&doc.root, "6").unwrap();
    assert_eq!(dep_task_ids(signer), vec![("5".to_string(), false)]);
    assert!(outcome.warnings.iter().any(|w| matches!(
        w,
        moderntodolist_lib::domain::transfer::TransferWarning::ExternalDependencyPreserved { dep_task_id, .. }
            if dep_task_id == "1"
    )));

    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════════ F02: move subtree — target-first ordering, source delete last ═══════════════

/// QA-M10-F02: a cross-document MOVE runs the target-first protocol (progress
/// events prove CommitTarget precedes any source phase), leaves the orphaned
/// source dependency in place (never silently dropped), and produces
/// fingerprints that match the committed bytes.
#[test]
fn qa_m10_f02_move_subtree_target_first_protocol() {
    let env = setup_env("f02");
    let (src_titles, tgt_titles) = fixture_titles();
    let mut events: Vec<TransferProgress> = Vec::new();

    let outcome = move_task(&env.source, &env.target, &TaskId::new("5"), &plan_opts(TransferOperation::Move, "txn-f02"), &env.ctx, |p| events.push(p)).unwrap();

    // Target-first ordering is observable through the real progress stream.
    assert_eq!(events.first(), Some(&TransferProgress::Planning));
    assert_eq!(events.last(), Some(&TransferProgress::Finishing));
    let pos = |p: &TransferProgress| events.iter().position(|e| e == p).expect("phase must run");
    assert!(pos(&TransferProgress::CommittingTarget) < pos(&TransferProgress::BackingUpSource));
    assert!(pos(&TransferProgress::BackingUpSource) < pos(&TransferProgress::CommittingSource));

    // Final state: subtree gone from source, present (remapped) in target.
    assert_eq!(task_ids(&env.source), vec!["1", "2"]);
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3", "4", "5", "6"]);

    // Fingerprints match the committed bytes (real bookkeeping, not mocks).
    assert!(outcome.target_fingerprint.matches_bytes(&fs::read(&env.target).unwrap()));
    assert!(outcome
        .source_fingerprint_after
        .as_ref()
        .unwrap()
        .matches_bytes(&fs::read(&env.source).unwrap()));

    // The source-side dependency 1→5 is orphaned by the move: preserved in
    // the XML (never silently dropped) and surfaced as a warning.
    let src_doc = parse_xml(&fs::read(&env.source).unwrap()).unwrap();
    let stay = find_task_element(&src_doc.root, "1").unwrap();
    assert_eq!(dep_task_ids(stay), vec![("5".to_string(), false)]);
    assert!(outcome.warnings.iter().any(|w| matches!(
        w,
        moderntodolist_lib::domain::transfer::TransferWarning::OrphanedSourceDependency { task_id, dep_task_id }
            if task_id == "1" && dep_task_id == "5"
    )));

    // Durable undo artifacts exist for the transaction.
    assert!(env.ctx.undo_dir.join("txn-f02").join("source.before.xml").is_file());
    assert!(env.ctx.undo_dir.join("txn-f02").join("target.before.xml").is_file());

    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════════ F03: moved subtree is re-indexed and searchable in the target ═══════════════

/// QA-M10-F03: after a cross-document move, the M4 index + M9 search
/// pipeline (rebuilt from the saved XML alone) finds the moved tasks ONLY
/// under the target document with their NEW task keys, and the remapped
/// dependency lands in `task_dependencies` — the remap survives the move AND
/// stays searchable.
#[test]
fn qa_m10_f03_moved_subtree_remains_searchable_under_new_keys() {
    let env = setup_env("f03");
    let (src_titles, tgt_titles) = fixture_titles();
    move_task(&env.source, &env.target, &TaskId::new("5"), &plan_opts(TransferOperation::Move, "txn-f03"), &env.ctx, |_| {}).unwrap();

    let conn = rusqlite::Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();
    conn.execute("INSERT INTO workspaces (id, name, root_path) VALUES ('ws-f03','F03','.')", []).unwrap();
    for (id, path) in [("doc-source", &env.source), ("doc-target", &env.target)] {
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES (?1,'ws-f03',?2,'managed')",
            rusqlite::params![id, path.to_string_lossy().as_ref()],
        )
        .unwrap();
        index_document(&conn, id, path).unwrap();
    }
    rebuild_search_index(
        &conn,
        &[
            ("doc-source".to_string(), env.source.clone()),
            ("doc-target".to_string(), env.target.clone()),
        ],
        &IndexTableExtrasProvider,
    )
    .unwrap();

    // Moved tasks are findable ONLY under the target with the new keys.
    let page = search_fts::search(&conn, &SearchQuery::new("Release Epic")).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].document_context.document_id, "doc-target");
    assert_eq!(page.results[0].task_key.task_id.as_str(), "4", "new target key, not source key 5");
    let page = search_fts::search(&conn, &SearchQuery::new("Sign Binaries")).unwrap();
    assert_eq!(page.results[0].task_key.task_id.as_str(), "6");
    // Body text moved with the subtree.
    let page = search_fts::search(&conn, &SearchQuery::new("installer")).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.results[0].document_context.document_id, "doc-target");

    // The source keeps only its remaining tasks.
    let src_only = search_fts::search(&conn, &SearchQuery::new("Stay Behind")).unwrap();
    assert_eq!(src_only.total, 1);
    assert_eq!(src_only.results[0].document_context.document_id, "doc-source");

    // The remapped internal dependency is in the index under the target doc.
    let deps: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT task_key, depends_on_key FROM task_dependencies WHERE document_id='doc-target' ORDER BY task_key")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
    };
    assert!(deps.contains(&("5".to_string(), "4".to_string())), "remapped edge 5→4 indexed: {deps:?}");
    assert!(deps.contains(&("6".to_string(), "5".to_string())), "remapped edge 6→5 indexed: {deps:?}");
    // The orphaned source dependency is indexed too (preserved, not lost).
    let src_deps: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_dependencies WHERE document_id='doc-source' AND task_key='1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(src_deps, 1);

    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════════ F04: moved subtree round-trips through XML losslessly ═══════════════

/// QA-M10-F04: a moved subtree carries ALL of its legacy payload across the
/// document boundary — unknown attributes (M8CUSTOM), METADATA, unknown
/// child elements, comments, dates — changing only what the remap must
/// change (IDs, POS, dependency targets), and the target document stays a
/// fixed point of the domain rewrite afterwards.
#[test]
fn qa_m10_f04_move_preserves_unknown_data_losslessly() {
    let env = setup_env("f04");
    let (src_titles, tgt_titles) = fixture_titles();

    // Domain snapshot of the source subtree BEFORE the move.
    let src_doc = parse_xml(&fs::read(&env.source).unwrap()).unwrap();
    let before = |id: &str| read_task(find_task_element(&src_doc.root, id).unwrap());
    let (s5, s6, s7) = (before("5"), before("6"), before("7"));
    assert!(s5.unknown_attrs.iter().any(|(k, v)| k == "M8CUSTOM" && v == "keep-me-5"));

    move_task(&env.source, &env.target, &TaskId::new("5"), &plan_opts(TransferOperation::Move, "txn-f04"), &env.ctx, |_| {}).unwrap();

    let tgt_doc = parse_xml(&fs::read(&env.target).unwrap()).unwrap();
    let after = |id: &str| read_task(find_task_element(&tgt_doc.root, id).unwrap());
    let (t4, t5, t6) = (after("4"), after("5"), after("6"));

    // Identity fields remap; everything else is carried across verbatim.
    assert_eq!(t4.title, s5.title);
    assert_eq!(t4.unknown_attrs, s5.unknown_attrs, "unknown attributes survive the move");
    assert_eq!(t4.metadata, s5.metadata, "METADATA survives the move");
    assert_eq!(t4.unknown_children, s5.unknown_children, "UNKNOWNEL payload survives");
    assert_eq!(t4.categories.iter().map(|c| &c.name).collect::<Vec<_>>(), vec!["release"]);
    assert_eq!(t4.creation_date, s5.creation_date);
    assert_eq!(t4.created_by, s5.created_by);
    assert_eq!(t5.comments, s6.comments, "COMMENTS survive the move");
    assert_eq!(t5.file_links.len(), s6.file_links.len(), "asset refs survive");
    assert_eq!(t6.title, s7.title);
    assert_eq!(t5.percent_done, s6.percent_done);

    // Only the remapped references differ.
    assert_eq!(dep_task_ids(find_task_element(&tgt_doc.root, "5").unwrap()), vec![("4".to_string(), false), ("1".to_string(), true)]);

    // The target document (root attrs included) survives a full domain
    // rewrite as a fixed point: the moved data is byte-stable from now on.
    fn rewrite(bytes: &[u8]) -> Vec<u8> {
        use moderntodolist_lib::domain::mappers::write_task;
        use moderntodolist_lib::domain::xml_tree::XmlNode;
        fn rec(el: &mut XmlElement) {
            if el.tag == "TASK" {
                let t = read_task(el);
                write_task(&t, el);
            }
            for c in &mut el.children {
                if let XmlNode::Element(child) = c {
                    rec(child);
                }
            }
        }
        let mut doc = parse_xml(bytes).unwrap();
        rec(&mut doc.root);
        moderntodolist_lib::domain::xml_serializer::serialize_xml(&doc)
    }
    let once = rewrite(&fs::read(&env.target).unwrap());
    assert_eq!(rewrite(&once), once, "moved subtree is a rewrite fixed point");
    let t4b = read_task(find_task_element(&parse_xml(&once).unwrap().root, "4").unwrap());
    assert_eq!(t4b, t4);

    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════════ F05: crash before target commit → clean rollback ═══════════════

/// QA-M10-F05: a kill before CommitTarget rolls the transaction back — both
/// documents byte-identical, journal cleaned, no staged leftovers — and a
/// subsequent clean move still works (recovery poisoned nothing).
#[test]
fn qa_m10_f05_crash_before_target_commit_rolls_back_cleanly() {
    let env = setup_env("f05");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_bytes = fs::read(&env.source).unwrap();
    let tgt_bytes = fs::read(&env.target).unwrap();

    run_killed(&env, TransferOperation::Move, "txn-f05", TransferPhase::CommitTarget);

    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].action, TransferRecoveryAction::RolledBack);
    assert_eq!(fs::read(&env.source).unwrap(), src_bytes, "source untouched");
    assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes, "target untouched");
    assert!(recover_interrupted_transfers(&env.ctx.journal_dir).is_empty(), "journal cleaned after rollback");
    assert_no_task_loss(&env, &src_titles, &tgt_titles);

    // The same workspace still completes a clean move afterwards.
    move_task(&env.source, &env.target, &TaskId::new("5"), &plan_opts(TransferOperation::Move, "txn-f05b"), &env.ctx, |_| {}).unwrap();
    assert_eq!(task_ids(&env.source), vec!["1", "2"]);
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3", "4", "5", "6"]);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════════ F06: crash after target commit → duplicate, not loss ═══════════════

/// QA-M10-F06: a kill after the target committed but before the source
/// backup must leave BOTH documents holding the subtree (duplicate-not-loss);
/// recovery refuses to auto-delete source data and the explicit resolution
/// finishes the move deterministically.
#[test]
fn qa_m10_f06_crash_after_target_commit_keeps_duplicate_then_resolves() {
    let env = setup_env("f06");
    let (src_titles, tgt_titles) = fixture_titles();

    run_killed(&env, TransferOperation::Move, "txn-f06", TransferPhase::BackupSource);

    // Immediately after the crash: the data exists in BOTH documents.
    assert!(task_ids(&env.source).contains(&"5".to_string()));
    assert!(task_ids(&env.target).contains(&"4".to_string()));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);

    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].action, TransferRecoveryAction::CompletedWithDuplicate);
    assert!(!reports[0].duplicate_task_ids.is_empty(), "duplicates enumerated for the user");
    assert_no_task_loss(&env, &src_titles, &tgt_titles);

    // Explicit user-confirmed resolution completes the move.
    let report = complete_interrupted_move(&env.ctx.journal_dir, "txn-f06").unwrap();
    assert_eq!(report.action, TransferRecoveryAction::Completed);
    assert_eq!(task_ids(&env.source), vec!["1", "2"]);
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3", "4", "5", "6"]);
    assert!(recover_interrupted_transfers(&env.ctx.journal_dir).is_empty(), "journal cleaned");
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════════ F07: corrupt journal → quarantine, data intact ═══════════════

/// QA-M10-F07: a garbled journal (torn/corrupt crash artifact) is
/// QUARANTINED, never acted upon: both documents keep their data, the
/// artifacts are preserved for forensics, and repeated recovery stays safe.
#[test]
fn qa_m10_f07_corrupt_journal_quarantined_data_intact() {
    let env = setup_env("f07");
    let (src_titles, tgt_titles) = fixture_titles();

    run_killed(&env, TransferOperation::Move, "txn-f07", TransferPhase::BackupSource);
    let src_ids = task_ids(&env.source);
    let tgt_ids = task_ids(&env.target);

    // Garble BOTH the primary journal and its .bak (worst-case crash).
    let journal = TransferJournal::path_for(&env.ctx.journal_dir, "txn-f07");
    assert!(journal.exists());
    fs::write(&journal, b"\xff\xfe not json at all {{{").unwrap();
    fs::write(journal.with_extension("bak"), b"also broken ###").unwrap();

    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].action, TransferRecoveryAction::Quarantined);

    // Quarantine touched nothing: both documents still parse and hold their
    // tasks; the corrupt artifacts are preserved (not deleted).
    assert_eq!(task_ids(&env.source), src_ids);
    assert_eq!(task_ids(&env.target), tgt_ids);
    assert!(journal.exists(), "quarantine preserves the evidence");
    assert_no_task_loss(&env, &src_titles, &tgt_titles);

    // Recovery is safe to re-run (idempotent decision, no new damage).
    let again = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].action, TransferRecoveryAction::Quarantined);
    assert_eq!(task_ids(&env.source), src_ids);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════════ F08: transaction-level undo of move and copy ═══════════════

/// QA-M10-F08: completed transfers are reversible at the transaction level.
/// Undoing a MOVE restores the source byte-exact and removes the subtree
/// from the target; undoing a COPY leaves the source untouched; in both
/// cases the target's NEXTUNIQUEID never regresses (IDs are never
/// recycled); a second undo is an explicit error.
#[test]
fn qa_m10_f08_transfer_undo_move_and_copy_byte_exact() {
    // ── Move + undo ──
    let env = setup_env("f08m");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_before = fs::read(&env.source).unwrap();
    let outcome = move_task(&env.source, &env.target, &TaskId::new("5"), &plan_opts(TransferOperation::Move, "txn-f08m"), &env.ctx, |_| {}).unwrap();
    assert_eq!(task_ids(&env.source), vec!["1", "2"]);

    undo_transfer(&env.ctx, &outcome.transaction_id).unwrap();
    assert_eq!(fs::read(&env.source).unwrap(), src_before, "undo restores the source byte-exact");
    let tgt_doc = parse_xml(&fs::read(&env.target).unwrap()).unwrap();
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3"], "moved subtree removed from target");
    assert_eq!(
        tgt_doc.root.get_attr("NEXTUNIQUEID"),
        Some("7"),
        "NEXTUNIQUEID never regresses below the committed allocation (ids not recycled)"
    );
    // Undo is one-shot.
    assert!(matches!(
        undo_transfer(&env.ctx, &outcome.transaction_id),
        Err(TransferError::AlreadyUndone { .. })
    ));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();

    // ── Copy + undo ──
    let env = setup_env("f08c");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_before = fs::read(&env.source).unwrap();
    let outcome = copy_task(&env.source, &env.target, &TaskId::new("5"), &plan_opts(TransferOperation::Copy, "txn-f08c"), &env.ctx, |_| {}).unwrap();
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3", "4", "5", "6"]);

    undo_transfer(&env.ctx, &outcome.transaction_id).unwrap();
    assert_eq!(fs::read(&env.source).unwrap(), src_before, "copy undo never touches the source");
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3"]);
    let tgt_doc = parse_xml(&fs::read(&env.target).unwrap()).unwrap();
    assert_eq!(tgt_doc.root.get_attr("NEXTUNIQUEID"), Some("7"));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();

    // ── Missing undo artifacts are an explicit error, never a silent no-op ──
    let env = setup_env("f08x");
    assert!(matches!(
        undo_transfer(&env.ctx, "txn-never-happened"),
        Err(TransferError::UndoArtifactsMissing { .. })
    ));
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════════ F09: concurrent external edits are detected, duplicate kept ═══════════════

/// QA-M10-F09: fingerprint revalidation guards BOTH boundaries — executing a
/// plan whose source changed externally aborts before any side effect, and
/// resolving an interrupted move after an external source edit refuses to
/// delete source data (SourceConcurrentlyModified), keeping the duplicate.
#[test]
fn qa_m10_f09_concurrent_source_edit_fails_safe_with_duplicate() {
    // ── Execute-time revalidation: plan, external edit, execute → abort ──
    let env = setup_env("f09a");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_bytes = fs::read(&env.source).unwrap();
    let tgt_bytes = fs::read(&env.target).unwrap();
    let plan = plan_transfer(&env.source, &env.target, &TaskId::new("5"), &plan_opts(TransferOperation::Move, "txn-f09a")).unwrap();
    let mut edited = src_bytes.clone();
    edited.extend_from_slice(b"<!-- edited externally -->");
    fs::write(&env.source, &edited).unwrap();
    let err = execute_transfer(plan, &env.ctx, &TransferHooks::default(), |_| {}).unwrap_err();
    assert!(matches!(err, TransferError::SourceConcurrentlyModified { .. }), "got {err:?}");
    assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes, "target untouched by the aborted execute");
    // Whatever journal state the aborted execute left, recovery must not
    // claim anything was committed.
    for report in recover_interrupted_transfers(&env.ctx.journal_dir) {
        assert_ne!(report.action, TransferRecoveryAction::Completed, "nothing may complete from an aborted execute");
    }
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();

    // ── Resolution-time revalidation: crash, external edit, resolve → refuse ──
    let env = setup_env("f09b");
    let (src_titles, tgt_titles) = fixture_titles();
    run_killed(&env, TransferOperation::Move, "txn-f09b", TransferPhase::BackupSource);
    let mut edited = fs::read(&env.source).unwrap();
    edited.extend_from_slice(b"<!-- edited while interrupted -->");
    fs::write(&env.source, &edited).unwrap();

    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports[0].action, TransferRecoveryAction::CompletedWithDuplicate);
    let err = complete_interrupted_move(&env.ctx.journal_dir, "txn-f09b").unwrap_err();
    assert!(matches!(err, TransferError::SourceConcurrentlyModified { .. }), "got {err:?}");

    // Fail-safe: the subtree lives in BOTH documents (duplicate, not loss).
    assert!(task_ids(&env.source).contains(&"5".to_string()));
    assert!(task_ids(&env.target).contains(&"4".to_string()));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════════ F10: copy into a fresh managed document + trash lifecycle ═══════════════

/// QA-M10-F10: the full cross-document lifecycle — create a fresh managed
/// document through the file library, copy a subtree (with its assets) into
/// it, delete the document to the trash, and restore it: at every step the
/// union of source + destination (+ trash store) holds every task, and the
/// copied tasks, remapped dependencies and staged asset bytes survive the
/// trash round-trip intact.
#[test]
fn qa_m10_f10_copy_into_new_document_then_trash_restore_no_loss() {
    let dir = std::env::temp_dir().join(format!("mtdl_rc_f10_{}", uuid::Uuid::new_v4()));
    let src_dir = dir.join("origin");
    let ws_dir = dir.join("workspace");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(ws_dir.join("assets")).unwrap();
    fs::copy(fixtures_root().join("source-project.xml"), src_dir.join("source-project.xml")).unwrap();
    fs::create_dir_all(src_dir.join("assets")).unwrap();
    fs::copy(fixtures_root().join("assets").join("spec.txt"), src_dir.join("assets").join("spec.txt")).unwrap();
    let source = src_dir.join("source-project.xml");
    let src_titles = task_titles(&source);

    // Fresh managed target document through the real file library.
    let mut ws = workspace::Workspace::create(&ws_dir, "RC F10".into()).unwrap();
    let (target_doc_id, _receipt) = file_library::create_managed_document(&mut ws, "inbox.xml", "RC Inbox").unwrap();
    let target = ws_dir.join("inbox.xml");
    assert!(target.is_file());
    assert_eq!(parse_xml(&fs::read(&target).unwrap()).unwrap().root.get_attr("NEXTUNIQUEID"), Some("1"));

    // Copy the whole subtree into the fresh document (empty target id space:
    // 5→1, 6→2, 7→3), staging the relative asset next to the target.
    let ctx = TransferContext::for_workspace_root(&ws_dir);
    let outcome = copy_task(&source, &target, &TaskId::new("5"), &plan_opts(TransferOperation::Copy, "txn-f10"), &ctx, |_| {}).unwrap();
    assert_eq!(outcome.target_task_ids.iter().map(|t| t.as_str()).collect::<Vec<_>>(), vec!["1", "2", "3"]);
    assert_eq!(task_ids(&target), vec!["1", "2", "3"]);
    let staged = ws_dir.join("assets").join("spec.txt");
    assert!(staged.is_file(), "copy-required asset staged next to the target");
    assert_eq!(fs::read(&staged).unwrap(), fs::read(src_dir.join("assets").join("spec.txt")).unwrap());
    // External dep preserved with the FILENAME marker even though the target
    // now has its own task "1" — the marker keeps the reference unambiguous.
    let tgt_doc = parse_xml(&fs::read(&target).unwrap()).unwrap();
    assert_eq!(dep_task_ids(find_task_element(&tgt_doc.root, "2").unwrap()), vec![("1".to_string(), false), ("1".to_string(), true)]);
    assert_no_task_loss_paths(&source, &target, &src_titles);

    // Delete the target document to the trash: tasks leave the workspace but
    // are NOT destroyed — the trash store holds them.
    let entry = trash::delete_document_to_trash(&mut ws, &target_doc_id).unwrap();
    assert!(!target.exists());
    assert!(ws.find_document(&target_doc_id).is_none());
    let stored_titles = task_titles(&entry.stored_path);
    for title in ["Release Epic", "Build Installer", "Sign Binaries"] {
        assert!(stored_titles.contains(title), "trashed store lost {title:?}");
    }
    // Union of source + trash store still holds everything.
    let union: HashSet<String> = src_titles.union(&stored_titles).cloned().collect();
    assert!(union.is_superset(&src_titles));

    // Restore: same DocumentId, tasks and remapped deps intact, asset still
    // resolvable next to the document.
    let restored = trash::restore(&ws_dir, &entry.trash_id, Some(&mut ws)).unwrap();
    assert_eq!(restored, target);
    assert!(target.is_file());
    assert!(ws.find_document(&target_doc_id).is_some(), "DocumentId survives the trash round-trip");
    assert_eq!(task_ids(&target), vec!["1", "2", "3"]);
    let restored_doc = parse_xml(&fs::read(&target).unwrap()).unwrap();
    assert_eq!(dep_task_ids(find_task_element(&restored_doc.root, "2").unwrap()), vec![("1".to_string(), false), ("1".to_string(), true)]);
    assert!(staged.is_file());
    assert!(trash::list_trash(&ws_dir).unwrap().is_empty());
    assert_no_task_loss_paths(&source, &target, &src_titles);

    fs::remove_dir_all(&dir).ok();
}

/// No-loss invariant for F10 (destination path varies across the lifecycle).
fn assert_no_task_loss_paths(source: &Path, destination: &Path, src_titles: &HashSet<String>) {
    let union: HashSet<String> = task_titles(source).union(&task_titles(destination)).cloned().collect();
    for title in src_titles {
        assert!(union.contains(title), "TASK LOST: {title:?}");
    }
}
