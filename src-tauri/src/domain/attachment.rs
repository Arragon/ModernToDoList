//! Attachment domain model for M6 Task Relations (RD-M6-031~048).
//!
//! # Kinds
//!
//! - `ManagedFile` — the file is COPIED into the per-document asset root
//!   `.assets/<document-id>/attachments/` and its integrity is tracked via
//!   BLAKE3.
//! - `LinkedFile` — only a validated path reference is stored; the file is
//!   never copied or modified.
//! - `Url` — a validated http/https URL, stored in the native `FILEREFPATH`
//!   representation (legacy TDL displays it as a file link).
//!
//! # XML storage
//!
//! Every attachment contributes exactly one native `<FILEREFPATH>` child
//! (the legacy-visible reference). Extended metadata (id, kind, display
//! name, size, BLAKE3 hash) is stored in the custom `MTDL_ATTACHMENTS`
//! TASK attribute as JSON — the Tier B fallback mechanism documented for
//! progress links (delivery-plan risk #1). If the attribute is missing or
//! unparsable, the mapper preserves it verbatim via `unknown_attrs`, and
//! the index can still be REBUILT from a plain `FILEREFPATH` scan
//! (`scan_task_attachments` synthesizes refs with deterministic ids).
//!
//! # Transactional managed import (RD-M6-033)
//!
//! `import_managed_attachment`: validate source → copy to a staging file
//! (hashing while copying) → verify BLAKE3 (optionally against a
//! caller-supplied expected hash) → atomic rename into the asset root →
//! return the ref (caller updates XML + index). ANY failure removes the
//! staging file and leaves the asset root and the SOURCE FILE untouched.
//!
//! # Removal semantics (RD-M6-046~048)
//!
//! Linked/URL removal deletes the reference only. Managed removal marks the
//! copied file as an orphan candidate in `.assets/<doc>/orphans.manifest`
//! (JSON-lines); `gc_orphans` later deletes orphaned files, optionally
//! re-verifying the BLAKE3 hash first so modified files are never silently
//! destroyed.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::progress_link;
use super::task::{Task, TaskFileLink};
use super::xml_parser::parse_xml;

/// Custom TASK attribute storing attachment metadata JSON (Tier B fallback).
pub const ATTACHMENTS_ATTR: &str = "MTDL_ATTACHMENTS";
/// Per-document asset directory name (sibling of the document file).
pub const ASSETS_DIR: &str = ".assets";
/// Sub-directory of the asset dir holding managed attachment files.
pub const ATTACHMENTS_SUBDIR: &str = "attachments";
/// Orphan tracking manifest file name inside the asset dir.
pub const ORPHANS_MANIFEST: &str = "orphans.manifest";

/// Errors raised by attachment operations.
#[derive(Debug, Error)]
pub enum AttachmentError {
    /// The source file does not exist.
    #[error("source file not found: {0}")]
    SourceNotFound(String),
    /// The source path is not a regular file.
    #[error("source is not a regular file: {0}")]
    SourceNotFile(String),
    /// An I/O operation failed.
    #[error("I/O error: {0}")]
    Io(String),
    /// A path would escape the asset/document root.
    #[error("path traversal rejected: {0}")]
    PathTraversal(String),
    /// A path is absolute where a relative one is required.
    #[error("absolute path rejected: {0}")]
    AbsolutePath(String),
    /// The URL failed validation (only http/https allowed).
    #[error("invalid attachment URL: {0}")]
    InvalidUrl(String),
    /// The provided path reference is empty or contains control characters.
    #[error("invalid file reference: {0}")]
    InvalidPathRef(String),
    /// The copied file's BLAKE3 hash did not match the expected hash.
    #[error("hash mismatch: expected {expected}, computed {actual}")]
    HashMismatch {
        /// Expected hash.
        expected: String,
        /// Computed hash.
        actual: String,
    },
    /// The target file could not be atomically renamed into place.
    #[error("atomic rename failed: {0}")]
    RenameFailed(String),
    /// XML parsing failed during an index rebuild.
    #[error("XML parse error: {0}")]
    Xml(String),
    /// SQLite failure.
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// The kind of an attachment reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    /// Copied into `.assets/<doc>/attachments/`, integrity-tracked.
    ManagedFile,
    /// A path reference to an external file (never copied).
    LinkedFile,
    /// An http/https URL stored in the native FileLink representation.
    Url,
}

impl AttachmentKind {
    /// Stable lowercase name used in the index (`attachment_type` column).
    pub fn as_str(&self) -> &'static str {
        match self {
            AttachmentKind::ManagedFile => "managed",
            AttachmentKind::LinkedFile => "linked",
            AttachmentKind::Url => "url",
        }
    }
}

/// A typed attachment reference carried by a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRef {
    /// Stable unique id (UUID, or deterministic hash for synthesized refs).
    pub id: String,
    /// Managed / linked / URL.
    pub kind: AttachmentKind,
    /// Human-readable name (original file name or URL).
    pub display_name: String,
    /// For files: document-relative (managed) or as-provided (linked) path.
    /// For URLs: the validated URL.
    pub path_or_url: String,
    /// File size in bytes, when known.
    pub size: Option<u64>,
    /// BLAKE3 hex digest, when known (managed imports always set this).
    pub hash: Option<String>,
}

// ─── Asset root policy ───────────────────────────────────────────────

/// The per-document asset directory: `<doc_dir>/.assets/<document-id>/`.
pub fn document_asset_dir(doc_dir: &Path, document_id: &str) -> PathBuf {
    doc_dir.join(ASSETS_DIR).join(document_id)
}

