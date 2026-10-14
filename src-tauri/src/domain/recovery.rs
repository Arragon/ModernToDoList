//! Recovery journal for incomplete save operations.
//!
//! The recovery journal tracks the state of save operations so that
//! incomplete saves can be detected and recovered at application startup.
//!
//! Journal phases:
//! 1. PreCommit - About to start save
//! 2. Writing - Temp file being written
//! 3. Validating - Temp file being validated
//! 4. Replacing - Atomic replacement in progress
//! 5. Complete - Save finished successfully
//! 6. Orphaned - Journal found at startup (incomplete save)

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Recovery journal entry for a single document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryJournalEntry {
    /// Path to the document being saved.
    pub document_path: PathBuf,
    /// Path to the temp file (if one was created).
    pub temp_path: Option<PathBuf>,
    /// Path to the backup file (if original was renamed).
    pub backup_path: Option<PathBuf>,
    /// Current phase of the save operation.
    pub phase: RecoveryPhase,
    /// Timestamp when the journal was created.
    pub created_at: String,
    /// Save generation number.
    pub generation: u64,
}

/// Phases of the save recovery process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryPhase {
    /// Save about to start.
    PreCommit,
    /// Temp file being written.
    Writing,
    /// Temp file being validated.
    Validating,
    /// Atomic replacement in progress.
    Replacing,
    /// Save completed successfully (journal can be deleted).
    Complete,
    /// Incomplete save detected at startup.
    Orphaned,
}

/// The recovery journal manages multiple document entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecoveryJournal {
    pub entries: Vec<RecoveryJournalEntry>,
}

impl RecoveryJournal {
    /// Creates a new empty journal.
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Adds a new entry for a save operation.
    pub fn begin_save(&mut self, document_path: PathBuf, generation: u64) -> &mut RecoveryJournalEntry {
        let entry = RecoveryJournalEntry {
            document_path,
            temp_path: None,
            backup_path: None,
            phase: RecoveryPhase::PreCommit,
            created_at: String::new(), // Simplified
            generation,
        };
        self.entries.push(entry);
        self.entries.last_mut().unwrap()
    }

    /// Updates the phase of an entry by document path.
    pub fn update_phase(&mut self, document_path: &Path, phase: RecoveryPhase) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.document_path == document_path) {
            entry.phase = phase;
        }
    }

    /// Removes completed entries.
    pub fn cleanup_completed(&mut self) {
        self.entries.retain(|e| e.phase != RecoveryPhase::Complete);
    }

    /// Finds orphaned entries (incomplete saves).
    pub fn orphaned_entries(&self) -> Vec<&RecoveryJournalEntry> {
        self.entries
            .iter()
            .filter(|e| {
                e.phase != RecoveryPhase::Complete
            })
            .collect()
    }

    /// Loads a journal from a file.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let data = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read journal: {}", e))?;
        serde_json::from_str(&data)
            .map_err(|e| format!("failed to parse journal: {}", e))
    }

    /// Saves the journal to a file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize journal: {}", e))?;
        std::fs::write(path, data)
            .map_err(|e| format!("failed to write journal: {}", e))
    }

    /// Attempts to recover from orphaned journal entries.
    ///
    /// For each orphaned entry:
    /// - If temp file exists and backup exists: temp was being written, restore from backup
    /// - If temp exists and no backup: replacement was in progress, try to complete
    /// - If no temp and no backup: save never started, nothing to do
    pub fn recover_entries(&mut self) -> Vec<RecoveryAction> {
        let mut actions = Vec::new();

        for entry in &self.entries {
            if entry.phase == RecoveryPhase::Complete {
                continue;
            }

            let temp_exists = entry.temp_path.as_ref().map(|p| p.exists()).unwrap_or(false);
            let backup_exists = entry.backup_path.as_ref().map(|p| p.exists()).unwrap_or(false);

            let action = match (temp_exists, backup_exists) {
                (true, true) => {
                    // Temp was written but replacement didn't complete
                    // Try to complete the replacement
                    RecoveryAction::CompleteReplacement {
                        document_path: entry.document_path.clone(),
                        temp_path: entry.temp_path.clone().unwrap(),
                        backup_path: entry.backup_path.clone().unwrap(),
                    }
                }
                (true, false) => {
                    // Temp exists but no backup - unusual state
                    // Keep the temp file for manual recovery
                    RecoveryAction::PreserveTemp {
                        document_path: entry.document_path.clone(),
                        temp_path: entry.temp_path.clone().unwrap(),
                    }
                }
                (false, true) => {
                    // Backup exists but no temp - rename was interrupted
                    // Restore from backup
                    RecoveryAction::RestoreBackup {
                        document_path: entry.document_path.clone(),
                        backup_path: entry.backup_path.clone().unwrap(),
                    }
                }
                (false, false) => {
                    // Nothing to recover
                    RecoveryAction::NothingNeeded {
                        document_path: entry.document_path.clone(),
                    }
                }
            };
            actions.push(action);
        }

        actions
    }
}

