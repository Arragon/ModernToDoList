//! Workspace Trash (M8, RD-M8-039~042).
//!
//! Deletion of workspace documents is never a direct file delete. Files are
//! moved into a trash area under `<workspace_root>/.moderntodo/trash/`:
//!
//! ```text
//! .moderntodo/trash/
//!   manifest.json          ← durable manifest of trashed items
//!   files/<trash_id>__<original file name>
//! ```
//!
//! Guarantees:
//! - The manifest is written durably (tmp + fsync + rename) and the previous
//!   version is kept as `.bak`, mirroring the transfer journal protocol.
//! - `restore` puts the file back to its ORIGINAL path (collision-safe:
//!     a `(restored-N)` suffix is used when the original path is occupied),
//!     and re-registers the document in `workspace.json` with its ORIGINAL
//!     stable DocumentId when it was registered before deletion.
//! - `purge` (explicit permanent deletion) removes the stored file and the
//!   manifest entry; `empty_trash` purges everything.
//! - Moving to trash across volumes falls back to copy + verify + delete.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::fingerprint::FileFingerprint;
use super::workspace::{DocumentEntry, DocumentType, Workspace};

/// Trash-specific errors.
#[derive(Debug, Error)]
pub enum TrashError {
    #[error("trash I/O error ({context}): {reason}")]
    Io { context: String, reason: String },

    #[error("trash manifest corrupt: {reason}")]
    ManifestCorrupt { reason: String },

    #[error("trash entry not found: {0}")]
    EntryNotFound(String),

    #[error("stored trash file missing for entry {trash_id} (expected at {path})")]
    StoredFileMissing { trash_id: String, path: PathBuf },

    #[error("file to trash does not exist: {0}")]
    FileNotFound(PathBuf),

    #[error("workspace error: {0}")]
    Workspace(#[from] super::workspace::WorkspaceError),
}

pub type TrashResult<T> = Result<T, TrashError>;

/// Sub-layout of the trash area inside `.moderntodo/`.
pub const TRASH_DIR_NAME: &str = "trash";
const FILES_DIR_NAME: &str = "files";
const MANIFEST_NAME: &str = "manifest.json";

/// One trashed item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashEntry {
    /// Unique trash item id.
    pub trash_id: String,
    /// Absolute original path of the file.
    pub original_path: PathBuf,
    /// Workspace-relative path when the file was a managed document.
    pub workspace_rel_path: Option<String>,
    /// Stable workspace DocumentId, when the file was registered.
    pub doc_id: Option<String>,
    /// Document type at deletion time.
    pub doc_type: Option<DocumentType>,
    /// The full workspace entry at deletion time (for exact re-registration).
    pub workspace_entry: Option<DocumentEntry>,
    /// Absolute path of the stored copy inside the trash area.
    pub stored_path: PathBuf,
    /// BLAKE3 fingerprint of the stored bytes (integrity on restore).
    pub fingerprint: String,
    /// Deletion timestamp (epoch seconds).
    pub deleted_at_epoch_secs: u64,
}

/// The durable trash manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrashManifest {
    pub version: u32,
    pub entries: Vec<TrashEntry>,
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Absolute path of the trash root for a workspace.
pub fn trash_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".moderntodo").join(TRASH_DIR_NAME)
}

fn manifest_path(workspace_root: &Path) -> PathBuf {
    trash_root(workspace_root).join(MANIFEST_NAME)
}

fn files_dir(workspace_root: &Path) -> PathBuf {
    trash_root(workspace_root).join(FILES_DIR_NAME)
}

fn io_err(context: impl Into<String>, e: impl std::fmt::Display) -> TrashError {
    TrashError::Io {
        context: context.into(),
        reason: e.to_string(),
    }
}

