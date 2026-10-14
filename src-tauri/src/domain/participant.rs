//! Participant domain model for M6 Task Relations (RD-M6-001~003).
//!
//! A participant is a person assigned to a task. In the legacy TDL XML
//! format participants are stored natively in the `ALLOCATEDTO` attribute
//! as a semicolon-separated list of display names. This module provides:
//!
//! - `ParticipantRef`, the typed domain reference (kept SEPARATE from the
//!   raw `Task::allocated_to` strings, but synchronized with them).
//! - Mutation helpers (`add_participant` / `remove_participant`) that keep
//!   `Task::participants` and `Task::allocated_to` in sync so that the
//!   native XML mapping in `mappers.rs` stays authoritative for
//!   serialization and source ordering is preserved exactly.
//! - Index helpers that populate the `task_participants` SQLite table
//!   (schema lives in `infrastructure/schema.rs`). The index is disposable
//!   derived state and can always be rebuilt from an XML scan.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::task::Task;

/// Errors that can occur while working with participants.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParticipantError {
    /// The display name is empty or whitespace-only.
    #[error("participant name is empty or whitespace-only")]
    EmptyName,
    /// The display name contains ASCII control characters (would corrupt
    /// the `ALLOCATEDTO` attribute or the index).
    #[error("participant name contains control characters")]
    ControlCharacters,
    /// The display name contains the `;` separator, which would split it
    /// into multiple participants in the native XML representation.
    #[error("participant name must not contain the ';' separator")]
    ContainsSeparator,
}

/// A typed reference to a participant by display name.
///
/// Deliberately minimal: the legacy format only stores display names, so
/// there is no stable identity beyond the name itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParticipantRef {
    /// The participant's display name (never empty, trimmed).
    pub display_name: String,
}

impl ParticipantRef {
    /// Creates a validated `ParticipantRef` from a raw name.
    ///
    /// The name is trimmed; empty names, control characters and the `;`
    /// separator (which would corrupt the native `ALLOCATEDTO` mapping)
    /// are rejected.
    pub fn new(name: impl Into<String>) -> Result<Self, ParticipantError> {
        let raw: String = name.into();
        if raw.chars().any(|c| c.is_ascii_control()) {
            return Err(ParticipantError::ControlCharacters);
        }
        if raw.contains(';') {
            return Err(ParticipantError::ContainsSeparator);
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ParticipantError::EmptyName);
        }
        Ok(Self {
            display_name: trimmed.to_string(),
        })
    }

    /// Wraps an already-validated display name without re-checking.
    /// Used when mirroring values parsed from `ALLOCATEDTO`.
    pub(crate) fn from_trusted(display_name: String) -> Self {
        Self { display_name }
    }
}

/// Parses a raw `ALLOCATEDTO` attribute value into ordered participant
/// refs. Source ordering is preserved exactly; empty segments are dropped.
pub fn parse_allocated_to(raw: &str) -> Vec<ParticipantRef> {
    raw.split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| ParticipantRef::from_trusted(s.to_string()))
        .collect()
}

