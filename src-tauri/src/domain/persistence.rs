//! Atomic save pipeline for data safety.
//!
//! Implements the safe save sequence:
//! 1. Serialize document to bytes
//! 2. Write to temp file in same directory as target
//! 3. Flush and sync temp file
//! 4. Re-read and validate temp file
//! 5. Atomic replace original with temp file
//! 6. Clean up temp file on success
//!
//! On failure at any step, the original file remains intact.

use std::path::{Path, PathBuf};

use super::fingerprint::FileFingerprint;
use super::session::{SaveErrorCode, SaveState};

/// Configuration for the atomic save pipeline.
#[derive(Debug, Clone)]
pub struct SaveConfig {
    /// Target file path.
    pub target_path: PathBuf,
    /// Whether to validate after writing temp.
    pub validate_temp: bool,
}

/// Result of a save operation.
#[derive(Debug)]
pub struct SaveResult {
    /// Final state of the save pipeline.
    pub state: SaveState,
    /// Fingerprint of the saved content.
    pub fingerprint: Option<FileFingerprint>,
    /// Error code if the save failed.
    pub error: Option<SaveErrorCode>,
    /// Path to the temp file (if it still exists after failure).
    pub temp_path: Option<PathBuf>,
}

/// Executes the atomic save pipeline.
///
/// The `content` parameter is the serialized XML bytes to save.
/// The `validate` callback is called with the temp file content before replacement.
pub fn atomic_save<F>(config: &SaveConfig, content: &[u8], validate: F) -> SaveResult
where
    F: FnOnce(&[u8]) -> bool,
{
    let target = &config.target_path;
    let temp_path = temp_file_path(target);

    // Step 1: Write temp file
    if let Err(e) = write_temp_file(&temp_path, content) {
        return SaveResult {
            state: SaveState::Failed,
            fingerprint: None,
            error: Some(classify_io_error(&e)),
            temp_path: Some(temp_path),
        };
    }

    // Step 2: Validate temp file by re-reading
    if config.validate_temp {
        match std::fs::read(&temp_path) {
            Ok(reread) => {
                if reread != content {
                    let _ = std::fs::remove_file(&temp_path);
                    return SaveResult {
                        state: SaveState::Failed,
                        fingerprint: None,
                        error: Some(SaveErrorCode::ValidationFailed),
                        temp_path: None,
                    };
                }
                if !validate(&reread) {
                    let _ = std::fs::remove_file(&temp_path);
                    return SaveResult {
                        state: SaveState::Failed,
                        fingerprint: None,
                        error: Some(SaveErrorCode::ValidationFailed),
                        temp_path: None,
                    };
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&temp_path);
                return SaveResult {
                    state: SaveState::Failed,
                    fingerprint: None,
                    error: Some(classify_io_error(&e)),
                    temp_path: None,
                };
            }
        }
    }

    // Step 3: Atomic replace
    if let Err(e) = replace_file(target, &temp_path) {
        return SaveResult {
            state: SaveState::Failed,
            fingerprint: None,
            error: Some(classify_io_error(&e)),
            temp_path: Some(temp_path),
        };
    }

    // Step 4: Compute fingerprint
    let fingerprint = Some(FileFingerprint::from_bytes(content));

    SaveResult {
        state: SaveState::Completed,
        fingerprint,
        error: None,
        temp_path: None,
    }
}

/// Generates a temp file path in the same directory as the target.
fn temp_file_path(target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    dir.join(format!(".{}.tmp", name))
}

/// Writes content to a temp file.
fn write_temp_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    file.write_all(content)?;
    file.sync_all()?; // Flush to disk
    Ok(())
}

/// Replaces the target file with the temp file.
///
/// On Windows, this uses rename which is atomic on NTFS.
/// If the target exists, it's first removed, then the temp is renamed.
fn replace_file(target: &Path, temp: &Path) -> std::io::Result<()> {
    // On Windows, std::fs::rename will fail if target exists.
    // Use a backup-rename approach for safety.
    let backup_path = target.with_extension("bak");

    // Remove old backup if it exists
    let _ = std::fs::remove_file(&backup_path);

    // Rename target to backup (if target exists)
    if target.exists() {
        std::fs::rename(target, &backup_path)?;
    }

    // Rename temp to target
    match std::fs::rename(temp, target) {
        Ok(()) => {
            // Success - remove backup
            let _ = std::fs::remove_file(&backup_path);
            Ok(())
        }
        Err(e) => {
            // Failed - restore from backup
            if backup_path.exists() {
                let _ = std::fs::rename(&backup_path, target);
            }
            Err(e)
        }
    }
}

