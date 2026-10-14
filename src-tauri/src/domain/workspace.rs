//! Workspace domain model for ModernToDoList 2.0
//!
//! A Workspace is the top-level organizational unit. It contains:
//! - A root directory on disk
//! - A `workspace.json` metadata file
//! - Managed Documents (files within the workspace directory)
//! - Linked Documents (external files referenced by the workspace)

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::domain::fingerprint::FileFingerprint;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("Workspace directory does not exist: {0}")]
    DirectoryNotFound(PathBuf),

    #[error("workspace.json not found: {0}")]
    MetadataNotFound(PathBuf),

    #[error("Failed to read workspace.json: {0}")]
    MetadataReadError(String),

    #[error("Failed to write workspace.json: {0}")]
    MetadataWriteError(String),

    #[error("Invalid workspace.json: {0}")]
    InvalidMetadata(String),

    #[error("Document not found: {0}")]
    DocumentNotFound(String),

    #[error("Document already registered: {0}")]
    DocumentAlreadyRegistered(String),

    #[error("Scan error: {0}")]
    ScanError(String),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Index error: {0}")]
    IndexError(String),
}

pub type WorkspaceResult<T> = Result<T, WorkspaceError>;

/// The type of a document registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentType {
    /// File is within the workspace directory tree.
    Managed,
    /// File is outside the workspace, referenced by path.
    Linked,
}

impl DocumentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DocumentType::Managed => "managed",
            DocumentType::Linked => "linked",
        }
    }
}

/// Supported file extensions for task documents.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["xml", "tdl"];

/// Check if a file extension is a supported task document type.
pub fn is_supported_extension(ext: &str) -> bool {
    SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Metadata for a registered document within a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentEntry {
    /// Stable unique identifier for this document.
    pub id: String,
    /// Path relative to workspace root (for managed) or absolute (for linked).
    pub file_path: String,
    /// Whether this is a managed or linked document.
    pub doc_type: DocumentType,
    /// BLAKE3 fingerprint of the file content (hex string).
    pub fingerprint: Option<String>,
    /// Last time this document was indexed (ISO 8601).
    pub last_indexed: Option<String>,
}

/// The workspace metadata, persisted as `workspace.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMetadata {
    /// Workspace version for future migration.
    pub version: u32,
    /// Human-readable workspace name.
    pub name: String,
    /// Unique workspace ID.
    pub id: String,
    /// Creation timestamp (ISO 8601).
    pub created_at: String,
    /// Last modification timestamp (ISO 8601).
    pub updated_at: String,
    /// Registered documents.
    pub documents: Vec<DocumentEntry>,
}

impl WorkspaceMetadata {
    /// Create a new workspace metadata with defaults.
    pub fn new(name: String) -> Self {
        let now = chrono_now();
        Self {
            version: 1,
            name,
            id: uuid::Uuid::new_v4().to_string(),
            created_at: now.clone(),
            updated_at: now,
            documents: Vec::new(),
        }
    }
}

/// A Workspace instance with its root path and metadata.
#[derive(Debug)]
pub struct Workspace {
    /// Absolute path to the workspace root directory.
    pub root_path: PathBuf,
    /// Workspace metadata (loaded from workspace.json).
    pub metadata: WorkspaceMetadata,
    /// In-memory document lookup by ID.
    documents_by_id: HashMap<String, usize>,
}

impl Workspace {
    /// Create a new workspace at the given directory.
    ///
    /// Creates the directory if it doesn't exist, and writes `workspace.json`.
    pub fn create(root_path: &Path, name: String) -> WorkspaceResult<Self> {
        if !root_path.exists() {
            std::fs::create_dir_all(root_path)
                .map_err(|e| WorkspaceError::DirectoryNotFound(root_path.join(e.to_string())))?;
        }

        let metadata = WorkspaceMetadata::new(name);
        Self::save_metadata(root_path, &metadata)?;

        let mut ws = Self {
            root_path: root_path.to_path_buf(),
            metadata,
            documents_by_id: HashMap::new(),
        };
        ws.rebuild_document_index();
        Ok(ws)
    }

