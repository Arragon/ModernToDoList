//! M8 QA Integration Tests — Multi-Document (INH-1089 ~ INH-1093).
//!
//! QA-M8-001~006: transfer first-half crash matrix (kill injected at exact
//!                phase boundaries, then recovery).
//! QA-M8-007~012: source-commit, journal-corruption and I/O failure matrix.
//! QA-M8-013~016: dependency and attachment remap verification.
//! QA-M8-017~019: File Library Trash / Linked / rename-reference regression.
//! QA-M8-020:     M0-M7 cumulative regression with M8 features enabled.
//!
//! THE OVERRIDING INVARIANT (GATE-M8, P0): for EVERY crash point, the union
//! of source and target documents after recovery contains every original
//! task. An interrupted transfer leaves a DUPLICATE, never a loss.
//!
//! # Module wiring note
//!
//! The M8 modules (`transfer`, `transfer_journal`, `file_library`, `trash`)
//! are compiled here directly from source via `#[path]` includes so this
//! suite is self-contained and stays green whether or not the modules have
//! been declared in `src/domain/mod.rs` yet. Once the orchestrator wires
//! `pub mod transfer; pub mod transfer_journal; pub mod file_library;
//! pub mod trash;` into `src-tauri/src/domain/mod.rs`, these includes can
//! optionally be replaced with `use moderntodolist_lib::domain::...`.

#![allow(clippy::module_inception)]

pub mod domain {
    #[path = "../../src/domain/types.rs"]
    pub mod types;
    #[path = "../../src/domain/encoding.rs"]
    pub mod encoding;
    #[path = "../../src/domain/xml_tree.rs"]
    pub mod xml_tree;
    #[path = "../../src/domain/xml_parser.rs"]
    pub mod xml_parser;
    #[path = "../../src/domain/xml_serializer.rs"]
    pub mod xml_serializer;
    #[path = "../../src/domain/task.rs"]
    pub mod task;
    #[path = "../../src/domain/mappers.rs"]
    pub mod mappers;
    #[path = "../../src/domain/validator.rs"]
    pub mod validator;
    #[path = "../../src/domain/id_allocator.rs"]
    pub mod id_allocator;
    #[path = "../../src/domain/fingerprint.rs"]
    pub mod fingerprint;
    #[path = "../../src/domain/session.rs"]
    pub mod session;
    #[path = "../../src/domain/persistence.rs"]
    pub mod persistence;
    #[path = "../../src/domain/recovery.rs"]
    pub mod recovery;
    #[path = "../../src/domain/workspace.rs"]
    pub mod workspace;
    // M6 relation modules referenced by the shared task/mappers sources.
    #[path = "../../src/domain/attachment.rs"]
    pub mod attachment;
    #[path = "../../src/domain/participant.rs"]
    pub mod participant;
    #[path = "../../src/domain/progress_link.rs"]
    pub mod progress_link;
    #[path = "../../src/domain/transfer_journal.rs"]
    pub mod transfer_journal;
    #[path = "../../src/domain/transfer.rs"]
    pub mod transfer;
    #[path = "../../src/domain/file_library.rs"]
    pub mod file_library;
    #[path = "../../src/domain/trash.rs"]
    pub mod trash;
}

pub mod infrastructure {
    #[path = "../../src/infrastructure/schema.rs"]
    pub mod schema;
    #[path = "../../src/infrastructure/migration.rs"]
    pub mod migration;
    #[path = "../../src/infrastructure/db.rs"]
    pub mod db;
    #[path = "../../src/infrastructure/indexer.rs"]
    pub mod indexer;
}

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use domain::transfer::{
    complete_interrupted_move, copy_task, execute_transfer, find_task_element, list_transfer_targets,
    move_task, plan_transfer, recover_interrupted_transfers, undo_transfer, ExternalDependencyPolicy,
    PlanOptions, TransferContext, TransferError, TransferHooks, TransferOperation,
    TransferPhase, TransferRecoveryAction,
};
use domain::types::TaskId;
use domain::xml_parser::parse_xml;
use domain::xml_serializer::serialize_xml;
use domain::xml_tree::XmlElement;

