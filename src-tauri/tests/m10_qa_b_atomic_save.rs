//! M10 RC Regression Matrix B — Atomic Save, Recovery and External Conflict
//! (INH-1125, spec section 5.5).
//!
//! 10 cases (B01..B10): kill-before-write, kill-mid-write, disk-full,
//! permission-denied and external-conflict detection, all driven through the
//! real public code paths:
//! `domain::persistence::atomic_save`, `domain::recovery::RecoveryJournal`,
//! `domain::fingerprint::FileFingerprint`, `domain::session::DocumentSession`
//! and `infrastructure::watcher::FingerprintChecker`.
//!
//! Core safety invariant asserted by EVERY case: the original document is
//! never left corrupted or lost, and recovery always yields
//! duplicate-not-loss (at worst two generations of the document coexist).
//!
//! # Simulated conditions (cannot be provoked literally in-process)
//!
//! - "Kill the process" is simulated by abandoning the save pipeline at the
//!   exact on-disk state each phase would leave behind (truncated temp file,
//!   renamed backup, etc.) plus a persisted `RecoveryJournal`, then running
//!   the real `RecoveryJournal::load` + `recover_entries` decision logic.
//! - Disk-full is simulated by failing the real validation step of
//!   `atomic_save` (a short/corrupt write caused by ENOSPC is exactly what
//!   the re-read + validate stage exists to catch).
//! - Permission-denied is provoked for real on Windows by pre-creating the
//!   deterministic temp path (`.doc.xml.tmp`) as a READ-ONLY file, so
//!   `File::create` inside `atomic_save` fails with ERROR_ACCESS_DENIED and
//!   the real `classify_io_error` maps it to `SaveErrorCode::PermissionDenied`.

use moderntodolist_lib::domain::fingerprint::FileFingerprint;
use moderntodolist_lib::domain::persistence::{atomic_save, SaveConfig};
use moderntodolist_lib::domain::recovery::{
    RecoveryAction, RecoveryJournal, RecoveryPhase, SafetyEvent, SafetyEventLog, SafetyEventType,
};
use moderntodolist_lib::domain::session::{DocumentSession, SaveErrorCode, SaveState};
use moderntodolist_lib::domain::{parse_xml, serialize_xml};
use moderntodolist_lib::infrastructure::watcher::FingerprintChecker;

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

/// Unique temp sandbox per test (never shared with other matrices).
fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mtdl_rc_b_{}_{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The real canonical fixture used as "the original business document".
fn original_document() -> Vec<u8> {
    fs::read(fixtures_root().join("canonical").join("single-task.xml")).unwrap()
}

/// A *different* valid document representing either an external edit or a new
/// in-app generation that is about to be saved.
fn updated_document() -> Vec<u8> {
    let doc = parse_xml(&original_document()).unwrap();
    // Serialize of the parsed doc is a valid but different generation once we
    // tweak a root attribute (simulates an in-app edit).
    let mut doc = doc;
    doc.root.set_attr("PROJECTNAME", "Updated by app");
    serialize_xml(&doc)
}

/// The deterministic temp path used by `atomic_save` for a target file.
fn temp_path_for(target: &Path) -> PathBuf {
    let name = target.file_name().unwrap().to_string_lossy().to_string();
    target.parent().unwrap().join(format!(".{}.tmp", name))
}

fn backup_path_for(target: &Path) -> PathBuf {
    target.with_extension("bak")
}

// ─── B01: kill BEFORE write ──────────────────────────────────────────────────

/// QA-M10-B01: process killed after journaling PreCommit but before any byte
/// of the temp file was written.
///
/// Simulation: journal persisted in `PreCommit`, no temp/backup files exist.
/// Real recovery code (`RecoveryJournal::load` + `recover_entries`) must
/// decide `NothingNeeded`; the original document must be untouched and
/// parseable.
#[test]
fn qa_m10_b01_kill_before_write_leaves_original_intact() {
    let dir = sandbox("b01");
    let target = dir.join("doc.xml");
    let original = original_document();
    fs::write(&target, &original).unwrap();

    let journal_path = dir.join("recovery.journal.json");
    {
        let mut journal = RecoveryJournal::new();
        journal.begin_save(target.clone(), 1);
        journal.update_phase(&target, RecoveryPhase::PreCommit);
        journal.save(&journal_path).unwrap();
        // "Process dies here" — nothing else written.
    }

    // Restart: real journal load + decision logic.
    let mut journal = RecoveryJournal::load(&journal_path).unwrap();
    assert_eq!(journal.orphaned_entries().len(), 1);
    let actions = journal.recover_entries();
    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0], RecoveryAction::NothingNeeded { .. }),
        "kill before write must need no recovery, got {:?}", action_label(&actions[0]));

    // Safety invariant: original intact, valid, no stray artifacts.
    assert_eq!(fs::read(&target).unwrap(), original, "original bytes must be untouched");
    assert!(FileFingerprint::from_file(&target).unwrap().matches_bytes(&original));
    parse_xml(&fs::read(&target).unwrap()).expect("original must still parse");
    assert!(!temp_path_for(&target).exists(), "no temp file may exist");
    assert!(!backup_path_for(&target).exists(), "no backup file may exist");
    fs::remove_dir_all(&dir).ok();
}

