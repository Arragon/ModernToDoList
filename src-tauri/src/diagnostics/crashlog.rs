//! Privacy-safe crash reports and bug-report export (RD-M10-035~036 / INH-1122).
//!
//! A [`CrashReport`] captures everything support needs — timestamp, sanitized
//! stack frames, the trail of recent operations and system info — while
//! guaranteeing two privacy invariants:
//!
//! 1. **No task content.** Operation names are coarse-grained verbs
//!    (`open_document`, `save_document`, `rebuild_index`); titles, comments
//!    and other business data are never recorded.
//! 2. **No raw file paths.** Every path is reduced to a stable SHA-256-based
//!    hash prefix ([`hash_path`]); free-text messages (panic strings, log
//!    lines) are scrubbed of path-shaped substrings by [`sanitize_message`].
//!
//! [`export_bug_report`] bundles the JSON report plus optional pre-sanitized
//! log files into a single ZIP the user can attach to a support request.

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime as ZipDateTime, ZipWriter};

/// Bump if the report schema changes.
pub const CRASH_REPORT_SCHEMA_VERSION: u32 = 1;

/// Number of hex characters kept from the path hash.
const PATH_HASH_LEN: usize = 16;

/// Stable, non-reversible hash of a filesystem path.
///
/// Normalization: backslashes → `/`, lowercased on Windows-style drive
/// letters, then SHA-256; the first [`PATH_HASH_LEN`] hex chars are kept.
/// The same path always produces the same hash, so support can correlate
/// operations without ever seeing the path.
pub fn hash_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let digest = Sha256::digest(normalized.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..PATH_HASH_LEN].to_string()
}

/// Replace path-shaped substrings (drive-letter paths and UNC paths) in a
/// free-text message with `[path:<hash8>]` placeholders.
///
/// The patterns also consume space-separated continuations when the next
/// token still looks like a path segment (contains a separator), so Chinese
/// directory names with spaces are scrubbed as a whole.
pub fn sanitize_message(message: &str) -> String {
    // Continuation: ` <token containing \ or />`, repeated.
    let cont = r#"(?:\s+[^\s"'<>|]*[\\/][^\s"'<>|]*)*"#;
    let drive_re = Regex::new(&format!(r#"[A-Za-z]:[\\/][^\s"'<>|]*{cont}"#))
        .expect("valid regex");
    let unc_re = Regex::new(&format!(r#"\\\\[^\s"'<>|]*{cont}"#)).expect("valid regex");

    let mut out = unc_re
        .replace_all(message, |caps: &regex::Captures| {
            format!("[path:{}] (unc)", &hash_path(&caps[0])[..8])
        })
        .into_owned();
    out = drive_re
        .replace_all(&out, |caps: &regex::Captures| {
            format!("[path:{}]", &hash_path(&caps[0])[..8])
        })
        .into_owned();
    out
}

/// One recorded operation in the trail preceding the crash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub sequence: u32,
    /// Coarse operation verb, e.g. `save_document`. Never contains task data.
    pub operation: String,
    /// Hash of the path involved (see [`hash_path`]), empty when no path.
    pub path_hash: String,
    pub duration_ms: u64,
    /// True when the operation failed.
    pub failed: bool,
}

/// Bounded ring buffer of recent operations. Keeps at most `capacity`
/// entries; the oldest are dropped first.
#[derive(Debug, Clone)]
pub struct OperationTrail {
    capacity: usize,
    next_sequence: u32,
    records: VecDeque<OperationRecord>,
}

impl OperationTrail {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_sequence: 1,
            records: VecDeque::new(),
        }
    }

    /// Record an operation. `path` is hashed immediately and then dropped —
    /// the trail never holds the raw path.
    pub fn record(&mut self, operation: &str, path: Option<&str>, duration_ms: u64, failed: bool) {
        let record = OperationRecord {
            sequence: self.next_sequence,
            operation: operation.to_string(),
            path_hash: path.map(hash_path).unwrap_or_default(),
            duration_ms,
            failed,
        };
        self.next_sequence += 1;
        if self.records.len() == self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    pub fn records(&self) -> Vec<OperationRecord> {
        self.records.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// One sanitized stack frame. Module/function names only — never file paths
/// of the user's data (Rust source locations are omitted by design).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    pub function: String,
    pub module: Option<String>,
    pub offset: Option<u64>,
}

impl StackFrame {
    pub fn new(function: impl Into<String>) -> Self {
        // Defense in depth: even symbol strings pass through the scrubber.
        Self {
            function: sanitize_message(&function.into()),
            module: None,
            offset: None,
        }
    }
}

/// Non-identifying system information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub pointer_width_bits: u32,
    /// Hash of the portable root directory (not the path itself).
    pub portable_root_hash: String,
    /// Hash of the Data directory (not the path itself).
    pub data_dir_hash: String,
}

impl SystemInfo {
    /// Collect system info for the current machine. Paths are hashed.
    pub fn collect(portable_root: Option<&Path>, data_dir: Option<&Path>) -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            pointer_width_bits: (usize::BITS),
            portable_root_hash: portable_root
                .map(|p| hash_path(&p.to_string_lossy()))
                .unwrap_or_default(),
            data_dir_hash: data_dir
                .map(|p| hash_path(&p.to_string_lossy()))
                .unwrap_or_default(),
        }
    }
}