    /// Open an existing workspace from the given directory.
    pub fn open(root_path: &Path) -> WorkspaceResult<Self> {
        if !root_path.exists() {
            return Err(WorkspaceError::DirectoryNotFound(root_path.to_path_buf()));
        }

        let meta_path = root_path.join("workspace.json");
        if !meta_path.exists() {
            return Err(WorkspaceError::MetadataNotFound(meta_path));
        }

        let content = std::fs::read_to_string(&meta_path)
            .map_err(|e| WorkspaceError::MetadataReadError(e.to_string()))?;
        let metadata: WorkspaceMetadata = serde_json::from_str(&content)
            .map_err(|e| WorkspaceError::InvalidMetadata(e.to_string()))?;

        let mut ws = Self {
            root_path: root_path.to_path_buf(),
            metadata,
            documents_by_id: HashMap::new(),
        };
        ws.rebuild_document_index();
        Ok(ws)
    }

    /// Save the workspace metadata to disk.
    pub fn save(&self) -> WorkspaceResult<()> {
        Self::save_metadata(&self.root_path, &self.metadata)
    }

    fn save_metadata(root_path: &Path, metadata: &WorkspaceMetadata) -> WorkspaceResult<()> {
        let meta_path = root_path.join("workspace.json");
        let content = serde_json::to_string_pretty(metadata)
            .map_err(|e| WorkspaceError::MetadataWriteError(e.to_string()))?;
        std::fs::write(&meta_path, content)
            .map_err(|e| WorkspaceError::MetadataWriteError(e.to_string()))?;
        Ok(())
    }

    /// Rebuild the in-memory document lookup index.
    fn rebuild_document_index(&mut self) {
        self.documents_by_id.clear();
        for (i, doc) in self.metadata.documents.iter().enumerate() {
            self.documents_by_id.insert(doc.id.clone(), i);
        }
    }

    /// Get the workspace ID.
    pub fn id(&self) -> &str {
        &self.metadata.id
    }

    /// Get the workspace name.
    pub fn name(&self) -> &str {
        &self.metadata.name
    }

    /// Get all registered documents.
    pub fn documents(&self) -> &[DocumentEntry] {
        &self.metadata.documents
    }

    /// Find a document by its ID.
    pub fn find_document(&self, id: &str) -> Option<&DocumentEntry> {
        self.documents_by_id.get(id).map(|&i| &self.metadata.documents[i])
    }

    /// Resolve a document's file path to an absolute path.
    pub fn resolve_document_path(&self, doc: &DocumentEntry) -> PathBuf {
        match doc.doc_type {
            DocumentType::Managed => self.root_path.join(&doc.file_path),
            DocumentType::Linked => PathBuf::from(&doc.file_path),
        }
    }