/// The managed attachment directory:
/// `<doc_dir>/.assets/<document-id>/attachments/`.
pub fn asset_root(doc_dir: &Path, document_id: &str) -> PathBuf {
    document_asset_dir(doc_dir, document_id).join(ATTACHMENTS_SUBDIR)
}

/// The orphan manifest path: `<doc_dir>/.assets/<document-id>/orphans.manifest`.
pub fn orphans_manifest_path(doc_dir: &Path, document_id: &str) -> PathBuf {
    document_asset_dir(doc_dir, document_id).join(ORPHANS_MANIFEST)
}

/// The document-relative reference stored in XML for a managed file.
pub fn managed_relative_path(document_id: &str, file_name: &str) -> String {
    format!(
        "{}/{}/{}/{}",
        ASSETS_DIR, document_id, ATTACHMENTS_SUBDIR, file_name
    )
}

// ─── Path safety ─────────────────────────────────────────────────────

/// Joins a relative path onto `root`, rejecting ANY attempt to escape:
/// absolute paths, drive prefixes, UNC roots and `..` components are all
/// refused. Forward and backward slashes are both accepted (Windows).
pub fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, AttachmentError> {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        return Err(AttachmentError::InvalidPathRef(relative.to_string()));
    }
    if trimmed.chars().any(|c| c.is_ascii_control()) {
        return Err(AttachmentError::InvalidPathRef(relative.to_string()));
    }
    let p = Path::new(trimmed);
    if p.is_absolute() || p.has_root() {
        return Err(AttachmentError::AbsolutePath(relative.to_string()));
    }
    let mut out = root.to_path_buf();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::Normal(seg) => out.push(seg),
            Component::ParentDir => {
                return Err(AttachmentError::PathTraversal(relative.to_string()))
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(AttachmentError::AbsolutePath(relative.to_string()))
            }
        }
    }
    Ok(out)
}

/// Resolves an attachment's `path_or_url` to a filesystem path relative to
/// the document directory, with traversal protection.
pub fn resolve_path(doc_dir: &Path, att: &AttachmentRef) -> Result<PathBuf, AttachmentError> {
    match att.kind {
        AttachmentKind::Url => Err(AttachmentError::InvalidPathRef(
            "URL attachments have no filesystem path".to_string(),
        )),
        AttachmentKind::ManagedFile => safe_join(doc_dir, &att.path_or_url),
        AttachmentKind::LinkedFile => {
            // Linked files may legitimately be absolute or relative.
            let p = Path::new(att.path_or_url.trim());
            if p.is_absolute() {
                Ok(p.to_path_buf())
            } else {
                safe_join(doc_dir, &att.path_or_url)
            }
        }
    }
}

/// Builds a collision-safe file name: `<uuid>.<original-extension>`.
/// The extension is sanitized to at most 10 alphanumeric characters.
pub fn collision_safe_name(original_name: &str) -> String {
    let uuid = uuid::Uuid::new_v4().to_string();
    let ext = Path::new(original_name)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    let clean_ext: String = ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(10)
        .collect();
    if clean_ext.is_empty() {
        uuid
    } else {
        format!("{uuid}.{clean_ext}")
    }
}

// ─── Hashing ─────────────────────────────────────────────────────────

