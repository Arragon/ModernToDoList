//! M9: Global search + command palette IPC (RD-M9-008~013, RD-M9-031~034).
//!
//! Thin Tauri wrappers over the domain logic in [`crate::domain::search`]
//! (command registry, fuzzy palette search, jump resolution) and the search
//! engine in [`crate::infrastructure::search_fts`] (FTS5 + CJK LIKE
//! fallback). The registry/palette/jump logic lives in the domain layer so
//! integration tests can reach it (this `commands` module is crate-private).

use serde::Serialize;
use tauri::State;

use crate::commands::workspace::WorkspaceState;
use crate::domain::search::{
    self, CommandDescriptor, CommandHit, SearchPage, SearchQuery, SearchResult, ShortcutConflict,
};
use crate::domain::types::TaskKey;
use crate::infrastructure::search_fts;

// ── IPC commands ─────────────────────────────────────────────────────────────

/// Bridge search errors into the DatabaseResult used by `with_connection`.
fn to_db_error(e: search_fts::SearchError) -> crate::infrastructure::DatabaseError {
    match e {
        search_fts::SearchError::Sqlite(s) => crate::infrastructure::DatabaseError::Sqlite(s),
        other => crate::infrastructure::DatabaseError::Sqlite(
            rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        ),
    }
}

/// Response of the search IPC command.
#[derive(Debug, Clone, Serialize)]
pub struct SearchTasksResponse {
    pub page: SearchPage,
}

/// Global task search (FTS5 with CJK LIKE fallback).
#[tauri::command]
pub fn search_tasks(
    state: State<'_, WorkspaceState>,
    query: String,
    limit: Option<usize>,
    offset: Option<usize>,
    document_id: Option<String>,
) -> Result<SearchTasksResponse, String> {
    let db = state.db.lock().map_err(|_| "Lock error")?;
    if !db.is_available() {
        // Graceful degradation: no index → no results (never an error).
        return Ok(SearchTasksResponse {
            page: SearchPage {
                results: Vec::new(),
                total: 0,
                limit: limit.unwrap_or(crate::domain::search::DEFAULT_PAGE_SIZE),
                offset: offset.unwrap_or(0),
                backend: "unavailable".into(),
            },
        });
    }

    let mut q = SearchQuery::new(query).with_page(
        limit.unwrap_or(crate::domain::search::DEFAULT_PAGE_SIZE),
        offset.unwrap_or(0),
    );
    if let Some(doc) = document_id {
        q = q.with_document(doc);
    }

    db.with_connection(|conn| {
        search_fts::ensure_search_schema(conn);
        let page = search_fts::search(conn, &q).map_err(to_db_error)?;
        Ok(SearchTasksResponse { page })
    })
    .map_err(|e| e.to_string())
}

/// Rebuild the search index from an XML scan of every registered document
/// (proves the search store is disposable).
#[derive(Debug, Clone, Serialize)]
pub struct RebuildSearchResponse {
    pub indexed_tasks: usize,
    pub backend: String,
}

#[tauri::command]
pub fn rebuild_search_index_cmd(
    state: State<'_, WorkspaceState>,
) -> Result<RebuildSearchResponse, String> {
    let db = state.db.lock().map_err(|_| "Lock error")?;
    if !db.is_available() {
        return Err("Index database unavailable".into());
    }
    db.with_connection(|conn| {
        let backend = search_fts::ensure_search_schema(conn);
        let mut stmt = conn.prepare("SELECT id, file_path FROM documents ORDER BY id")?;
        let docs: Vec<(String, std::path::PathBuf)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, std::path::PathBuf::from(row.get::<_, String>(1)?)))
            })?
            .filter_map(|r| r.ok())
            .collect();
        let n = search_fts::rebuild_search_index(conn, &docs, &search_fts::IndexTableExtrasProvider)
            .map_err(to_db_error)?;
        Ok(RebuildSearchResponse {
            indexed_tasks: n,
            backend: backend.as_str().to_string(),
        })
    })
    .map_err(|e| e.to_string())
}