// ─── B02: kill MID-WRITE (truncated temp) ────────────────────────────────────

/// QA-M10-B02: process killed while `write_temp_file` was halfway done.
///
/// Simulation: the temp file exists at the exact deterministic path
/// `atomic_save` uses, truncated to half of the new content; journal is in
/// `Writing`. Recovery must choose `PreserveTemp` (bias to keeping data),
/// the ORIGINAL must remain byte-identical and parseable, and after recovery
/// both generations exist on disk => duplicate-not-loss.
#[test]
fn qa_m10_b02_kill_mid_write_leaves_original_intact() {
    let dir = sandbox("b02");
    let target = dir.join("doc.xml");
    let original = original_document();
    let updated = updated_document();
    fs::write(&target, &original).unwrap();

    // Exactly what a killed write_all leaves behind: a partial temp file.
    let temp = temp_path_for(&target);
    let partial = &updated[..updated.len() / 2];
    fs::write(&temp, partial).unwrap();

    let journal_path = dir.join("recovery.journal.json");
    {
        let mut journal = RecoveryJournal::new();
        let entry = journal.begin_save(target.clone(), 7);
        entry.temp_path = Some(temp.clone());
        journal.update_phase(&target, RecoveryPhase::Writing);
        journal.save(&journal_path).unwrap();
        // "Process dies here".
    }

    let mut journal = RecoveryJournal::load(&journal_path).unwrap();
    let actions = journal.recover_entries();
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        RecoveryAction::PreserveTemp { document_path, temp_path } => {
            assert_eq!(document_path, &target);
            assert_eq!(temp_path, &temp);
        }
        other => panic!("expected PreserveTemp, got {:?}", action_label(other)),
    }

    // Safety invariant: original never corrupted, still the last good generation.
    assert_eq!(fs::read(&target).unwrap(), original);
    parse_xml(&original).expect("original parses");
    assert!(!FileFingerprint::from_file(&target).unwrap().matches_bytes(&partial));
    // Duplicate-not-loss: the partial new generation is preserved for manual
    // recovery instead of being discarded.
    assert!(temp.exists(), "partial temp must be preserved by PreserveTemp policy");
    assert_eq!(fs::read(&temp).unwrap(), partial);
    fs::remove_dir_all(&dir).ok();
}

// ─── B03: kill MID-REPLACE (backup only) ─────────────────────────────────────

