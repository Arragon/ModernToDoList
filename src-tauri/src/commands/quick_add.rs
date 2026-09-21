//! M9: Quick Add IPC + global shortcut seam (RD-M9-035~043).
//!
//! Quick Add parses a one-line grammar (`Task title #tag @participant
//! !priority due:2024-01-15`) into a [`QuickAddDraft`] and can build a full
//! domain [`Task`] ready for insertion.
//!
//! M3 SESSION SEAM: the actual tree mutation must go through
//! `commands::session` (AddTaskCommand + UndoRedoManager). `SessionEntry`'s
//! fields are module-private to session.rs, so this module intentionally
//! stops at producing the task payload — the orchestrator wires
//! `quick_add_build_task`'s output into the session pipeline.
//!
//! GLOBAL SHORTCUT SEAM (RD-M9-042~043): OS-level hotkeys require
//! `tauri-plugin-global-shortcut` (v2), which is NOT in Cargo.toml and was
//! not added per the file-ownership rules. This module defines the full
//! backend seam ([`GlobalShortcutBackend`]), an inert backend that degrades
//! gracefully, in-app conflict detection against the command registry, and a
//! persisted settings toggle (`ui_state` key). When the plugin is added, a
//! ~30-line `TauriGlobalShortcutBackend` implementing the trait completes
//! the feature.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::workspace::WorkspaceState;
use crate::domain::quick_add::{build_task, parse_quick_add_with_now, QuickAddDraft};
use crate::domain::search::command_registry;
use crate::domain::task::Task;
use crate::domain::types::TaskId;

// ── Quick Add IPC ────────────────────────────────────────────────────────────

/// Parse a quick-add line (UI preview). `now` is an optional ISO date used
/// for relative date tokens (`due:today`), injectable for deterministic tests.
#[tauri::command]
pub fn parse_quick_add(
    input: String,
    now: Option<String>,
) -> Result<QuickAddDraft, String> {
    let today = resolve_now(now.as_deref())?;
    Ok(parse_quick_add_with_now(&input, today))
}

/// Response of `quick_add_build_task`.
#[derive(Debug, Clone, Serialize)]
pub struct QuickAddTaskResponse {
    /// The parsed draft (title/tags/participants/priority/dates).
    pub draft: QuickAddDraft,
    /// A fully built domain task, ready for AddTaskCommand.
    pub task: Task,
    /// Tokens that looked special but were rejected (kept in the title).
    pub rejected_tokens: Vec<String>,
}

/// Parse AND build the domain task for a quick-add line.
///
/// The caller supplies the allocated `task_id` (from `allocate_task_id`) and
/// the session layer executes the returned [`Task`] as an `AddTaskCommand`
/// under `parent_task_id` (or at root when None) — see module docs for the
/// M3 seam.
#[tauri::command]
pub fn quick_add_build_task(
    input: String,
    task_id: String,
    parent_task_id: Option<String>,
    now: Option<String>,
) -> Result<QuickAddTaskResponse, String> {
    let today = resolve_now(now.as_deref())?;
    let draft = parse_quick_add_with_now(&input, today);
    if draft.is_empty() {
        return Err("Quick add input is empty".into());
    }
    let task = build_task(&draft, TaskId::new(&task_id));
    // `parent_task_id` is part of the IPC contract: the session layer uses it
    // as `AddTaskCommand.parent_id` (None → insert at root).
    let _ = parent_task_id;
    Ok(QuickAddTaskResponse {
        rejected_tokens: draft.rejected_tokens.clone(),
        draft,
        task,
    })
}

fn resolve_now(now: Option<&str>) -> Result<NaiveDate, String> {
    match now {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|e| format!("Invalid 'now' date {s}: {e}")),
        None => Ok(chrono::Local::now().date_naive()),
    }
}

// ── Global shortcut seam (RD-M9-042~043) ─────────────────────────────────────

/// Outcome of a shortcut registration attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ShortcutStatus {
    /// Successfully registered with the OS.
    Registered,
    /// The OS or another application (or an in-app command) already holds it.
    Conflict { holder: String },
    /// No OS-level backend available — feature degrades to in-app shortcuts.
    Unavailable { reason: String },
    /// The user disabled the global shortcut in settings.
    Disabled,
}

/// Abstraction over the OS global-shortcut facility.
///
/// Implement this on top of `tauri_plugin_global_shortcut::GlobalShortcut`
/// once the plugin crate is added. All conflict detection, persistence and
/// degradation logic lives outside the backend and is testable without it.
pub trait GlobalShortcutBackend {
    fn register(&mut self, id: &str, accelerator: &str) -> ShortcutStatus;
    fn unregister(&mut self, id: &str);
    fn is_registered(&self, id: &str) -> bool;
}

/// Default backend while `tauri-plugin-global-shortcut` is not bundled:
/// always degrades gracefully instead of failing.
#[derive(Debug, Default)]
pub struct InertGlobalShortcutBackend {
    registered: HashMap<String, String>,
}

