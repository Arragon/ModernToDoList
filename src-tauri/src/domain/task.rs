//! Task domain model for ModernToDoList 2.0.
//!
//! This module defines the domain types for tasks, documents, and their fields.
//! These types are independent of XML serialization and represent the canonical
//! in-memory representation of task data.
//!
//! # Architecture
//!
//! - `Task` is the core domain object, representing a single task with all its fields.
//! - `TaskTree` is a container for tasks, supporting hierarchical parent-child relationships.
//! - `DocumentMetadata` captures document-level information (encoding, file info, etc.).
//! - Field types (`TaskPriority`, `TaskStatus`, etc.) provide type safety for task fields.

use serde::{Deserialize, Serialize};

use super::encoding::XmlEncodingMeta;
use super::types::TaskId;

/// A single task in the domain model.
///
/// This represents the canonical in-memory form of a `<TASK>` element.
/// All known fields are mapped to typed Rust types. Unknown attributes
/// and child elements are preserved for lossless round-tripping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// The task ID (from the XML `ID` attribute).
    pub id: TaskId,
    /// The task title (from `TITLE`).
    pub title: String,
    /// Reference ID (from `REFID`).
    pub ref_id: String,
    /// Comments type (from `COMMENTSTYPE`). Write-protected.
    pub comments_type: CommentType,
    /// Priority (from `PRIORITY`, 0-10).
    pub priority: TaskPriority,
    /// Risk level (from `RISK`, 0-10).
    pub risk: u8,
    /// Percent complete (from `PERCENTDONE`, 0-100).
    pub percent_done: u8,
    /// Start date as OLE automation date (from `STARTDATE`).
    pub start_date: Option<f64>,
    /// Start date string (from `STARTDATESTRING`).
    pub start_date_string: Option<String>,
    /// Due date as OLE automation date (from `DUEDATE`).
    pub due_date: Option<f64>,
    /// Due date string (from `DUEDATESTRING`).
    pub due_date_string: Option<String>,
    /// Creation date as OLE automation date (from `CREATIONDATE`).
    pub creation_date: Option<f64>,
    /// Creation date string (from `CREATIONDATESTRING`).
    pub creation_date_string: Option<String>,
    /// Completion date (from `COMPLETIONDATE`).
    pub completion_date: Option<f64>,
    /// Completion date string (from `COMPLETIONDATESTRING`).
    pub completion_date_string: Option<String>,
    /// Last modification date as OLE automation date (from `LASTMOD`).
    pub last_mod: Option<f64>,
    /// Last modification date string (from `LASTMODSTRING`).
    pub last_mod_string: Option<String>,
    /// Who created the task (from `CREATEDBY`).
    pub created_by: Option<String>,
    /// Who last modified (from `LASTMODBY`).
    pub last_mod_by: Option<String>,
    /// Position index (from `POS`).
    pub pos: u32,
    /// Position string like "1.2.3" (from `POSSTRING`).
    pub pos_string: Option<String>,
    /// Allocated to (from `ALLOCATEDTO`).
    pub allocated_to: Vec<String>,
    /// Allocated by (from `ALLOCATEDBY`).
    pub allocated_by: Option<String>,
    /// File links (from `<FILEREFPATH>` children).
    pub file_links: Vec<TaskFileLink>,
    /// Categories/tags (from `<CATEGORY>` children).
    pub categories: Vec<TaskCategory>,
    /// Dependencies (from `<DEPENDENCY>` children).
    pub dependencies: Vec<TaskDependency>,
    /// Comments (from `<COMMENTS>` child).
    pub comments: Option<TaskComment>,
    /// Custom metadata entries (from `<METADATA>` children).
    pub metadata: Vec<TaskMetadata>,
    /// Time estimate value (from `TIMEESTIMATE`).
    pub time_estimate: Option<f64>,
    /// Time estimate units (from `TIMEESTUNITS`: D=days, H=hours, M=minutes).
    pub time_est_units: Option<String>,
    /// Time spent value (from `TIMESPENT`).
    pub time_spent: Option<f64>,
    /// Time spent units (from `TIMESPENTUNITS`).
    pub time_spent_units: Option<String>,
    /// Subtask done count string (from `SUBTASKDONE`, e.g., "2/5"). Read-only.
    pub subtask_done: Option<String>,
    /// Display text color as RGB integer (from `TEXTCOLOR`). Read-only.
    pub text_color: Option<String>,
    /// Display text color as web color (from `TEXTWEBCOLOR`). Read-only.
    pub text_web_color: Option<String>,
    /// Priority color as RGB integer (from `PRIORITYCOLOR`). Read-only.
    pub priority_color: Option<String>,
    /// Priority color as web color (from `PRIORITYWEBCOLOR`). Read-only.
    pub priority_web_color: Option<String>,
    /// Child task IDs in order (from nested `<TASK>` children).
    pub children: Vec<TaskId>,
    /// Unknown attributes not recognized by the domain model. Preserved for lossless round-trip.
    pub unknown_attrs: Vec<(String, String)>,
    /// Unknown child elements not recognized by the domain model. Preserved as raw XML strings.
    pub unknown_children: Vec<String>,
}