/// Ctrl+K palette query: fuzzy commands + full-text task search.
/// Commands come first (keyboard-first navigation); tasks second.
#[derive(Debug, Clone, Serialize)]
pub struct PaletteResponse {
    pub commands: Vec<CommandHit>,
    pub tasks: Vec<SearchResult>,
    pub total_tasks: usize,
}

#[tauri::command]
pub fn palette_query(
    state: State<'_, WorkspaceState>,
    query: String,
    limit: Option<usize>,
) -> Result<PaletteResponse, String> {
    let task_limit = limit.unwrap_or(8);
    let commands = search::search_commands(&query, task_limit);

    let db = state.db.lock().map_err(|_| "Lock error")?;
    if !db.is_available() || query.trim().is_empty() {
        return Ok(PaletteResponse { commands, tasks: Vec::new(), total_tasks: 0 });
    }

    let q = SearchQuery::new(query).with_page(task_limit, 0);
    let page = db
        .with_connection(|conn| {
            search_fts::ensure_search_schema(conn);
            search_fts::search(conn, &q).map_err(to_db_error)
        })
        .map_err(|e| e.to_string())?;

    Ok(PaletteResponse {
        commands,
        total_tasks: page.total,
        tasks: page.results,
    })
}

/// List the canonical command registry (for the frontend to mirror).
#[tauri::command]
pub fn list_app_commands() -> Result<Vec<CommandDescriptor>, String> {
    Ok(search::command_registry())
}

/// Report duplicate shortcut bindings in the registry (QA-M9-012).
#[tauri::command]
pub fn get_shortcut_conflicts() -> Result<Vec<ShortcutConflict>, String> {
    Ok(search::find_shortcut_conflicts(&search::command_registry()))
}

/// Resolve a task-jump target for the palette/search UI: file path plus the
/// ancestor chain (root → parent) so the tree can expand and scroll.
#[derive(Debug, Clone, Serialize)]
pub struct JumpTarget {
    pub document_id: String,
    pub file_path: String,
    pub task_key: TaskKey,
    /// Task IDs from the root down to (but excluding) the target.
    pub ancestor_task_ids: Vec<String>,
}

#[tauri::command]
pub fn resolve_task_jump(
    state: State<'_, WorkspaceState>,
    document_id: String,
    task_id: String,
) -> Result<JumpTarget, String> {
    let db = state.db.lock().map_err(|_| "Lock error")?;
    if !db.is_available() {
        return Err("Index database unavailable".into());
    }
    db.with_connection(|conn| {
        let file_path: String = conn
            .query_row(
                "SELECT file_path FROM documents WHERE id = ?1",
                [&document_id],
                |r| r.get(0),
            )
            .map_err(|_| {
                crate::infrastructure::DatabaseError::Sqlite(rusqlite::Error::QueryReturnedNoRows)
            })?;
        let ancestors = std::fs::read(&file_path)
            .ok()
            .and_then(|bytes| search::resolve_ancestors(&bytes, &task_id))
            .unwrap_or_default();
        Ok(JumpTarget {
            document_id: document_id.clone(),
            file_path,
            task_key: TaskKey::new(
                crate::domain::types::DocumentId::new(&document_id),
                crate::domain::types::TaskId::new(&task_id),
            ),
            ancestor_task_ids: ancestors,
        })
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_registry_helpers_delegate_to_domain() {
        let cmds = list_app_commands().unwrap();
        assert!(cmds.iter().any(|c| c.id == "file.save"));
        assert!(get_shortcut_conflicts().unwrap().is_empty());
    }

    #[test]
    fn palette_response_orders_commands_before_tasks() {
        // Structural contract for the frontend: commands first.
        let resp = PaletteResponse {
            commands: search::search_commands("undo", 5),
            tasks: Vec::new(),
            total_tasks: 0,
        };
        assert_eq!(resp.commands[0].command.id, "edit.undo");
    }
}
