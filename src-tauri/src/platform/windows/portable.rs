// Portable mode implementation (RD-M1-007, RD-M1-008, RD-M1-009)

use std::fmt;
use std::path::PathBuf;

/// Errors that can occur during portable root resolution and data directory operations.
#[derive(Debug)]
#[allow(dead_code)] // Public API variants
pub enum PortableError {
    Io(std::io::Error),
    NoParentDir,
    NotWritable(PathBuf),
}

impl fmt::Display for PortableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortableError::Io(e) => write!(f, "IO error: {e}"),
            PortableError::NoParentDir => write!(f, "Cannot determine parent directory of executable"),
            PortableError::NotWritable(p) => write!(f, "Data directory is not writable: {}", p.display()),
        }
    }
}

impl std::error::Error for PortableError {}

impl From<std::io::Error> for PortableError {
    fn from(e: std::io::Error) -> Self {
        PortableError::Io(e)
    }
}

/// Resolve the portable root directory.
///
/// In release mode, this is the directory containing the executable.
/// In dev mode (`cargo tauri dev`), falls back to the project root (parent of `src-tauri/`).
pub fn resolve_portable_root() -> Result<PathBuf, PortableError> {
    let exe_path = std::env::current_exe().map_err(PortableError::Io)?;
    let exe_dir = exe_path.parent().ok_or(PortableError::NoParentDir)?;

    if cfg!(debug_assertions) {
        // In dev mode, the exe is in src-tauri/target/debug/.
        // Walk up to find the project root (where Cargo.toml of the workspace is).
        // CARGO_MANIFEST_DIR points to src-tauri/, so its parent is the project root.
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let manifest_path = PathBuf::from(&manifest_dir);
            if let Some(project_root) = manifest_path.parent() {
                return Ok(project_root.to_path_buf());
            }
        }
    }

    Ok(exe_dir.to_path_buf())
}

/// Resolve the Data directory: `<portable_root>/Data`.
pub fn resolve_data_dir() -> Result<PathBuf, PortableError> {
    Ok(resolve_portable_root()?.join("Data"))
}

/// Ensure the Data directory exists and is writable.
///
/// Creates the directory if it doesn't exist, then verifies writability
/// by writing and removing a test file. Returns the data dir path on success.
/// Does NOT silently fall back to AppData — caller should show an error dialog.
pub fn ensure_data_dir() -> Result<PathBuf, PortableError> {
    let data_dir = resolve_data_dir()?;

    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)?;
    }

    // Verify writability — clean up any stale test file from a previous crash first
    let test_file = data_dir.join(".write_test");
    let _ = std::fs::remove_file(&test_file); // best-effort cleanup of stale file
    std::fs::write(&test_file, b"")?;
    let _ = std::fs::remove_file(&test_file); // best-effort cleanup after write

    Ok(data_dir)
}

/// Resolve the WebView2 user data directory: `<data_dir>/webview2`.
pub fn resolve_webview2_udf() -> Result<PathBuf, PortableError> {
    Ok(resolve_data_dir()?.join("webview2"))
}

/// Resolve the logs directory: `<data_dir>/logs`.
pub fn resolve_logs_dir() -> Result<PathBuf, PortableError> {
    Ok(resolve_data_dir()?.join("logs"))
}

/// Resolve the settings file path: `<data_dir>/settings.json`.
pub fn resolve_settings_path() -> Result<PathBuf, PortableError> {
    Ok(resolve_data_dir()?.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_portable_root_returns_ok() {
        let root = resolve_portable_root();
        assert!(root.is_ok());
    }

    #[test]
    fn test_resolve_data_dir_ends_with_data() {
        let data_dir = resolve_data_dir().unwrap();
        assert!(data_dir.ends_with("Data"));
    }
}
