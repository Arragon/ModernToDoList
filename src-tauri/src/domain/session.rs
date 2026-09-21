//! Document session management with revision tracking and mutation locks.
//!
//! Each open document has a `DocumentSession` that tracks:
//! - Current revision number (incremented on each mutation)
//! - Saved revision number (last persisted state)
//! - Dirty flag (current != saved)
//! - Expected revision for stale mutation rejection
//! - Serialization lock for mutation/save ordering

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

use super::fingerprint::FileFingerprint;

/// A document session tracking the lifecycle of an open document.
///
/// The session is created when a document is opened and destroyed when closed.
/// It tracks the revision model and provides the serialization lock.
#[derive(Debug)]
pub struct DocumentSession {
    /// Current revision (incremented on every mutation).
    current_revision: AtomicU64,
    /// Revision at last successful save.
    saved_revision: AtomicU64,
    /// The fingerprint of the file as last read or written.
    last_fingerprint: std::sync::Mutex<Option<FileFingerprint>>,
    /// Save generation counter.
    save_generation: AtomicU64,
    /// Whether a save is currently in progress.
    saving: std::sync::atomic::AtomicBool,
}

impl DocumentSession {
    /// Creates a new session for a freshly opened document.
    pub fn new(initial_fingerprint: Option<FileFingerprint>) -> Self {
        Self {
            current_revision: AtomicU64::new(1),
            saved_revision: AtomicU64::new(0),
            last_fingerprint: std::sync::Mutex::new(initial_fingerprint),
            save_generation: AtomicU64::new(0),
            saving: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Returns the current revision number.
    pub fn current_revision(&self) -> u64 {
        self.current_revision.load(Ordering::SeqCst)
    }

    /// Returns the saved revision number.
    pub fn saved_revision(&self) -> u64 {
        self.saved_revision.load(Ordering::SeqCst)
    }

    /// Returns true if the document has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.current_revision() > self.saved_revision()
    }

    /// Records a mutation, incrementing the current revision.
    /// Returns the new revision number.
    pub fn record_mutation(&self) -> u64 {
        self.current_revision.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Records a successful save, updating the saved revision.
    pub fn record_save(&self) {
        let current = self.current_revision.load(Ordering::SeqCst);
        self.saved_revision.store(current, Ordering::SeqCst);
        let gen = self.save_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = gen; // save generation for tracking
    }

    /// Returns the save generation counter.
    pub fn save_generation(&self) -> u64 {
        self.save_generation.load(Ordering::SeqCst)
    }

    /// Checks if a mutation request with the given expected revision is stale.
    ///
    /// Returns `Ok(())` if the revision matches, or `Err(StaleRevisionError)` if not.
    pub fn check_revision(&self, expected: u64) -> Result<(), StaleRevisionError> {
        let current = self.current_revision();
        if expected != current {
            Err(StaleRevisionError {
                expected,
                actual: current,
            })
        } else {
            Ok(())
        }
    }

    /// Attempts to begin a save operation (acquires the save lock).
    /// Returns false if a save is already in progress.
    pub fn begin_save(&self) -> bool {
        self.saving
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Completes a save operation (releases the save lock).
    pub fn end_save(&self) {
        self.saving.store(false, Ordering::SeqCst);
    }

    /// Returns true if a save is currently in progress.
    pub fn is_saving(&self) -> bool {
        self.saving.load(Ordering::SeqCst)
    }

    /// Gets the last known fingerprint.
    pub fn fingerprint(&self) -> Option<FileFingerprint> {
        self.last_fingerprint.lock().unwrap().clone()
    }

    /// Updates the stored fingerprint.
    pub fn set_fingerprint(&self, fp: FileFingerprint) {
        *self.last_fingerprint.lock().unwrap() = Some(fp);
    }
}

/// Error returned when a mutation has a stale expected revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleRevisionError {
    pub expected: u64,
    pub actual: u64,
}

impl std::fmt::Display for StaleRevisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "stale revision: expected {}, actual {}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for StaleRevisionError {}

/// Save state machine states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveState {
    /// No save in progress. Document may be dirty or clean.
    Idle,
    /// Serializing the document to bytes.
    Serializing,
    /// Writing temp file to disk.
    WritingTemp,
    /// Validating the temp file before replacement.
    Validating,
    /// Performing atomic replacement of the original file.
    Replacing,
    /// Updating the recovery journal.
    UpdatingJournal,
    /// Save completed successfully.
    Completed,
    /// Save failed at some stage.
    Failed,
}

/// Structured save error codes for UI context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveErrorCode {
    /// Disk is full.
    DiskFull,
    /// Permission denied.
    PermissionDenied,
    /// File is locked by another process.
    FileLocked,
    /// Network path unavailable.
    NetworkUnavailable,
    /// Serialization failed.
    SerializationFailed,
    /// Validation of saved content failed.
    ValidationFailed,
    /// Atomic replacement failed.
    ReplacementFailed,
    /// Unknown error.
    Unknown(String),
}

impl std::fmt::Display for SaveErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DiskFull => write!(f, "disk full"),
            Self::PermissionDenied => write!(f, "permission denied"),
            Self::FileLocked => write!(f, "file locked"),
            Self::NetworkUnavailable => write!(f, "network unavailable"),
            Self::SerializationFailed => write!(f, "serialization failed"),
            Self::ValidationFailed => write!(f, "validation failed"),
            Self::ReplacementFailed => write!(f, "replacement failed"),
            Self::Unknown(m) => write!(f, "unknown: {}", m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_initial_state() {
        let session = DocumentSession::new(None);
        assert_eq!(session.current_revision(), 1);
        assert_eq!(session.saved_revision(), 0);
        assert!(session.is_dirty()); // 1 > 0
        assert!(!session.is_saving());
    }

    #[test]
    fn session_mutation_increments_revision() {
        let session = DocumentSession::new(None);
        let rev = session.record_mutation();
        assert_eq!(rev, 2);
        assert_eq!(session.current_revision(), 2);
    }

    #[test]
    fn session_save_clears_dirty() {
        let session = DocumentSession::new(None);
        session.record_mutation();
        session.record_mutation();
        assert!(session.is_dirty());
        session.record_save();
        assert!(!session.is_dirty());
    }

    #[test]
    fn session_stale_revision_check() {
        let session = DocumentSession::new(None);
        assert!(session.check_revision(1).is_ok());
        session.record_mutation(); // now 2
        assert!(session.check_revision(1).is_err());
        assert!(session.check_revision(2).is_ok());
    }

    #[test]
    fn session_save_lock() {
        let session = DocumentSession::new(None);
        assert!(session.begin_save());
        assert!(!session.begin_save()); // Already saving
        assert!(session.is_saving());
        session.end_save();
        assert!(!session.is_saving());
        assert!(session.begin_save()); // Can save again
    }

    #[test]
    fn session_fingerprint() {
        let fp = FileFingerprint::from_bytes(b"test");
        let session = DocumentSession::new(Some(fp.clone()));
        assert_eq!(session.fingerprint(), Some(fp.clone()));

        let fp2 = FileFingerprint::from_bytes(b"updated");
        session.set_fingerprint(fp2.clone());
        assert_eq!(session.fingerprint(), Some(fp2));
    }

    #[test]
    fn save_generation_increments() {
        let session = DocumentSession::new(None);
        assert_eq!(session.save_generation(), 0);
        session.record_save();
        assert_eq!(session.save_generation(), 1);
        session.record_save();
        assert_eq!(session.save_generation(), 2);
    }
}