impl GlobalShortcutBackend for InertGlobalShortcutBackend {
    fn register(&mut self, _id: &str, _accelerator: &str) -> ShortcutStatus {
        ShortcutStatus::Unavailable {
            reason: "tauri-plugin-global-shortcut is not bundled; \
                     global hotkeys degrade to in-app shortcuts"
                .to_string(),
        }
    }
    fn unregister(&mut self, _id: &str) {}
    fn is_registered(&self, _id: &str) -> bool {
        false
    }
}

/// Test/simulation backend modelling an OS where some accelerators are
/// already held by other applications.
#[derive(Debug, Default)]
pub struct SimulatedOsBackend {
    /// accelerator (lowercase) → holder name; pre-populated with "OS-held" ones.
    os_held: HashMap<String, String>,
    registered: HashMap<String, String>,
}

impl SimulatedOsBackend {
    pub fn with_os_held(mut self, accelerator: &str, holder: &str) -> Self {
        self.os_held
            .insert(accelerator.to_lowercase(), holder.to_string());
        self
    }
}

impl GlobalShortcutBackend for SimulatedOsBackend {
    fn register(&mut self, id: &str, accelerator: &str) -> ShortcutStatus {
        let key = accelerator.to_lowercase();
        if let Some(holder) = self.os_held.get(&key) {
            return ShortcutStatus::Conflict { holder: holder.clone() };
        }
        if let Some((other_id, _)) = self
            .registered
            .iter()
            .find(|(oid, acc)| oid.as_str() != id && acc.to_lowercase() == key)
        {
            return ShortcutStatus::Conflict { holder: other_id.clone() };
        }
        self.registered.insert(id.to_string(), accelerator.to_string());
        ShortcutStatus::Registered
    }
    fn unregister(&mut self, id: &str) {
        self.registered.remove(id);
    }
    fn is_registered(&self, id: &str) -> bool {
        self.registered.contains_key(id)
    }
}

/// Persisted user settings for the global quick-add hotkey.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalShortcutSettings {
    pub quick_add_enabled: bool,
    pub quick_add_accelerator: String,
    /// Result of the last registration attempt (informational for the UI).
    #[serde(default)]
    pub last_status: Option<ShortcutStatus>,
}

impl Default for GlobalShortcutSettings {
    fn default() -> Self {
        Self {
            quick_add_enabled: false,
            quick_add_accelerator: "Ctrl+Shift+Q".to_string(),
            last_status: None,
        }
    }
}

const SETTINGS_KEY: &str = "m9.global_shortcut";

/// In-app conflict detection: does the requested accelerator collide with a
/// command-registry shortcut? (Runs BEFORE touching the OS.)
pub fn find_in_app_conflict(accelerator: &str) -> Option<String> {
    let want = accelerator.trim().to_lowercase();
    command_registry()
        .iter()
        .find(|c| c.shortcut.map_or(false, |s| s.to_lowercase() == want))
        .map(|c| c.id.to_string())
}

fn backend() -> &'static Mutex<Box<dyn GlobalShortcutBackend + Send>> {
    static BACKEND: OnceLock<Mutex<Box<dyn GlobalShortcutBackend + Send>>> = OnceLock::new();
    BACKEND.get_or_init(|| Mutex::new(Box::new(InertGlobalShortcutBackend::default())))
}

/// Replace the process-wide backend (used by tests and by the future
/// tauri-plugin-global-shortcut integration).
pub fn set_global_shortcut_backend(new_backend: Box<dyn GlobalShortcutBackend + Send>) {
    if let Ok(mut guard) = backend().lock() {
        *guard = new_backend;
    }
}

fn load_settings(conn: &rusqlite::Connection) -> GlobalShortcutSettings {
    conn.query_row(
        "SELECT value_json FROM ui_state WHERE key = ?1",
        [SETTINGS_KEY],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|json| serde_json::from_str(&json).ok())
    .unwrap_or_default()
}

fn save_settings(
    conn: &rusqlite::Connection,
    settings: &GlobalShortcutSettings,
) -> Result<(), rusqlite::Error> {
    let json = serde_json::to_string(settings).unwrap_or_else(|_| "{}".to_string());
    conn.execute(
        "INSERT INTO ui_state (key, value_json, updated_at) \
         VALUES (?1, ?2, datetime('now')) \
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, \
                                        updated_at = excluded.updated_at",
        rusqlite::params![SETTINGS_KEY, json],
    )?;
    Ok(())
}

/// Read the persisted global-shortcut settings.
#[tauri::command]
pub fn get_global_shortcut_settings(
    state: State<'_, WorkspaceState>,
) -> Result<GlobalShortcutSettings, String> {
    let db = state.db.lock().map_err(|_| "Lock error")?;
    if !db.is_available() {
        return Ok(GlobalShortcutSettings::default());
    }
    db.with_connection(|conn| Ok(load_settings(conn)))
        .map_err(|e| e.to_string())
}