/// Classifies an I/O error into a SaveErrorCode.
fn classify_io_error(e: &std::io::Error) -> SaveErrorCode {
    match e.raw_os_error() {
        Some(code) => {
            // Windows error codes
            match code {
                39 | 112 => SaveErrorCode::DiskFull,       // ERROR_DISK_FULL
                5 => SaveErrorCode::PermissionDenied,      // ERROR_ACCESS_DENIED
                32 | 33 => SaveErrorCode::FileLocked,      // ERROR_SHARING_VIOLATION
                _ => SaveErrorCode::Unknown(e.to_string()),
            }
        }
        None => SaveErrorCode::Unknown(e.to_string()),
    }
}

/// Autosave debounce coordinator.
///
/// Manages the autosave timer for a document. When the document is modified,
/// the autosave timer is reset. When the timer expires, a save is triggered.
#[derive(Debug)]
pub struct AutosaveCoordinator {
    /// Whether autosave is enabled.
    enabled: bool,
    /// Debounce interval in milliseconds.
    interval_ms: u64,
    /// Whether a save is pending.
    pending: bool,
}

impl AutosaveCoordinator {
    /// Creates a new coordinator with the given interval.
    pub fn new(interval_ms: u64) -> Self {
        Self {
            enabled: true,
            interval_ms,
            pending: false,
        }
    }

    /// Notifies the coordinator that a mutation occurred.
    pub fn on_mutation(&mut self) {
        if self.enabled {
            self.pending = true;
        }
    }

    /// Returns true if an autosave should be triggered.
    pub fn should_save(&self) -> bool {
        self.enabled && self.pending
    }

    /// Marks the autosave as completed.
    pub fn on_save_complete(&mut self) {
        self.pending = false;
    }

    /// Enables or disables autosave.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.pending = false;
        }
    }

    /// Returns the debounce interval in milliseconds.
    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_path_same_dir() {
        let target = Path::new("C:/docs/test.xml");
        let temp = temp_file_path(target);
        assert_eq!(temp.parent().unwrap(), Path::new("C:/docs"));
        assert!(temp.file_name().unwrap().to_str().unwrap().starts_with(".test"));
        assert!(temp.to_str().unwrap().ends_with(".tmp"));
    }

    #[test]
    fn atomic_save_success() {
        let dir = std::env::temp_dir().join("mtl_test_save");
        let _ = std::fs::create_dir_all(&dir);
        let target = dir.join("test.xml");
        let content = b"<ROOT>test</ROOT>";

        let config = SaveConfig {
            target_path: target.clone(),
            validate_temp: true,
        };

        let result = atomic_save(&config, content, |data| data == content);
        assert_eq!(result.state, SaveState::Completed);
        assert!(result.fingerprint.is_some());
        assert!(result.error.is_none());

        // Verify file was written
        let saved = std::fs::read(&target).unwrap();
        assert_eq!(saved, content);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_save_validation_failure() {
        let dir = std::env::temp_dir().join("mtl_test_save_fail");
        let _ = std::fs::create_dir_all(&dir);
        let target = dir.join("test.xml");
        let content = b"<ROOT>test</ROOT>";

        let config = SaveConfig {
            target_path: target.clone(),
            validate_temp: true,
        };

        // Validator always fails
        let result = atomic_save(&config, content, |_| false);
        assert_eq!(result.state, SaveState::Failed);
        assert_eq!(result.error, Some(SaveErrorCode::ValidationFailed));

        // Original file should not exist
        assert!(!target.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autosave_coordinator() {
        let mut coord = AutosaveCoordinator::new(2000);
        assert!(!coord.should_save());

        coord.on_mutation();
        assert!(coord.should_save());

        coord.on_save_complete();
        assert!(!coord.should_save());

        coord.set_enabled(false);
        coord.on_mutation();
        assert!(!coord.should_save()); // Disabled
    }
}
