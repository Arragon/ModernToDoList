//! Strong domain ID types for ModernToDoList 2.0.
//!
//! These newtypes provide type safety for identifiers that flow through the
//! system. Using distinct types for each ID category prevents accidental
//! confusion (e.g., passing a `TaskId` where a `DocumentId` is expected).
//!
//! # ID Types
//!
//! - `WorkspaceId`: Identifies a workspace (a collection of documents).
//! - `DocumentId`: Identifies a document (an XML/TDL file) within a workspace.
//! - `TaskId`: The raw ID attribute from the XML `<TASK ID="...">` attribute.
//!   This is typically a numeric string assigned by AbstractSpoon TDL.
//! - `TaskKey`: A composite key `(DocumentId, TaskId)` that uniquely identifies
//!   a task across the entire workspace.
//!
//! # Design notes
//!
//! - All ID types are `Clone + Eq + Hash` for use in collections.
//! - `TaskId` stores the raw string value from XML to preserve compatibility
//!   with various ID formats (numeric, GUID, etc.).
//! - These types are intentionally independent of XML serialization details.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a workspace.
///
/// A workspace is a directory containing one or more task documents.
/// The ID is typically derived from the workspace directory path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    /// Creates a new `WorkspaceId` from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the string representation of this workspace ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for WorkspaceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for WorkspaceId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a document within a workspace.
///
/// A document corresponds to a single XML/TDL file. The ID may be derived
/// from the file path or assigned when the document is added to a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(String);

impl DocumentId {
    /// Creates a new `DocumentId` from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the string representation of this document ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for DocumentId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for DocumentId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// The raw task ID from the XML `ID` attribute.
///
/// In AbstractSpoon TDL files, task IDs are typically positive integers
/// represented as decimal strings (e.g., `"1"`, `"42"`, `"100"`).
/// However, the format is not strictly guaranteed, so we store the raw string.
///
/// # Invariants
///
/// - The ID string is non-empty.
/// - The ID is unique within its document (enforced by the ID allocator).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(String);

impl TaskId {
    /// Creates a new `TaskId` from a string.
    ///
    /// # Panics
    ///
    /// Panics if the ID string is empty.
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        assert!(!id.is_empty(), "TaskId must not be empty");
        Self(id)
    }

    /// Creates a `TaskId` from a numeric value.
    pub fn from_u64(id: u64) -> Self {
        Self(id.to_string())
    }

    /// Returns the string representation of this task ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Attempts to parse this task ID as a `u64`.
    ///
    /// Returns `None` if the ID is not a valid decimal number.
    pub fn as_u64(&self) -> Option<u64> {
        self.0.parse::<u64>().ok()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for TaskId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<u64> for TaskId {
    fn from(id: u64) -> Self {
        Self::from_u64(id)
    }
}

/// A composite key that uniquely identifies a task across the workspace.
///
/// `TaskKey` combines the document ID and task ID to form a globally unique
/// reference. This is used throughout the application to refer to specific
/// tasks without ambiguity, even when multiple documents are open.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskKey {
    /// The document containing this task.
    pub document_id: DocumentId,
    /// The task's ID within its document.
    pub task_id: TaskId,
}

impl TaskKey {
    /// Creates a new `TaskKey`.
    pub fn new(document_id: DocumentId, task_id: TaskId) -> Self {
        Self {
            document_id,
            task_id,
        }
    }
}

impl fmt::Display for TaskKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.document_id, self.task_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_id_creation() {
        let id = WorkspaceId::new("ws-001");
        assert_eq!(id.as_str(), "ws-001");
        assert_eq!(id.to_string(), "ws-001");
    }

    #[test]
    fn workspace_id_from_str() {
        let id: WorkspaceId = "ws-002".into();
        assert_eq!(id.as_str(), "ws-002");
    }

    #[test]
    fn workspace_id_equality() {
        let a = WorkspaceId::new("ws-001");
        let b = WorkspaceId::new("ws-001");
        let c = WorkspaceId::new("ws-002");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn document_id_creation() {
        let id = DocumentId::new("doc-001");
        assert_eq!(id.as_str(), "doc-001");
        assert_eq!(id.to_string(), "doc-001");
    }

    #[test]
    fn document_id_from_str() {
        let id: DocumentId = "doc-002".into();
        assert_eq!(id.as_str(), "doc-002");
    }

    #[test]
    fn task_id_creation() {
        let id = TaskId::new("42");
        assert_eq!(id.as_str(), "42");
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn task_id_from_u64() {
        let id = TaskId::from_u64(100);
        assert_eq!(id.as_str(), "100");
        assert_eq!(id.as_u64(), Some(100));
    }

    #[test]
    #[should_panic(expected = "TaskId must not be empty")]
    fn task_id_rejects_empty() {
        TaskId::new("");
    }

    #[test]
    fn task_id_as_u64_non_numeric() {
        let id = TaskId::new("abc");
        assert_eq!(id.as_u64(), None);
    }

    #[test]
    fn task_key_creation() {
        let key = TaskKey::new(
            DocumentId::new("doc-1"),
            TaskId::new("42"),
        );
        assert_eq!(key.document_id.as_str(), "doc-1");
        assert_eq!(key.task_id.as_str(), "42");
        assert_eq!(key.to_string(), "doc-1:42");
    }

    #[test]
    fn task_key_equality() {
        let a = TaskKey::new(DocumentId::new("doc-1"), TaskId::new("1"));
        let b = TaskKey::new(DocumentId::new("doc-1"), TaskId::new("1"));
        let c = TaskKey::new(DocumentId::new("doc-1"), TaskId::new("2"));
        let d = TaskKey::new(DocumentId::new("doc-2"), TaskId::new("1"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn task_id_serde_roundtrip() {
        let id = TaskId::new("42");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, decoded);
    }

    #[test]
    fn task_key_serde_roundtrip() {
        let key = TaskKey::new(DocumentId::new("doc-1"), TaskId::new("42"));
        let json = serde_json::to_string(&key).unwrap();
        let decoded: TaskKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, decoded);
    }
}