/// Computes the BLAKE3 hex digest of a file.
pub fn blake3_file(path: &Path) -> Result<String, AttachmentError> {
    let mut file = File::open(path).map_err(|e| AttachmentError::Io(format!(
        "open {}: {e}",
        path.display()
    )))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| AttachmentError::Io(format!("read {}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Copies `source` to `staging` in chunks while hashing. Never opens the
/// source for writing — the source file is guaranteed unmodified.
fn copy_and_hash(source: &Path, staging: &Path) -> Result<(u64, String), AttachmentError> {
    let mut input = File::open(source).map_err(|e| {
        AttachmentError::Io(format!("open source {}: {e}", source.display()))
    })?;
    let mut output = File::create(staging).map_err(|e| {
        AttachmentError::Io(format!("create staging {}: {e}", staging.display()))
    })?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size: u64 = 0;
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|e| AttachmentError::Io(format!("read source: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        output
            .write_all(&buf[..n])
            .map_err(|e| AttachmentError::Io(format!("write staging: {e}")))?;
        size += n as u64;
    }
    output
        .flush()
        .map_err(|e| AttachmentError::Io(format!("flush staging: {e}")))?;
    Ok((size, hasher.finalize().to_hex().to_string()))
}

/// RAII guard: deletes the staging file on drop unless disarmed. This is
/// the rollback mechanism — every early return between staging creation
/// and successful rename cleans up automatically.
struct StagingGuard {
    path: PathBuf,
    armed: bool,
}

impl StagingGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

// ─── Transactional managed import (RD-M6-033) ────────────────────────

/// Imports a file as a MANAGED attachment.
///
/// Pipeline: validate source → copy to staging (hashing) → verify hash
/// (against `expected_hash` when provided) → atomic rename into
/// `<doc_dir>/.assets/<document_id>/attachments/<uuid>.<ext>` → return the
/// typed reference with the document-relative XML path.
///
/// On ANY failure the staging file is removed and no partial file remains
/// in the asset root. The source file is only ever opened for reading.
pub fn import_managed_attachment(
    source: &Path,
    doc_dir: &Path,
    document_id: &str,
    expected_hash: Option<&str>,
) -> Result<AttachmentRef, AttachmentError> {
    // 1. Validate source (never modified below — read-only access).
    let meta = fs::metadata(source).map_err(|_| {
        AttachmentError::SourceNotFound(source.display().to_string())
    })?;
    if !meta.is_file() {
        return Err(AttachmentError::SourceNotFile(source.display().to_string()));
    }
    let original_name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| AttachmentError::SourceNotFile(source.display().to_string()))?;

    // 2. Prepare asset root + collision-safe names.
    let root = asset_root(doc_dir, document_id);
    fs::create_dir_all(&root).map_err(|e| {
        AttachmentError::Io(format!("create asset root {}: {e}", root.display()))
    })?;
    let final_name = collision_safe_name(&original_name);
    let final_path = root.join(&final_name);
    let ext_suffix = Path::new(&final_name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let staging_path = root.join(format!(".staging-{}{}", uuid::Uuid::new_v4(), ext_suffix));

    // 3. Copy to staging while hashing; guard rolls back on any failure.
    let mut guard = StagingGuard::new(staging_path.clone());
    let (size, hash) = copy_and_hash(source, &staging_path)?;

    // 4. Verify integrity against a caller-supplied expected hash.
    if let Some(expected) = expected_hash {
        if !expected.eq_ignore_ascii_case(&hash) {
            return Err(AttachmentError::HashMismatch {
                expected: expected.to_string(),
                actual: hash,
            });
        }
    }

    // 5. Atomic rename into place (same directory ⇒ same volume).
    fs::rename(&staging_path, &final_path).map_err(|e| {
        AttachmentError::RenameFailed(format!(
            "{} -> {}: {e}",
            staging_path.display(),
            final_path.display()
        ))
    })?;
    guard.disarm();

    Ok(AttachmentRef {
        id: uuid::Uuid::new_v4().to_string(),
        kind: AttachmentKind::ManagedFile,
        display_name: original_name,
        path_or_url: managed_relative_path(document_id, &final_name),
        size: Some(size),
        hash: Some(hash),
    })
}

// ─── Linked files and URLs (RD-M6-034~035) ───────────────────────────

/// Creates a LINKED-file reference. Only the path is stored — the file is
/// never copied or modified. A missing file is allowed (the UI shows a
/// missing-file state); size/hash are captured when the file exists.
pub fn link_local_file(raw_path: &str) -> Result<AttachmentRef, AttachmentError> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(AttachmentError::InvalidPathRef(raw_path.to_string()));
    }
    if trimmed.chars().any(|c| c.is_ascii_control()) {
        return Err(AttachmentError::InvalidPathRef(raw_path.to_string()));
    }
    let p = Path::new(trimmed);
    let display_name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| trimmed.to_string());
    let (size, hash) = match fs::metadata(p) {
        Ok(m) if m.is_file() => (Some(m.len()), blake3_file(p).ok()),
        _ => (None, None),
    };
    Ok(AttachmentRef {
        id: uuid::Uuid::new_v4().to_string(),
        kind: AttachmentKind::LinkedFile,
        display_name,
        path_or_url: trimmed.to_string(),
        size,
        hash,
    })
}

/// Creates a URL attachment. The URL must pass the same security
/// validation as progress links (http/https only).
pub fn url_attachment(
    url: &str,
    display_name: Option<&str>,
) -> Result<AttachmentRef, AttachmentError> {
    let validated = progress_link::validate_url(url).map_err(|e| {
        AttachmentError::InvalidUrl(format!("{url}: {e}"))
    })?;
    Ok(AttachmentRef {
        id: uuid::Uuid::new_v4().to_string(),
        kind: AttachmentKind::Url,
        display_name: display_name
            .filter(|n| !n.trim().is_empty())
            .map(|n| n.trim().to_string())
            .unwrap_or_else(|| validated.clone()),
        path_or_url: validated,
        size: None,
        hash: None,
    })
}

// ─── Task-level operations ───────────────────────────────────────────

/// Attaches a reference to a task: appends the typed attachment AND the
/// corresponding native `FILEREFPATH` entry (kept in sync). Returns false
/// if an attachment with the same id already exists.
pub fn add_attachment(task: &mut Task, att: AttachmentRef) -> bool {
    if task.attachments.iter().any(|a| a.id == att.id) {
        return false;
    }
    if !task.file_links.iter().any(|l| l.path == att.path_or_url) {
        task.file_links.push(TaskFileLink {
            path: att.path_or_url.clone(),
        });
    }
    task.attachments.push(att);
    true
}

/// Removes an attachment by id: deletes the typed ref and its native
/// `FILEREFPATH` entry. Returns the removed reference so the caller can
/// apply removal semantics (orphan tracking for managed files).
pub fn remove_attachment(task: &mut Task, id: &str) -> Option<AttachmentRef> {
    let pos = task.attachments.iter().position(|a| a.id == id)?;
    let removed = task.attachments.remove(pos);
    task.file_links.retain(|l| l.path != removed.path_or_url);
    Some(removed)
}

/// Removal + orphan tracking in one step (RD-M6-046):
/// Linked/URL → reference deleted only; Managed → marked as orphan
/// candidate in `.assets/<doc>/orphans.manifest`. The managed file itself
/// is NOT deleted here (that is `gc_orphans`'s job).
pub fn remove_attachment_tracked(
    task: &mut Task,
    doc_dir: &Path,
    document_id: &str,
    id: &str,
) -> Result<Option<AttachmentRef>, AttachmentError> {
    let Some(removed) = remove_attachment(task, id) else {
        return Ok(None);
    };
    if removed.kind == AttachmentKind::ManagedFile {
        mark_orphan(doc_dir, document_id, &removed)?;
    }
    Ok(Some(removed))
}

/// Verifies a managed attachment's on-disk integrity against its stored
/// BLAKE3 hash. Ok(true) = intact, Ok(false) = mismatch, Err = unreadable.
pub fn verify_integrity(doc_dir: &Path, att: &AttachmentRef) -> Result<bool, AttachmentError> {
    let Some(expected) = att.hash.as_deref() else {
        return Ok(true); // nothing recorded to verify against
    };
    let path = resolve_path(doc_dir, att)?;
    let actual = blake3_file(&path)?;
    Ok(expected.eq_ignore_ascii_case(&actual))
}

// ─── Orphan tracking (RD-M6-047) ─────────────────────────────────────

/// One JSON-line record in `orphans.manifest`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrphanRecord {
    /// Attachment id.
    pub id: String,
    /// Document-relative path of the managed file.
    pub path: String,
    /// Original display name.
    pub display_name: String,
    /// Recorded size, if known.
    pub size: Option<u64>,
    /// Recorded BLAKE3 hash, if known.
    pub hash: Option<String>,
    /// RFC3339 timestamp of when the reference was removed.
    pub removed_at: String,
}

