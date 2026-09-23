//! M5: Task query IPC commands
//!
//! Provides read access to the SQLite task index for the frontend.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::workspace::WorkspaceState;

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskSummary {
    pub task_key: String,
    pub document_id: String,
    pub title: String,
    pub priority: i32,
    pub status: String,
    pub percent_done: i32,
    pub risk: i32,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub completed_date: Option<String>,
    pub parent_key: Option<String>,
    pub position: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskQueryResult {
    pub tasks: Vec<TaskSummary>,
    pub total: usize,
}

/// Query tasks from the SQLite index.
#[tauri::command]
pub fn query_tasks(
    state: State<'_, WorkspaceState>,
    document_id: Option<String>,
    parent_key: Option<String>,
    status_filter: Option<String>,
    limit: Option<i32>,
) -> Result<TaskQueryResult, String> {
    let db = state.db.lock().map_err(|_| "Lock error")?;
    if !db.is_available() {
        return Ok(TaskQueryResult { tasks: Vec::new(), total: 0 });
    }

    let result = db.with_connection(|conn| {
        let mut sql = String::from(
            "SELECT task_key, document_id, title, priority, status, percent_done, risk, \
             start_date, due_date, completed_date, parent_key, position \
             FROM task_index WHERE 1=1"
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref doc_id) = document_id {
            sql.push_str(" AND document_id = ?");
            params.push(Box::new(doc_id.clone()));
        } else if let Ok(guard) = state.workspace.lock() {
            // No explicit document: scope to the currently-open workspace so
            // tasks from other workspaces never leak into this task tree.
            if let Some(ws) = guard.as_ref() {
                sql.push_str(" AND document_id IN (SELECT id FROM documents WHERE workspace_id = ?)");
                params.push(Box::new(ws.metadata.id.clone()));
            }
        }
        if let Some(ref parent) = parent_key {
            sql.push_str(" AND parent_key = ?");
            params.push(Box::new(parent.clone()));
        }
        if let Some(ref status) = status_filter {
            sql.push_str(" AND status = ?");
            params.push(Box::new(status.clone()));
        }

        sql.push_str(" ORDER BY COALESCE(parent_key, ''), position, task_key");

        if let Some(lim) = limit {
            sql.push_str(&format!(" LIMIT {}", lim));
        }

        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            Ok(TaskSummary {
                task_key: row.get(0)?,
                document_id: row.get(1)?,
                title: row.get(2)?,
                priority: row.get(3)?,
                status: row.get(4)?,
                // The `percent_done` column is REAL; rusqlite will not coerce a
                // REAL into i32, so read it as f64 and cast (mirrors saved_views).
                percent_done: row.get::<_, f64>(5)? as i32,
                risk: row.get(6)?,
                start_date: row.get(7)?,
                due_date: row.get(8)?,
                completed_date: row.get(9)?,
                parent_key: row.get(10)?,
                position: row.get(11)?,
            })
        })?;

        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row?);
        }
        let total = tasks.len();
        Ok(TaskQueryResult { tasks, total })
    });

    result.map_err(|e| e.to_string())
}

/// Get task tags for a given task.
#[tauri::command]
pub fn get_task_tags(
    state: State<'_, WorkspaceState>,
    task_key: String,
) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|_| "Lock error")?;
    if !db.is_available() {
        return Ok(Vec::new());
    }

    let result = db.with_connection(|conn| {
        let mut stmt = conn.prepare("SELECT tag FROM task_tags WHERE task_key = ?1 ORDER BY tag")?;
        let tags: Vec<String> = stmt
            .query_map([&task_key], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(tags)
    });

    result.map_err(|e| e.to_string())
}
