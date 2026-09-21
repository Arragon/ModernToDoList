//! M9: Smart Views + Saved Views IPC (RD-M9-014~030).
//!
//! - `get_smart_view`: evaluates one of the eight built-in smart views over
//!   rows loaded from the DISPOSABLE task index. The "now" date can be
//!   injected (ISO string) so QA can pin exact boundaries; production callers
//!   omit it and get the local date.
//! - Saved-view CRUD + evaluation: views are disposable application state
//!   persisted in the `saved_views` table (never business data).

use chrono::{Local, NaiveDate};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::workspace::WorkspaceState;
use crate::domain::smart_view::{
    by_participant_view, by_tag_view, completed_view, flagged_view, overdue_view, parse_index_date,
    today_view, upcoming_view, unscheduled_view, SmartViewKind, TaskGroup, UpcomingGroups, ViewTask,
};
use crate::infrastructure::saved_views::{
    self, PredicateNode, SavedView, ViewRow,
};

// ── Smart view loading ───────────────────────────────────────────────────────

/// Load every indexed task of a workspace (optionally one document) as
/// [`ViewTask`] projections, enriching with tags and participants.
pub fn load_view_tasks(
    conn: &Connection,
    document_id: Option<&str>,
) -> Result<Vec<ViewTask>, rusqlite::Error> {
    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match document_id {
        Some(doc) => (
            "SELECT t.task_key, t.document_id, t.title, t.status, t.percent_done, t.priority, \
                    t.start_date, t.due_date, t.completed_date \
             FROM task_index t WHERE t.document_id = ?1 \
             ORDER BY t.document_id ASC, t.task_key ASC"
                .to_string(),
            vec![Box::new(doc.to_string())],
        ),
        None => (
            "SELECT t.task_key, t.document_id, t.title, t.status, t.percent_done, t.priority, \
                    t.start_date, t.due_date, t.completed_date \
             FROM task_index t ORDER BY t.document_id ASC, t.task_key ASC"
                .to_string(),
            Vec::new(),
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, f64>(4)?,
            row.get::<_, i32>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
        ))
    })?;

    let mut tasks = Vec::new();
    for row in rows {
        let (task_key, document_id, title, status, percent_done, priority, start, due, completed) =
            row?;
        let task_id = task_key.clone();
        let tags = fetch_strings(
            conn,
            "SELECT tag FROM task_tags WHERE task_key = ?1 AND document_id = ?2 ORDER BY tag",
            &task_id,
            &document_id,
        );
        let participants = fetch_strings(
            conn,
            "SELECT DISTINCT participant FROM task_participants \
             WHERE task_key = ?1 AND document_id = ?2 ORDER BY participant",
            &task_id,
            &document_id,
        );
        tasks.push(ViewTask {
            task_key,
            document_id,
            task_id,
            title,
            status,
            percent_done: percent_done.round().clamp(0.0, 100.0) as u8,
            priority: priority.clamp(0, 10) as u8,
            start_date: parse_index_date(start.as_deref()),
            due_date: parse_index_date(due.as_deref()),
            completed_date: parse_index_date(completed.as_deref()),
            tags,
            participants,
            flagged: false,
        });
    }
    Ok(tasks)
}

fn fetch_strings(conn: &Connection, sql: &str, task_key: &str, document_id: &str) -> Vec<String> {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map([task_key, document_id], |r| r.get::<_, String>(0))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

// ── Smart view IPC ───────────────────────────────────────────────────────────

/// Result of a smart-view evaluation. Flat views fill `tasks`; Upcoming fills
/// `groups_upcoming`; ByParticipant/ByTag fill `groups`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartViewResponse {
    pub view: String,
    /// The date the view was evaluated against (ISO) — injected in tests.
    pub evaluated_for: String,
    pub tasks: Vec<ViewTask>,
    pub groups: Vec<TaskGroup>,
    pub groups_upcoming: Option<UpcomingGroups>,
    pub total: usize,
}

/// Evaluate a built-in smart view.
///
/// `now` is an optional ISO date ("2024-03-13") used for ALL date arithmetic;
/// when omitted the current local date is used.
#[tauri::command]
pub fn get_smart_view(
    state: State<'_, WorkspaceState>,
    view: String,
    document_id: Option<String>,
    now: Option<String>,
) -> Result<SmartViewResponse, String> {
    let kind = SmartViewKind::parse(&view).ok_or_else(|| format!("Unknown smart view: {view}"))?;
    let today = match &now {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|e| format!("Invalid 'now' date {s}: {e}"))?,
        None => Local::now().date_naive(),
    };

    let db = state.db.lock().map_err(|_| "Lock error")?;
    if !db.is_available() {
        return Ok(SmartViewResponse {
            view: kind.as_str().to_string(),
            evaluated_for: today.to_string(),
            tasks: Vec::new(),
            groups: Vec::new(),
            groups_upcoming: None,
            total: 0,
        });
    }

    db.with_connection(|conn| {
        let tasks = load_view_tasks(conn, document_id.as_deref())
            .map_err(crate::infrastructure::DatabaseError::Sqlite)?;
        Ok(evaluate_smart_view(kind, &tasks, today))
    })
    .map_err(|e| e.to_string())
}