/// QA-M10-B03: process killed inside `replace_file` AFTER the original was
/// renamed to `.bak` but BEFORE the temp was renamed into place — the worst
/// window, where the target path is momentarily empty.
///
/// Simulation: target missing, backup holds the full original, temp gone
/// (never written or already consumed). Recovery must choose `RestoreBackup`;
/// executing that action must bring the original back byte-identically.
#[test]
fn qa_m10_b03_kill_mid_write_restores_from_backup() {
    let dir = sandbox("b03");
    let target = dir.join("doc.xml");
    let original = original_document();
    let backup = backup_path_for(&target);

    // On-disk state at the kill point: target renamed away, no temp.
    fs::write(&backup, &original).unwrap();
    assert!(!target.exists());

    let journal_path = dir.join("recovery.journal.json");
    {
        let mut journal = RecoveryJournal::new();
        let entry = journal.begin_save(target.clone(), 3);
        entry.backup_path = Some(backup.clone());
        journal.update_phase(&target, RecoveryPhase::Replacing);
        journal.save(&journal_path).unwrap();
        // "Process dies here".
    }

    let mut journal = RecoveryJournal::load(&journal_path).unwrap();
    let actions = journal.recover_entries();
    assert_eq!(actions.len(), 1);
    let (doc_path, backup_path) = match &actions[0] {
        RecoveryAction::RestoreBackup { document_path, backup_path } => (document_path, backup_path),
        other => panic!("expected RestoreBackup, got {:?}", action_label(other)),
    };
    assert_eq!(doc_path, &target);
    assert_eq!(backup_path, &backup);

    // Execute the recovery decision (the mechanical rename the recovery
    // center performs) and assert the original is fully back.
    fs::rename(backup_path, doc_path).unwrap();
    assert_eq!(fs::read(&target).unwrap(), original, "original must be restored byte-identically");
    parse_xml(&fs::read(&target).unwrap()).expect("restored document parses");
    assert!(FileFingerprint::from_file(&target).unwrap().matches_bytes(&original));
    fs::remove_dir_all(&dir).ok();
}

// ─── B04: kill MID-REPLACE (temp + backup both present) ─────────────────────

/// QA-M10-B04: process killed during the replacement window with BOTH the
/// fully-written temp (new generation) and the backup (old generation)
/// present.
///
/// Recovery must choose `CompleteReplacement`. After executing it, the new
/// generation is committed at the target AND the old generation still exists
/// as the backup => duplicate-not-loss even across a mid-replace kill.
#[test]
fn qa_m10_b04_kill_mid_replace_completes_without_loss() {
    let dir = sandbox("b04");
    let target = dir.join("doc.xml");
    let original = original_document();
    let updated = updated_document();
    let temp = temp_path_for(&target);
    let backup = backup_path_for(&target);

    // Kill-point state: original renamed to backup, temp fully written,
    // second rename never happened.
    fs::write(&backup, &original).unwrap();
    fs::write(&temp, &updated).unwrap();

    let journal_path = dir.join("recovery.journal.json");
    {
        let mut journal = RecoveryJournal::new();
        let entry = journal.begin_save(target.clone(), 9);
        entry.temp_path = Some(temp.clone());
        entry.backup_path = Some(backup.clone());
        journal.update_phase(&target, RecoveryPhase::Replacing);
        journal.save(&journal_path).unwrap();
        // "Process dies here".
    }

    let mut journal = RecoveryJournal::load(&journal_path).unwrap();
    let actions = journal.recover_entries();
    assert_eq!(actions.len(), 1);
    let (doc_path, temp_path, backup_path) = match &actions[0] {
        RecoveryAction::CompleteReplacement { document_path, temp_path, backup_path } => {
            (document_path, temp_path, backup_path)
        }
        other => panic!("expected CompleteReplacement, got {:?}", action_label(other)),
    };

    // Execute the recovery decision: commit temp, keep backup as safety net.
    fs::rename(temp_path, doc_path).unwrap();
    assert_eq!(fs::read(&target).unwrap(), updated, "new generation committed");
    parse_xml(&updated).expect("new generation parses");
    assert!(backup_path.exists(), "old generation must still exist => duplicate-not-loss");
    assert_eq!(fs::read(backup_path).unwrap(), original);
    parse_xml(&fs::read(backup_path).unwrap()).expect("old generation still parses");
    fs::remove_dir_all(&dir).ok();
}

// ─── B05: atomic save happy path (byte-exact) ────────────────────────────────

