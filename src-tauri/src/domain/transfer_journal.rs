//! Durable transfer journal for cross-document transfer transactions (M8).
//!
//! Every transfer (copy/move of a task subtree between documents) records its
//! phase transitions in a per-transaction journal file BEFORE the phase it
//! authorizes begins. This makes the transfer crash-recoverable:
//!
//! ```text
//! Started ──► AssetsStaged ──► TargetCommitted ──► SourceBackedUp ──► SourceCommitted ──► Completed
//! ```
//!
//! Durability protocol for every append:
//! 1. Serialize the full record to JSON.
//! 2. Write to `<journal>.tmp`, `sync_all()` (fsync).
//! 3. Copy the current journal to `<journal>.bak` (best effort, keeps the
//!    previous consistent version readable if the rename is interrupted).
//! 4. Rename tmp over the journal (atomic on NTFS).
//!
//! Loading falls back to the `.bak` copy when the primary file is corrupt or
//! truncated. A journal whose primary AND backup are unreadable is reported as
//! [`JournalError::Corrupt`] so recovery can quarantine the transaction
//! instead of guessing (bias: duplicate-not-loss).
//!
//! The journal lives under `<workspace_root>/.moderntodo/transfer/` and is
//! deleted only after a `Completed` entry has been durably written.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::transfer::{AssetPlanEntry, TransferManifest, TransferOperation};

/// Sub-directory (under `.moderntodo/`) holding transfer journals.
pub const JOURNAL_DIR_NAME: &str = "transfer";

/// File extension of a transfer journal.
pub const JOURNAL_EXTENSION: &str = "journal.json";

/// Errors specific to journal I/O.
#[derive(Debug, Error)]
pub enum JournalError {
    #[error("journal I/O error at {path}: {reason}")]
    Io { path: PathBuf, reason: String },

    #[error("journal is corrupt at {path}: {reason}")]
    Corrupt { path: PathBuf, reason: String },

    #[error("journal not found: {0}")]
    NotFound(PathBuf),
}

/// Durable phase transitions of a transfer transaction.
///
/// Ordered: recovery uses the highest durably recorded phase to decide the
/// safe action. A phase entry is written BEFORE the work it authorizes
/// starts, except `TargetCommitted` / `SourceCommitted` / `Completed`, which
/// are written AFTER the corresponding filesystem mutation succeeded (they
/// record proven facts, and authorize the FOLLOWING phase).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalPhase {
    /// Transaction planned; manifest recorded. Authorizes asset staging.
    Started,
    /// All copy-required assets staged next to the target. Authorizes target commit.
    AssetsStaged,
    /// Target document atomically replaced and validated. Authorizes source backup/delete.
    TargetCommitted,
    /// Source document bytes backed up (move only). Authorizes source commit.
    SourceBackedUp,
    /// Source document rewritten without the moved subtree (move only).
    SourceCommitted,
    /// Transaction finished; journal may be removed.
    Completed,
}

impl JournalPhase {
    /// True if this phase proves the target document contains the transferred subtree.
    pub fn target_is_committed(self) -> bool {
        self >= JournalPhase::TargetCommitted
    }
}

/// One durable journal line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Monotonic sequence number (1-based).
    pub seq: u64,
    /// The phase recorded.
    pub phase: JournalPhase,
    /// ISO-ish timestamp (seconds since UNIX epoch, portable, no locale).
    pub at_epoch_secs: u64,
    /// Optional human-readable detail (never contains task content).
    pub detail: Option<String>,
}

/// Full durable record: the manifest plus every phase entry written so far.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferJournalRecord {
    /// Format version of the journal file.
    pub journal_version: u32,
    /// The immutable transaction manifest.
    pub manifest: TransferManifest,
    /// Durable phase entries, in write order.
    pub entries: Vec<JournalEntry>,
    /// Path of the source-bytes backup (move only; recorded with SourceBackedUp).
    pub source_backup_path: Option<PathBuf>,
    /// Path of the target-bytes backup used for transaction Undo.
    pub target_undo_backup_path: Option<PathBuf>,
}

