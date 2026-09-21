//! Portable path hardening (RD-M10-030~033 / INH-1121).
//!
//! Four hardening concerns for the Windows portable app:
//!
//! 1. **Removable drives / drive-letter changes** — the install location is
//!    identified by a marker file (`.portable-id.json` with a stable install
//!    UUID), not by its drive letter. If the letter changes (USB replug),
//!    [`find_install_by_marker`] rescans candidate roots for the marker.
//! 2. **Chinese characters and spaces** — every path operation in this module
//!    works on `&Path` (UTF-16 on Windows) and is tested with real temp dirs
//!    containing CJK characters and spaces.
//! 3. **Long paths (>260 chars)** — [`extended_path`] applies the `\\?\`
//!    (or `\\?\UNC\`) prefix so Win32 file APIs bypass `MAX_PATH` limits.
//! 4. **Read-only EXE directory** — [`ensure_writable_data_dir`] probes
//!    `<exe_dir>/Data` and relocates to `%LOCALAPPDATA%\ModernToDoList\Data`
//!    (falling back to `%USERPROFILE%`) when the EXE directory is not
//!    writable.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Win32 legacy path limit (including the NUL terminator).
pub const MAX_PATH: usize = 260;
/// Safety margin: the Win32 APIs add up to 12 chars (8.3 name) internally.
const MAX_PATH_SAFE: usize = MAX_PATH - 12;
/// Marker file that identifies a portable installation.
pub const PORTABLE_MARKER_FILE: &str = ".portable-id.json";
/// App-local fallback directory name under %LOCALAPPDATA%.
pub const FALLBACK_DIR_NAME: &str = "ModernToDoList";

#[derive(Debug, Error)]
pub enum PathHardeningError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Path has no parent directory: {path}")]
    NoParent { path: PathBuf },

    #[error("No writable location found for the Data directory (tried: {tried:?})")]
    NoWritableDataDir { tried: Vec<PathBuf> },

    #[error("Portable install marker not found in any candidate root")]
    MarkerNotFound,

    #[error("Portable install marker is corrupt: {reason}")]
    MarkerCorrupt { reason: String },
}

pub type PathHardeningResult<T> = Result<T, PathHardeningError>;

// ---------------------------------------------------------------------------
// Long paths (RD-M10-032)
// ---------------------------------------------------------------------------

/// Apply the Windows extended-length prefix (`\\?\`) when needed.
///
/// - Relative paths and already-prefixed paths are returned unchanged.
/// - UNC paths `\\server\share` become `\\?\UNC\server\share`.
/// - On non-Windows platforms the path is returned unchanged.
///
/// The prefix is added whenever the path's character length reaches the
/// legacy `MAX_PATH` safety margin; callers may also use
/// [`extended_path_always`] to prefix unconditionally.
pub fn extended_path(path: &Path) -> PathBuf {
    let needs_prefix = path.to_string_lossy().chars().count() >= MAX_PATH_SAFE;
    if needs_prefix {
        extended_path_always(path)
    } else {
        path.to_path_buf()
    }
}

/// Unconditionally apply the extended-length prefix (Windows only).
pub fn extended_path_always(path: &Path) -> PathBuf {
    if cfg!(not(windows)) {
        return path.to_path_buf();
    }
    if !path.is_absolute() {
        return path.to_path_buf();
    }
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        return path.to_path_buf();
    }
    if let Some(rest) = s.strip_prefix(r"\\") {
        // UNC: \\server\share\... -> \\?\UNC\server\share\...
        return PathBuf::from(format!(r"\\?\UNC\{rest}"));
    }
    PathBuf::from(format!(r"\\?\{s}"))
}

/// True when a path exceeds the legacy Win32 MAX_PATH limit.
pub fn exceeds_max_path(path: &Path) -> bool {
    path.to_string_lossy().chars().count() >= MAX_PATH
}

// ---------------------------------------------------------------------------
// Writability probing (RD-M10-033)
// ---------------------------------------------------------------------------

/// Probe whether `dir` exists (or can be created) and accepts file writes.
pub fn is_dir_writable(dir: &Path) -> bool {
    let probe_dir = extended_path(dir);
    if fs::create_dir_all(&probe_dir).is_err() {
        return false;
    }
    let test_file = probe_dir.join(format!(
        ".mtl_write_probe_{}",
        std::process::id()
    ));
    match fs::write(&test_file, b"probe") {
        Ok(()) => {
            let _ = fs::remove_file(&test_file);
            true
        }
        Err(_) => false,
    }
}