impl Task {
    /// Creates a new task with the given ID and default values.
    pub fn new(id: TaskId) -> Self {
        Self {
            id,
            title: String::new(),
            ref_id: "0".to_string(),
            comments_type: CommentType::Plain,
            priority: TaskPriority::default(),
            risk: 0,
            percent_done: 0,
            start_date: None,
            start_date_string: None,
            due_date: None,
            due_date_string: None,
            creation_date: None,
            creation_date_string: None,
            completion_date: None,
            completion_date_string: None,
            last_mod: None,
            last_mod_string: None,
            created_by: None,
            last_mod_by: None,
            pos: 0,
            pos_string: None,
            allocated_to: Vec::new(),
            allocated_by: None,
            file_links: Vec::new(),
            categories: Vec::new(),
            dependencies: Vec::new(),
            comments: None,
            metadata: Vec::new(),
            time_estimate: None,
            time_est_units: None,
            time_spent: None,
            time_spent_units: None,
            subtask_done: None,
            text_color: None,
            text_web_color: None,
            priority_color: None,
            priority_web_color: None,
            children: Vec::new(),
            unknown_attrs: Vec::new(),
            unknown_children: Vec::new(),
        }
    }

    /// Returns the task status based on percent_done.
    pub fn status(&self) -> TaskStatus {
        if self.percent_done >= 100 {
            TaskStatus::Done
        } else if self.percent_done > 0 {
            TaskStatus::InProgress
        } else {
            TaskStatus::NotStarted
        }
    }

    /// Returns true if this task has child tasks.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

/// Task completion status derived from percent_done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Not started (0% done).
    NotStarted,
    /// In progress (1-99% done).
    InProgress,
    /// Done (100% done).
    Done,
}

/// Task priority level (0-10 scale).
///
/// The TDL format uses a 0-10 scale where:
/// 0 = None, 1-3 = Low, 4-6 = Medium, 7-9 = High, 10 = Very High
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskPriority(u8);

impl TaskPriority {
    /// Creates a new priority, clamping to 0-10 range.
    pub fn new(value: u8) -> Self {
        Self(value.min(10))
    }

    /// Returns the raw priority value.
    pub fn value(&self) -> u8 {
        self.0
    }
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self(5) // Medium priority
    }
}

impl From<u8> for TaskPriority {
    fn from(v: u8) -> Self {
        Self::new(v)
    }
}

/// Comment type for a task.
///
/// This is a write-protected field. The COMMENTSTYPE attribute must not
/// be silently modified. Only PLAIN_TEXT comments are editable in 2.0.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommentType {
    /// Plain text comments.
    #[default]
    Plain,
    /// HTML formatted comments.
    Html,
    /// Unknown comment type (preserved but not editable).
    Unknown(String),
}

impl CommentType {
    /// Returns the XML attribute value for this comment type.
    pub fn as_attr_value(&self) -> &str {
        match self {
            CommentType::Plain => "PLAIN_TEXT",
            CommentType::Html => "HTML",
            CommentType::Unknown(s) => s.as_str(),
        }
    }

    /// Parses a comment type from an XML attribute value.
    pub fn from_attr_value(value: &str) -> Self {
        match value {
            "PLAIN_TEXT" => CommentType::Plain,
            "HTML" => CommentType::Html,
            other => CommentType::Unknown(other.to_string()),
        }
    }