/// The structured crash dump. Serialized as JSON inside the bug-report bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    pub schema_version: u32,
    pub crash_id: String,
    pub timestamp_utc: String,
    pub app_version: String,
    pub system: SystemInfo,
    /// Error class, e.g. `panic`, `oom`, `unhandled_io`.
    pub error_kind: String,
    /// Sanitized message (paths replaced by hash placeholders).
    pub error_message: String,
    pub stack_frames: Vec<StackFrame>,
    pub last_operations: Vec<OperationRecord>,
    pub privacy_notice: String,
}

impl CrashReport {
    /// Build a report. `raw_message` and every operation path are sanitized
    /// here, at the boundary, so a report can never be constructed with
    /// verbatim user paths.
    pub fn capture(
        app_version: &str,
        error_kind: &str,
        raw_message: &str,
        stack_frames: Vec<StackFrame>,
        trail: &OperationTrail,
        system: SystemInfo,
        timestamp_utc: String,
    ) -> Self {
        Self {
            schema_version: CRASH_REPORT_SCHEMA_VERSION,
            crash_id: uuid::Uuid::new_v4().to_string(),
            timestamp_utc,
            app_version: app_version.to_string(),
            system,
            error_kind: error_kind.to_string(),
            error_message: sanitize_message(raw_message),
            stack_frames,
            last_operations: trail.records(),
            privacy_notice: "This report contains no task content and no raw file paths; \
                             paths appear only as SHA-256 hash prefixes."
                .to_string(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("CrashReport serializes")
    }
}

/// Current UTC timestamp in RFC 3339 format.
pub fn now_utc_string() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Export a user-submittable bug-report bundle.
///
/// Produces `<out_dir>/ModernToDoList-bugreport-<crash_id8>.zip` containing:
/// - `crash-report.json` — the structured dump,
/// - `README.txt` — submission instructions,
/// - any `extra_files` the caller provides (must already be sanitized;
///   e.g. redacted log tails).
///
/// Returns the bundle path.
pub fn export_bug_report(
    out_dir: &Path,
    report: &CrashReport,
    extra_files: &[(&str, Vec<u8>)],
) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(out_dir)?;

    let short_id = report.crash_id.split('-').next().unwrap_or("unknown");
    let bundle_name = format!("ModernToDoList-bugreport-{short_id}.zip");
    let bundle_path = out_dir.join(&bundle_name);

    let readme = format!(
        "ModernToDoList bug report {id}\n\
         ================================\n\n\
         How to submit:\n\
         1. Attach this ZIP to your support request (or Linear issue).\n\
         2. Describe what you were doing when the problem occurred.\n\
         3. If a file failed to open, mention its name only if you are\n\
            comfortable sharing it — the report itself contains no paths\n\
            or task content, only hashes.\n\n\
         Report privacy: paths are stored as SHA-256 hash prefixes so\n\
         support can correlate events without seeing your file names.\n",
        id = report.crash_id
    );

    // Fixed timestamp keeps bundles reproducible for identical inputs.
    let ts = ZipDateTime::from_date_and_time(2025, 1, 1, 0, 0, 0)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(9))
        .last_modified_time(ts);

    let file = std::fs::File::create(&bundle_path)?;
    let mut zip = ZipWriter::new(file);

    zip.start_file("crash-report.json", opts)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    zip.write_all(report.to_json().as_bytes())?;

    zip.start_file("README.txt", opts)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    zip.write_all(readme.as_bytes())?;

    for (name, bytes) in extra_files {
        zip.start_file(*name, opts)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        zip.write_all(bytes)?;
    }