/// Pure evaluation used by the IPC command and by tests.
pub fn evaluate_smart_view(
    kind: SmartViewKind,
    tasks: &[ViewTask],
    today: NaiveDate,
) -> SmartViewResponse {
    let mut resp = SmartViewResponse {
        view: kind.as_str().to_string(),
        evaluated_for: today.to_string(),
        tasks: Vec::new(),
        groups: Vec::new(),
        groups_upcoming: None,
        total: 0,
    };
    match kind {
        SmartViewKind::Today => {
            resp.tasks = today_view(tasks, today).into_iter().cloned().collect();
        }
        SmartViewKind::Upcoming => {
            let groups = upcoming_view(tasks, today);
            resp.total = groups.total();
            resp.groups_upcoming = Some(groups);
            return resp;
        }
        SmartViewKind::Overdue => {
            resp.tasks = overdue_view(tasks, today).into_iter().cloned().collect();
        }
        SmartViewKind::Unscheduled => {
            resp.tasks = unscheduled_view(tasks).into_iter().cloned().collect();
        }
        SmartViewKind::Completed => resp.tasks = completed_view(tasks),
        SmartViewKind::Flagged => resp.tasks = flagged_view(tasks),
        SmartViewKind::ByParticipant => {
            resp.groups = by_participant_view(tasks);
            resp.total = resp.groups.iter().map(|g| g.tasks.len()).sum();
            return resp;
        }
        SmartViewKind::ByTag => {
            resp.groups = by_tag_view(tasks);
            resp.total = resp.groups.iter().map(|g| g.tasks.len()).sum();
            return resp;
        }
    }
    resp.total = resp.tasks.len();
    resp
}

// ── Saved view IPC (RD-M9-028~030) ───────────────────────────────────────────

/// Bridge saved-view errors into the DatabaseResult used by `with_connection`.
fn sv_err(e: saved_views::SavedViewError) -> crate::infrastructure::DatabaseError {
    match e {
        saved_views::SavedViewError::Sqlite(s) => crate::infrastructure::DatabaseError::Sqlite(s),
        other => crate::infrastructure::DatabaseError::Sqlite(
            rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        ),
    }
}

/// DTO for a predicate node on the IPC boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedViewDto {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub predicates: PredicateNode,
    pub sort_order: i64,
    pub created_at: String,
}

impl From<SavedView> for SavedViewDto {
    fn from(v: SavedView) -> Self {
        Self {
            id: v.id,
            workspace_id: v.workspace_id,
            name: v.name,
            predicates: v.predicates,
            sort_order: v.sort_order,
            created_at: v.created_at,
        }
    }
}

/// Create a saved view from the current filter state.
#[tauri::command]
pub fn create_saved_view(
    state: State<'_, WorkspaceState>,
    workspace_id: String,
    name: String,
    predicates: PredicateNode,
) -> Result<SavedViewDto, String> {
    with_db(&state, |conn| {
        saved_views::create_view(conn, &workspace_id, &name, &predicates)
            .map(SavedViewDto::from)
            .map_err(sv_err)
    })
}

/// List saved views for the sidebar.
#[tauri::command]
pub fn list_saved_views(
    state: State<'_, WorkspaceState>,
    workspace_id: String,
) -> Result<Vec<SavedViewDto>, String> {
    with_db(&state, |conn| {
        saved_views::list_views(conn, &workspace_id)
            .map(|views| views.into_iter().map(SavedViewDto::from).collect())
            .map_err(sv_err)
    })
}

/// Rename a saved view.
#[tauri::command]
pub fn rename_saved_view(
    state: State<'_, WorkspaceState>,
    id: String,
    new_name: String,
) -> Result<(), String> {
    with_db(&state, |conn| saved_views::rename_view(conn, &id, &new_name).map_err(sv_err))
}

/// Replace a saved view's predicates (re-save current filter state).
#[tauri::command]
pub fn update_saved_view(
    state: State<'_, WorkspaceState>,
    id: String,
    predicates: PredicateNode,
) -> Result<(), String> {
    with_db(&state, |conn| {
        saved_views::update_view(conn, &id, &predicates).map_err(sv_err)
    })
}