    /// Returns true if this comment type is editable.
    pub fn is_editable(&self) -> bool {
        matches!(self, CommentType::Plain)
    }
}

/// A file link attached to a task (from `<FILEREFPATH>` child element).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskFileLink {
    /// The file path (relative or absolute).
    pub path: String,
}

/// A category/tag assigned to a task (from `<CATEGORY>` child element).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCategory {
    /// The category name.
    pub name: String,
}

/// A task dependency (from `<DEPENDENCY>` child element).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDependency {
    /// The ID of the dependent task (from `<TASKID>`).
    pub task_id: String,
    /// The dependency type (from `<DEPENDENCYTYPE>`).
    /// 0=Finish-to-Finish, 1=Start-to-Start, etc.
    pub dependency_type: u8,
    /// Raw XML content for unknown sub-elements. Preserved for lossless round-trip.
    pub raw_xml: Option<String>,
}

/// Task comments (from `<COMMENTS>` child element).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskComment {
    /// The comment type (must match the task's COMMENTSTYPE).
    pub comment_type: CommentType,
    /// The raw comment content.
    pub content: String,
}

/// Custom metadata entry (from `<METADATA>` child element).
///
/// METADATA elements use GUID-keyed attributes. The exact structure
/// varies by plugin/tool, so we preserve the raw attribute map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskMetadata {
    /// The metadata attributes (GUID key → value).
    pub attrs: Vec<(String, String)>,
}

/// Document-level metadata.
///
/// Captures information about the XML document as a whole,
/// including the TODOLIST root element attributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentMetadata {
    /// Document encoding metadata.
    pub encoding_meta: XmlEncodingMeta,
    /// Project name (from `PROJECTNAME`).
    pub project_name: Option<String>,
    /// Original filename (from `FILENAME`).
    pub filename: Option<String>,
    /// Next unique ID (from `NEXTUNIQUEID`).
    pub next_unique_id: u64,
    /// File version (from `FILEVERSION`).
    pub file_version: Option<String>,
    /// Application version that created the file (from `APPVER`).
    pub app_ver: Option<String>,
    /// File format version (from `FILEFORMAT`).
    pub file_format: Option<String>,
    /// Earliest due date (from `EARLIESTDUEDATE`). Read-only.
    pub earliest_due_date: Option<f64>,
    /// Last modification date (from `LASTMOD`).
    pub last_mod: Option<f64>,
    /// Last modification date string (from `LASTMODSTRING`).
    pub last_mod_string: Option<String>,
    /// Unknown root-level attributes. Preserved for lossless round-trip.
    pub unknown_root_attrs: Vec<(String, String)>,
    /// Root-level METADATA element attributes. Preserved for lossless round-trip.
    pub root_metadata: Vec<TaskMetadata>,
}

impl DocumentMetadata {
    /// Creates a new `DocumentMetadata` with default values.
    pub fn new(encoding_meta: XmlEncodingMeta) -> Self {
        Self {
            encoding_meta,
            project_name: None,
            filename: None,
            next_unique_id: 1,
            file_version: None,
            app_ver: None,
            file_format: None,
            earliest_due_date: None,
            last_mod: None,
            last_mod_string: None,
            unknown_root_attrs: Vec::new(),
            root_metadata: Vec::new(),
        }
    }
}

/// A hierarchical container for tasks within a document.
///
/// `TaskTree` stores all tasks in a flat map for O(1) lookup by ID,
/// and maintains the hierarchy through parent-child relationships.
/// The root tasks (top-level) are stored separately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskTree {
    /// All tasks indexed by ID for fast lookup.
    tasks: std::collections::HashMap<String, Task>,
    /// Root-level task IDs in document order.
    pub(crate) root_ids: Vec<TaskId>,
}

impl TaskTree {
    /// Creates a new empty task tree.
    pub fn new() -> Self {
        Self {
            tasks: std::collections::HashMap::new(),
            root_ids: Vec::new(),
        }
    }

    /// Returns the number of tasks in the tree.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Returns true if the tree has no tasks.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Gets a task by ID.
    pub fn get(&self, id: &TaskId) -> Option<&Task> {
        self.tasks.get(id.as_str())
    }