/// Data directory location actually in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataDirLocation {
    /// Portable mode: `<exe_dir>\Data`.
    Portable(PathBuf),
    /// EXE directory was not writable; relocated under the user profile.
    RelocatedToProfile(PathBuf),
}

impl DataDirLocation {
    pub fn path(&self) -> &Path {
        match self {
            DataDirLocation::Portable(p) | DataDirLocation::RelocatedToProfile(p) => p,
        }
    }

    pub fn is_relocated(&self) -> bool {
        matches!(self, DataDirLocation::RelocatedToProfile(_))
    }
}

/// Resolve the Data directory, guaranteeing writability (RD-M10-033).
///
/// Tries, in order:
/// 1. `<exe_dir>/Data` — normal portable mode.
/// 2. `%LOCALAPPDATA%/ModernToDoList/Data` — read-only EXE directory
///    (e.g. running from `C:\Program Files` without admin, or a write-
///    protected USB stick).
/// 3. `%USERPROFILE%/.ModernToDoList/Data` — LOCALAPPDATA unavailable.
///
/// `exe_dir` is injected so the logic is testable without moving the EXE.
pub fn ensure_writable_data_dir(exe_dir: &Path) -> PathHardeningResult<DataDirLocation> {
    let mut tried = Vec::new();

    let portable = exe_dir.join("Data");
    tried.push(portable.clone());
    if is_dir_writable(&portable) {
        return Ok(DataDirLocation::Portable(portable));
    }

    for base in profile_fallback_bases() {
        let candidate = base.join(FALLBACK_DIR_NAME).join("Data");
        tried.push(candidate.clone());
        if is_dir_writable(&candidate) {
            log::warn!(
                "EXE directory is not writable; Data directory relocated to {}",
                candidate.display()
            );
            return Ok(DataDirLocation::RelocatedToProfile(candidate));
        }
    }

    Err(PathHardeningError::NoWritableDataDir { tried })
}

/// Fallback base directories under the user profile, in preference order.
fn profile_fallback_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if !local.trim().is_empty() {
            bases.push(PathBuf::from(local));
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.trim().is_empty() {
            bases.push(PathBuf::from(profile).join(".ModernToDoList"));
        }
    }
    if bases.is_empty() {
        bases.push(PathBuf::from("."));
    }
    bases
}

// ---------------------------------------------------------------------------
// Removable drives / drive-letter changes (RD-M10-030)
// ---------------------------------------------------------------------------

/// Marker stored at the root of a portable installation. Identifies the
/// install across drive-letter changes without storing any path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableMarker {
    /// Always `ModernToDoList`.
    pub app: String,
    /// Stable UUID generated once at first run.
    pub install_id: String,
    /// App version that created the marker.
    pub created_by_version: String,
    /// RFC 3339 creation timestamp.
    pub created_utc: String,
}

impl PortableMarker {
    pub fn new(version: &str) -> Self {
        Self {
            app: "ModernToDoList".to_string(),
            install_id: uuid::Uuid::new_v4().to_string(),
            created_by_version: version.to_string(),
            created_utc: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Write (or refresh) the install marker inside `root`.
pub fn write_marker(root: &Path, marker: &PortableMarker) -> PathHardeningResult<()> {
    let path = extended_path(&root.join(PORTABLE_MARKER_FILE));
    fs::create_dir_all(extended_path(root))?;
    fs::write(&path, serde_json::to_string_pretty(marker).unwrap_or_default())?;
    Ok(())
}

/// Read the install marker from `root`, if present and valid.
pub fn read_marker(root: &Path) -> Option<PortableMarker> {
    let path = extended_path(&root.join(PORTABLE_MARKER_FILE));
    let text = fs::read_to_string(&path).ok()?;
    let marker: PortableMarker = serde_json::from_str(&text).ok()?;
    if marker.app != "ModernToDoList" || marker.install_id.trim().is_empty() {
        return None;
    }
    Some(marker)
}

/// Scan candidate roots (e.g. all fixed/removable drive roots) for a
/// portable install whose marker matches `install_id`.
///
/// Returns the first matching root. This is how the app finds itself again
/// after a USB stick is replugged under a different drive letter.
pub fn find_install_by_marker(install_id: &str, candidate_roots: &[PathBuf]) -> Option<PathBuf> {
    for root in candidate_roots {
        if let Some(marker) = read_marker(root) {
            if marker.install_id == install_id {
                return Some(root.clone());
            }
        }
    }
    None
}

/// Enumerate currently mounted drive-letter roots (`C:\`, `D:\`, …).
/// Windows only; empty on other platforms.
pub fn mounted_drive_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        for letter in b'A'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", letter as char));
            if root.exists() {
                roots.push(root);
            }
        }
    }
    roots
}