/// Delete a saved view.
#[tauri::command]
pub fn delete_saved_view(
    state: State<'_, WorkspaceState>,
    id: String,
) -> Result<(), String> {
    with_db(&state, |conn| saved_views::delete_view(conn, &id).map_err(sv_err))
}

/// Persist a new sidebar ordering of saved views.
#[tauri::command]
pub fn reorder_saved_views(
    state: State<'_, WorkspaceState>,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    with_db(&state, |conn| {
        saved_views::reorder_views(conn, &ordered_ids).map_err(sv_err)
    })
}

/// Evaluate a persisted saved view and return matching task rows.
#[tauri::command]
pub fn evaluate_saved_view(
    state: State<'_, WorkspaceState>,
    workspace_id: String,
    view_id: String,
) -> Result<Vec<ViewRow>, String> {
    with_db(&state, |conn| {
        let view = saved_views::get_view(conn, &view_id)
            .map_err(sv_err)?
            .ok_or(crate::infrastructure::DatabaseError::Unavailable)?;
        saved_views::evaluate(conn, &workspace_id, &view.predicates).map_err(sv_err)
    })
}

fn with_db<F, R>(state: &State<'_, WorkspaceState>, f: F) -> Result<R, String>
where
    F: FnOnce(&Connection) -> Result<R, crate::infrastructure::DatabaseError>,
{
    let db = state.db.lock().map_err(|_| "Lock error")?;
    if !db.is_available() {
        return Err("Index database unavailable".into());
    }
    db.with_connection(f).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::smart_view::ViewTask;
    use crate::infrastructure::migration;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn task(key: &str) -> ViewTask {
        ViewTask {
            task_key: key.into(),
            document_id: "doc1".into(),
            task_id: key.into(),
            title: format!("Task {key}"),
            status: "NotStarted".into(),
            percent_done: 0,
            priority: 5,
            start_date: None,
            due_date: None,
            completed_date: None,
            tags: vec![],
            participants: vec![],
            flagged: false,
        }
    }

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migration::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES ('ws1', 'WS', '/tmp')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path) VALUES ('doc1', 'ws1', 'a.tdl')",
            [],
        )
        .unwrap();
        // due 2024-03-13 = OLE 45364 (1899-12-30 + 45364)
        conn.execute(
            "INSERT INTO task_index (task_key, document_id, title, status, percent_done, priority, due_date) \
             VALUES ('1', 'doc1', 'Due today', 'NotStarted', 0, 5, '45364')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_tags (task_key, document_id, tag) VALUES ('1', 'doc1', 'now')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn load_view_tasks_reads_index_and_tags() {
        let conn = setup();
        let tasks = load_view_tasks(&conn, None).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Due today");
        assert_eq!(tasks[0].due_date, Some(d(2024, 3, 13)));
        assert_eq!(tasks[0].tags, vec!["now"]);
    }

    #[test]
    fn smart_view_dispatch_with_injected_now() {
        let conn = setup();
        let tasks = load_view_tasks(&conn, None).unwrap();
        let today = d(2024, 3, 13);

        let resp = evaluate_smart_view(SmartViewKind::Today, &tasks, today);
        assert_eq!(resp.total, 1);
        assert_eq!(resp.evaluated_for, "2024-03-13");

        let resp = evaluate_smart_view(SmartViewKind::Today, &tasks, d(2024, 3, 14));
        assert_eq!(resp.total, 0, "yesterday's due date is not Today tomorrow");

        let resp = evaluate_smart_view(SmartViewKind::Overdue, &tasks, d(2024, 3, 14));
        assert_eq!(resp.total, 1);

        let resp = evaluate_smart_view(SmartViewKind::Upcoming, &tasks, today);
        assert_eq!(resp.groups_upcoming.as_ref().unwrap().today.len(), 1);

        let resp = evaluate_smart_view(SmartViewKind::ByTag, &tasks, today);
        assert_eq!(resp.groups.len(), 1);
        assert_eq!(resp.groups[0].name, "now");
    }

    #[test]
    fn unscheduled_and_completed_dispatch() {
        let open = task("a");
        let done = {
            let mut t = task("b");
            t.percent_done = 100;
            t.completed_date = Some(d(2024, 5, 1));
            t
        };
        let tasks = vec![open, done];
        let resp = evaluate_smart_view(SmartViewKind::Unscheduled, &tasks, d(2024, 3, 13));
        assert_eq!(resp.total, 1);
        let resp = evaluate_smart_view(SmartViewKind::Completed, &tasks, d(2024, 3, 13));
        assert_eq!(resp.total, 1);
        assert_eq!(resp.tasks[0].task_key, "b");
    }
}