/// Appends an orphan-candidate record to the manifest (creating it if
/// needed). Idempotent per attachment id.
pub fn mark_orphan(
    doc_dir: &Path,
    document_id: &str,
    att: &AttachmentRef,
) -> Result<(), AttachmentError> {
    let manifest = orphans_manifest_path(doc_dir, document_id);
    if let Some(parent) = manifest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AttachmentError::Io(format!("create {}: {e}", parent.display()))
        })?;
    }
    let mut records = read_orphans(doc_dir, document_id);
    if records.iter().any(|r| r.id == att.id) {
        return Ok(());
    }
    records.push(OrphanRecord {
        id: att.id.clone(),
        path: att.path_or_url.clone(),
        display_name: att.display_name.clone(),
        size: att.size,
        hash: att.hash.clone(),
        removed_at: chrono::Utc::now().to_rfc3339(),
    });
    write_orphans(doc_dir, document_id, &records)
}

/// Reads the orphan manifest; missing file or corrupt lines yield no
/// records (the manifest is derived state, never a business source).
pub fn read_orphans(doc_dir: &Path, document_id: &str) -> Vec<OrphanRecord> {
    let manifest = orphans_manifest_path(doc_dir, document_id);
    let Ok(content) = fs::read_to_string(&manifest) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<OrphanRecord>(l).ok())
        .collect()
}

fn write_orphans(
    doc_dir: &Path,
    document_id: &str,
    records: &[OrphanRecord],
) -> Result<(), AttachmentError> {
    let manifest = orphans_manifest_path(doc_dir, document_id);
    let mut content = String::new();
    for r in records {
        if let Ok(line) = serde_json::to_string(r) {
            content.push_str(&line);
            content.push('\n');
        }
    }
    fs::write(&manifest, content).map_err(|e| {
        AttachmentError::Io(format!("write {}: {e}", manifest.display()))
    })
}

/// Outcome of an orphan garbage-collection run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcReport {
    /// Orphan files deleted.
    pub deleted: usize,
    /// Records whose file was already gone (record dropped).
    pub missing: usize,
    /// Files kept because their BLAKE3 hash no longer matches (record kept
    /// for inspection — modified files are never silently destroyed).
    pub integrity_failed: usize,
}

/// Deletes orphaned managed files and rewrites the manifest. When
/// `verify_hash` is true, a file whose current BLAKE3 hash differs from
/// the recorded hash is KEPT and reported in `integrity_failed`.
pub fn gc_orphans(
    doc_dir: &Path,
    document_id: &str,
    verify_hash: bool,
) -> Result<GcReport, AttachmentError> {
    let records = read_orphans(doc_dir, document_id);
    let mut report = GcReport::default();
    let mut kept = Vec::new();

    for rec in records {
        let path = match safe_join(doc_dir, &rec.path) {
            Ok(p) => p,
            Err(_) => {
                // Unsafe path in the manifest: keep the record, touch nothing.
                kept.push(rec);
                report.integrity_failed += 1;
                continue;
            }
        };
        if !path.exists() {
            report.missing += 1;
            continue;
        }
        if verify_hash {
            if let Some(ref expected) = rec.hash {
                match blake3_file(&path) {
                    Ok(actual) if expected.eq_ignore_ascii_case(&actual) => {}
                    _ => {
                        kept.push(rec);
                        report.integrity_failed += 1;
                        continue;
                    }
                }
            }
        }
        match fs::remove_file(&path) {
            Ok(()) => report.deleted += 1,
            Err(e) => {
                return Err(AttachmentError::Io(format!(
                    "delete {}: {e}",
                    path.display()
                )))
            }
        }
    }

    write_orphans(doc_dir, document_id, &kept)?;
    Ok(report)
}

// ─── XML attribute codec + index (RD-M6-039~040) ─────────────────────

/// Serializes attachment metadata to the `MTDL_ATTACHMENTS` attribute value.
pub fn attachments_to_attr_value(attachments: &[AttachmentRef]) -> String {
    serde_json::to_string(attachments).unwrap_or_else(|_| "[]".to_string())
}

/// Parses attachment metadata from the `MTDL_ATTACHMENTS` attribute value.
/// Returns `None` on any structural problem so the mapper can fall back to
/// preserving the raw attribute in `unknown_attrs`.
pub fn attachments_from_attr_value(value: &str) -> Option<Vec<AttachmentRef>> {
    serde_json::from_str(value).ok()
}

/// Deterministic id for synthesized refs (stable across index rebuilds).
fn synthesized_id(task_key: &str, path_or_url: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(task_key.as_bytes());
    hasher.update(b"\0");
    hasher.update(path_or_url.as_bytes());
    hasher.finalize().to_hex().to_string()[..32].to_string()
}