impl TransferJournalRecord {
    /// The highest phase durably recorded, or `None` when no entry exists.
    pub fn last_phase(&self) -> Option<JournalPhase> {
        self.entries.last().map(|e| e.phase)
    }

    /// True when a specific phase has been durably recorded.
    pub fn has_phase(&self, phase: JournalPhase) -> bool {
        self.entries.iter().any(|e| e.phase == phase)
    }

    /// True when the transaction completed successfully.
    pub fn is_completed(&self) -> bool {
        self.has_phase(JournalPhase::Completed)
    }
}

/// Handle to an on-disk transfer journal.
#[derive(Debug)]
pub struct TransferJournal {
    path: PathBuf,
    record: TransferJournalRecord,
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Durably writes `bytes` to `path`: tmp file + fsync + rename over target,
/// keeping the previous version as `path.bak`.
fn durable_write(path: &Path, bytes: &[u8]) -> Result<(), JournalError> {
    let tmp = path.with_extension("tmp");
    let bak = path.with_extension("bak");

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| JournalError::Io { path: path.to_path_buf(), reason: e.to_string() })?;
    }

    // 1. Write + fsync tmp.
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp)
            .map_err(|e| JournalError::Io { path: tmp.clone(), reason: e.to_string() })?;
        f.write_all(bytes)
            .map_err(|e| JournalError::Io { path: tmp.clone(), reason: e.to_string() })?;
        f.sync_all()
            .map_err(|e| JournalError::Io { path: tmp.clone(), reason: e.to_string() })?;
    }

    // 2. Best-effort backup of the current version (before overwrite).
    if path.exists() {
        let _ = fs::remove_file(&bak);
        let _ = fs::rename(path, &bak);
        if bak.exists() {
            // Restore a readable primary path target for the rename below:
            // if rename(path -> bak) succeeded, path no longer exists.
        } else {
            // rename failed (e.g. locked): copy instead, leave original in place.
            let _ = fs::copy(path, &bak);
        }
    }

    // 3. Atomic rename tmp -> path.
    fs::rename(&tmp, path).map_err(|e| {
        // Try to put the backup back so the primary path stays readable.
        if !path.exists() && bak.exists() {
            let _ = fs::rename(&bak, path);
        }
        JournalError::Io { path: path.to_path_buf(), reason: e.to_string() }
    })?;

    // 4. Best-effort fsync of the parent directory (POSIX only; on Windows
    //    opening a directory for sync is not supported, the rename is durable
    //    enough for our recovery guarantees).
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            if let Ok(dir) = fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
    }
    Ok(())
}

impl TransferJournal {
    /// Journal file path for a transaction inside `journal_dir`.
    pub fn path_for(journal_dir: &Path, transaction_id: &str) -> PathBuf {
        journal_dir.join(format!("{}.{}", transaction_id, JOURNAL_EXTENSION))
    }

    /// Creates a new journal recording `Started` and flushes it durably.
    ///
    /// The `Started` entry is on disk (fsync'd) before this returns, so the
    /// transaction is discoverable by recovery from the very first moment.
    pub fn create(
        journal_dir: &Path,
        manifest: TransferManifest,
        target_undo_backup_path: Option<PathBuf>,
    ) -> Result<Self, JournalError> {
        let path = Self::path_for(journal_dir, &manifest.transaction_id);
        if path.exists() {
            return Err(JournalError::Io {
                path,
                reason: "journal for this transaction already exists".into(),
            });
        }
        let record = TransferJournalRecord {
            journal_version: 1,
            manifest,
            entries: vec![JournalEntry {
                seq: 1,
                phase: JournalPhase::Started,
                at_epoch_secs: now_epoch_secs(),
                detail: None,
            }],
            source_backup_path: None,
            target_undo_backup_path,
        };
        let journal = Self { path, record };
        journal.flush()?;
        Ok(journal)
    }