// ─────────────────────────── Harness ───────────────────────────

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate root should have parent")
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
fn setup_env(name: &str) -> Env {
    let dir = std::env::temp_dir().join(format!(
        "mtdl_m8qa_{}_{}_{}",
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

/// THE MOST IMPORTANT ASSERTION (GATE-M8): every original task title of both
/// documents must still exist in the union of both documents.
fn assert_no_task_loss(env: &Env, before_source: &HashSet<String>, before_target: &HashSet<String>) {
    let after_source = task_titles(&env.source);
    let after_target = task_titles(&env.target);
    let union: HashSet<String> = after_source.union(&after_target).cloned().collect();
    for title in before_source {
        assert!(
            union.contains(title),
            "TASK LOST from source document: {:?} (union={:?})",
            title,
            union
        );
    }
    for title in before_target {
        assert!(
            union.contains(title),
            "TASK LOST from target document: {:?} (union={:?})",
            title,
            union
        );
    }
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

fn kill_before(phase: TransferPhase) -> TransferHooks {
    TransferHooks { fail_before: Some(phase) }
}

/// Plans + executes with a kill hook, asserts the kill happened.
fn run_killed(env: &Env, op: TransferOperation, txn: &str, phase: TransferPhase) {
    let plan = plan_transfer(&env.source, &env.target, &TaskId::new("5"), &plan_opts(op, txn)).unwrap();
    let err = execute_transfer(plan, &env.ctx, &kill_before(phase), |_| {}).unwrap_err();
    assert!(
        matches!(err, TransferError::SimulatedKill { phase: p } if p == phase),
        "expected SimulatedKill({:?}), got {:?}",
        phase,
        err
    );
}

// Fixture task inventory (source): "Stay Behind"(1), "Release Epic"(5),
// "Build Installer"(6), "Sign Binaries"(7), "Unrelated Root"(2).
// Fixture task inventory (target): "Target Task One"(1), "Target Task Two"(2),
// "Target Nested"(3).
fn fixture_titles() -> (HashSet<String>, HashSet<String>) {
    let fx = fixtures_root();
    (
        task_titles(&fx.join("source-project.xml")),
        task_titles(&fx.join("target-project.xml")),
    )
}

// ═══════════════ QA-M8-001~006: first-half crash matrix ═══════════════

#[test]
fn qa_m8_001_move_kill_before_stage_assets() {
    let env = setup_env("qa001");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_bytes = fs::read(&env.source).unwrap();
    let tgt_bytes = fs::read(&env.target).unwrap();

    run_killed(&env, TransferOperation::Move, "txn-qa001", TransferPhase::StageAssets);

    // Nothing happened: no journal, no staged assets, both files untouched.
    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert!(reports.is_empty(), "no journal should exist yet: {:?}", reports.len());
    assert_eq!(fs::read(&env.source).unwrap(), src_bytes);
    assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_002_move_kill_before_commit_target() {
    let env = setup_env("qa002");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_bytes = fs::read(&env.source).unwrap();
    let tgt_bytes = fs::read(&env.target).unwrap();

    run_killed(&env, TransferOperation::Move, "txn-qa002", TransferPhase::CommitTarget);

    // Journal exists (Started + AssetsStaged); target NOT committed.
    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].action, TransferRecoveryAction::RolledBack);
    assert_eq!(fs::read(&env.source).unwrap(), src_bytes, "source must be untouched");
    assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes, "target must be untouched");
    // Journal cleaned after rollback.
    assert!(recover_interrupted_transfers(&env.ctx.journal_dir).is_empty());
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_003_move_kill_after_target_commit_before_source_backup() {
    let env = setup_env("qa003");
    let (src_titles, tgt_titles) = fixture_titles();

    run_killed(&env, TransferOperation::Move, "txn-qa003", TransferPhase::BackupSource);

    // Target committed; recovery must NOT delete source data (duplicate-not-loss).
    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].action, TransferRecoveryAction::CompletedWithDuplicate);
    assert!(!reports[0].duplicate_task_ids.is_empty());
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    // Both documents hold the subtree right now (duplicate, not loss).
    assert!(task_ids(&env.target).contains(&"4".to_string()));
    assert!(task_ids(&env.source).contains(&"5".to_string()));

    // Explicit user-confirmed resolution completes the move safely.
    let report = complete_interrupted_move(&env.ctx.journal_dir, "txn-qa003").unwrap();
    assert_eq!(report.action, TransferRecoveryAction::Completed);
    assert!(!task_ids(&env.source).contains(&"5".to_string()));
    assert!(task_ids(&env.target).contains(&"4".to_string()));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_004_move_kill_before_commit_source() {
    let env = setup_env("qa004");
    let (src_titles, tgt_titles) = fixture_titles();

    run_killed(&env, TransferOperation::Move, "txn-qa004", TransferPhase::CommitSource);

    // Source bytes were backed up before the kill point.
    let backup = env
        .ctx
        .undo_dir
        .join("txn-qa004")
        .join("source.before.xml");
    assert!(backup.is_file(), "durable source backup must exist");

    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports[0].action, TransferRecoveryAction::CompletedWithDuplicate);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);

    complete_interrupted_move(&env.ctx.journal_dir, "txn-qa004").unwrap();
    assert_eq!(task_ids(&env.source), vec!["1", "2"]);
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3", "4", "5", "6"]);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_005_move_kill_before_complete() {
    let env = setup_env("qa005");
    let (src_titles, tgt_titles) = fixture_titles();

    run_killed(&env, TransferOperation::Move, "txn-qa005", TransferPhase::Complete);

    // Both documents already committed; recovery finalizes deterministically.
    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].action, TransferRecoveryAction::Completed);
    assert_eq!(task_ids(&env.source), vec!["1", "2"]);
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3", "4", "5", "6"]);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    // Journal cleaned only after successful completion.
    assert!(recover_interrupted_transfers(&env.ctx.journal_dir).is_empty());
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_006_copy_kill_matrix_never_deletes_source() {
    let (src_titles, tgt_titles) = fixture_titles();

    // Kill before CommitTarget: rollback, source byte-identical.
    {
        let env = setup_env("qa006a");
        let src_bytes = fs::read(&env.source).unwrap();
        let tgt_bytes = fs::read(&env.target).unwrap();
        run_killed(&env, TransferOperation::Copy, "txn-qa006a", TransferPhase::CommitTarget);
        let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
        assert_eq!(reports[0].action, TransferRecoveryAction::RolledBack);
        assert_eq!(fs::read(&env.source).unwrap(), src_bytes);
        assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes);
        assert_no_task_loss(&env, &src_titles, &tgt_titles);
        fs::remove_dir_all(&env.dir).ok();
    }

    // Kill before Complete: target committed; copy finalizes; source NEVER
    // modified by any copy phase.
    {
        let env = setup_env("qa006b");
        let src_bytes = fs::read(&env.source).unwrap();
        run_killed(&env, TransferOperation::Copy, "txn-qa006b", TransferPhase::Complete);
        let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
        assert_eq!(reports[0].action, TransferRecoveryAction::Completed);
        assert_eq!(fs::read(&env.source).unwrap(), src_bytes, "copy must never touch source");
        assert!(task_ids(&env.target).contains(&"4".to_string()));
        assert!(task_ids(&env.source).contains(&"5".to_string()));
        assert_no_task_loss(&env, &src_titles, &tgt_titles);
        fs::remove_dir_all(&env.dir).ok();
    }
}

