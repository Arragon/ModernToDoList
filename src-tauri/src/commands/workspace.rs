//! M4: Workspace IPC commands
//!
//! Tauri command handlers for workspace management, document indexing,
//! and file watcher integration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

use crate::domain::workspace::{Workspace, DocumentType};
use crate::infrastructure::{DatabaseManager, WorkspaceWatcher, WatcherConfig, FingerprintChecker};
use crate::infrastructure::indexer;

/// Application state for M4 workspace management.
pub struct WorkspaceState {
    pub db: Mutex<DatabaseManager>,
    pub workspace: Mutex<Option<Workspace>>,
    pub watcher: Mutex<WorkspaceWatcher>,
    pub fingerprint_checker: Mutex<FingerprintChecker>,
}

impl WorkspaceState {
    pub fn new(data_dir: &std::path::Path) -> Self {
        Self {
            db: Mutex::new(DatabaseManager::open(data_dir)),
            workspace: Mutex::new(None),
            watcher: Mutex::new(WorkspaceWatcher::new(WatcherConfig::default())),
            fingerprint_checker: Mutex::new(FingerprintChecker::new()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub document_count: usize,
    pub db_available: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentInfo {
    pub id: String,
    pub file_path: String,
    pub doc_type: String,
    pub fingerprint: Option<String>,
    pub last_indexed: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexProgressInfo {
    pub total_files: usize,
    pub processed_files: usize,
    pub total_tasks: usize,
    pub current_file: String,
}

/// Create a new workspace at the given path.
#[tauri::command]
pub fn create_workspace(
    state: State<'_, WorkspaceState>,
    path: String,
    name: String,
) -> Result<WorkspaceInfo, String> {
    let root = PathBuf::from(&path);
    let ws = Workspace::create(&root, name)
        .map_err(|e| e.to_string())?;

    // Register in database
    if let Ok(db) = state.db.lock() {
        if db.is_available() {
            let _ = db.with_connection(|conn| {
                crate::domain::workspace::ensure_workspace_in_db(conn, &ws)
                    .map_err(|e| crate::infrastructure::DatabaseError::Sqlite(
                        rusqlite::Error::ToSqlConversionFailure(Box::new(e))
                    ))?;
                Ok(())
            });
        }
    }

    // Start watching the workspace directory
    if let Ok(mut watcher) = state.watcher.lock() {
        let _ = watcher.watch(&root);
    }

    let info = WorkspaceInfo {
        id: ws.id().to_string(),
        name: ws.name().to_string(),
        root_path: ws.root_path.to_string_lossy().to_string(),
        document_count: ws.documents().len(),
        db_available: state.db.lock().map(|db| db.is_available()).unwrap_or(false),
    };

    *state.workspace.lock().map_err(|_| "Lock error")? = Some(ws);
    Ok(info)
}

/// Open an existing workspace.
#[tauri::command]
pub fn open_workspace(
    state: State<'_, WorkspaceState>,
    path: String,
) -> Result<WorkspaceInfo, String> {
    let root = PathBuf::from(&path);
    let ws = Workspace::open(&root)
        .map_err(|e| e.to_string())?;

    // Start watching
    if let Ok(mut watcher) = state.watcher.lock() {
        let _ = watcher.watch(&root);
    }

    let info = WorkspaceInfo {
        id: ws.id().to_string(),
        name: ws.name().to_string(),
        root_path: ws.root_path.to_string_lossy().to_string(),
        document_count: ws.documents().len(),
        db_available: state.db.lock().map(|db| db.is_available()).unwrap_or(false),
    };

    *state.workspace.lock().map_err(|_| "Lock error")? = Some(ws);
    Ok(info)
}

/// Close the current workspace.
#[tauri::command]
pub fn close_workspace(
    state: State<'_, WorkspaceState>,
) -> Result<(), String> {
    // Stop watcher
    if let Ok(mut watcher) = state.watcher.lock() {
        watcher.stop();
    }

    // Clear workspace
    *state.workspace.lock().map_err(|_| "Lock error")? = None;
    Ok(())
}

/// Get information about the current workspace.
#[tauri::command]
pub fn get_workspace_status(
    state: State<'_, WorkspaceState>,
) -> Result<Option<WorkspaceInfo>, String> {
    let ws_guard = state.workspace.lock().map_err(|_| "Lock error")?;
    match ws_guard.as_ref() {
        Some(ws) => Ok(Some(WorkspaceInfo {
            id: ws.id().to_string(),
            name: ws.name().to_string(),
            root_path: ws.root_path.to_string_lossy().to_string(),
            document_count: ws.documents().len(),
            db_available: state.db.lock().map(|db| db.is_available()).unwrap_or(false),
        })),
        None => Ok(None),
    }
}

/// List all documents in the current workspace.
#[tauri::command]
pub fn list_documents(
    state: State<'_, WorkspaceState>,
) -> Result<Vec<DocumentInfo>, String> {
    let ws_guard = state.workspace.lock().map_err(|_| "Lock error")?;
    match ws_guard.as_ref() {
        Some(ws) => {
            let docs = ws.documents().iter().map(|d| DocumentInfo {
                id: d.id.clone(),
                file_path: d.file_path.clone(),
                doc_type: d.doc_type.as_str().to_string(),
                fingerprint: d.fingerprint.clone(),
                last_indexed: d.last_indexed.clone(),
            }).collect();
            Ok(docs)
        }
        None => Ok(Vec::new()),
    }
}

/// Scan the workspace for new documents and index them.
#[tauri::command]
pub fn scan_and_index(
    state: State<'_, WorkspaceState>,
) -> Result<IndexProgressInfo, String> {
    let mut ws_guard = state.workspace.lock().map_err(|_| "Lock error")?;
    let ws = ws_guard.as_mut().ok_or("No workspace open")?;

    // Scan for documents
    let found = ws.scan_documents().map_err(|e| e.to_string())?;
    let mut new_count = 0;

    for rel_path in &found {
        let path_str = rel_path.to_string_lossy().to_string();
        // Register if not already registered
        if ws.find_document(&path_str).is_none() {
            let _ = ws.register_document(path_str, DocumentType::Managed);
            new_count += 1;
        }
    }

    // Collect document info first to avoid borrow checker issues
    let doc_infos: Vec<(String, PathBuf)> = ws.documents().iter().map(|doc| {
        (doc.id.clone(), ws.resolve_document_path(doc))
    }).collect();

    // Index all documents
    let db = state.db.lock().map_err(|_| "Lock error")?;
    let mut total_tasks = 0;
    let mut fingerprint_updates: Vec<(String, String)> = Vec::new();

    if db.is_available() {
        let _ = db.with_connection(|conn| {
            // Sync workspace to DB
            let _ = crate::domain::workspace::sync_documents_to_db(conn, ws);

            // Index each document
            for (doc_id, abs_path) in &doc_infos {
                if abs_path.exists() {
                    match indexer::index_document(conn, doc_id, abs_path) {
                        Ok(count) => {
                            total_tasks += count;
                            // Collect fingerprint for later update
                            if let Ok(fp) = indexer::compute_fingerprint(abs_path) {
                                fingerprint_updates.push((doc_id.clone(), fp));
                            }
                        }
                        Err(e) => log::warn!("Index failed for {}: {}", abs_path.display(), e),
                    }
                }
            }
            Ok(())
        });
    }

    // Apply fingerprint updates after iteration is complete
    for (doc_id, fp) in &fingerprint_updates {
        let _ = ws.update_fingerprint(doc_id, fp);
    }

    let doc_count = ws.documents().len();

    // Save workspace metadata
    let _ = ws.save();

    Ok(IndexProgressInfo {
        total_files: doc_count,
        processed_files: doc_count,
        total_tasks,
        current_file: String::new(),
    })
}

/// Rebuild the entire index from scratch.
#[tauri::command]
pub fn rebuild_index(
    state: State<'_, WorkspaceState>,
) -> Result<IndexProgressInfo, String> {
    let mut ws_guard = state.workspace.lock().map_err(|_| "Lock error")?;
    let ws = ws_guard.as_mut().ok_or("No workspace open")?;

    let db = state.db.lock().map_err(|_| "Lock error")?;

    if !db.is_available() {
        return Ok(IndexProgressInfo {
            total_files: 0,
            processed_files: 0,
            total_tasks: 0,
            current_file: "DB unavailable".to_string(),
        });
    }

    // Collect document info to avoid borrow issues
    let doc_infos: Vec<(String, PathBuf)> = ws.documents().iter().map(|doc| {
        (doc.id.clone(), ws.resolve_document_path(doc))
    }).collect();
    let doc_count = ws.documents().len();
    let ws_id = ws.id().to_string();

    let result = db.with_connection(|conn| {
        // Clear all index data
        let _ = indexer::clear_workspace_index(conn, &ws_id);

        // Re-ensure workspace
        let _ = crate::domain::workspace::ensure_workspace_in_db(conn, ws);
        let _ = crate::domain::workspace::sync_documents_to_db(conn, ws);

        // Re-index all documents
        let mut total_tasks = 0;
        for (doc_id, abs_path) in &doc_infos {
            if abs_path.exists() {
                match indexer::index_document(conn, doc_id, abs_path) {
                    Ok(count) => total_tasks += count,
                    Err(e) => log::warn!("Reindex failed for {}: {}", abs_path.display(), e),
                }
            }
        }
        Ok(IndexProgressInfo {
            total_files: doc_count,
            processed_files: doc_count,
            total_tasks,
            current_file: String::new(),
        })
    });

    Ok(result.unwrap_or(IndexProgressInfo {
        total_files: 0,
        processed_files: 0,
        total_tasks: 0,
        current_file: String::new(),
    }))
}

/// Get database status information.
#[tauri::command]
pub fn get_db_status(
    state: State<'_, WorkspaceState>,
) -> Result<serde_json::Value, String> {
    let db = state.db.lock().map_err(|_| "Lock error")?;
    Ok(serde_json::json!({
        "available": db.is_available(),
        "mode": format!("{:?}", db.mode()),
        "path": db.db_path().to_string_lossy(),
        "schema_version": db.schema_version().unwrap_or(0),
    }))
}