    /// Gets a mutable reference to a task by ID.
    pub fn get_mut(&mut self, id: &TaskId) -> Option<&mut Task> {
        self.tasks.get_mut(id.as_str())
    }

    /// Returns the root-level task IDs in order.
    pub fn root_ids(&self) -> &[TaskId] {
        &self.root_ids
    }

    /// Returns an iterator over all tasks.
    pub fn iter(&self) -> impl Iterator<Item = &Task> {
        self.tasks.values()
    }

    /// Returns the root-level tasks in order.
    pub fn roots(&self) -> Vec<&Task> {
        self.root_ids
            .iter()
            .filter_map(|id| self.tasks.get(id.as_str()))
            .collect()
    }

    /// Returns the children of a task.
    pub fn children_of(&self, id: &TaskId) -> Vec<&Task> {
        self.tasks
            .get(id.as_str())
            .map(|t| {
                t.children
                    .iter()
                    .filter_map(|cid| self.tasks.get(cid.as_str()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Adds a task to the tree. Returns false if a task with the same ID exists.
    pub fn add_task(&mut self, task: Task) -> bool {
        let id_str = task.id.as_str().to_string();
        if self.tasks.contains_key(&id_str) {
            return false;
        }
        self.tasks.insert(id_str, task);
        true
    }

    /// Adds a root-level task ID.
    pub fn add_root_id(&mut self, id: TaskId) {
        if !self.root_ids.contains(&id) {
            self.root_ids.push(id);
        }
    }

    /// Removes a task and its descendants from the tree.
    ///
    /// Also removes the task ID from its parent's children list
    /// and from the root_ids list.
    ///
    /// Returns the removed task IDs (including descendants).
    pub fn remove_task(&mut self, id: &TaskId) -> Vec<TaskId> {
        let mut removed = Vec::new();
        self.remove_recursive(id, &mut removed);

        // Remove from root_ids
        self.root_ids.retain(|rid| rid != id);

        // Remove from parent's children list
        for task in self.tasks.values_mut() {
            task.children.retain(|cid| cid != id);
        }

        removed
    }

    fn remove_recursive(&mut self, id: &TaskId, removed: &mut Vec<TaskId>) {
        // First collect children to remove
        if let Some(task) = self.tasks.get(id.as_str()) {
            let children: Vec<TaskId> = task.children.clone();
            for child_id in children {
                self.remove_recursive(&child_id, removed);
            }
        }

        if let Some(task) = self.tasks.remove(id.as_str()) {
            removed.push(task.id);
        }
    }

    /// Moves a task to a new position among its siblings.
    ///
    /// `parent_id` is None for root-level tasks.
    /// `new_index` is the target position in the sibling list.
    pub fn reorder_task(
        &mut self,
        id: &TaskId,
        parent_id: Option<&TaskId>,
        new_index: usize,
    ) -> bool {
        // First, remove from current sibling list
        let removed = if let Some(pid) = parent_id {
            if let Some(parent) = self.tasks.get_mut(pid.as_str()) {
                let pos = parent.children.iter().position(|sid| sid == id);
                if pos.is_none() {
                    return false;
                }
                parent.children.remove(pos.unwrap())
            } else {
                return false;
            }
        } else {
            let pos = self.root_ids.iter().position(|sid| sid == id);
            if pos.is_none() {
                return false;
            }
            self.root_ids.remove(pos.unwrap())
        };

        // Insert at new position
        if let Some(pid) = parent_id {
            if let Some(parent) = self.tasks.get_mut(pid.as_str()) {
                let insert_pos = new_index.min(parent.children.len());
                parent.children.insert(insert_pos, removed);
            }
        } else {
            let insert_pos = new_index.min(self.root_ids.len());
            self.root_ids.insert(insert_pos, removed);
        }

        // Update POS values
        let sibling_ids: Vec<TaskId> = if let Some(pid) = parent_id {
            self.tasks
                .get(pid.as_str())
                .map(|p| p.children.clone())
                .unwrap_or_default()
        } else {
            self.root_ids.clone()
        };

        for (i, sid) in sibling_ids.iter().enumerate() {
            if let Some(task) = self.tasks.get_mut(sid.as_str()) {
                task.pos = i as u32;
            }
        }

        true
    }

    /// Moves a task to a new parent (reparent operation).
    ///
    /// The task is removed from its current parent (or root) and added
    /// to the new parent's children at the specified position.
    pub fn reparent_task(
        &mut self,
        id: &TaskId,
        new_parent_id: Option<&TaskId>,
        position: usize,
    ) -> bool {
        // Prevent reparenting to self or to a descendant
        if let Some(npid) = new_parent_id {
            if npid == id {
                return false;
            }
            if self.is_descendant_of(npid, id) {
                return false;
            }
        }

        // Remove from current parent or root
        for task in self.tasks.values_mut() {
            task.children.retain(|cid| cid != id);
        }
        self.root_ids.retain(|rid| rid != id);

        // Add to new parent or root
        if let Some(npid) = new_parent_id {
            if let Some(parent) = self.tasks.get_mut(npid.as_str()) {
                let pos = position.min(parent.children.len());
                parent.children.insert(pos, id.clone());
            } else {
                return false;
            }
        } else {
            let pos = position.min(self.root_ids.len());
            self.root_ids.insert(pos, id.clone());
        }

        // Update POS values
        self.update_pos_for(id, new_parent_id);

        true
    }

    /// Returns true if `potential_ancestor` is an ancestor of `id`.
    fn is_descendant_of(&self, id: &TaskId, potential_ancestor: &TaskId) -> bool {
        if let Some(task) = self.tasks.get(potential_ancestor.as_str()) {
            for child_id in &task.children {
                if child_id == id || self.is_descendant_of(id, child_id) {
                    return true;
                }
            }
        }
        false
    }

    /// Updates POS values for siblings of a task.
    fn update_pos_for(&mut self, _id: &TaskId, parent_id: Option<&TaskId>) {
        let sibling_list = if let Some(pid) = parent_id {
            self.tasks
                .get(pid.as_str())
                .map(|p| p.children.clone())
                .unwrap_or_default()
        } else {
            self.root_ids.clone()
        };

        for (i, sid) in sibling_list.iter().enumerate() {
            if let Some(task) = self.tasks.get_mut(sid.as_str()) {
                task.pos = i as u32;
            }
        }
    }

    /// Returns all task IDs in depth-first order.
    pub fn depth_first_ids(&self) -> Vec<TaskId> {
        let mut result = Vec::new();
        for root_id in &self.root_ids {
            self.dfs_collect(root_id, &mut result);
        }
        result
    }

    fn dfs_collect(&self, id: &TaskId, result: &mut Vec<TaskId>) {
        result.push(id.clone());
        if let Some(task) = self.tasks.get(id.as_str()) {
            for child_id in &task.children {
                self.dfs_collect(child_id, result);
            }
        }
    }

    /// Checks if a task ID exists in the tree.
    pub fn contains(&self, id: &TaskId) -> bool {
        self.tasks.contains_key(id.as_str())
    }
}

impl Default for TaskTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(id: &str, title: &str) -> Task {
        let mut t = Task::new(TaskId::new(id));
        t.title = title.to_string();
        t
    }

    #[test]
    fn task_default_values() {
        let task = Task::new(TaskId::new("1"));
        assert_eq!(task.title, "");
        assert_eq!(task.priority.value(), 5);
        assert_eq!(task.risk, 0);
        assert_eq!(task.percent_done, 0);
        assert_eq!(task.status(), TaskStatus::NotStarted);
        assert!(task.children.is_empty());
    }

    #[test]
    fn task_status_derived_from_percent() {
        let mut task = Task::new(TaskId::new("1"));
        assert_eq!(task.status(), TaskStatus::NotStarted);
        task.percent_done = 50;
        assert_eq!(task.status(), TaskStatus::InProgress);
        task.percent_done = 100;
        assert_eq!(task.status(), TaskStatus::Done);
    }

    #[test]
    fn priority_clamped() {
        assert_eq!(TaskPriority::new(0).value(), 0);
        assert_eq!(TaskPriority::new(5).value(), 5);
        assert_eq!(TaskPriority::new(10).value(), 10);
        assert_eq!(TaskPriority::new(15).value(), 10); // Clamped
    }

    #[test]
    fn comment_type_roundtrip() {
        assert_eq!(CommentType::from_attr_value("PLAIN_TEXT"), CommentType::Plain);
        assert_eq!(CommentType::from_attr_value("HTML"), CommentType::Html);
        assert_eq!(
            CommentType::from_attr_value("RTF"),
            CommentType::Unknown("RTF".into())
        );
        assert_eq!(CommentType::Plain.as_attr_value(), "PLAIN_TEXT");
        assert!(CommentType::Plain.is_editable());
        assert!(!CommentType::Html.is_editable());
    }

    #[test]
    fn task_tree_add_and_get() {
        let mut tree = TaskTree::new();
        let task = make_task("1", "Test");
        assert!(tree.add_task(task));
        assert!(!tree.add_task(make_task("1", "Duplicate"))); // Duplicate
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.get(&TaskId::new("1")).unwrap().title, "Test");
    }

    #[test]
    fn task_tree_remove() {
        let mut tree = TaskTree::new();
        tree.add_task(make_task("1", "Root"));
        tree.add_task(make_task("2", "Child"));
        tree.add_root_id(TaskId::new("1"));
        tree.get_mut(&TaskId::new("1")).unwrap().children.push(TaskId::new("2"));

        let removed = tree.remove_task(&TaskId::new("1"));
        assert_eq!(removed.len(), 2); // Root + child
        assert!(tree.is_empty());
    }

    #[test]
    fn task_tree_reorder() {
        let mut tree = TaskTree::new();
        tree.add_task(make_task("1", "A"));
        tree.add_task(make_task("2", "B"));
        tree.add_task(make_task("3", "C"));
        tree.add_root_id(TaskId::new("1"));
        tree.add_root_id(TaskId::new("2"));
        tree.add_root_id(TaskId::new("3"));

        // Move task 3 to position 0
        assert!(tree.reorder_task(&TaskId::new("3"), None, 0));
        assert_eq!(tree.root_ids()[0].as_str(), "3");
        assert_eq!(tree.root_ids()[1].as_str(), "1");
        assert_eq!(tree.root_ids()[2].as_str(), "2");
    }

    #[test]
    fn task_tree_reparent() {
        let mut tree = TaskTree::new();
        tree.add_task(make_task("1", "Root"));
        tree.add_task(make_task("2", "To Move"));
        tree.add_task(make_task("3", "New Parent"));
        tree.add_root_id(TaskId::new("1"));
        tree.add_root_id(TaskId::new("2"));
        tree.add_root_id(TaskId::new("3"));

        // Move task 2 under task 3
        assert!(tree.reparent_task(&TaskId::new("2"), Some(&TaskId::new("3")), 0));
        assert_eq!(tree.root_ids().len(), 2); // 1 and 3
        assert_eq!(
            tree.get(&TaskId::new("3")).unwrap().children[0].as_str(),
            "2"
        );
    }

    #[test]
    fn task_tree_reparent_prevents_cycle() {
        let mut tree = TaskTree::new();
        tree.add_task(make_task("1", "Root"));
        tree.add_task(make_task("2", "Child"));
        tree.add_root_id(TaskId::new("1"));
        tree.get_mut(&TaskId::new("1")).unwrap().children.push(TaskId::new("2"));

        // Try to reparent task 1 under task 2 (would create cycle)
        assert!(!tree.reparent_task(
            &TaskId::new("1"),
            Some(&TaskId::new("2")),
            0
        ));
    }

    #[test]
    fn task_tree_depth_first() {
        let mut tree = TaskTree::new();
        tree.add_task(make_task("1", "Root"));
        tree.add_task(make_task("2", "Child"));
        tree.add_task(make_task("3", "Grandchild"));
        tree.add_root_id(TaskId::new("1"));
        tree.get_mut(&TaskId::new("1")).unwrap().children.push(TaskId::new("2"));
        tree.get_mut(&TaskId::new("2")).unwrap().children.push(TaskId::new("3"));

        let ids: Vec<_> = tree
            .depth_first_ids()
            .iter()
            .map(|id| id.as_str().to_string())
            .collect();
        assert_eq!(ids, vec!["1", "2", "3"]);
    }
}