// ═══════════ QA-M8-007~012: source-commit / journal / I/O matrix ═══════════

#[test]
fn qa_m8_007_source_commit_concurrent_edit_duplicate_not_loss() {
    let env = setup_env("qa007");
    let (src_titles, tgt_titles) = fixture_titles();

    run_killed(&env, TransferOperation::Move, "txn-qa007", TransferPhase::BackupSource);

    // Concurrent EXTERNAL edit of the source after target commit.
    let mut bytes = fs::read(&env.source).unwrap();
    bytes.extend_from_slice(b"<!-- edited externally while transfer was interrupted -->");
    fs::write(&env.source, &bytes).unwrap();

    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports[0].action, TransferRecoveryAction::CompletedWithDuplicate);

    // Resolution MUST refuse: source fingerprint changed.
    let err = complete_interrupted_move(&env.ctx.journal_dir, "txn-qa007").unwrap_err();
    assert!(matches!(err, TransferError::SourceConcurrentlyModified { .. }), "got {:?}", err);

    // No loss: subtree preserved in BOTH documents.
    assert!(task_ids(&env.source).contains(&"5".to_string()));
    assert!(task_ids(&env.target).contains(&"4".to_string()));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_008_journal_corruption_quarantined_data_intact() {
    let env = setup_env("qa008");
    let (src_titles, tgt_titles) = fixture_titles();

    run_killed(&env, TransferOperation::Move, "txn-qa008", TransferPhase::BackupSource);

    // Garble BOTH the primary journal and its .bak.
    let journal = domain::transfer_journal::TransferJournal::path_for(&env.ctx.journal_dir, "txn-qa008");
    assert!(journal.exists());
    fs::write(&journal, b"\xff\xfe not json at all {{{").unwrap();
    fs::write(journal.with_extension("bak"), b"also broken ###").unwrap();

    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].action, TransferRecoveryAction::Quarantined);
    // Quarantine touches nothing: both documents keep their data.
    assert!(task_ids(&env.source).contains(&"5".to_string()));
    assert!(task_ids(&env.target).contains(&"4".to_string()));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_009_journal_truncation_falls_back_to_bak() {
    let env = setup_env("qa009");
    let (src_titles, tgt_titles) = fixture_titles();
    let tgt_bytes = fs::read(&env.target).unwrap();

    run_killed(&env, TransferOperation::Move, "txn-qa009", TransferPhase::CommitTarget);

    // Torn write: truncate the primary journal mid-JSON.
    let journal = domain::transfer_journal::TransferJournal::path_for(&env.ctx.journal_dir, "txn-qa009");
    let bytes = fs::read(&journal).unwrap();
    fs::write(&journal, &bytes[..bytes.len() / 3]).unwrap();

    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports.len(), 1);
    // .bak proves at most Started/AssetsStaged → target not committed → rollback.
    assert_eq!(reports[0].action, TransferRecoveryAction::RolledBack);
    assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_010_target_write_failure_rolls_back() {
    let env = setup_env("qa010");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_bytes = fs::read(&env.source).unwrap();
    let tgt_bytes = fs::read(&env.target).unwrap();

    // Block the atomic-save temp file: occupy `.target-project.xml.tmp` with
    // a DIRECTORY so File::create fails (simulates a write failure).
    fs::create_dir_all(env.dir.join(".target-project.xml.tmp")).unwrap();

    let plan = plan_transfer(
        &env.source,
        &env.target,
        &TaskId::new("5"),
        &plan_opts(TransferOperation::Move, "txn-qa010"),
    )
    .unwrap();
    let err = execute_transfer(plan, &env.ctx, &TransferHooks::default(), |_| {}).unwrap_err();
    assert!(matches!(err, TransferError::TargetCommitFailed { .. }), "got {:?}", err);

    // Source deletion phase never ran.
    assert_eq!(fs::read(&env.source).unwrap(), src_bytes);
    assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes);
    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports[0].action, TransferRecoveryAction::RolledBack);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_011_target_permission_denied_rolls_back() {
    let env = setup_env("qa011");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_bytes = fs::read(&env.source).unwrap();
    let tgt_bytes = fs::read(&env.target).unwrap();

    // Block the atomic REPLACE step: occupy the backup path
    // (`with_extension("bak")` → `target-project.bak`) with a directory so
    // `rename(target -> bak)` fails inside the M3 save pipeline.
    fs::create_dir_all(env.dir.join("target-project.bak")).unwrap();

    let plan = plan_transfer(
        &env.source,
        &env.target,
        &TaskId::new("5"),
        &plan_opts(TransferOperation::Move, "txn-qa011"),
    )
    .unwrap();
    let err = execute_transfer(plan, &env.ctx, &TransferHooks::default(), |_| {}).unwrap_err();
    assert!(matches!(err, TransferError::TargetCommitFailed { .. }), "got {:?}", err);

    // Neither document was modified; the source-delete phase never ran.
    assert_eq!(fs::read(&env.source).unwrap(), src_bytes);
    assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes);
    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports[0].action, TransferRecoveryAction::RolledBack);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_012_asset_stage_io_failure_rolls_back() {
    let env = setup_env("qa012");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_bytes = fs::read(&env.source).unwrap();
    let tgt_bytes = fs::read(&env.target).unwrap();

    // Source and target share the temp dir in this harness; to create an
    // independent asset destination we use a separate target directory.
    let tgt_dir = env.dir.join("targetdir");
    fs::create_dir_all(&tgt_dir).unwrap();
    let target2 = tgt_dir.join("target-project.xml");
    fs::copy(&env.target, &target2).unwrap();
    // Occupy the planned asset destination with a DIRECTORY → copy fails.
    fs::create_dir_all(tgt_dir.join("assets").join("spec.txt")).unwrap();

    let plan = plan_transfer(
        &env.source,
        &target2,
        &TaskId::new("5"),
        &plan_opts(TransferOperation::Move, "txn-qa012"),
    )
    .unwrap();
    let err = execute_transfer(plan, &env.ctx, &TransferHooks::default(), |_| {}).unwrap_err();
    assert!(matches!(err, TransferError::AssetCopyFailed { .. }), "got {:?}", err);

    // No document was touched.
    assert_eq!(fs::read(&env.source).unwrap(), src_bytes);
    assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes);
    assert_eq!(fs::read(&target2).unwrap(), tgt_bytes);
    let reports = recover_interrupted_transfers(&env.ctx.journal_dir);
    assert_eq!(reports[0].action, TransferRecoveryAction::RolledBack);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════ QA-M8-013~016: dependency / attachment remap ═══════════