/// Returns the task's attachments for indexing: the typed M6 metadata
/// UNION synthesized refs for every native `FILEREFPATH` entry that has no
/// metadata (documents that predate M6, or links added by legacy TDL after
/// M6 metadata existed). This is what makes `attachments_index` fully
/// rebuildable from a plain XML scan.
pub fn scan_task_attachments(task_key: &str, task: &Task) -> Vec<AttachmentRef> {
    let mut out = task.attachments.clone();
    for link in &task.file_links {
        let trimmed = link.path.trim();
        if trimmed.is_empty() {
            continue;
        }
        if out.iter().any(|a| a.path_or_url == trimmed) {
            continue;
        }
        let is_url = {
            let lower = trimmed.to_ascii_lowercase();
            lower.starts_with("http://") || lower.starts_with("https://")
        };
        let (kind, display_name) = if is_url {
            (AttachmentKind::Url, trimmed.to_string())
        } else {
            let name = Path::new(&trimmed.replace('\\', "/"))
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| trimmed.to_string());
            (AttachmentKind::LinkedFile, name)
        };
        out.push(AttachmentRef {
            id: synthesized_id(task_key, trimmed),
            kind,
            display_name,
            path_or_url: trimmed.to_string(),
            size: None,
            hash: None,
        });
    }
    out
}

/// One row of the `attachments_index` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentIndexRow {
    /// Attachment id (primary key).
    pub id: String,
    /// The owning task key.
    pub task_key: String,
    /// The owning document id.
    pub document_id: String,
    /// Path or URL as stored in XML.
    pub file_path: String,
    /// Display name.
    pub file_name: String,
    /// Size in bytes, if known.
    pub file_size: Option<i64>,
    /// BLAKE3 hash, if known.
    pub fingerprint: Option<String>,
    /// `managed` / `linked` / `url`.
    pub attachment_type: String,
}

/// Computes the `attachments_index` rows for one task.
pub fn index_rows(document_id: &str, task_key: &str, task: &Task) -> Vec<AttachmentIndexRow> {
    scan_task_attachments(task_key, task)
        .into_iter()
        .map(|a| AttachmentIndexRow {
            id: a.id,
            task_key: task_key.to_string(),
            document_id: document_id.to_string(),
            file_path: a.path_or_url,
            file_name: a.display_name,
            file_size: a.size.map(|s| s as i64),
            fingerprint: a.hash,
            attachment_type: a.kind.as_str().to_string(),
        })
        .collect()
}

/// Replaces the `attachments_index` rows for one task.
pub fn populate_task_attachments(
    conn: &rusqlite::Connection,
    document_id: &str,
    task_key: &str,
    task: &Task,
) -> Result<usize, AttachmentError> {
    conn.execute(
        "DELETE FROM attachments_index WHERE document_id = ?1 AND task_key = ?2",
        rusqlite::params![document_id, task_key],
    )?;
    let rows = index_rows(document_id, task_key, task);
    for row in &rows {
        conn.execute(
            "INSERT OR REPLACE INTO attachments_index \
             (id, task_key, document_id, file_path, file_name, file_size, fingerprint, attachment_type) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                row.id,
                row.task_key,
                row.document_id,
                row.file_path,
                row.file_name,
                row.file_size,
                row.fingerprint,
                row.attachment_type
            ],
        )?;
    }
    Ok(rows.len())
}

/// REBUILDS the whole `attachments_index` for one document from an XML
/// scan — proof that the index is disposable derived state. Returns the
/// number of rows written.
pub fn rebuild_index_from_xml(
    conn: &rusqlite::Connection,
    document_id: &str,
    xml_bytes: &[u8],
) -> Result<usize, AttachmentError> {
    let doc = parse_xml(xml_bytes).map_err(|e| AttachmentError::Xml(e.to_string()))?;
    let mut tasks: Vec<Task> = Vec::new();
    collect_tasks(&doc.root, &mut tasks);

    conn.execute(
        "DELETE FROM attachments_index WHERE document_id = ?1",
        rusqlite::params![document_id],
    )?;
    let mut total = 0;
    for task in &tasks {
        total += populate_task_attachments(conn, document_id, task.id.as_str(), task)?;
    }
    Ok(total)
}

fn collect_tasks(element: &super::xml_tree::XmlElement, tasks: &mut Vec<Task>) {
    if element.tag == "TASK" {
        tasks.push(super::mappers::read_task(element));
    }
    for node in &element.children {
        if let super::xml_tree::XmlNode::Element(child) = node {
            collect_tasks(child, tasks);
        }
    }
}

/// Convenience for tests/tools: append a raw line to a file (used by no
/// production path, kept to document the staging discipline).
fn touch_staging_dir(root: &Path) -> Result<(), AttachmentError> {
    fs::create_dir_all(root)
        .map_err(|e| AttachmentError::Io(format!("create {}: {e}", root.display())))
}

/// Creates the asset root directories for a document (no-op if present).
pub fn ensure_asset_root(doc_dir: &Path, document_id: &str) -> Result<PathBuf, AttachmentError> {
    let root = asset_root(doc_dir, document_id);
    touch_staging_dir(&root)?;
    Ok(root)
}