/// Durably writes the manifest (tmp + fsync + rename, keeping `.bak`).
fn save_manifest(workspace_root: &Path, manifest: &TrashManifest) -> TrashResult<()> {
    let path = manifest_path(workspace_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_err("create trash dir", e))?;
    }
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|e| TrashError::ManifestCorrupt { reason: e.to_string() })?;
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp).map_err(|e| io_err("create manifest tmp", e))?;
        f.write_all(&bytes).map_err(|e| io_err("write manifest tmp", e))?;
        f.sync_all().map_err(|e| io_err("fsync manifest tmp", e))?;
    }
    if path.exists() {
        let bak = path.with_extension("bak");
        let _ = fs::remove_file(&bak);
        let _ = fs::rename(&path, &bak);
    }
    fs::rename(&tmp, &path).map_err(|e| io_err("replace manifest", e))?;
    Ok(())
}

/// Loads the manifest, falling back to `.bak` when the primary is corrupt.
pub fn load_manifest(workspace_root: &Path) -> TrashResult<TrashManifest> {
    let path = manifest_path(workspace_root);
    if !path.exists() {
        return Ok(TrashManifest { version: 1, entries: Vec::new() });
    }
    let try_parse = |p: &Path| -> Option<TrashManifest> {
        fs::read(p)
            .ok()
            .and_then(|b| serde_json::from_slice::<TrashManifest>(&b).ok())
    };
    if let Some(m) = try_parse(&path) {
        return Ok(m);
    }
    if let Some(m) = try_parse(&path.with_extension("bak")) {
        return Ok(m);
    }
    Err(TrashError::ManifestCorrupt {
        reason: format!("{} (and .bak) unreadable", path.display()),
    })
}

/// Moves a file into the trash. Used by every document-deletion flow.
///
/// `doc_id` optionally identifies the workspace registration to look up (the
/// caller may also pass `None` for unregistered files). The file itself is
/// NEVER deleted by this function; use [`purge`] for permanent deletion.
pub fn move_file_to_trash(
    workspace_root: &Path,
    file: &Path,
    doc_id: Option<&str>,
) -> TrashResult<TrashEntry> {
    move_file_to_trash_ws(workspace_root, file, doc_id, None)
}

/// Variant that also accepts a mutable workspace to unregister the document
/// atomically with the trash move (unregister happens only AFTER the file is
/// safely stored).
pub fn move_file_to_trash_ws(
    workspace_root: &Path,
    file: &Path,
    doc_id: Option<&str>,
    ws: Option<&mut Workspace>,
) -> TrashResult<TrashEntry> {
    if !file.is_file() {
        return Err(TrashError::FileNotFound(file.to_path_buf()));
    }
    let fingerprint = FileFingerprint::from_file(file)
        .map_err(|e| io_err("fingerprint trashed file", e))?
        .hash;

    // Resolve workspace metadata for this file (if registered).
    let mut workspace_entry: Option<DocumentEntry> = None;
    let mut workspace_rel: Option<String> = None;
    let mut doc_type: Option<DocumentType> = None;
    if let Some(ws) = ws.as_ref() {
        for d in ws.documents() {
            let matches_id = doc_id.map(|id| id == d.id).unwrap_or(false);
            let resolved = ws.resolve_document_path(d);
            let matches_path = same_file_best_effort(&resolved, file);
            if matches_id || (doc_id.is_none() && matches_path) {
                workspace_entry = Some(d.clone());
                doc_type = Some(d.doc_type);
                if d.doc_type == DocumentType::Managed {
                    workspace_rel = Some(d.file_path.clone());
                }
                break;
            }
        }
    }

    let trash_id = uuid::Uuid::new_v4().simple().to_string();
    let file_name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document.xml".into());
    let stored = files_dir(workspace_root).join(format!("{}__{}", trash_id, file_name));
    fs::create_dir_all(files_dir(workspace_root))
        .map_err(|e| io_err("create trash files dir", e))?;

    move_file(file, &stored).map_err(|e| io_err("move file into trash", e))?;

    // Verify the stored bytes before touching any registration.
    let stored_fp = FileFingerprint::from_file(&stored)
        .map_err(|e| io_err("fingerprint stored trash file", e))?
        .hash;
    if stored_fp != fingerprint {
        // Integrity failure: put the file back, change nothing else.
        let _ = move_file(&stored, file);
        return Err(TrashError::Io {
            context: "trash store verification".into(),
            reason: "fingerprint mismatch after move to trash".into(),
        });
    }

    let entry = TrashEntry {
        trash_id: trash_id.clone(),
        original_path: file.to_path_buf(),
        workspace_rel_path: workspace_rel,
        doc_id: workspace_entry.as_ref().map(|e| e.id.clone()).or_else(|| doc_id.map(|s| s.to_string())),
        doc_type,
        workspace_entry,
        stored_path: stored,
        fingerprint,
        deleted_at_epoch_secs: now_epoch_secs(),
    };

    // Durable manifest FIRST, then unregister from the workspace.
    let mut manifest = load_manifest(workspace_root)?;
    manifest.entries.push(entry.clone());
    save_manifest(workspace_root, &manifest)?;

    if let (Some(ws), Some(id)) = (ws, entry.doc_id.as_deref()) {
        if ws.find_document(id).is_some() {
            ws.unregister_document(id)?;
            ws.save()?;
        }
    }
    Ok(entry)
}