    /// Loads a journal, falling back to the `.bak` copy when the primary is
    /// corrupt or truncated.
    pub fn load(path: &Path) -> Result<Self, JournalError> {
        if !path.exists() && !path.with_extension("bak").exists() {
            return Err(JournalError::NotFound(path.to_path_buf()));
        }
        match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<TransferJournalRecord>(&bytes) {
                Ok(record) => Ok(Self { path: path.to_path_buf(), record }),
                Err(primary_err) => {
                    // Primary corrupt/truncated: try the backup copy.
                    let bak = path.with_extension("bak");
                    match fs::read(&bak).and_then(|b| {
                        serde_json::from_slice::<TransferJournalRecord>(&b)
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                    }) {
                    Ok(record) => Ok(Self { path: path.to_path_buf(), record }),
                        Err(_) => Err(JournalError::Corrupt {
                            path: path.to_path_buf(),
                            reason: format!(
                                "primary unreadable ({}) and no usable .bak",
                                primary_err
                            ),
                        }),
                    }
                }
            },
            Err(read_err) => {
                let bak = path.with_extension("bak");
                match fs::read(&bak).and_then(|b| {
                    serde_json::from_slice::<TransferJournalRecord>(&b)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                }) {
                    Ok(record) => Ok(Self { path: path.to_path_buf(), record }),
                    Err(_) => Err(JournalError::Corrupt {
                        path: path.to_path_buf(),
                        reason: format!("unreadable ({}) and no usable .bak", read_err),
                    }),
                }
            }
        }
    }

    /// The durable record.
    pub fn record(&self) -> &TransferJournalRecord {
        &self.record
    }

    /// The manifest.
    pub fn manifest(&self) -> &TransferManifest {
        &self.record.manifest
    }

    /// Journal file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends a phase entry and flushes durably. The caller must only record
    /// a phase once the facts it claims are true on disk (for committed
    /// phases) or immediately before starting the work it authorizes (for
    /// Started / AssetsStaged / SourceBackedUp).
    pub fn append(&mut self, phase: JournalPhase, detail: Option<String>) -> Result<(), JournalError> {
        let seq = self.record.entries.last().map(|e| e.seq).unwrap_or(0) + 1;
        self.record.entries.push(JournalEntry {
            seq,
            phase,
            at_epoch_secs: now_epoch_secs(),
            detail,
        });
        self.flush()
    }

    /// Records the source backup path together with the `SourceBackedUp` phase.
    pub fn append_source_backed_up(&mut self, backup_path: &Path) -> Result<(), JournalError> {
        self.record.source_backup_path = Some(backup_path.to_path_buf());
        self.append(JournalPhase::SourceBackedUp, Some(backup_path.to_string_lossy().into_owned()))
    }

    fn flush(&self) -> Result<(), JournalError> {
        let bytes = serde_json::to_vec_pretty(&self.record)
            .map_err(|e| JournalError::Io { path: self.path.clone(), reason: e.to_string() })?;
        durable_write(&self.path, &bytes)
    }

    /// Deletes the journal (and its `.bak`/`.tmp` siblings).
    ///
    /// Only call after `Completed` was durably recorded, or when recovery has
    /// proven the transaction left the filesystem in a safe final state.
    pub fn remove(&self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("bak"));
        let _ = fs::remove_file(self.path.with_extension("tmp"));
    }
}

/// Outcome of loading one journal during a recovery scan.
#[derive(Debug)]
pub enum ScannedJournal {
    /// Loaded successfully.
    Ok(TransferJournal),
    /// Unreadable even after `.bak` fallback; transaction must be quarantined.
    Corrupt { path: PathBuf, reason: String },
}