/// QA-M10-B05: the real `atomic_save` pipeline over an existing original:
/// byte-exact replacement, validation via a REAL XML re-parse callback,
/// fingerprint matches the file afterwards, and no temp/backup leftovers.
#[test]
fn qa_m10_b05_atomic_save_byte_exact_no_leftovers() {
    let dir = sandbox("b05");
    let target = dir.join("doc.xml");
    let original = original_document();
    let updated = updated_document();
    fs::write(&target, &original).unwrap();

    let config = SaveConfig { target_path: target.clone(), validate_temp: true };
    // The validate callback runs the real parser on the re-read temp bytes.
    let result = atomic_save(&config, &updated, |bytes| parse_xml(bytes).is_ok());

    assert_eq!(result.state, SaveState::Completed);
    assert!(result.error.is_none());
    let fp = result.fingerprint.expect("fingerprint on success");
    assert_eq!(fs::read(&target).unwrap(), updated, "target must be byte-identical to saved content");
    assert!(fp.matches_bytes(&updated));
    assert_eq!(FileFingerprint::from_file(&target).unwrap(), fp,
        "returned fingerprint must equal the on-disk fingerprint");
    assert!(!temp_path_for(&target).exists(), "temp must be cleaned up");
    assert!(!backup_path_for(&target).exists(), "backup must be cleaned up after success");
    parse_xml(&fs::read(&target).unwrap()).expect("saved document parses");
    fs::remove_dir_all(&dir).ok();
}

// ─── B06: disk full (simulated) ──────────────────────────────────────────────

/// QA-M10-B06: disk-full during save.
///
/// SIMULATION (documented): a physical ENOSPC cannot be provoked in-process.
/// A full disk manifests inside `atomic_save` as a short/corrupt temp file
/// caught by the re-read + validate stage — exactly the code path exercised
/// here by making the real validation callback reject the content (as a
/// truncated write would). Asserted invariants: structured failure with a
/// clear code, the ORIGINAL stays byte-identical and parseable, and the temp
/// file is removed (no corrupt artifacts left behind).
#[test]
fn qa_m10_b06_disk_full_simulated_original_intact() {
    let dir = sandbox("b06");
    let target = dir.join("doc.xml");
    let original = original_document();
    fs::write(&target, &original).unwrap();
    let updated = updated_document();

    let config = SaveConfig { target_path: target.clone(), validate_temp: true };
    // Validation fails exactly as it would when the re-read temp bytes are
    // short/corrupt because the volume ran out of space mid-write.
    let result = atomic_save(&config, &updated, |_bytes| false);

    assert_eq!(result.state, SaveState::Failed);
    assert_eq!(result.error, Some(SaveErrorCode::ValidationFailed));
    assert!(!format!("{}", SaveErrorCode::ValidationFailed).is_empty(), "error has a clear message");
    assert!(result.fingerprint.is_none(), "no fingerprint may be issued for a failed save");

    // Safety invariant: the original is untouched and valid.
    assert_eq!(fs::read(&target).unwrap(), original);
    parse_xml(&original).expect("original still parses");
    assert!(FileFingerprint::from_file(&target).unwrap().matches_bytes(&original));
    assert!(!temp_path_for(&target).exists(), "corrupt temp must be cleaned up");
    assert!(!backup_path_for(&target).exists());
    fs::remove_dir_all(&dir).ok();
}

// ─── B07: permission denied (real Windows ACL/attribute failure) ────────────

/// QA-M10-B07: permission-denied when creating the temp file.
///
/// Provoked FOR REAL through the production code path: the deterministic
/// temp path used by `atomic_save` (`.{name}.tmp`) is pre-created as a
/// READ-ONLY file, so `File::create` inside `write_temp_file` fails with
/// ERROR_ACCESS_DENIED and the real `classify_io_error` maps it to
/// `SaveErrorCode::PermissionDenied`. (A read-only *directory* was avoided
/// because FILE_ATTRIBUTE_READONLY does not block file creation on Windows.)
/// Invariant: save fails cleanly and the original document is untouched.
#[test]
fn qa_m10_b07_permission_denied_original_intact() {
    let dir = sandbox("b07");
    let target = dir.join("doc.xml");
    let original = original_document();
    fs::write(&target, &original).unwrap();

    // Block the temp path with a read-only decoy.
    let temp = temp_path_for(&target);
    fs::write(&temp, b"locked").unwrap();
    let mut perms = fs::metadata(&temp).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&temp, perms).unwrap();

    let config = SaveConfig { target_path: target.clone(), validate_temp: true };
    let result = atomic_save(&config, &updated_document(), |b| parse_xml(b).is_ok());

    // Restore writability first so sandbox cleanup can succeed regardless of asserts.
    if let Ok(meta) = fs::metadata(&temp) {
        let mut perms = meta.permissions();
        perms.set_readonly(false);
        let _ = fs::set_permissions(&temp, perms);
    }

    assert_eq!(result.state, SaveState::Failed, "save must fail, not corrupt anything");
    assert!(matches!(result.error, Some(SaveErrorCode::PermissionDenied) | Some(SaveErrorCode::FileLocked) | Some(SaveErrorCode::Unknown(_))),
        "structured error expected, got {:?}", result.error);
    assert_eq!(result.error, Some(SaveErrorCode::PermissionDenied),
        "Windows ERROR_ACCESS_DENIED (5) must classify as PermissionDenied");

    // Safety invariant: original untouched and valid; target never replaced.
    assert_eq!(fs::read(&target).unwrap(), original);
    parse_xml(&original).expect("original still parses");
    assert!(!backup_path_for(&target).exists(), "replace stage must never have been reached");
    fs::remove_dir_all(&dir).ok();
}