/// Renames (same volume) or copy+verify+delete (cross volume) a file.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(from, to)?;
            // Verify the copy before removing the original.
            let a = fs::read(from)?;
            let b = fs::read(to)?;
            if a != b {
                let _ = fs::remove_file(to);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "copy verification failed",
                ));
            }
            fs::remove_file(from)
        }
    }
}

fn same_file_best_effort(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => {
            // Case-insensitive comparison (Windows).
            a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
        }
    }
}

/// Lists all trashed items (most recent first).
pub fn list_trash(workspace_root: &Path) -> TrashResult<Vec<TrashEntry>> {
    let mut manifest = load_manifest(workspace_root)?;
    manifest
        .entries
        .sort_by(|a, b| b.deleted_at_epoch_secs.cmp(&a.deleted_at_epoch_secs));
    Ok(manifest.entries)
}

/// Restores a trashed item to its original location and (when it was a
/// registered document) re-registers it with its ORIGINAL stable DocumentId.
///
/// Collision policy: if the original path is occupied, the file is restored
/// as `<stem> (restored-N)<ext>` and the workspace entry path is updated
/// accordingly. The stored copy is only removed after the restore is
/// verified byte-identical.
pub fn restore(
    workspace_root: &Path,
    trash_id: &str,
    ws: Option<&mut Workspace>,
) -> TrashResult<PathBuf> {
    let mut manifest = load_manifest(workspace_root)?;
    let idx = manifest
        .entries
        .iter()
        .position(|e| e.trash_id == trash_id)
        .ok_or_else(|| TrashError::EntryNotFound(trash_id.to_string()))?;
    let entry = manifest.entries[idx].clone();

    if !entry.stored_path.is_file() {
        return Err(TrashError::StoredFileMissing {
            trash_id: trash_id.to_string(),
            path: entry.stored_path.clone(),
        });
    }
    // Integrity check before restore.
    let fp = FileFingerprint::from_file(&entry.stored_path)
        .map_err(|e| io_err("fingerprint stored file", e))?
        .hash;
    if fp != entry.fingerprint {
        return Err(TrashError::Io {
            context: "restore integrity check".into(),
            reason: "stored trash file fingerprint mismatch".into(),
        });
    }

    let mut dest = entry.original_path.clone();
    if dest.exists() {
        dest = collision_free_restore_path(&entry.original_path);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| io_err("recreate original dir", e))?;
    }
    fs::copy(&entry.stored_path, &dest).map_err(|e| io_err("restore copy", e))?;
    let restored_fp = FileFingerprint::from_file(&dest)
        .map_err(|e| io_err("fingerprint restored file", e))?
        .hash;
    if restored_fp != entry.fingerprint {
        let _ = fs::remove_file(&dest);
        return Err(TrashError::Io {
            context: "restore verification".into(),
            reason: "restored file fingerprint mismatch".into(),
        });
    }
    // Verified: remove the stored copy and the manifest entry.
    let _ = fs::remove_file(&entry.stored_path);
    manifest.entries.remove(idx);
    save_manifest(workspace_root, &manifest)?;

    // Re-register with the ORIGINAL DocumentId when applicable.
    if let (Some(ws), Some(orig)) = (ws, entry.workspace_entry.clone()) {
        let mut re_entry = orig;
        if re_entry.doc_type == DocumentType::Managed {
            if let Ok(rel) = dest.strip_prefix(workspace_root) {
                re_entry.file_path = rel.to_string_lossy().replace('/', "\\");
            }
        } else {
            re_entry.file_path = dest.to_string_lossy().into_owned();
        }
        re_entry.fingerprint = Some(restored_fp);
        if ws.find_document(&re_entry.id).is_none() {
            // Push + reload so Workspace's private id index is rebuilt
            // through public API only (no edits to workspace.rs allowed).
            ws.metadata.documents.push(re_entry);
            ws.save()?;
            let reloaded = Workspace::open(workspace_root)?;
            *ws = reloaded;
        }
    }
    Ok(dest)
}