/// Scans `journal_dir` for transfer journals.
///
/// Returns every `*.journal.json` file: successfully loaded ones and corrupt
/// ones (so recovery can quarantine instead of silently ignoring them).
/// Completed journals are excluded (they are normally deleted on completion;
/// a stray completed journal is cleaned by recovery).
pub fn scan_journal_dir(journal_dir: &Path) -> Vec<ScannedJournal> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(journal_dir) {
        Ok(e) => e,
        Err(_) => return out, // no journal dir => nothing to recover
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().map(|e| e == "json").unwrap_or(false)
                && p.file_name()
                    .map(|n| n.to_string_lossy().ends_with(JOURNAL_EXTENSION))
                    .unwrap_or(false)
        })
        .collect();
    paths.sort();
    for path in paths {
        match TransferJournal::load(&path) {
            Ok(j) => out.push(ScannedJournal::Ok(j)),
            Err(JournalError::Corrupt { path, reason }) => {
                out.push(ScannedJournal::Corrupt { path, reason })
            }
            Err(e) => out.push(ScannedJournal::Corrupt {
                path,
                reason: e.to_string(),
            }),
        }
    }
    out
}

/// Builds the default journal directory for a workspace root.
pub fn default_journal_dir(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".moderntodo")
        .join(JOURNAL_DIR_NAME)
}

/// Convenience: is this manifest a move?
pub fn is_move(manifest: &TransferManifest) -> bool {
    manifest.operation == TransferOperation::Move
}