/// Formats participant refs back into a native `ALLOCATEDTO` value.
pub fn format_allocated_to(participants: &[ParticipantRef]) -> String {
    participants
        .iter()
        .map(|p| p.display_name.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

/// Builds typed `ParticipantRef`s mirroring `Task::allocated_to`,
/// preserving source ordering. Called by the mapper after parsing.
pub fn refs_from_names(names: &[String]) -> Vec<ParticipantRef> {
    names
        .iter()
        .map(|n| ParticipantRef::from_trusted(n.clone()))
        .collect()
}

/// Adds a participant to a task, keeping `participants` and the native
/// `allocated_to` field in sync. Returns `Ok(true)` if added,
/// `Ok(false)` if the participant was already present (idempotent).
pub fn add_participant(task: &mut Task, name: &str) -> Result<bool, ParticipantError> {
    let participant = ParticipantRef::new(name)?;
    if task
        .participants
        .iter()
        .any(|p| p.display_name == participant.display_name)
    {
        return Ok(false);
    }
    if !task
        .allocated_to
        .iter()
        .any(|n| *n == participant.display_name)
    {
        task.allocated_to.push(participant.display_name.clone());
    }
    task.participants.push(participant);
    Ok(true)
}

/// Removes a participant from a task by display name, keeping
/// `participants` and `allocated_to` in sync. Returns true if removed.
pub fn remove_participant(task: &mut Task, name: &str) -> bool {
    let target = name.trim();
    if target.is_empty() {
        return false;
    }
    let before = task.participants.len();
    task.participants
        .retain(|p| p.display_name != target);
    let removed = task.participants.len() < before;
    // Keep the native field in sync regardless (defensive).
    task.allocated_to.retain(|n| n != target);
    removed
}

/// Re-synchronizes `Task::participants` from `Task::allocated_to`
/// (e.g. after a field-level undo that only touched `allocated_to`).
pub fn refresh_from_allocated(task: &mut Task) {
    task.participants = refs_from_names(&task.allocated_to);
}

/// One row of the `task_participants` index table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantIndexRow {
    /// The task key within the document.
    pub task_key: String,
    /// The owning document id.
    pub document_id: String,
    /// The participant display name.
    pub participant: String,
    /// Role: `allocated_to` or `allocated_by`.
    pub role: String,
}

/// Computes the `task_participants` index rows for one task.
/// Pure function of the task — the index is rebuildable from an XML scan.
pub fn index_rows(document_id: &str, task_key: &str, task: &Task) -> Vec<ParticipantIndexRow> {
    let mut rows = Vec::new();
    for p in &task.participants {
        rows.push(ParticipantIndexRow {
            task_key: task_key.to_string(),
            document_id: document_id.to_string(),
            participant: p.display_name.clone(),
            role: "allocated_to".to_string(),
        });
    }
    // Defensive: if `participants` was never populated (e.g. a Task built
    // by hand), fall back to the native `allocated_to` list.
    if task.participants.is_empty() {
        for name in &task.allocated_to {
            rows.push(ParticipantIndexRow {
                task_key: task_key.to_string(),
                document_id: document_id.to_string(),
                participant: name.clone(),
                role: "allocated_to".to_string(),
            });
        }
    }
    if let Some(ref by) = task.allocated_by {
        if !by.trim().is_empty() {
            rows.push(ParticipantIndexRow {
                task_key: task_key.to_string(),
                document_id: document_id.to_string(),
                participant: by.clone(),
                role: "allocated_by".to_string(),
            });
        }
    }
    rows
}

/// Replaces the `task_participants` rows for one task in the index DB.
/// Returns the number of rows inserted.
pub fn populate_task_participants(
    conn: &rusqlite::Connection,
    document_id: &str,
    task_key: &str,
    task: &Task,
) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM task_participants WHERE document_id = ?1 AND task_key = ?2",
        rusqlite::params![document_id, task_key],
    )?;
    let rows = index_rows(document_id, task_key, task);
    for row in &rows {
        conn.execute(
            "INSERT OR IGNORE INTO task_participants (task_key, document_id, participant, role) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![row.task_key, row.document_id, row.participant, row.role],
        )?;
    }
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::TaskId;

    fn task_with(allocated: &str, by: Option<&str>) -> Task {
        let mut t = Task::new(TaskId::new("7"));
        t.allocated_to = parse_allocated_to(allocated)
            .into_iter()
            .map(|p| p.display_name)
            .collect();
        t.participants = refs_from_names(&t.allocated_to);
        t.allocated_by = by.map(|s| s.to_string());
        t
    }

    #[test]
    fn participant_ref_validates_and_trims() {
        assert_eq!(
            ParticipantRef::new("  Alice ").unwrap().display_name,
            "Alice"
        );
        assert_eq!(ParticipantRef::new(""), Err(ParticipantError::EmptyName));
        assert_eq!(
            ParticipantRef::new("   "),
            Err(ParticipantError::EmptyName)
        );
        assert_eq!(
            ParticipantRef::new("bad\u{0}name"),
            Err(ParticipantError::ControlCharacters)
        );
        assert_eq!(
            ParticipantRef::new("a;b"),
            Err(ParticipantError::ContainsSeparator)
        );
    }

    #[test]
    fn parse_preserves_source_order() {
        let refs = parse_allocated_to("Zoe; Alice; Bob;Alice");
        let names: Vec<_> = refs.iter().map(|p| p.display_name.as_str()).collect();
        assert_eq!(names, vec!["Zoe", "Alice", "Bob", "Alice"]);
        assert_eq!(format_allocated_to(&refs), "Zoe; Alice; Bob; Alice");
    }

    #[test]
    fn parse_handles_empty_segments() {
        assert!(parse_allocated_to("").is_empty());
        assert!(parse_allocated_to(" ; ; ").is_empty());
    }

    #[test]
    fn add_remove_keeps_fields_in_sync() {
        let mut t = task_with("Alice; Bob", None);
        assert_eq!(add_participant(&mut t, "Carol").unwrap(), true);
        assert_eq!(t.allocated_to, vec!["Alice", "Bob", "Carol"]);
        assert_eq!(t.participants.len(), 3);
        assert_eq!(t.participants[2].display_name, "Carol");
        // Idempotent
        assert_eq!(add_participant(&mut t, " Carol ").unwrap(), false);
        assert_eq!(t.participants.len(), 3);
        // Removal syncs both
        assert!(remove_participant(&mut t, "Alice"));
        assert_eq!(t.allocated_to, vec!["Bob", "Carol"]);
        assert_eq!(t.participants.len(), 2);
        assert!(!remove_participant(&mut t, "Nobody"));
        // Validation failure leaves task untouched
        assert!(add_participant(&mut t, "  ").is_err());
        assert_eq!(t.participants.len(), 2);
    }

    #[test]
    fn refresh_from_allocated_resyncs() {
        let mut t = task_with("Alice", None);
        t.allocated_to.push("Dave".to_string()); // out-of-band mutation
        refresh_from_allocated(&mut t);
        assert_eq!(t.participants.len(), 2);
        assert_eq!(t.participants[1].display_name, "Dave");
    }

    #[test]
    fn index_rows_cover_both_roles() {
        let t = task_with("Alice; Bob", Some("Lead"));
        let rows = index_rows("doc-1", "7", &t);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].role, "allocated_to");
        assert_eq!(rows[2].role, "allocated_by");
        assert_eq!(rows[2].participant, "Lead");
    }

    #[test]
    fn populate_task_participants_roundtrip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE task_participants (
                task_key TEXT NOT NULL, document_id TEXT NOT NULL,
                participant TEXT NOT NULL, role TEXT NOT NULL DEFAULT 'allocated_to',
                PRIMARY KEY (task_key, document_id, participant, role));",
        )
        .unwrap();
        let t = task_with("Alice; Bob", Some("Lead"));
        let n = populate_task_participants(&conn, "doc-1", "7", &t).unwrap();
        assert_eq!(n, 3);
        // Idempotent replace
        let n2 = populate_task_participants(&conn, "doc-1", "7", &t).unwrap();
        assert_eq!(n2, 3);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_participants WHERE document_id='doc-1' AND task_key='7'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
        let distinct: String = conn
            .query_row(
                "SELECT participant FROM task_participants WHERE role='allocated_by'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(distinct, "Lead");
    }
}