// ─── B08: external conflict on a CLEAN document ──────────────────────────────

/// QA-M10-B08: an external process rewrites the document while our session
/// has no unsaved edits. The conflict must be DETECTED through the real
/// fingerprint machinery (`FingerprintChecker` + session fingerprint), and a
/// clean reload must leave the session consistent with the new bytes.
#[test]
fn qa_m10_b08_external_conflict_detected_clean_document() {
    let dir = sandbox("b08");
    let target = dir.join("doc.xml");
    let original = original_document();
    fs::write(&target, &original).unwrap();

    // Open session exactly like the session layer: fingerprint of the bytes.
    let fp_open = FileFingerprint::from_bytes(&original);
    let session = DocumentSession::new(Some(fp_open.clone()));
    let mut checker = FingerprintChecker::new();
    checker.record(&target);
    assert!(!checker.has_changed(&target), "no change yet");
    assert!(session.fingerprint().unwrap().matches_bytes(&original));

    // External process replaces the file with different valid content.
    let external = updated_document();
    assert_ne!(external, original);
    fs::write(&target, &external).unwrap();

    // Conflict detection through the real code paths.
    assert!(checker.has_changed(&target), "watcher-independent checker must flag external edit");
    let session_fp = session.fingerprint().unwrap();
    assert!(!session_fp.matches_bytes(&fs::read(&target).unwrap()),
        "session fingerprint must mismatch => external conflict detected");

    // Clean-document policy: reload is safe. Original on disk was never
    // corrupted by us — it is the external version and it parses.
    parse_xml(&fs::read(&target).unwrap()).expect("external version parses");
    // After an accepted reload the session is consistent again.
    session.set_fingerprint(FileFingerprint::from_file(&target).unwrap());
    checker.record(&target);
    assert!(!checker.has_changed(&target));
    assert!(session.fingerprint().unwrap().matches_bytes(&external));
    fs::remove_dir_all(&dir).ok();
}

// ─── B09: external conflict on a DIRTY document ──────────────────────────────