    /// Register a new document in the workspace.
    pub fn register_document(&mut self, file_path: String, doc_type: DocumentType) -> WorkspaceResult<String> {
        // Check for duplicate
        if self.metadata.documents.iter().any(|d| d.file_path == file_path) {
            return Err(WorkspaceError::DocumentAlreadyRegistered(file_path));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let entry = DocumentEntry {
            id: id.clone(),
            file_path,
            doc_type,
            fingerprint: None,
            last_indexed: None,
        };

        self.metadata.documents.push(entry);
        self.documents_by_id.insert(id.clone(), self.metadata.documents.len() - 1);
        self.metadata.updated_at = chrono_now();
        Ok(id)
    }

    /// Remove a document from the workspace.
    pub fn unregister_document(&mut self, id: &str) -> WorkspaceResult<()> {
        if let Some(&idx) = self.documents_by_id.get(id) {
            self.metadata.documents.remove(idx);
            self.rebuild_document_index();
            self.metadata.updated_at = chrono_now();
            Ok(())
        } else {
            Err(WorkspaceError::DocumentNotFound(id.to_string()))
        }
    }

    /// Scan the workspace directory for task documents.
    ///
    /// Returns a list of discovered file paths (relative to workspace root).
    pub fn scan_documents(&self) -> WorkspaceResult<Vec<PathBuf>> {
        let mut found = Vec::new();
        self.scan_directory(&self.root_path, &mut found)?;
        Ok(found)
    }

    fn scan_directory(&self, dir: &Path, found: &mut Vec<PathBuf>) -> WorkspaceResult<()> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| WorkspaceError::ScanError(format!("{}: {}", dir.display(), e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| WorkspaceError::ScanError(e.to_string()))?;
            let path = entry.path();

            if path.is_dir() {
                // Skip hidden directories and common non-document dirs
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" {
                    continue;
                }
                self.scan_directory(&path, found)?;
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    if is_supported_extension(&ext.to_string_lossy()) {
                        // Store as relative path if within workspace
                        if let Ok(rel) = path.strip_prefix(&self.root_path) {
                            found.push(rel.to_path_buf());
                        } else {
                            found.push(path.clone());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Detect changes between the workspace state and the filesystem.
    ///
    /// Returns (new_files, changed_files, removed_doc_ids).
    pub fn detect_changes(&self) -> WorkspaceResult<(Vec<PathBuf>, Vec<PathBuf>, Vec<String>)> {
        let scanned = self.scan_documents()?;
        let scanned_set: std::collections::HashSet<String> = scanned
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        let registered_set: std::collections::HashSet<String> = self.metadata.documents
            .iter()
            .filter(|d| d.doc_type == DocumentType::Managed)
            .map(|d| d.file_path.clone())
            .collect();

        // New files: in scanned but not registered
        let new_files: Vec<PathBuf> = scanned
            .iter()
            .filter(|p| !registered_set.contains(&p.to_string_lossy().to_string()))
            .cloned()
            .collect();

        // Removed: registered but not in scanned
        let removed: Vec<String> = self.metadata.documents
            .iter()
            .filter(|d| d.doc_type == DocumentType::Managed && !scanned_set.contains(&d.file_path))
            .map(|d| d.id.clone())
            .collect();

        // Changed: fingerprint mismatch
        let mut changed_files = Vec::new();
        for doc in &self.metadata.documents {
            if doc.doc_type != DocumentType::Managed {
                continue;
            }
            let abs_path = self.resolve_document_path(doc);
            if !abs_path.exists() {
                continue;
            }
            if let Ok(current_fp) = FileFingerprint::from_file(&abs_path) {
                let current_hex = current_fp.hash.clone();
                if let Some(ref stored_fp) = doc.fingerprint {
                    if *stored_fp != current_hex {
                        changed_files.push(PathBuf::from(&doc.file_path));
                    }
                }
            }
        }

        Ok((new_files, changed_files, removed))
    }

    /// Update the fingerprint for a document.
    pub fn update_fingerprint(&mut self, doc_id: &str, fingerprint: &str) -> WorkspaceResult<()> {
        if let Some(&idx) = self.documents_by_id.get(doc_id) {
            self.metadata.documents[idx].fingerprint = Some(fingerprint.to_string());
            self.metadata.documents[idx].last_indexed = Some(chrono_now());
            self.metadata.updated_at = chrono_now();
            Ok(())
        } else {
            Err(WorkspaceError::DocumentNotFound(doc_id.to_string()))
        }
    }
}

/// Recent workspace entry for the registry in settings.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentWorkspace {
    pub path: String,
    pub name: String,
    pub last_opened: String,
}

/// Get a simple ISO 8601 timestamp without pulling in chrono.
fn chrono_now() -> String {
    // Use a simple approach: we just need a timestamp string.
    // In production, this would use chrono or time crate.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}s", d.as_secs()))
        .unwrap_or_else(|_| "0s".to_string())
}

/// Sync workspace documents to the SQLite index database.
pub fn sync_documents_to_db(conn: &Connection, workspace: &Workspace) -> WorkspaceResult<()> {
    // Clear existing documents for this workspace
    conn.execute(
        "DELETE FROM documents WHERE workspace_id = ?1",
        [workspace.id()],
    )?;

    // Insert all documents
    for doc in workspace.documents() {
        conn.execute(
            "INSERT OR REPLACE INTO documents (id, workspace_id, file_path, doc_type, fingerprint, last_indexed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                doc.id,
                workspace.id(),
                doc.file_path,
                doc.doc_type.as_str(),
                doc.fingerprint,
                doc.last_indexed,
            ],
        )?;
    }

    Ok(())
}

/// Ensure the workspace record exists in the database.
pub fn ensure_workspace_in_db(conn: &Connection, workspace: &Workspace) -> WorkspaceResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO workspaces (id, name, root_path, updated_at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            workspace.id(),
            workspace.name(),
            workspace.root_path.to_string_lossy(),
            workspace.metadata.updated_at,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_workspace_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mtdl_ws_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_workspace_writes_metadata() {
        let dir = temp_workspace_dir();
        let ws = Workspace::create(&dir, "Test WS".to_string()).unwrap();
        assert!(dir.join("workspace.json").exists());
        assert_eq!(ws.name(), "Test WS");
        assert!(!ws.id().is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_workspace_reads_metadata() {
        let dir = temp_workspace_dir();
        let ws = Workspace::create(&dir, "Open Test".to_string()).unwrap();
        let ws_id = ws.id().to_string();
        drop(ws);

        let ws2 = Workspace::open(&dir).unwrap();
        assert_eq!(ws2.id(), ws_id);
        assert_eq!(ws2.name(), "Open Test");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_nonexistent_directory_fails() {
        let result = Workspace::open(Path::new("/nonexistent/ws/path"));
        assert!(result.is_err());
    }

    #[test]
    fn open_without_metadata_fails() {
        let dir = temp_workspace_dir();
        let result = Workspace::open(&dir);
        assert!(result.is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn register_and_find_document() {
        let dir = temp_workspace_dir();
        let mut ws = Workspace::create(&dir, "Doc Test".to_string()).unwrap();
        let doc_id = ws.register_document("test.xml".to_string(), DocumentType::Managed).unwrap();
        let doc = ws.find_document(&doc_id).unwrap();
        assert_eq!(doc.file_path, "test.xml");
        assert_eq!(doc.doc_type, DocumentType::Managed);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_registration_fails() {
        let dir = temp_workspace_dir();
        let mut ws = Workspace::create(&dir, "Dup Test".to_string()).unwrap();
        ws.register_document("test.xml".to_string(), DocumentType::Managed).unwrap();
        let result = ws.register_document("test.xml".to_string(), DocumentType::Managed);
        assert!(result.is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unregister_document() {
        let dir = temp_workspace_dir();
        let mut ws = Workspace::create(&dir, "Unreg Test".to_string()).unwrap();
        let doc_id = ws.register_document("test.xml".to_string(), DocumentType::Managed).unwrap();
        assert!(ws.find_document(&doc_id).is_some());
        ws.unregister_document(&doc_id).unwrap();
        assert!(ws.find_document(&doc_id).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_finds_xml_files() {
        let dir = temp_workspace_dir();
        let ws = Workspace::create(&dir, "Scan Test".to_string()).unwrap();

        // Create some test files
        fs::write(dir.join("task1.xml"), "<TDL/>").unwrap();
        fs::write(dir.join("task2.tdl"), "<TDL/>").unwrap();
        fs::write(dir.join("readme.txt"), "not a task").unwrap();
        fs::create_dir(dir.join("subdir")).unwrap();
        fs::write(dir.join("subdir/task3.xml"), "<TDL/>").unwrap();

        let found = ws.scan_documents().unwrap();
        assert_eq!(found.len(), 3, "should find 3 task files");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_skips_hidden_directories() {
        let dir = temp_workspace_dir();
        let ws = Workspace::create(&dir, "Hidden Test".to_string()).unwrap();

        fs::create_dir(dir.join(".hidden")).unwrap();
        fs::write(dir.join(".hidden/secret.xml"), "<TDL/>").unwrap();
        fs::write(dir.join("visible.xml"), "<TDL/>").unwrap();

        let found = ws.scan_documents().unwrap();
        assert_eq!(found.len(), 1, "should skip hidden directory");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detect_changes_finds_new_files() {
        let dir = temp_workspace_dir();
        let mut ws = Workspace::create(&dir, "Change Test".to_string()).unwrap();

        // Register one file
        fs::write(dir.join("existing.xml"), "<TDL/>").unwrap();
        ws.register_document("existing.xml".to_string(), DocumentType::Managed).unwrap();

        // Add a new file
        fs::write(dir.join("new.xml"), "<TDL/>").unwrap();

        let (new, _changed, _removed) = ws.detect_changes().unwrap();
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].to_string_lossy(), "new.xml");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detect_changes_finds_removed_files() {
        let dir = temp_workspace_dir();
        let mut ws = Workspace::create(&dir, "Remove Test".to_string()).unwrap();

        // Register a file that doesn't exist
        ws.register_document("gone.xml".to_string(), DocumentType::Managed).unwrap();

        let (_new, _changed, removed) = ws.detect_changes().unwrap();
        assert_eq!(removed.len(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn supported_extensions() {
        assert!(is_supported_extension("xml"));
        assert!(is_supported_extension("tdl"));
        assert!(is_supported_extension("XML"));
        assert!(!is_supported_extension("txt"));
        assert!(!is_supported_extension("json"));
    }

    #[test]
    fn document_type_serialization() {
        assert_eq!(DocumentType::Managed.as_str(), "managed");
        assert_eq!(DocumentType::Linked.as_str(), "linked");
    }

    #[test]
    fn resolve_managed_path() {
        let dir = temp_workspace_dir();
        let ws = Workspace::create(&dir, "Path Test".to_string()).unwrap();
        let doc = DocumentEntry {
            id: "test".to_string(),
            file_path: "subdir/task.xml".to_string(),
            doc_type: DocumentType::Managed,
            fingerprint: None,
            last_indexed: None,
        };
        let resolved = ws.resolve_document_path(&doc);
        assert!(resolved.starts_with(&dir));
        assert!(resolved.ends_with("subdir/task.xml"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_linked_path_absolute() {
        let dir = temp_workspace_dir();
        let ws = Workspace::create(&dir, "Linked Test".to_string()).unwrap();
        let doc = DocumentEntry {
            id: "test".to_string(),
            file_path: "C:\\external\\file.xml".to_string(),
            doc_type: DocumentType::Linked,
            fingerprint: None,
            last_indexed: None,
        };
        let resolved = ws.resolve_document_path(&doc);
        assert_eq!(resolved, PathBuf::from("C:\\external\\file.xml"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sync_to_db_and_ensure_in_db() {
        let dir = temp_workspace_dir();
        let mut ws = Workspace::create(&dir, "DB Sync Test".to_string()).unwrap();
        ws.register_document("task.xml".to_string(), DocumentType::Managed).unwrap();

        // Create in-memory DB with schema
        let conn = Connection::open_in_memory().unwrap();
        crate::infrastructure::migration::run_migrations(&conn).unwrap();

        // Ensure workspace in DB
        ensure_workspace_in_db(&conn, &ws).unwrap();

        // Sync documents
        sync_documents_to_db(&conn, &ws).unwrap();

        // Verify
        let ws_name: String = conn.query_row(
            "SELECT name FROM workspaces WHERE id = ?1",
            [ws.id()],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(ws_name, "DB Sync Test");

        let doc_count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE workspace_id = ?1",
            [ws.id()],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(doc_count, 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn workspace_metadata_roundtrip() {
        let dir = temp_workspace_dir();
        let mut ws = Workspace::create(&dir, "Roundtrip".to_string()).unwrap();
        ws.register_document("a.xml".to_string(), DocumentType::Managed).unwrap();
        ws.register_document("b.xml".to_string(), DocumentType::Linked).unwrap();
        ws.save().unwrap();

        let ws2 = Workspace::open(&dir).unwrap();
        assert_eq!(ws2.metadata.documents.len(), 2);
        assert_eq!(ws2.name(), "Roundtrip");
        fs::remove_dir_all(&dir).ok();
    }
}