/// Toggle/configure the global quick-add hotkey.
///
/// Flow: settings toggle → in-app conflict check → backend registration
/// (OS conflict detection) → persist last status. Every failure mode
/// degrades gracefully; the toggle never hard-errors.
#[tauri::command]
pub fn set_global_quick_add_shortcut(
    state: State<'_, WorkspaceState>,
    enabled: bool,
    accelerator: Option<String>,
) -> Result<GlobalShortcutSettings, String> {
    let db = state.db.lock().map_err(|_| "Lock error")?;
    let accel = accelerator
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty());

    let mut guard = backend()
        .lock()
        .map_err(|_| "Shortcut backend lock poisoned")?;

    // Always drop the previous registration first.
    guard.unregister("quick_add");

    let mut settings = if db.is_available() {
        db.with_connection(|conn| Ok(load_settings(conn)))
            .map_err(|e| e.to_string())?
    } else {
        GlobalShortcutSettings::default()
    };
    if let Some(a) = accel {
        settings.quick_add_accelerator = a;
    }
    settings.quick_add_enabled = enabled;

    settings.last_status = Some(if !enabled {
        ShortcutStatus::Disabled
    } else if let Some(holder) = find_in_app_conflict(&settings.quick_add_accelerator) {
        ShortcutStatus::Conflict { holder }
    } else {
        guard.register("quick_add", &settings.quick_add_accelerator)
    });

    if db.is_available() {
        let to_save = settings.clone();
        let _ = db.with_connection(move |conn| {
            save_settings(conn, &to_save)
                .map_err(crate::infrastructure::DatabaseError::Sqlite)
        });
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::migration;

    fn conn() -> rusqlite::Connection {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        migration::run_migrations(&c).unwrap();
        c
    }

    #[test]
    fn parse_command_preview_injects_now() {
        let draft = parse_quick_add(
            "写周报 #工作 !high due:tomorrow".into(),
            Some("2024-03-13".into()),
        )
        .unwrap();
        assert_eq!(draft.title, "写周报");
        assert_eq!(draft.tags, vec!["工作"]);
        assert_eq!(draft.priority, Some(7));
        assert_eq!(
            draft.due_date,
            Some(NaiveDate::from_ymd_opt(2024, 3, 14).unwrap())
        );
    }

    #[test]
    fn build_task_command_produces_domain_task() {
        let resp = quick_add_build_task(
            "Ship it #m9 @Bob !high due:2024-01-15".into(),
            "77".into(),
            Some("3".into()),
            Some("2024-01-10".into()),
        )
        .unwrap();
        assert_eq!(resp.task.id.as_str(), "77");
        assert_eq!(resp.task.title, "Ship it");
        assert_eq!(resp.task.allocated_to, vec!["Bob".to_string()]);
        assert!(resp.task.due_date.is_some());
    }

    #[test]
    fn build_task_rejects_empty() {
        assert!(quick_add_build_task("   ".into(), "1".into(), None, None).is_err());
    }

    #[test]
    fn in_app_conflict_detection() {
        assert_eq!(
            find_in_app_conflict("ctrl+s"),
            Some("file.save".to_string())
        );
        assert_eq!(find_in_app_conflict("Ctrl+K"), Some("search.open".to_string()));
        assert_eq!(find_in_app_conflict("Ctrl+Shift+Q"), None);
    }

    #[test]
    fn os_conflict_detected_by_backend() {
        let mut backend = SimulatedOsBackend::default().with_os_held("ctrl+alt+q", "OtherApp");
        assert_eq!(
            backend.register("quick_add", "Ctrl+Alt+Q"),
            ShortcutStatus::Conflict { holder: "OtherApp".into() }
        );
        assert_eq!(
            backend.register("quick_add", "Ctrl+Shift+Q"),
            ShortcutStatus::Registered
        );
        assert!(backend.is_registered("quick_add"));
        // Second id, same accelerator → conflict with first holder.
        assert_eq!(
            backend.register("other", "ctrl+shift+q"),
            ShortcutStatus::Conflict { holder: "quick_add".into() }
        );
        backend.unregister("quick_add");
        assert!(!backend.is_registered("quick_add"));
    }

    #[test]
    fn inert_backend_degrades_gracefully() {
        let mut backend = InertGlobalShortcutBackend::default();
        match backend.register("quick_add", "Ctrl+Shift+Q") {
            ShortcutStatus::Unavailable { reason } => {
                assert!(reason.contains("global-shortcut"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn settings_persist_roundtrip() {
        let c = conn();
        let mut s = GlobalShortcutSettings::default();
        s.quick_add_enabled = true;
        s.quick_add_accelerator = "Ctrl+Alt+T".into();
        save_settings(&c, &s).unwrap();
        let loaded = load_settings(&c);
        assert_eq!(loaded, s);
        // Update overwrites.
        s.quick_add_enabled = false;
        save_settings(&c, &s).unwrap();
        assert!(!load_settings(&c).quick_add_enabled);
    }

    #[test]
    fn settings_default_is_disabled() {
        let s = GlobalShortcutSettings::default();
        assert!(!s.quick_add_enabled);
        assert_eq!(s.last_status, None);
    }
}