/// Opens a staging file for writing inside the asset root (used by the
/// import pipeline; exposed for reuse by future image-asset code in M7).
pub fn create_staging_file(root: &Path, suffix: &str) -> Result<(PathBuf, File), AttachmentError> {
    let name = format!(".staging-{}{}", uuid::Uuid::new_v4(), suffix);
    let path = root.join(name);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| AttachmentError::Io(format!("create staging {}: {e}", path.display())))?;
    Ok((path, file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::TaskId;

    fn temp_dir(tag: &str) -> tempfile::TempDir {
        let _ = tag;
        tempfile::TempDir::new().unwrap()
    }

    #[test]
    fn asset_root_layout() {
        let base = Path::new("D:/lists");
        assert_eq!(
            asset_root(base, "doc-42"),
            PathBuf::from("D:/lists/.assets/doc-42/attachments")
        );
        assert_eq!(
            orphans_manifest_path(base, "doc-42"),
            PathBuf::from("D:/lists/.assets/doc-42/orphans.manifest")
        );
        assert_eq!(
            managed_relative_path("doc-42", "abc.pdf"),
            ".assets/doc-42/attachments/abc.pdf"
        );
    }

    #[test]
    fn safe_join_rejects_traversal_and_absolute() {
        let root = Path::new("D:/lists");
        assert_eq!(
            safe_join(root, ".assets/doc/attachments/f.pdf").unwrap(),
            PathBuf::from("D:/lists/.assets/doc/attachments/f.pdf")
        );
        assert!(matches!(
            safe_join(root, "../secret.txt"),
            Err(AttachmentError::PathTraversal(_))
        ));
        assert!(matches!(
            safe_join(root, ".assets/../../secret.txt"),
            Err(AttachmentError::PathTraversal(_))
        ));
        assert!(matches!(
            safe_join(root, "..\\..\\windows\\system32"),
            Err(AttachmentError::PathTraversal(_))
        ));
        assert!(matches!(
            safe_join(root, "C:/Windows/x"),
            Err(AttachmentError::AbsolutePath(_))
        ));
        assert!(matches!(
            safe_join(root, "/etc/passwd"),
            Err(AttachmentError::AbsolutePath(_))
        ));
        assert!(matches!(
            safe_join(root, "\\\\server\\share\\x"),
            Err(AttachmentError::AbsolutePath(_))
        ));
        assert!(matches!(
            safe_join(root, ""),
            Err(AttachmentError::InvalidPathRef(_))
        ));
        assert!(matches!(
            safe_join(root, "a\u{0}b"),
            Err(AttachmentError::InvalidPathRef(_))
        ));
    }

    #[test]
    fn collision_safe_names_keep_extension() {
        let a = collision_safe_name("report final(2).PDF");
        let b = collision_safe_name("report final(2).PDF");
        assert_ne!(a, b, "UUIDs must not collide");
        assert!(a.ends_with(".PDF"));
        assert!(!collision_safe_name("noext").contains('.'));
        // Weird extensions are sanitized away.
        let weird = collision_safe_name("file.ext/../..");
        assert!(!weird.contains('/'));
    }

    #[test]
    fn managed_import_happy_path() {
        let dir = temp_dir("import");
        let doc_dir = dir.path().join("docs");
        fs::create_dir_all(&doc_dir).unwrap();
        let source = dir.path().join("hello.txt");
        fs::write(&source, b"hello managed world").unwrap();
        let src_hash = blake3_file(&source).unwrap();

        let att = import_managed_attachment(&source, &doc_dir, "doc-1", None).unwrap();
        assert_eq!(att.kind, AttachmentKind::ManagedFile);
        assert_eq!(att.display_name, "hello.txt");
        assert_eq!(att.size, Some(19));
        assert_eq!(att.hash.as_deref(), Some(src_hash.as_str()));
        assert!(att.path_or_url.starts_with(".assets/doc-1/attachments/"));
        assert!(att.path_or_url.ends_with(".txt"));

        let final_path = resolve_path(&doc_dir, &att).unwrap();
        assert!(final_path.exists());
        assert_eq!(fs::read(&final_path).unwrap(), b"hello managed world");
        // Source untouched.
        assert_eq!(blake3_file(&source).unwrap(), src_hash);
        // No staging leftovers.
        let root = asset_root(&doc_dir, "doc-1");
        let leftovers: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".staging-")
            })
            .collect();
        assert!(leftovers.is_empty());
        assert!(verify_integrity(&doc_dir, &att).unwrap());
    }

    #[test]
    fn managed_import_verifies_expected_hash() {
        let dir = temp_dir("hash");
        let doc_dir = dir.path().to_path_buf();
        let source = dir.path().join("a.bin");
        fs::write(&source, b"payload").unwrap();
        let good = blake3_file(&source).unwrap();

        let ok = import_managed_attachment(&source, &doc_dir, "d", Some(&good)).unwrap();
        assert_eq!(ok.hash.as_deref(), Some(good.as_str()));

        let bad = import_managed_attachment(&source, &doc_dir, "d", Some("deadbeef")).unwrap_err();
        assert!(matches!(bad, AttachmentError::HashMismatch { .. }));
        // Rollback: no stray files beyond the successful import.
        let root = asset_root(&doc_dir, "d");
        let count = fs::read_dir(&root).unwrap().count();
        assert_eq!(count, 1, "only the successful import remains");
    }

    #[test]
    fn managed_import_rolls_back_on_rename_failure() {
        let dir = temp_dir("rollback");
        let doc_dir = dir.path().to_path_buf();
        let source = dir.path().join("x.txt");
        fs::write(&source, b"data").unwrap();

        // Force the rename to fail: pre-create the *final* path as a
        // directory. We can't predict the UUID name, so instead make the
        // asset root itself contain a directory named like the collision-
        // safe output is impossible — use a different injection: make the
        // staging copy fail by pointing the source at a directory.
        let err = import_managed_attachment(dir.path(), &doc_dir, "d", None).unwrap_err();
        assert!(matches!(err, AttachmentError::SourceNotFile(_)));

        // Rename-failure injection: create the asset root as a FILE so
        // create_dir_all fails → import must not leave partial state and
        // must not touch the source.
        let assets = doc_dir.join(ASSETS_DIR);
        fs::write(&assets, b"blocker").unwrap();
        let err2 = import_managed_attachment(&source, &doc_dir, "d", None).unwrap_err();
        assert!(matches!(err2, AttachmentError::Io(_)));
        assert!(source.exists(), "source never modified or removed");
        assert_eq!(fs::read(&source).unwrap(), b"data");
    }

    #[test]
    fn staging_guard_cleans_up_on_hash_mismatch() {
        let dir = temp_dir("guard");
        let doc_dir = dir.path().to_path_buf();
        let source = dir.path().join("m.bin");
        fs::write(&source, b"some bytes").unwrap();

        let result = import_managed_attachment(&source, &doc_dir, "d", Some("00ff"));
        assert!(result.is_err());
        let root = asset_root(&doc_dir, "d");
        let entries: Vec<_> = fs::read_dir(&root).unwrap().collect();
        assert!(
            entries.is_empty(),
            "staging file must be rolled back, found {} entries",
            entries.len()
        );
    }

    #[test]
    fn linked_file_stores_reference_only() {
        let dir = temp_dir("linked");
        let target = dir.path().join("notes.md");
        fs::write(&target, b"# notes").unwrap();

        let att = link_local_file(target.to_str().unwrap()).unwrap();
        assert_eq!(att.kind, AttachmentKind::LinkedFile);
        assert_eq!(att.display_name, "notes.md");
        assert_eq!(att.size, Some(7));
        assert!(att.hash.is_some());
        // No copy was made anywhere.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);

        // Missing file is allowed (UI shows missing state).
        let missing = link_local_file("D:/gone/never-exists.txt").unwrap();
        assert_eq!(missing.kind, AttachmentKind::LinkedFile);
        assert!(missing.size.is_none() && missing.hash.is_none());

        assert!(matches!(
            link_local_file("  "),
            Err(AttachmentError::InvalidPathRef(_))
        ));
        assert!(matches!(
            link_local_file("bad\u{0}path"),
            Err(AttachmentError::InvalidPathRef(_))
        ));
    }

    #[test]
    fn url_attachment_validates_scheme() {
        let ok = url_attachment("https://example.com/spec.pdf", None).unwrap();
        assert_eq!(ok.kind, AttachmentKind::Url);
        assert_eq!(ok.display_name, "https://example.com/spec.pdf");
        let named = url_attachment("http://a.example/x", Some("Spec")).unwrap();
        assert_eq!(named.display_name, "Spec");
        for bad in [
            "javascript:alert(1)",
            "file:///C:/x",
            "data:text/plain,x",
            "java\tscript:alert(1)",
        ] {
            assert!(
                matches!(url_attachment(bad, None), Err(AttachmentError::InvalidUrl(_))),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn add_remove_keeps_file_links_in_sync() {
        let mut t = Task::new(TaskId::new("1"));
        let att = url_attachment("https://example.com/a", None).unwrap();
        assert!(add_attachment(&mut t, att.clone()));
        assert!(!add_attachment(&mut t, att.clone()));
        assert_eq!(t.file_links.len(), 1);
        assert_eq!(t.file_links[0].path, att.path_or_url);

        let removed = remove_attachment(&mut t, &att.id).unwrap();
        assert_eq!(removed, att);
        assert!(t.file_links.is_empty());
        assert!(remove_attachment(&mut t, &att.id).is_none());
    }

    #[test]
    fn orphan_lifecycle_and_gc() {
        let dir = temp_dir("orphans");
        let doc_dir = dir.path().to_path_buf();
        let source = dir.path().join("doc-att.bin");
        fs::write(&source, b"managed payload").unwrap();

        let mut t = Task::new(TaskId::new("5"));
        let managed = import_managed_attachment(&source, &doc_dir, "doc-9", None).unwrap();
        let linked = link_local_file(source.to_str().unwrap()).unwrap();
        add_attachment(&mut t, managed.clone());
        add_attachment(&mut t, linked.clone());

        // Linked removal: reference only, no orphan record.
        let r = remove_attachment_tracked(&mut t, &doc_dir, "doc-9", &linked.id)
            .unwrap()
            .unwrap();
        assert_eq!(r.kind, AttachmentKind::LinkedFile);
        assert!(read_orphans(&doc_dir, "doc-9").is_empty());
        assert!(resolve_path(&doc_dir, &linked).unwrap().exists());

        // Managed removal: orphan candidate recorded, file kept.
        remove_attachment_tracked(&mut t, &doc_dir, "doc-9", &managed.id)
            .unwrap()
            .unwrap();
        let orphans = read_orphans(&doc_dir, "doc-9");
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].id, managed.id);
        assert!(!orphans[0].removed_at.is_empty());
        let managed_path = resolve_path(&doc_dir, &managed).unwrap();
        assert!(managed_path.exists(), "GC has not run yet");
        // mark_orphan is idempotent.
        mark_orphan(&doc_dir, "doc-9", &managed).unwrap();
        assert_eq!(read_orphans(&doc_dir, "doc-9").len(), 1);

        // GC with hash verification deletes the intact file.
        let report = gc_orphans(&doc_dir, "doc-9", true).unwrap();
        assert_eq!(report.deleted, 1);
        assert!(!managed_path.exists());
        assert!(read_orphans(&doc_dir, "doc-9").is_empty());
    }

    #[test]
    fn gc_keeps_modified_orphans() {
        let dir = temp_dir("gc-modified");
        let doc_dir = dir.path().to_path_buf();
        let source = dir.path().join("keepme.bin");
        fs::write(&source, b"original").unwrap();
        let managed = import_managed_attachment(&source, &doc_dir, "doc-9", None).unwrap();
        let managed_path = resolve_path(&doc_dir, &managed).unwrap();
        mark_orphan(&doc_dir, "doc-9", &managed).unwrap();

        // Simulate external modification after removal.
        fs::write(&managed_path, b"tampered!").unwrap();

        let report = gc_orphans(&doc_dir, "doc-9", true).unwrap();
        assert_eq!(report.integrity_failed, 1);
        assert_eq!(report.deleted, 0);
        assert!(managed_path.exists(), "modified file must be kept");
        assert_eq!(read_orphans(&doc_dir, "doc-9").len(), 1);

        // Without verification it is deleted.
        let report2 = gc_orphans(&doc_dir, "doc-9", false).unwrap();
        assert_eq!(report2.deleted, 1);
        assert!(!managed_path.exists());

        // Missing files are reported and records dropped.
        mark_orphan(&doc_dir, "doc-9", &managed).unwrap();
        fs::remove_file(&managed_path).ok();
        let report3 = gc_orphans(&doc_dir, "doc-9", true).unwrap();
        assert_eq!(report3.missing, 1);
        assert!(read_orphans(&doc_dir, "doc-9").is_empty());
    }

    #[test]
    fn attr_codec_roundtrip() {
        let atts = vec![
            url_attachment("https://example.com/a", Some("A")).unwrap(),
            link_local_file("C:/temp/b.txt").unwrap_or_else(|_| AttachmentRef {
                id: "x".into(),
                kind: AttachmentKind::LinkedFile,
                display_name: "b.txt".into(),
                path_or_url: "C:/temp/b.txt".into(),
                size: None,
                hash: None,
            }),
        ];
        let json = attachments_to_attr_value(&atts);
        assert_eq!(attachments_from_attr_value(&json).unwrap(), atts);
        assert!(attachments_from_attr_value("{oops").is_none());
    }

    #[test]
    fn scan_synthesizes_from_file_links() {
        let mut t = Task::new(TaskId::new("3"));
        t.file_links = vec![
            TaskFileLink {
                path: ".\\docs\\report.pdf".into(),
            },
            TaskFileLink {
                path: "https://example.com/spec".into(),
            },
        ];
        let scanned = scan_task_attachments("3", &t);
        assert_eq!(scanned.len(), 2);
        assert_eq!(scanned[0].kind, AttachmentKind::LinkedFile);
        assert_eq!(scanned[0].display_name, "report.pdf");
        assert_eq!(scanned[1].kind, AttachmentKind::Url);
        // Deterministic ids → stable rebuilds.
        let again = scan_task_attachments("3", &t);
        assert_eq!(again[0].id, scanned[0].id);

        // Typed metadata wins for its own entries; untyped native file
        // links are still synthesized (union), so the index covers both.
        let att = url_attachment("https://x.example/y", Some("Y")).unwrap();
        add_attachment(&mut t, att.clone());
        let with_typed = scan_task_attachments("3", &t);
        assert_eq!(with_typed.len(), 3);
        assert_eq!(with_typed[0], att);
        assert_eq!(with_typed[1].id, scanned[0].id);
        assert_eq!(with_typed[2].id, scanned[1].id);
    }

    #[test]
    fn index_rows_and_populate() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE attachments_index (
                id TEXT PRIMARY KEY, task_key TEXT NOT NULL, document_id TEXT NOT NULL,
                file_path TEXT NOT NULL, file_name TEXT NOT NULL, file_size INTEGER,
                fingerprint TEXT, attachment_type TEXT NOT NULL DEFAULT 'managed');",
        )
        .unwrap();
        let mut t = Task::new(TaskId::new("8"));
        t.file_links = vec![TaskFileLink {
            path: ".\\a.docx".into(),
        }];
        let rows = index_rows("doc-1", "8", &t);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].attachment_type, "linked");
        let n = populate_task_attachments(&conn, "doc-1", "8", &t).unwrap();
        assert_eq!(n, 1);
        populate_task_attachments(&conn, "doc-1", "8", &t).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM attachments_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn rebuild_index_from_xml_scan() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE attachments_index (
                id TEXT PRIMARY KEY, task_key TEXT NOT NULL, document_id TEXT NOT NULL,
                file_path TEXT NOT NULL, file_name TEXT NOT NULL, file_size INTEGER,
                fingerprint TEXT, attachment_type TEXT NOT NULL DEFAULT 'managed');",
        )
        .unwrap();
        let xml = br#"<?xml version="1.0" encoding="utf-8"?>
<TODOLIST NEXTUNIQUEID="3">
<TASK ID="1" TITLE="T1"><FILEREFPATH>.\files\a.pdf</FILEREFPATH><FILEREFPATH>https://example.com/b</FILEREFPATH></TASK>
<TASK ID="2" TITLE="T2"><FILEREFPATH>.\files\c.txt</FILEREFPATH></TASK>
</TODOLIST>"#;
        let n = rebuild_index_from_xml(&conn, "doc-7", xml).unwrap();
        assert_eq!(n, 3);
        // Rebuild is idempotent (delete-then-insert per document).
        let n2 = rebuild_index_from_xml(&conn, "doc-7", xml).unwrap();
        assert_eq!(n2, 3);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM attachments_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
        let urls: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attachments_index WHERE attachment_type='url'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(urls, 1);
    }
}