    zip.finish()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    Ok(bundle_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> CrashReport {
        let mut trail = OperationTrail::new(8);
        trail.record("open_document", Some(r"D:\用户文档\我的项目 副本\lists\秘密任务.xml"), 12, false);
        trail.record("save_document", Some(r"D:\用户文档\我的项目 副本\lists\秘密任务.xml"), 30, true);
        trail.record("rebuild_index", None, 900, false);

        CrashReport::capture(
            "2.0.0",
            "panic",
            r"called `Result::unwrap()` on an `Err` value: Io error opening D:\用户文档\我的项目 副本\Data\index.db",
            vec![
                StackFrame::new("moderntodolist_lib::infrastructure::db::open"),
                StackFrame::new("core::result::unwrap_failed"),
            ],
            &trail,
            SystemInfo::collect(None, None),
            "2026-01-15T08:30:00Z".to_string(),
        )
    }

    #[test]
    fn hash_path_is_stable_and_hides_input() {
        let h1 = hash_path(r"D:\数据\列表.xml");
        let h2 = hash_path("D:/数据/列表.xml");
        assert_eq!(h1, h2, "separator style must not change the hash");
        assert_eq!(h1.len(), PATH_HASH_LEN);
        assert_ne!(hash_path("a"), hash_path("b"));
    }

    #[test]
    fn sanitize_message_replaces_paths_including_unicode() {
        let msg = r"failed to open D:\用户文档\我的项目 副本\Data\index.db (locked)";
        let s = sanitize_message(msg);
        assert!(!s.contains("用户文档"));
        assert!(!s.contains("index.db"));
        assert!(s.contains("[path:"));
    }

    #[test]
    fn serialized_report_never_contains_verbatim_unicode_path() {
        let report = sample_report();
        let json = report.to_json();

        // The raw Chinese path fragments must not appear anywhere.
        for forbidden in [
            "用户文档",
            "我的项目 副本",
            "秘密任务",
            r"D:\用户文档",
            "D:/用户文档",
        ] {
            assert!(
                !json.contains(forbidden),
                "privacy violation: serialized report contains '{forbidden}'"
            );
        }
        // But the hash of the path IS present, so support can correlate.
        let expected = hash_path(r"D:\用户文档\我的项目 副本\lists\秘密任务.xml");
        assert!(json.contains(&expected));
    }

    #[test]
    fn trail_is_bounded_and_sequential() {
        let mut trail = OperationTrail::new(3);
        for i in 0..5 {
            trail.record("op", Some(&format!("C:/f{i}.xml")), i, false);
        }
        assert_eq!(trail.len(), 3);
        let recs = trail.records();
        // Oldest two were dropped; sequences continue.
        assert_eq!(recs[0].sequence, 3);
        assert_eq!(recs[2].sequence, 5);
    }

    #[test]
    fn report_json_roundtrips() {
        let report = sample_report();
        let back: CrashReport = serde_json::from_str(&report.to_json()).unwrap();
        assert_eq!(back, report);
        assert_eq!(back.schema_version, CRASH_REPORT_SCHEMA_VERSION);
    }

    #[test]
    fn stack_frames_are_scrubbed() {
        let f = StackFrame::new(r"open at C:\Users\张三\secret.rs");
        assert!(!f.function.contains("张三"));
        assert!(f.function.contains("[path:"));
    }

    #[test]
    fn export_produces_readable_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let report = sample_report();
        let bundle = export_bug_report(
            tmp.path(),
            &report,
            &[("logs/app.log.txt", b"sanitized log line".to_vec())],
        )
        .unwrap();

        assert!(bundle.exists());
        assert!(bundle
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("ModernToDoList-bugreport-"));

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&bundle).unwrap()).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "crash-report.json".to_string(),
                "README.txt".to_string(),
                "logs/app.log.txt".to_string()
            ]
        );

        let mut json = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("crash-report.json").unwrap(), &mut json)
            .unwrap();
        assert!(!json.contains("用户文档"));
        assert!(json.contains(&report.crash_id));
    }

    #[test]
    fn system_info_hashes_directories() {
        let info = SystemInfo::collect(
            Some(Path::new(r"E:\便携 应用")),
            Some(Path::new(r"E:\便携 应用\Data")),
        );
        assert_eq!(info.portable_root_hash.len(), PATH_HASH_LEN);
        assert_eq!(info.data_dir_hash.len(), PATH_HASH_LEN);
        assert!(!info.os.is_empty());
        assert_eq!(info.pointer_width_bits, usize::BITS);
    }
}