/// An action to take during recovery.
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Complete the atomic replacement.
    CompleteReplacement {
        document_path: PathBuf,
        temp_path: PathBuf,
        backup_path: PathBuf,
    },
    /// Restore from backup file.
    RestoreBackup {
        document_path: PathBuf,
        backup_path: PathBuf,
    },
    /// Preserve the temp file for manual recovery.
    PreserveTemp {
        document_path: PathBuf,
        temp_path: PathBuf,
    },
    /// No recovery needed.
    NothingNeeded {
        document_path: PathBuf,
    },
}

/// Bounded data-safety event log (RD-M3-025).
///
/// Records data-safety events without including task content.
/// Used for diagnostics and the Recovery Center.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyEvent {
    /// Event type.
    pub event_type: SafetyEventType,
    /// Document path.
    pub document_path: PathBuf,
    /// Timestamp (simplified).
    pub timestamp: String,
    /// Optional message.
    pub message: Option<String>,
}

/// Types of data-safety events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SafetyEventType {
    SaveStarted,
    SaveCompleted,
    SaveFailed,
    AutosaveTriggered,
    RecoveryDetected,
    RecoveryCompleted,
    ConflictDetected,
    StaleRevisionRejected,
}

/// Bounded event log that keeps the last N events.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SafetyEventLog {
    events: Vec<SafetyEvent>,
    max_entries: usize,
}

impl SafetyEventLog {
    pub fn new(max_entries: usize) -> Self {
        Self {
            events: Vec::new(),
            max_entries,
        }
    }

    pub fn push(&mut self, event: SafetyEvent) {
        self.events.push(event);
        while self.events.len() > self.max_entries {
            self.events.remove(0);
        }
    }

    pub fn recent(&self, count: usize) -> &[SafetyEvent] {
        let start = self.events.len().saturating_sub(count);
        &self.events[start..]
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_begin_and_update() {
        let mut journal = RecoveryJournal::new();
        journal.begin_save(PathBuf::from("test.xml"), 1);
        assert_eq!(journal.entries.len(), 1);
        assert_eq!(journal.entries[0].phase, RecoveryPhase::PreCommit);

        journal.update_phase(Path::new("test.xml"), RecoveryPhase::Writing);
        assert_eq!(journal.entries[0].phase, RecoveryPhase::Writing);

        journal.update_phase(Path::new("test.xml"), RecoveryPhase::Complete);
        journal.cleanup_completed();
        assert!(journal.entries.is_empty());
    }

    #[test]
    fn journal_orphaned_detection() {
        let mut journal = RecoveryJournal::new();
        journal.begin_save(PathBuf::from("a.xml"), 1);
        journal.begin_save(PathBuf::from("b.xml"), 2);
        journal.update_phase(Path::new("a.xml"), RecoveryPhase::Complete);

        let orphaned = journal.orphaned_entries();
        assert_eq!(orphaned.len(), 1);
        assert_eq!(orphaned[0].document_path, PathBuf::from("b.xml"));
    }

    #[test]
    fn journal_serde_roundtrip() {
        let mut journal = RecoveryJournal::new();
        journal.begin_save(PathBuf::from("test.xml"), 1);
        let json = serde_json::to_string(&journal).unwrap();
        let loaded: RecoveryJournal = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.entries.len(), 1);
    }

    #[test]
    fn recovery_nothing_needed() {
        let mut journal = RecoveryJournal::new();
        let entry = journal.begin_save(PathBuf::from("test.xml"), 1);
        entry.phase = RecoveryPhase::PreCommit;
        // No temp or backup files exist

        let actions = journal.recover_entries();
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], RecoveryAction::NothingNeeded { .. }));
    }

    #[test]
    fn safety_event_log_bounded() {
        let mut log = SafetyEventLog::new(3);
        for i in 0..5 {
            log.push(SafetyEvent {
                event_type: SafetyEventType::SaveCompleted,
                document_path: PathBuf::from(format!("test_{}.xml", i)),
                timestamp: format!("2026-09-13T{:02}:00:00", i),
                message: None,
            });
        }
        assert_eq!(log.len(), 3); // Bounded to 3
        let recent = log.recent(2);
        assert_eq!(recent.len(), 2);
    }
}