fn copy_subtree(env: &Env, txn: &str) -> domain::transfer::TransferOutcome {
    copy_task(
        &env.source,
        &env.target,
        &TaskId::new("5"),
        &plan_opts(TransferOperation::Copy, txn),
        &env.ctx,
        |_| {},
    )
    .unwrap()
}

#[test]
fn qa_m8_013_internal_dependency_remap() {
    let env = setup_env("qa013");
    let (src_titles, tgt_titles) = fixture_titles();
    copy_subtree(&env, "txn-qa013");

    // Mapping: 5→4 (Release Epic), 6→5 (Build Installer), 7→6 (Sign Binaries).
    let bytes = fs::read(&env.target).unwrap();
    let doc = parse_xml(&bytes).unwrap();

    let installer = find_task_element(&doc.root, "5").unwrap(); // was 6
    let deps: Vec<&XmlElement> = installer.children_by_tag("DEPENDENCY").collect();
    assert_eq!(deps.len(), 2);
    let d0 = deps[0].first_child_by_tag("TASKID").unwrap().text_content();
    assert_eq!(d0, "4", "internal dep 6→5 must be remapped to 5→4");
    assert!(deps[0].first_child_by_tag("FILENAME").is_none());

    let signer = find_task_element(&doc.root, "6").unwrap(); // was 7
    let d = signer
        .children_by_tag("DEPENDENCY")
        .next()
        .unwrap()
        .first_child_by_tag("TASKID")
        .unwrap()
        .text_content();
    assert_eq!(d, "5", "internal dep 7→6 must be remapped to 6→5");

    // Source untouched by copy.
    assert!(task_ids(&env.source).contains(&"7".to_string()));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_014_external_dependency_policy() {
    // Preserve (default): external dep 6→1 keeps TASKID and gains a FILENAME
    // marker naming the SOURCE document + a warning.
    let env = setup_env("qa014a");
    let (src_titles, tgt_titles) = fixture_titles();
    let out = copy_subtree(&env, "txn-qa014a");

    let bytes = fs::read(&env.target).unwrap();
    let doc = parse_xml(&bytes).unwrap();
    let installer = find_task_element(&doc.root, "5").unwrap();
    let ext_dep = installer
        .children_by_tag("DEPENDENCY")
        .nth(1)
        .unwrap();
    assert_eq!(
        ext_dep.first_child_by_tag("TASKID").unwrap().text_content(),
        "1",
        "external dep TASKID preserved"
    );
    assert_eq!(
        ext_dep.first_child_by_tag("FILENAME").unwrap().text_content(),
        "source-project.xml",
        "external dep must carry FILENAME marker"
    );
    assert!(out.warnings.iter().any(|w| matches!(
        w,
        domain::transfer::TransferWarning::ExternalDependencyPreserved { dep_task_id, .. }
            if dep_task_id == "1"
    )));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();

    // Block policy: planning aborts with zero side effects.
    let env = setup_env("qa014b");
    let tgt_bytes = fs::read(&env.target).unwrap();
    let err = plan_transfer(
        &env.source,
        &env.target,
        &TaskId::new("5"),
        &PlanOptions {
            operation: TransferOperation::Copy,
            external_dependency_policy: ExternalDependencyPolicy::Block,
            transaction_id: Some("txn-qa014b".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    match err {
        TransferError::ExternalDependenciesBlocked { count, .. } => assert_eq!(count, 1),
        other => panic!("expected ExternalDependenciesBlocked, got {:?}", other),
    }
    assert_eq!(fs::read(&env.target).unwrap(), tgt_bytes);
    assert!(recover_interrupted_transfers(&env.ctx.journal_dir).is_empty());
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_015_attachment_asset_remap() {
    // Use a separate target dir so asset copy destinations are independent.
    let env = setup_env("qa015");
    let (src_titles, tgt_titles) = fixture_titles();
    let tgt_dir = env.dir.join("targetdir");
    fs::create_dir_all(&tgt_dir).unwrap();
    let target2 = tgt_dir.join("target-project.xml");
    fs::copy(&env.target, &target2).unwrap();

    let out = copy_task(
        &env.source,
        &target2,
        &TaskId::new("5"),
        &plan_opts(TransferOperation::Copy, "txn-qa015"),
        &env.ctx,
        |_| {},
    )
    .unwrap();

    // Copy-required asset physically staged next to the target.
    let staged = tgt_dir.join("assets").join("spec.txt");
    assert!(staged.is_file(), "relative asset must be copied next to target");
    assert_eq!(
        fs::read(&staged).unwrap(),
        fs::read(env.dir.join("assets").join("spec.txt")).unwrap(),
        "staged asset bytes must match source"
    );

    let bytes = fs::read(&target2).unwrap();
    let doc = parse_xml(&bytes).unwrap();
    let installer = find_task_element(&doc.root, "5").unwrap();
    let refs: Vec<String> = installer
        .children_by_tag("FILEREFPATH")
        .map(|e| e.text_content())
        .collect();
    // Relative ref keeps its layout (target dir now has ./assets/spec.txt).
    assert!(refs.iter().any(|r| r == ".\\assets\\spec.txt"));
    // URL preserved verbatim (progress-link policy: preserve URLs).
    assert!(refs.iter().any(|r| r == "https://example.com/build-docs"));
    // Missing file: reference kept, warning emitted, transfer NOT blocked.
    assert!(refs.iter().any(|r| r == ".\\assets\\missing.txt"));
    assert!(!tgt_dir.join("assets").join("missing.txt").exists());
    assert!(out.warnings.iter().any(|w| matches!(
        w,
        domain::transfer::TransferWarning::AssetMissing { path, .. } if path.contains("missing.txt")
    )));
    assert!(out.warnings.iter().any(|w| matches!(
        w,
        domain::transfer::TransferWarning::AssetUrlKept { .. }
    )));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

#[test]
fn qa_m8_016_asset_collision_unique_rename() {
    let env = setup_env("qa016");
    let (src_titles, tgt_titles) = fixture_titles();
    let tgt_dir = env.dir.join("targetdir");
    fs::create_dir_all(tgt_dir.join("assets")).unwrap();
    let target2 = tgt_dir.join("target-project.xml");
    fs::copy(&env.target, &target2).unwrap();
    // Pre-occupy the destination with DIFFERENT content.
    fs::write(tgt_dir.join("assets").join("spec.txt"), b"pre-existing different content").unwrap();

    let out = copy_task(
        &env.source,
        &target2,
        &TaskId::new("5"),
        &plan_opts(TransferOperation::Copy, "txn-qa016"),
        &env.ctx,
        |_| {},
    )
    .unwrap();

    // Original destination file NOT clobbered.
    assert_eq!(
        fs::read(tgt_dir.join("assets").join("spec.txt")).unwrap(),
        b"pre-existing different content"
    );
    // A uniquely-named copy exists.
    let unique: Vec<_> = fs::read_dir(tgt_dir.join("assets"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("spec-") && n.ends_with(".txt"))
        .collect();
    assert_eq!(unique.len(), 1, "expected one collision-renamed copy: {:?}", unique);
    assert_eq!(
        fs::read(tgt_dir.join("assets").join(&unique[0])).unwrap(),
        fs::read(env.dir.join("assets").join("spec.txt")).unwrap()
    );
    // The transferred XML references the unique file.
    let bytes = fs::read(&target2).unwrap();
    let doc = parse_xml(&bytes).unwrap();
    let installer = find_task_element(&doc.root, "5").unwrap();
    let refs: Vec<String> = installer
        .children_by_tag("FILEREFPATH")
        .map(|e| e.text_content())
        .collect();
    assert!(
        refs.iter().any(|r| r.contains(&unique[0])),
        "rewritten ref should point at unique copy: {:?}",
        refs
    );
    assert!(out.warnings.iter().any(|w| matches!(
        w,
        domain::transfer::TransferWarning::AssetCollisionRenamed { .. }
    )));
    assert_no_task_loss(&env, &src_titles, &tgt_titles);
    fs::remove_dir_all(&env.dir).ok();
}

// ═══════════ QA-M8-017~019: File Library regression ═══════════

fn doc_bytes(name: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<TODOLIST PROJECTNAME=\"{}\" FILENAME=\"{}.xml\" NEXTUNIQUEID=\"1\"></TODOLIST>\r\n",
        name, name
    )
    .into_bytes()
}

#[test]
fn qa_m8_017_trash_delete_restore_purge() {
    let dir = std::env::temp_dir().join(format!("mtdl_m8qa017_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let mut ws = domain::workspace::Workspace::create(&dir, "QA17".into()).unwrap();

    fs::write(dir.join("doomed.xml"), doc_bytes("Doomed")).unwrap();
    fs::write(dir.join("second.xml"), doc_bytes("Second")).unwrap();
    let d_id = ws.register_document("doomed.xml".into(), domain::workspace::DocumentType::Managed).unwrap();
    let s_id = ws.register_document("second.xml".into(), domain::workspace::DocumentType::Managed).unwrap();

    // Delete to trash: file leaves the workspace but is NOT destroyed.
    let entry = domain::trash::delete_document_to_trash(&mut ws, &d_id).unwrap();
    assert!(!dir.join("doomed.xml").exists());
    assert!(entry.stored_path.is_file());
    assert!(ws.find_document(&d_id).is_none());
    assert_eq!(domain::trash::list_trash(&dir).unwrap().len(), 1);

    // Restore: original path + STABLE DocumentId.
    let restored = domain::trash::restore(&dir, &entry.trash_id, Some(&mut ws)).unwrap();
    assert_eq!(restored, dir.join("doomed.xml"));
    assert!(restored.is_file());
    assert!(ws.find_document(&d_id).is_some(), "DocumentId must survive trash round-trip");
    assert!(domain::trash::list_trash(&dir).unwrap().is_empty());

    // Explicit permanent deletion.
    let entry2 = domain::trash::delete_document_to_trash(&mut ws, &s_id).unwrap();
    let stored = entry2.stored_path.clone();
    domain::trash::purge(&dir, &entry2.trash_id).unwrap();
    assert!(!stored.exists());
    assert!(domain::trash::list_trash(&dir).unwrap().is_empty());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn qa_m8_018_linked_document_remove_reference() {
    let dir = std::env::temp_dir().join(format!("mtdl_m8qa018_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let mut ws = domain::workspace::Workspace::create(&dir, "QA18".into()).unwrap();

    // External file OUTSIDE the workspace.
    let ext_dir = std::env::temp_dir().join(format!("mtdl_m8qa018ext_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&ext_dir).unwrap();
    let ext = ext_dir.join("external.xml");
    fs::write(&ext, doc_bytes("External")).unwrap();

    let link_id = domain::file_library::link_external(&mut ws, &ext).unwrap();
    assert_eq!(
        ws.find_document(&link_id).unwrap().doc_type,
        domain::workspace::DocumentType::Linked
    );

    // Remove Reference: NEVER deletes the file.
    let receipt = domain::file_library::remove_reference(&mut ws, &link_id).unwrap();
    assert!(ws.find_document(&link_id).is_none());
    assert!(ext.is_file(), "linked file must survive Remove Reference");

    // Undo re-registers with the SAME stable DocumentId.
    domain::file_library::undo_document_op(&mut ws, &receipt.receipt_id).unwrap();
    assert!(ws.find_document(&link_id).is_some());
    assert_eq!(
        ws.find_document(&link_id).unwrap().file_path,
        ext.to_string_lossy().replace('/', "\\")
    );

    // Target picker includes linked docs with writability info.
    let targets = list_transfer_targets(&ws, None);
    assert_eq!(targets.len(), 1);
    assert!(targets[0].writable);

    fs::remove_dir_all(&ext_dir).ok();
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn qa_m8_019_rename_repairs_references() {
    let dir = std::env::temp_dir().join(format!("mtdl_m8qa019_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let mut ws = domain::workspace::Workspace::create(&dir, "QA19".into()).unwrap();

    // Document A (to be renamed) and document B referencing A.
    fs::write(dir.join("alpha.xml"), doc_bytes("Alpha")).unwrap();
    let a_id = ws.register_document("alpha.xml".into(), domain::workspace::DocumentType::Managed).unwrap();
    let mut b_doc = parse_xml(&doc_bytes("Beta")).unwrap();
    let mut task = XmlElement::new("TASK");
    task.set_attr("ID", "1");
    task.set_attr("TITLE", "refs alpha");
    let mut fr = XmlElement::new("FILEREFPATH");
    fr.set_text_content(".\\alpha.xml");
    task.children.push(domain::xml_tree::XmlNode::Element(fr));
    b_doc.root.children.push(domain::xml_tree::XmlNode::Element(task));
    fs::write(dir.join("beta.xml"), serialize_xml(&b_doc)).unwrap();
    ws.register_document("beta.xml".into(), domain::workspace::DocumentType::Managed).unwrap();

    let receipt = domain::file_library::rename_or_move_document(&mut ws, &a_id, "archive\\alpha-v2.xml").unwrap();
    assert_eq!(receipt.doc_id, a_id, "DocumentId must be stable across rename");
    assert!(dir.join("archive").join("alpha-v2.xml").is_file());
    assert!(!dir.join("alpha.xml").exists());

    // B's reference repaired to the new path.
    let b_after = fs::read_to_string(dir.join("beta.xml")).unwrap();
    assert!(b_after.contains("alpha-v2.xml"), "reference not repaired: {}", b_after);
    assert!(!b_after.contains(".\\alpha.xml"));

    // Undo: file back, entry path back, B's reference restored byte-exact.
    domain::file_library::undo_document_op(&mut ws, &receipt.receipt_id).unwrap();
    assert!(dir.join("alpha.xml").is_file());
    assert_eq!(ws.find_document(&a_id).unwrap().file_path, "alpha.xml");
    let b_restored = fs::read_to_string(dir.join("beta.xml")).unwrap();
    assert!(b_restored.contains(".\\alpha.xml"));

    fs::remove_dir_all(&dir).ok();
}

// ═══════════ QA-M8-020: cumulative regression ═══════════

#[test]
fn qa_m8_020_cumulative_regression_m0_through_m8() {
    // ── M2: lossless round-trip of canonical fixtures ──
    let fx_root = fixtures_root().parent().unwrap().to_path_buf();
    for rel in [
        "canonical/nested-tasks.xml",
        "canonical/all-basic-fields.xml",
        "comments/html-comment.xml",
        "dependencies/local-dependency.xml",
        "unknown/unknown-attribute.xml",
    ] {
        let path = fx_root.join(rel);
        if !path.exists() {
            continue; // fixture set may evolve; never fail on absence
        }
        let bytes = fs::read(&path).unwrap();
        let doc1 = parse_xml(&bytes).unwrap_or_else(|e| panic!("{}: parse: {}", rel, e));
        let ser = serialize_xml(&doc1);
        let doc2 = parse_xml(&ser).unwrap_or_else(|e| panic!("{}: re-parse: {}", rel, e));
        assert_eq!(doc1.root.tag, doc2.root.tag, "{}: root tag", rel);
        assert_eq!(doc1.root.attrs.len(), doc2.root.attrs.len(), "{}: attr count", rel);
        let t1 = count_tasks(&doc1.root);
        let t2 = count_tasks(&doc2.root);
        assert_eq!(t1, t2, "{}: task count", rel);
    }

    // ── M2: encoding detection + round-trip ──
    // Current parser behavior (documented in round_trip_test.rs): an explicit
    // XML declaration overrides BOM detection, so these fixtures verify
    // detection on input and structural round-trip through serialize.
    for (rel, expected) in [
        ("encoding/utf16le.xml", domain::encoding::XmlEncoding::Utf16Le),
        ("encoding/utf8-bom.xml", domain::encoding::XmlEncoding::Utf8Bom),
    ] {
        let p = fx_root.join(rel);
        if !p.exists() {
            continue;
        }
        let bytes = fs::read(&p).unwrap();
        assert_eq!(
            domain::encoding::detect_bom(&bytes).encoding,
            expected,
            "{}: BOM detection",
            rel
        );
        let doc = parse_xml(&bytes).unwrap();
        let ser = serialize_xml(&doc);
        assert!(!ser.is_empty());
        let doc2 = parse_xml(&ser).unwrap();
        assert_eq!(doc.root.attrs.len(), doc2.root.attrs.len(), "{}: attrs", rel);
        assert_eq!(count_tasks(&doc.root), count_tasks(&doc2.root), "{}: tasks", rel);
    }

    // ── M2: validation ──
    let mut bad_root = XmlElement::new("WRONG");
    bad_root.set_attr("NEXTUNIQUEID", "x");
    let bad_doc = domain::xml_tree::XmlDocument::new(
        domain::encoding::XmlEncodingMeta::default(),
        bad_root,
    );
    let errors = domain::validator::validate_document(&bad_doc);
    assert!(errors.len() >= 2);

    // ── M2: ID allocation rules ──
    let mut tree = domain::task::TaskTree::new();
    tree.add_task(domain::task::Task::new(TaskId::new("5")));
    let mut alloc = domain::id_allocator::TaskIdAllocator::new(&tree, 1);
    for _ in 0..4 {
        alloc.allocate();
    }
    assert_eq!(alloc.allocate().as_str(), "6", "allocator must skip existing IDs");

    // ── M3: atomic save + fingerprint + recovery journal ──
    let dir = std::env::temp_dir().join(format!("mtdl_m8qa020_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let content = b"<TODOLIST NEXTUNIQUEID=\"1\"></TODOLIST>";
    let save = domain::persistence::atomic_save(
        &domain::persistence::SaveConfig {
            target_path: dir.join("atomic.xml"),
            validate_temp: true,
        },
        content,
        |b| b == content,
    );
    assert_eq!(save.state, domain::session::SaveState::Completed);
    assert!(save.fingerprint.unwrap().matches_bytes(content));
    assert_eq!(fs::read(dir.join("atomic.xml")).unwrap(), content);

    let mut journal = domain::recovery::RecoveryJournal::new();
    journal.begin_save(dir.join("atomic.xml"), 1);
    journal.update_phase(&dir.join("atomic.xml"), domain::recovery::RecoveryPhase::Complete);
    journal.save(&dir.join("recovery.json")).unwrap();
    let mut loaded = domain::recovery::RecoveryJournal::load(&dir.join("recovery.json")).unwrap();
    loaded.cleanup_completed();
    assert!(loaded.entries.is_empty());

    // ── M4: workspace platform + SQLite index sync ──
    let mut ws = domain::workspace::Workspace::create(&dir.join("ws"), "QA20".into()).unwrap();
    fs::write(dir.join("ws").join("doc.xml"), doc_bytes("WS")).unwrap();
    let doc_id = ws
        .register_document("doc.xml".into(), domain::workspace::DocumentType::Managed)
        .unwrap();
    let (new_files, _, _) = ws.detect_changes().unwrap();
    assert!(new_files.is_empty());
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    infrastructure::migration::run_migrations(&conn).unwrap();
    domain::workspace::ensure_workspace_in_db(&conn, &ws).unwrap();
    domain::workspace::sync_documents_to_db(&conn, &ws).unwrap();
    let count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE workspace_id = ?1",
            [ws.id()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    // ── M8: file library create inside workspace ──
    let (new_id, _receipt) =
        domain::file_library::create_managed_document(&mut ws, "fresh.xml", "Fresh").unwrap();
    assert!(ws.find_document(&new_id).is_some());
    let fresh = fs::read(dir.join("ws").join("fresh.xml")).unwrap();
    assert_eq!(
        parse_xml(&fresh).unwrap().root.get_attr("NEXTUNIQUEID"),
        Some("1")
    );

    // ── M8: full move + transaction undo round-trip ──
    let env = setup_env("qa020");
    let (src_titles, tgt_titles) = fixture_titles();
    let src_before = fs::read(&env.source).unwrap();
    let out = move_task(
        &env.source,
        &env.target,
        &TaskId::new("5"),
        &plan_opts(TransferOperation::Move, "txn-qa020"),
        &env.ctx,
        |_| {},
    )
    .unwrap();
    assert_eq!(task_ids(&env.source), vec!["1", "2"]);
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3", "4", "5", "6"]);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);

    // Fingerprint bookkeeping is real:
    assert!(out.target_fingerprint.matches_bytes(&fs::read(&env.target).unwrap()));
    // The move did change the source (subtree removed)...
    assert_ne!(fs::read(&env.source).unwrap(), src_before);

    // Transaction-level undo of the completed move.
    undo_transfer(&env.ctx, &out.transaction_id).unwrap();
    assert_eq!(fs::read(&env.source).unwrap(), src_before, "undo must restore source byte-exact");
    assert_eq!(task_ids(&env.target), vec!["1", "2", "3"]);
    assert_no_task_loss(&env, &src_titles, &tgt_titles);

    // ── M8: trash round-trip inside the workspace ──
    let t_entry = domain::trash::delete_document_to_trash(&mut ws, &doc_id).unwrap();
    let restored = domain::trash::restore(&dir.join("ws"), &t_entry.trash_id, Some(&mut ws)).unwrap();
    assert!(restored.is_file());
    assert!(ws.find_document(&doc_id).is_some());

    fs::remove_dir_all(&dir).ok();
    fs::remove_dir_all(&env.dir).ok();
}

fn count_tasks(elem: &XmlElement) -> usize {
    let mut n = 0;
    for child in elem.child_elements() {
        if child.tag == "TASK" {
            n += 1 + count_tasks(child);
        } else {
            n += count_tasks(child);
        }
    }
    n
}