/// Convenience: number of assets that must be copied.
pub fn copy_required_assets(manifest: &TransferManifest) -> Vec<&AssetPlanEntry> {
    manifest
        .asset_refs
        .iter()
        .filter(|a| a.classification == super::transfer::AssetClassification::CopyRequired)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "mtdl_journal_{}_{}_{}",
            tag,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn test_manifest(txn: &str) -> TransferManifest {
        TransferManifest {
            transaction_id: txn.to_string(),
            operation: TransferOperation::Move,
            source_doc: super::super::transfer::DocumentEndpoint {
                document_id: Some("src-doc".into()),
                path: PathBuf::from("S:/src.xml"),
                fingerprint: super::super::fingerprint::FileFingerprint::from_bytes(b"src"),
            },
            target_doc: super::super::transfer::DocumentEndpoint {
                document_id: Some("tgt-doc".into()),
                path: PathBuf::from("T:/tgt.xml"),
                fingerprint: super::super::fingerprint::FileFingerprint::from_bytes(b"tgt"),
            },
            root_task_id: crate::domain::types::TaskId::new("5"),
            task_ids: vec![crate::domain::types::TaskId::new("5")],
            id_map: vec![(
                crate::domain::types::TaskId::new("5"),
                crate::domain::types::TaskId::new("9"),
            )],
            asset_refs: Vec::new(),
        }
    }

    #[test]
    fn create_writes_durable_started_entry() {
        let dir = tmp_dir("create");
        let j = TransferJournal::create(&dir, test_manifest("txn-1"), None).unwrap();
        assert!(j.path().exists());
        assert_eq!(j.record().last_phase(), Some(JournalPhase::Started));

        // Reload from disk proves durability.
        let reloaded = TransferJournal::load(j.path()).unwrap();
        assert_eq!(reloaded.record().last_phase(), Some(JournalPhase::Started));
        assert_eq!(reloaded.manifest().transaction_id, "txn-1");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_create_rejected() {
        let dir = tmp_dir("dup");
        let _j = TransferJournal::create(&dir, test_manifest("txn-dup"), None).unwrap();
        let err = TransferJournal::create(&dir, test_manifest("txn-dup"), None);
        assert!(err.is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_phase_sequence_and_reload() {
        let dir = tmp_dir("append");
        let mut j = TransferJournal::create(&dir, test_manifest("txn-2"), None).unwrap();
        j.append(JournalPhase::AssetsStaged, None).unwrap();
        j.append(JournalPhase::TargetCommitted, Some("fp=abc".into())).unwrap();
        assert!(j.record().has_phase(JournalPhase::TargetCommitted));
        assert!(j.record().last_phase().unwrap().target_is_committed());

        let reloaded = TransferJournal::load(j.path()).unwrap();
        let phases: Vec<_> = reloaded.record().entries.iter().map(|e| e.phase).collect();
        assert_eq!(
            phases,
            vec![
                JournalPhase::Started,
                JournalPhase::AssetsStaged,
                JournalPhase::TargetCommitted
            ]
        );
        assert_eq!(reloaded.record().entries[2].seq, 3);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_falls_back_to_bak_when_primary_garbled() {
        let dir = tmp_dir("bak");
        let mut j = TransferJournal::create(&dir, test_manifest("txn-3"), None).unwrap();
        j.append(JournalPhase::AssetsStaged, None).unwrap();
        // .bak now holds the Started-only version; primary holds AssetsStaged.
        fs::write(j.path(), b"{ this is not json !!!").unwrap();

        let reloaded = TransferJournal::load(j.path()).unwrap();
        // Fallback yields the last consistent backup (Started).
        assert_eq!(reloaded.record().last_phase(), Some(JournalPhase::Started));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_reports_corrupt_when_primary_and_bak_unreadable() {
        let dir = tmp_dir("corrupt");
        let j = TransferJournal::create(&dir, test_manifest("txn-4"), None).unwrap();
        fs::write(j.path(), b"garbage").unwrap();
        fs::write(j.path().with_extension("bak"), b"more garbage").unwrap();
        match TransferJournal::load(j.path()) {
            Err(JournalError::Corrupt { .. }) => {}
            other => panic!("expected Corrupt, got {:?}", other.map(|j| j.record().entries.len())),
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn truncated_journal_recovers_from_bak() {
        let dir = tmp_dir("trunc");
        let mut j = TransferJournal::create(&dir, test_manifest("txn-5"), None).unwrap();
        j.append(JournalPhase::AssetsStaged, None).unwrap();
        j.append(JournalPhase::TargetCommitted, None).unwrap();
        // Simulate a torn write: truncate the primary mid-JSON.
        let bytes = fs::read(j.path()).unwrap();
        fs::write(j.path(), &bytes[..bytes.len() / 2]).unwrap();

        let reloaded = TransferJournal::load(j.path()).unwrap();
        // .bak holds the AssetsStaged version: target NOT proven committed.
        assert_eq!(reloaded.record().last_phase(), Some(JournalPhase::AssetsStaged));
        assert!(!reloaded.record().has_phase(JournalPhase::TargetCommitted));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_dir_finds_ok_and_corrupt() {
        let dir = tmp_dir("scan");
        let _a = TransferJournal::create(&dir, test_manifest("txn-a"), None).unwrap();
        let b = TransferJournal::create(&dir, test_manifest("txn-b"), None).unwrap();
        fs::write(b.path(), b"\xff\xfe broken").unwrap();
        fs::write(b.path().with_extension("bak"), b"\xff\xfe broken too").unwrap();

        let scanned = scan_journal_dir(&dir);
        assert_eq!(scanned.len(), 2);
        let ok_count = scanned.iter().filter(|s| matches!(s, ScannedJournal::Ok(_))).count();
        let corrupt_count = scanned.iter().filter(|s| matches!(s, ScannedJournal::Corrupt { .. })).count();
        assert_eq!(ok_count, 1);
        assert_eq!(corrupt_count, 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_deletes_journal_and_siblings() {
        let dir = tmp_dir("remove");
        let mut j = TransferJournal::create(&dir, test_manifest("txn-6"), None).unwrap();
        j.append(JournalPhase::Completed, None).unwrap();
        assert!(j.path().with_extension("bak").exists());
        j.remove();
        assert!(!j.path().exists());
        assert!(!j.path().with_extension("bak").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn default_journal_dir_layout() {
        let d = default_journal_dir(Path::new("W:/ws"));
        assert_eq!(d, PathBuf::from("W:/ws/.moderntodo/transfer"));
    }
}