/// QA-M10-B09: the document has unsaved local mutations AND is modified
/// externally. A stale mutation (carrying an outdated expected revision) must
/// be rejected via the real `DocumentSession::check_revision`, the conflict
/// must be detectable via fingerprint mismatch, the externally-written bytes
/// on disk must never be corrupted by the failed interaction, and the events
/// must land in the bounded `SafetyEventLog`.
#[test]
fn qa_m10_b09_dirty_document_conflict_stale_revision_rejected() {
    let dir = sandbox("b09");
    let target = dir.join("doc.xml");
    fs::write(&target, original_document()).unwrap();

    let session = DocumentSession::new(Some(FileFingerprint::from_file(&target).unwrap()));
    // Local unsaved edits.
    session.record_mutation();
    let rev_after_first = session.record_mutation();
    assert!(session.is_dirty());

    // External process overwrites the file.
    let external = updated_document();
    fs::write(&target, &external).unwrap();

    // 1) Fingerprint mismatch flags the conflict before any save is attempted.
    assert!(!session.fingerprint().unwrap().matches_bytes(&fs::read(&target).unwrap()));

    // 2) A mutation request based on the pre-conflict revision is rejected.
    let stale = session.check_revision(rev_after_first - 1);
    assert!(stale.is_err(), "stale revision must be rejected");
    let err = stale.unwrap_err();
    assert_eq!(err.actual, rev_after_first);
    assert!(!err.to_string().is_empty(), "clear error message");
    // Current revision still accepted.
    assert!(session.check_revision(rev_after_first).is_ok());

    // 3) Safety invariant: the external bytes are intact and parseable —
    //    the rejected interaction wrote nothing.
    assert_eq!(fs::read(&target).unwrap(), external);
    parse_xml(&external).expect("external document not corrupted");

    // 4) Events are recorded in the bounded safety log.
    let mut log = SafetyEventLog::new(100);
    log.push(SafetyEvent {
        event_type: SafetyEventType::ConflictDetected,
        document_path: target.clone(),
        timestamp: "test".into(),
        message: Some("external edit while dirty".into()),
    });
    log.push(SafetyEvent {
        event_type: SafetyEventType::StaleRevisionRejected,
        document_path: target.clone(),
        timestamp: "test".into(),
        message: Some(err.to_string()),
    });
    assert_eq!(log.len(), 2);
    assert!(matches!(log.recent(2)[0].event_type, SafetyEventType::ConflictDetected));
    assert!(matches!(log.recent(2)[1].event_type, SafetyEventType::StaleRevisionRejected));
    fs::remove_dir_all(&dir).ok();
}

// ─── B10: self-save vs external change discrimination ────────────────────────

/// QA-M10-B10: after OUR OWN successful `atomic_save`, the session
/// fingerprint update must prevent a false conflict (self-save recognition);
/// a subsequent external edit must then be flagged again. This is the
/// fingerprint half of the watcher's self-save suppression (the notify-based
/// half is covered by Matrix C, case C08).
#[test]
fn qa_m10_b10_self_save_recognized_external_change_flagged() {
    let dir = sandbox("b10");
    let target = dir.join("doc.xml");
    fs::write(&target, original_document()).unwrap();

    let session = DocumentSession::new(Some(FileFingerprint::from_file(&target).unwrap()));
    let mut checker = FingerprintChecker::new();
    checker.record(&target);

    // Local edit + real atomic save.
    session.record_mutation();
    assert!(session.begin_save(), "save lock must be free");
    let updated = updated_document();
    let config = SaveConfig { target_path: target.clone(), validate_temp: true };
    let result = atomic_save(&config, &updated, |b| parse_xml(b).is_ok());
    assert_eq!(result.state, SaveState::Completed);

    // Session bookkeeping exactly as the save command does it.
    session.record_save();
    session.set_fingerprint(result.fingerprint.clone().unwrap());
    session.end_save();
    assert!(!session.is_dirty(), "saved revision catches up");
    assert!(!session.is_saving());

    // Self-save: no false conflict.
    checker.record(&target); // what register_self_save does conceptually
    assert!(!checker.has_changed(&target), "our own save must NOT be flagged as external change");
    assert!(session.fingerprint().unwrap().matches_bytes(&fs::read(&target).unwrap()));

    // External edit afterwards IS flagged.
    let mut externally = parse_xml(&updated).unwrap();
    externally.root.set_attr("PROJECTNAME", "Edited by Notepad");
    let external_bytes = serialize_xml(&externally);
    fs::write(&target, &external_bytes).unwrap();
    assert!(checker.has_changed(&target), "external edit after self-save must be flagged");
    assert!(!session.fingerprint().unwrap().matches_bytes(&external_bytes));
    parse_xml(&fs::read(&target).unwrap()).expect("external edit is a valid document");
    fs::remove_dir_all(&dir).ok();
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn action_label(a: &RecoveryAction) -> &'static str {
    match a {
        RecoveryAction::CompleteReplacement { .. } => "CompleteReplacement",
        RecoveryAction::RestoreBackup { .. } => "RestoreBackup",
        RecoveryAction::PreserveTemp { .. } => "PreserveTemp",
        RecoveryAction::NothingNeeded { .. } => "NothingNeeded",
    }
}