/// Recover the portable root after a drive-letter change.
///
/// `remembered_root` is the root from the previous session (its drive letter
/// may no longer exist). If it still carries the right marker, it is returned
/// as-is; otherwise all mounted drive roots are scanned for `install_id`.
pub fn resolve_after_drive_change(
    remembered_root: &Path,
    install_id: &str,
) -> Option<PathBuf> {
    if let Some(marker) = read_marker(remembered_root) {
        if marker.install_id == install_id {
            return Some(remembered_root.to_path_buf());
        }
    }
    find_install_by_marker(install_id, &mounted_drive_roots())
}

// ---------------------------------------------------------------------------
// Unicode helpers (RD-M10-031)
// ---------------------------------------------------------------------------

/// Verify a path round-trips through the OS unchanged (lossless UTF-8 →
/// UTF-16 → UTF-8). Used by tests exercising CJK characters and spaces.
pub fn path_roundtrips_lossless(path: &Path) -> bool {
    let s = path.to_str();
    match s {
        Some(s) => Path::new(s) == path,
        None => false,
    }
}

/// Reject path components that are invalid or unsafe on Windows
/// (control chars, reserved names, reserved separators).
pub fn has_windows_unsafe_components(path: &Path) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    for component in path.components() {
        if let Component::Normal(seg) = component {
            let s = seg.to_string_lossy();
            if s.chars().any(|c| (c as u32) < 0x20) {
                return true;
            }
            if s.contains([':', '*', '?', '"', '<', '>', '|']) {
                return true;
            }
            let stem = s.split('.').next().unwrap_or("").to_uppercase();
            if RESERVED.contains(&stem.as_str()) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temp dir whose name contains Chinese characters and spaces.
    fn unicode_tempdir() -> tempfile::TempDir {
        let base = std::env::temp_dir().join(format!(
            "现代待办 测试 dir {}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&base).expect("create CJK base dir");
        tempfile::TempDir::new_in(&base).expect("tempdir under CJK base")
    }

    #[test]
    fn extended_path_prefixes_long_windows_paths() {
        if !cfg!(windows) {
            return;
        }
        let long = Path::new(r"C:\data").join("长".repeat(300));
        assert!(exceeds_max_path(&long));
        let ext = extended_path(&long);
        assert!(ext.to_string_lossy().starts_with(r"\\?\"));
    }

    #[test]
    fn extended_path_leaves_short_and_prefixed_paths_alone() {
        let short = Path::new("Data/index.db");
        assert_eq!(extended_path(short), short);

        if cfg!(windows) {
            let abs = Path::new(r"C:\short\path.txt");
            assert_eq!(extended_path(abs), abs);
            let prefixed = Path::new(r"\\?\C:\already.txt");
            assert_eq!(extended_path_always(prefixed), prefixed);
            let unc = Path::new(r"\\server\share\file.xml");
            assert_eq!(
                extended_path_always(unc),
                PathBuf::from(r"\\?\UNC\server\share\file.xml")
            );
        }
    }

    #[test]
    fn create_and_read_file_beyond_260_chars() {
        // Short base + deeply nested CJK components, total > 260 chars.
        let base = std::env::temp_dir().join(format!("mtl_long_{}", std::process::id()));
        let _ = fs::remove_dir_all(extended_path(&base));
        let mut deep = base.clone();
        for i in 0..12 {
            deep = deep.join(format!("深层目录层级 编号 {i:02} with spaces"));
        }
        let file = deep.join("任务列表文件名称也非常长的情况.xml");
        assert!(exceeds_max_path(&file), "test path must exceed MAX_PATH");

        fs::create_dir_all(extended_path(&deep)).expect("mkdir beyond MAX_PATH");
        fs::write(extended_path(&file), "<TODOLIST/>").expect("write beyond MAX_PATH");
        let content = fs::read_to_string(extended_path(&file)).expect("read beyond MAX_PATH");
        assert_eq!(content, "<TODOLIST/>");

        let _ = fs::remove_dir_all(extended_path(&base));
    }

    #[test]
    fn unicode_paths_roundtrip_losslessly() {
        let dir = unicode_tempdir();
        let p = dir.path().join("我的 任务 列表.xml");
        assert!(path_roundtrips_lossless(&p));
        fs::write(&p, "内容 content").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "内容 content");
        assert!(!has_windows_unsafe_components(&p));
    }

    #[test]
    fn marker_survives_drive_letter_change_simulation() {
        // Simulate: install on "E:\" (dir A), then replug as "G:\" (dir B is
        // the *same* install copied to a new root — letter changed).
        let root_a = unicode_tempdir();
        let marker = PortableMarker::new("2.0.0");
        write_marker(root_a.path(), &marker).unwrap();

        // Same root still resolves.
        assert_eq!(
            read_marker(root_a.path()).unwrap().install_id,
            marker.install_id
        );

        // New letter: a different directory WITHOUT the marker does not match.
        let root_b = unicode_tempdir();
        assert_eq!(
            find_install_by_marker(&marker.install_id, &[root_b.path().to_path_buf()]),
            None
        );

        // After "replug", the marker reappears at the new root.
        write_marker(root_b.path(), &marker).unwrap();
        let found = find_install_by_marker(
            &marker.install_id,
            &[root_a.path().to_path_buf(), root_b.path().to_path_buf()],
        );
        assert_eq!(found, Some(root_a.path().to_path_buf()));

        // A stale remembered root (deleted) falls through to the scan.
        let stale = root_a.path().join("不存在的子目录");
        assert_eq!(
            find_install_by_marker(&marker.install_id, &[stale, root_b.path().to_path_buf()]),
            Some(root_b.path().to_path_buf())
        );
    }

    #[test]
    fn corrupt_marker_is_rejected() {
        let dir = unicode_tempdir();
        fs::write(dir.path().join(PORTABLE_MARKER_FILE), "{ not json").unwrap();
        assert!(read_marker(dir.path()).is_none());

        fs::write(
            dir.path().join(PORTABLE_MARKER_FILE),
            r#"{"app":"OtherApp","installId":"x","createdByVersion":"1","createdUtc":"now"}"#,
        )
        .unwrap();
        assert!(read_marker(dir.path()).is_none());
    }

    #[test]
    fn writable_dir_detection_and_relocation_fallback() {
        let exe_dir = unicode_tempdir();
        let loc = ensure_writable_data_dir(exe_dir.path()).unwrap();
        assert!(!loc.is_relocated());
        assert_eq!(loc.path(), &exe_dir.path().join("Data"));

        // Simulate a read-only EXE directory: <exe_dir>/Data exists as a
        // *file*, so create_dir_all fails and the probe reports not-writable.
        let blocked = unicode_tempdir();
        fs::write(blocked.path().join("Data"), b"i am a file, not a dir").unwrap();
        let loc = ensure_writable_data_dir(blocked.path()).unwrap();
        assert!(loc.is_relocated(), "must relocate under the user profile");
        assert!(loc.path().ends_with("Data"));
        assert!(is_dir_writable(loc.path()));
        // Clean up the relocated dir so tests do not leak into LOCALAPPDATA.
        let _ = fs::remove_dir_all(extended_path(
            loc.path().parent().unwrap_or(loc.path()),
        ));
    }

    #[test]
    fn is_dir_writable_false_for_file_path() {
        let dir = unicode_tempdir();
        let f = dir.path().join("blocker");
        fs::write(&f, b"x").unwrap();
        assert!(!is_dir_writable(&f));
    }

    #[test]
    fn unsafe_components_are_detected() {
        assert!(has_windows_unsafe_components(Path::new(r"C:\dir\CON")));
        assert!(has_windows_unsafe_components(Path::new(r"C:\dir\bad:name")));
        assert!(has_windows_unsafe_components(Path::new("C:/dir/na\u{1}me")));
        assert!(!has_windows_unsafe_components(Path::new(
            r"C:\正常 directory\connect.txt"
        )));
    }
}