fn collision_free_restore_path(original: &Path) -> PathBuf {
    let stem = original
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".into());
    let ext = original
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let dir = original.parent().unwrap_or(Path::new("."));
    for n in 1..1000 {
        let candidate = dir.join(format!("{} (restored-{}){}", stem, n, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!(
        "{} (restored-{}){}",
        stem,
        uuid::Uuid::new_v4().simple(),
        ext
    ))
}

/// Permanently deletes one trashed item (explicit user action).
pub fn purge(workspace_root: &Path, trash_id: &str) -> TrashResult<()> {
    let mut manifest = load_manifest(workspace_root)?;
    let idx = manifest
        .entries
        .iter()
        .position(|e| e.trash_id == trash_id)
        .ok_or_else(|| TrashError::EntryNotFound(trash_id.to_string()))?;
    let entry = manifest.entries.remove(idx);
    if entry.stored_path.exists() {
        fs::remove_file(&entry.stored_path)
            .map_err(|e| io_err("purge stored file", e))?;
    }
    save_manifest(workspace_root, &manifest)
}

/// Permanently deletes every trashed item.
pub fn empty_trash(workspace_root: &Path) -> TrashResult<usize> {
    let manifest = load_manifest(workspace_root)?;
    let count = manifest.entries.len();
    for entry in &manifest.entries {
        if entry.stored_path.exists() {
            fs::remove_file(&entry.stored_path)
                .map_err(|e| io_err("purge stored file", e))?;
        }
    }
    save_manifest(
        workspace_root,
        &TrashManifest { version: 1, entries: Vec::new() },
    )?;
    Ok(count)
}

/// Deletes a registered workspace document to the trash (the standard
/// document-deletion entry point). The file is never permanently removed.
pub fn delete_document_to_trash(
    ws: &mut Workspace,
    doc_id: &str,
) -> TrashResult<TrashEntry> {
    let entry = ws
        .find_document(doc_id)
        .ok_or_else(|| TrashError::EntryNotFound(doc_id.to_string()))?
        .clone();
    let abs = ws.resolve_document_path(&entry);
    let root = ws.root_path.clone();
    move_file_to_trash_ws(&root, &abs, Some(doc_id), Some(ws))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_ws(name: &str) -> (PathBuf, Workspace) {
        let dir = std::env::temp_dir().join(format!(
            "mtdl_trash_{}_{}_{}",
            name,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let ws = Workspace::create(&dir, "TrashWS".into()).unwrap();
        (dir, ws)
    }

    fn doc_xml(name: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<TODOLIST PROJECTNAME=\"{}\" NEXTUNIQUEID=\"1\"></TODOLIST>\r\n",
            name
        )
    }

    #[test]
    fn trash_delete_restore_roundtrip_keeps_document_id() {
        let (dir, mut ws) = tmp_ws("roundtrip");
        fs::write(dir.join("a.xml"), doc_xml("A")).unwrap();
        let doc_id = ws.register_document("a.xml".into(), DocumentType::Managed).unwrap();

        let entry = delete_document_to_trash(&mut ws, &doc_id).unwrap();
        assert!(!dir.join("a.xml").exists());
        assert!(entry.stored_path.is_file());
        assert!(ws.find_document(&doc_id).is_none());
        assert_eq!(list_trash(&dir).unwrap().len(), 1);

        let restored = restore(&dir, &entry.trash_id, Some(&mut ws)).unwrap();
        assert_eq!(restored, dir.join("a.xml"));
        assert!(restored.is_file());
        // Stable DocumentId preserved across trash round-trip.
        assert!(ws.find_document(&doc_id).is_some());
        assert_eq!(list_trash(&dir).unwrap().len(), 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn restore_collision_uses_suffix() {
        let (dir, mut ws) = tmp_ws("collision");
        fs::write(dir.join("a.xml"), doc_xml("A")).unwrap();
        let doc_id = ws.register_document("a.xml".into(), DocumentType::Managed).unwrap();
        let entry = delete_document_to_trash(&mut ws, &doc_id).unwrap();
        // Occupy the original path.
        fs::write(dir.join("a.xml"), doc_xml("Occupant")).unwrap();

        let restored = restore(&dir, &entry.trash_id, Some(&mut ws)).unwrap();
        assert_ne!(restored, dir.join("a.xml"));
        assert!(restored.file_name().unwrap().to_string_lossy().contains("restored-1"));
        assert!(restored.is_file());
        // Occupant untouched.
        assert!(fs::read_to_string(dir.join("a.xml")).unwrap().contains("Occupant"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn purge_is_permanent_and_explicit() {
        let (dir, mut ws) = tmp_ws("purge");
        fs::write(dir.join("a.xml"), doc_xml("A")).unwrap();
        let doc_id = ws.register_document("a.xml".into(), DocumentType::Managed).unwrap();
        let entry = delete_document_to_trash(&mut ws, &doc_id).unwrap();
        let stored = entry.stored_path.clone();
        assert!(stored.is_file());

        purge(&dir, &entry.trash_id).unwrap();
        assert!(!stored.exists());
        assert!(list_trash(&dir).unwrap().is_empty());
        assert!(matches!(
            purge(&dir, &entry.trash_id),
            Err(TrashError::EntryNotFound(_))
        ));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_trash_purges_all() {
        let (dir, mut ws) = tmp_ws("empty");
        for n in ["x.xml", "y.xml"] {
            fs::write(dir.join(n), doc_xml(n)).unwrap();
            let id = ws.register_document(n.into(), DocumentType::Managed).unwrap();
            delete_document_to_trash(&mut ws, &id).unwrap();
        }
        assert_eq!(list_trash(&dir).unwrap().len(), 2);
        let n = empty_trash(&dir).unwrap();
        assert_eq!(n, 2);
        assert!(list_trash(&dir).unwrap().is_empty());
        assert!(fs::read_dir(files_dir(&dir)).unwrap().count() == 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_corrupt_falls_back_to_bak() {
        let (dir, mut ws) = tmp_ws("manifest");
        fs::write(dir.join("a.xml"), doc_xml("A")).unwrap();
        let doc_id = ws.register_document("a.xml".into(), DocumentType::Managed).unwrap();
        let _e1 = delete_document_to_trash(&mut ws, &doc_id).unwrap();
        fs::write(dir.join("b.xml"), doc_xml("B")).unwrap();
        let id_b = ws.register_document("b.xml".into(), DocumentType::Managed).unwrap();
        let _e2 = delete_document_to_trash(&mut ws, &id_b).unwrap();
        // .bak now holds the 1-entry version; corrupt the primary.
        fs::write(manifest_path(&dir), b"{{{ broken").unwrap();
        let loaded = load_manifest(&dir).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unregistered_file_can_be_trashed_and_restored() {
        let (dir, _ws) = tmp_ws("unreg");
        let f = dir.join("loose.xml");
        fs::write(&f, doc_xml("Loose")).unwrap();
        let entry = move_file_to_trash(&dir, &f, None).unwrap();
        assert!(!f.exists());
        assert!(entry.doc_id.is_none());
        let restored = restore(&dir, &entry.trash_id, None).unwrap();
        assert_eq!(restored, f);
        assert!(f.is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_rejected() {
        let (dir, _ws) = tmp_ws("missing");
        assert!(matches!(
            move_file_to_trash(&dir, &dir.join("nope.xml"), None),
            Err(TrashError::FileNotFound(_))
        ));
        fs::remove_dir_all(&dir).ok();
    }
}
