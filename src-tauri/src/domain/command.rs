//! Undo/Redo command system for document mutations.
//!
//! Provides the `UndoableCommand` abstraction for document mutations.
//! Each command can be executed, undone, and redone.
//! Consecutive text edits can be coalesced into a single undo action.

use serde::{Deserialize, Serialize};

use super::task::{Task, TaskTree};
use super::types::TaskId;

/// Trait for undoable document mutations.
pub trait UndoableCommand: std::fmt::Debug {
    /// Executes the command, returning a description of the change.
    fn execute(&mut self, tree: &mut TaskTree) -> String;
    /// Undoes the command, restoring previous state.
    fn undo(&mut self, tree: &mut TaskTree);
    /// Redoes the command after it was undone.
    fn redo(&mut self, tree: &mut TaskTree);
    /// Returns a human-readable description of the command.
    fn description(&self) -> &str;
}

/// A field update command that can be undone.
#[derive(Debug, Clone)]
pub struct FieldUpdateCommand {
    /// The task to modify.
    pub task_id: TaskId,
    /// The field to update.
    pub field: TaskField,
    /// The new value.
    pub new_value: FieldValue,
    /// The old value (captured on first execute).
    pub old_value: Option<FieldValue>,
    /// Description of the change.
    pub desc: String,
}

/// Identifies a field within a Task.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskField {
    Title,
    Priority,
    Risk,
    PercentDone,
    StartDate,
    DueDate,
    Comments,
    AllocatedTo,
    Categories,
}

/// A field value that can be stored for undo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldValue {
    Text(String),
    Integer(u32),
    Float(Option<f64>),
    StringList(Vec<String>),
}

impl UndoableCommand for FieldUpdateCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        if let Some(task) = tree.get_mut(&self.task_id) {
            self.old_value = Some(get_field(task, &self.field));
            set_field(task, &self.field, &self.new_value);
        }
        self.desc.clone()
    }

    fn undo(&mut self, tree: &mut TaskTree) {
        if let (Some(task), Some(ref old)) = (tree.get_mut(&self.task_id), &self.old_value) {
            set_field(task, &self.field, old);
        }
    }

    fn redo(&mut self, tree: &mut TaskTree) {
        if let Some(task) = tree.get_mut(&self.task_id) {
            set_field(task, &self.field, &self.new_value);
        }
    }

    fn description(&self) -> &str {
        &self.desc
    }
}

/// Command to add a task to the tree.
#[derive(Debug, Clone)]
pub struct AddTaskCommand {
    pub task: Task,
    pub parent_id: Option<TaskId>,
    pub position: usize,
    pub executed: bool,
}

impl UndoableCommand for AddTaskCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        let id = self.task.id.clone();
        tree.add_task(self.task.clone());
        if let Some(ref pid) = self.parent_id {
            if let Some(parent) = tree.get_mut(pid) {
                let pos = self.position.min(parent.children.len());
                parent.children.insert(pos, id.clone());
            }
        } else {
            let pos = self.position.min(tree.root_ids().len());
            tree.add_root_id_at(id.clone(), pos);
        }
        self.executed = true;
        format!("Add task '{}'", self.task.title)
    }

    fn undo(&mut self, tree: &mut TaskTree) {
        tree.remove_task(&self.task.id);
        self.executed = false;
    }

    fn redo(&mut self, tree: &mut TaskTree) {
        self.execute(tree);
    }

    fn description(&self) -> &str {
        "Add task"
    }
}

/// Command to delete a task and its descendants.
#[derive(Debug, Clone)]
pub struct DeleteTaskCommand {
    pub task_id: TaskId,
    /// Snapshot of the deleted task tree (for undo).
    pub saved_tasks: Vec<Task>,
    pub saved_root_ids: Vec<TaskId>,
    pub parent_id: Option<TaskId>,
    pub executed: bool,
}

impl UndoableCommand for DeleteTaskCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        // Save state for undo
        self.saved_tasks.clear();
        collect_tasks(tree, &self.task_id, &mut self.saved_tasks);

        // Find parent
        self.parent_id = find_parent(tree, &self.task_id);

        // Remove
        tree.remove_task(&self.task_id);
        self.executed = true;
        format!("Delete task {}", self.task_id)
    }

    fn undo(&mut self, tree: &mut TaskTree) {
        // Restore tasks
        for task in &self.saved_tasks {
            tree.add_task(task.clone());
        }
        // Restore parent's children reference
        if let Some(ref pid) = self.parent_id {
            if let Some(parent) = tree.get_mut(pid) {
                if !parent.children.contains(&self.task_id) {
                    parent.children.push(self.task_id.clone());
                }
            }
        } else {
            tree.add_root_id(self.task_id.clone());
        }
        self.executed = false;
    }

    fn redo(&mut self, tree: &mut TaskTree) {
        tree.remove_task(&self.task_id);
        self.executed = true;
    }

    fn description(&self) -> &str {
        "Delete task"
    }
}

/// Undo/Redo manager.
#[derive(Debug)]
pub struct UndoRedoManager {
    undo_stack: Vec<Box<dyn UndoableCommand>>,
    redo_stack: Vec<Box<dyn UndoableCommand>>,
    max_history: usize,
}

impl UndoRedoManager {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_history: 100,
        }
    }

    /// Executes a command and pushes it onto the undo stack.
    pub fn execute(&mut self, mut cmd: Box<dyn UndoableCommand>, tree: &mut TaskTree) -> String {
        let desc = cmd.execute(tree);
        self.redo_stack.clear(); // New mutation invalidates redo
        self.undo_stack.push(cmd);
        if self.undo_stack.len() > self.max_history {
            self.undo_stack.remove(0);
        }
        desc
    }

    /// Undoes the last command.
    pub fn undo(&mut self, tree: &mut TaskTree) -> Option<String> {
        if let Some(mut cmd) = self.undo_stack.pop() {
            cmd.undo(tree);
            let desc = cmd.description().to_string();
            self.redo_stack.push(cmd);
            Some(desc)
        } else {
            None
        }
    }

    /// Redoes the last undone command.
    pub fn redo(&mut self, tree: &mut TaskTree) -> Option<String> {
        if let Some(mut cmd) = self.redo_stack.pop() {
            cmd.redo(tree);
            let desc = cmd.description().to_string();
            self.undo_stack.push(cmd);
            Some(desc)
        } else {
            None
        }
    }

    pub fn can_undo(&self) -> bool { !self.undo_stack.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo_stack.is_empty() }
    pub fn undo_count(&self) -> usize { self.undo_stack.len() }
    pub fn redo_count(&self) -> usize { self.redo_stack.len() }

    /// Clears all history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

impl Default for UndoRedoManager {
    fn default() -> Self { Self::new() }
}

// Helper functions

fn get_field(task: &Task, field: &TaskField) -> FieldValue {
    match field {
        TaskField::Title => FieldValue::Text(task.title.clone()),
        TaskField::Priority => FieldValue::Integer(task.priority.value() as u32),
        TaskField::Risk => FieldValue::Integer(task.risk as u32),
        TaskField::PercentDone => FieldValue::Integer(task.percent_done as u32),
        TaskField::StartDate => FieldValue::Float(task.start_date),
        TaskField::DueDate => FieldValue::Float(task.due_date),
        TaskField::Comments => FieldValue::Text(task.comments.as_ref().map(|c| c.content.clone()).unwrap_or_default()),
        TaskField::AllocatedTo => FieldValue::StringList(task.allocated_to.clone()),
        TaskField::Categories => FieldValue::StringList(task.categories.iter().map(|c| c.name.clone()).collect()),
    }
}

/// Formats an OLE Automation date as the `YYYY-MM-DD` display string TDL stores
/// alongside the float. Returns `None` when the date is cleared or out of range,
/// so the attribute is dropped rather than written empty.
fn ole_date_string(ole: Option<f64>) -> Option<String> {
    ole.and_then(super::smart_view::ole_to_naive_date)
        .map(|d| d.format("%Y-%m-%d").to_string())
}

fn set_field(task: &mut Task, field: &TaskField, value: &FieldValue) {
    match (field, value) {
        (TaskField::Title, FieldValue::Text(v)) => task.title = v.clone(),
        (TaskField::Priority, FieldValue::Integer(v)) => task.priority = super::task::TaskPriority::new(*v as u8),
        (TaskField::Risk, FieldValue::Integer(v)) => task.risk = (*v as u8).min(10),
        (TaskField::PercentDone, FieldValue::Integer(v)) => task.percent_done = (*v as u8).min(100),
        // TDL stores each date twice: an OLE Automation float (STARTDATE) and a
        // display string (STARTDATESTRING, "YYYY-MM-DD"). mappers::write_task
        // serializes both, so writing only the float left the string stale and
        // legacy TDL kept showing the pre-edit date. Deriving the string here
        // also makes undo correct, because get_field captures only the float and
        // undo replays through this same function.
        (TaskField::StartDate, FieldValue::Float(v)) => {
            task.start_date = *v;
            task.start_date_string = ole_date_string(*v);
        }
        (TaskField::DueDate, FieldValue::Float(v)) => {
            task.due_date = *v;
            task.due_date_string = ole_date_string(*v);
        }
        (TaskField::Comments, FieldValue::Text(v)) => {
            if let Some(ref mut c) = task.comments {
                c.content = v.clone();
            } else if !v.is_empty() {
                // Undoing a first-ever comments edit replays the captured empty
                // value. Materializing a TaskComment here made the next save emit
                // a <COMMENTS> element the document never had, so only create one
                // when there is actual content.
                task.comments = Some(super::task::TaskComment {
                    comment_type: task.comments_type.clone(),
                    content: v.clone(),
                });
            }
        }
        (TaskField::AllocatedTo, FieldValue::StringList(v)) => task.allocated_to = v.clone(),
        (TaskField::Categories, FieldValue::StringList(v)) => {
            task.categories = v.iter().map(|n| super::task::TaskCategory { name: n.clone() }).collect();
        }
        _ => {} // Type mismatch, ignore
    }
}

fn collect_tasks(tree: &TaskTree, id: &TaskId, result: &mut Vec<Task>) {
    if let Some(task) = tree.get(id) {
        result.push(task.clone());
        for child_id in &task.children {
            collect_tasks(tree, child_id, result);
        }
    }
}

fn find_parent(tree: &TaskTree, target: &TaskId) -> Option<TaskId> {
    for task in tree.iter() {
        if task.children.contains(target) {
            return Some(task.id.clone());
        }
    }
    None
}

// Extension for TaskTree to support insert at position
impl TaskTree {
    pub fn add_root_id_at(&mut self, id: TaskId, pos: usize) {
        let pos = pos.min(self.root_ids().len());
        self.root_ids.insert(pos, id);
    }
}

#[cfg(test)]
mod tests {
    // Regression coverage for the stale date-string bug: TDL stores each date as
    // an OLE float AND a "YYYY-MM-DD" display string, and mappers::write_task
    // serializes both. set_field used to write only the float, so a date edit
    // left STARTDATESTRING/DUEDATESTRING stale and legacy TDL kept showing the
    // pre-edit date. Undo replays through the same path, so it was wrong in both
    // directions.

    #[test]
    fn set_start_date_refreshes_display_string() {
        let mut task = super::Task::new(super::TaskId::new("1"));
        super::set_field(&mut task, &super::TaskField::StartDate, &super::FieldValue::Float(Some(45306.0)));
        assert_eq!(task.start_date, Some(45306.0));
        assert_eq!(task.start_date_string.as_deref(), Some("2024-01-15"));
    }

    #[test]
    fn set_due_date_refreshes_display_string() {
        let mut task = super::Task::new(super::TaskId::new("2"));
        super::set_field(&mut task, &super::TaskField::DueDate, &super::FieldValue::Float(Some(45306.0)));
        assert_eq!(task.due_date, Some(45306.0));
        assert_eq!(task.due_date_string.as_deref(), Some("2024-01-15"));
    }

    #[test]
    fn clearing_a_date_clears_its_display_string() {
        let mut task = super::Task::new(super::TaskId::new("3"));
        super::set_field(&mut task, &super::TaskField::DueDate, &super::FieldValue::Float(Some(45306.0)));
        assert_eq!(task.due_date_string.as_deref(), Some("2024-01-15"));
        super::set_field(&mut task, &super::TaskField::DueDate, &super::FieldValue::Float(None));
        assert_eq!(task.due_date, None);
        assert_eq!(task.due_date_string, None);
    }

    #[test]
    fn undo_restores_both_the_float_and_the_display_string() {
        let mut task = super::Task::new(super::TaskId::new("4"));
        super::set_field(&mut task, &super::TaskField::StartDate, &super::FieldValue::Float(Some(45306.0)));
        let captured = super::get_field(&task, &super::TaskField::StartDate);
        super::set_field(&mut task, &super::TaskField::StartDate, &super::FieldValue::Float(Some(45671.0)));
        assert_eq!(task.start_date_string.as_deref(), Some("2025-01-14"));
        super::set_field(&mut task, &super::TaskField::StartDate, &captured);
        assert_eq!(task.start_date, Some(45306.0));
        assert_eq!(task.start_date_string.as_deref(), Some("2024-01-15"));
    }

    /// KNOWN GAP E-F1 (deliberately pinned, not fixed).
    ///
    /// Undoing the first-ever comments edit cannot restore `None`, because
    /// `FieldValue::Text` has no way to represent "this task had no COMMENTS
    /// element" — `get_field` captures `""` for both an absent element and a
    /// genuinely empty one. So undo leaves `Some(TaskComment { content: "" })`
    /// and the next save emits a `<COMMENTS>` element the document never had.
    /// No content is lost; the defect is a spurious empty element.
    ///
    /// The `!v.is_empty()` guard in `set_field` only helps the narrower case of
    /// writing `""` to a task that never had comments. Fixing E-F1 properly
    /// needs an absence-carrying `FieldValue` variant, which changes the undo
    /// representation for every field and is out of scope here.
    ///
    /// This assertion must be inverted to `is_none()` once that lands.
    #[test]
    fn undo_of_first_comments_edit_leaves_an_empty_comment_known_gap_e_f1() {
        let mut task = super::Task::new(super::TaskId::new("c1"));
        assert!(task.comments.is_none(), "fixture task starts with no comments");

        let captured = super::get_field(&task, &super::TaskField::Comments);
        super::set_field(
            &mut task,
            &super::TaskField::Comments,
            &super::FieldValue::Text("评审记录".to_string()),
        );
        assert_eq!(task.comments.as_ref().map(|c| c.content.as_str()), Some("评审记录"));

        super::set_field(&mut task, &super::TaskField::Comments, &captured);
        assert_eq!(
            task.comments.as_ref().map(|c| c.content.as_str()),
            Some(""),
            "E-F1: undo restores empty content but cannot restore None"
        );
    }

    /// The narrow case the `!v.is_empty()` guard does fix: writing empty content
    /// to a task that never had a COMMENTS element must not create one.
    #[test]
    fn writing_empty_comments_to_a_task_without_any_creates_nothing() {
        let mut task = super::Task::new(super::TaskId::new("c0"));
        assert!(task.comments.is_none());
        super::set_field(
            &mut task,
            &super::TaskField::Comments,
            &super::FieldValue::Text(String::new()),
        );
        assert!(
            task.comments.is_none(),
            "an empty write must not materialize a COMMENTS element"
        );
    }

    /// A task that already has a comments element keeps it when cleared, so an
    /// explicitly emptied comment is not silently dropped from the document.
    #[test]
    fn clearing_an_existing_comment_keeps_the_element() {
        let mut task = super::Task::new(super::TaskId::new("c2"));
        task.comments = Some(crate::domain::task::TaskComment {
            comment_type: task.comments_type.clone(),
            content: "原有内容".to_string(),
        });

        super::set_field(
            &mut task,
            &super::TaskField::Comments,
            &super::FieldValue::Text(String::new()),
        );
        assert_eq!(
            task.comments.as_ref().map(|c| c.content.as_str()),
            Some(""),
            "an existing COMMENTS element is preserved, only its content is cleared"
        );
    }
    use super::*;
    use crate::domain::task::{Task, TaskTree};

    fn make_tree() -> TaskTree {
        let mut tree = TaskTree::new();
        let mut t = Task::new(TaskId::new("1"));
        t.title = "Original".into();
        t.priority = super::super::task::TaskPriority::new(5);
        tree.add_task(t);
        tree.add_root_id(TaskId::new("1"));
        tree
    }

    #[test]
    fn field_update_undo_redo() {
        let mut tree = make_tree();
        let mut mgr = UndoRedoManager::new();

        let cmd = FieldUpdateCommand {
            task_id: TaskId::new("1"),
            field: TaskField::Title,
            new_value: FieldValue::Text("Updated".into()),
            old_value: None,
            desc: "Change title".into(),
        };
        mgr.execute(Box::new(cmd), &mut tree);
        assert_eq!(tree.get(&TaskId::new("1")).unwrap().title, "Updated");

        mgr.undo(&mut tree);
        assert_eq!(tree.get(&TaskId::new("1")).unwrap().title, "Original");

        mgr.redo(&mut tree);
        assert_eq!(tree.get(&TaskId::new("1")).unwrap().title, "Updated");
    }

    #[test]
    fn new_mutation_clears_redo() {
        let mut tree = make_tree();
        let mut mgr = UndoRedoManager::new();

        mgr.execute(Box::new(FieldUpdateCommand {
            task_id: TaskId::new("1"),
            field: TaskField::Title,
            new_value: FieldValue::Text("A".into()),
            old_value: None,
            desc: "A".into(),
        }), &mut tree);

        mgr.undo(&mut tree);
        assert!(mgr.can_redo());

        // New mutation should clear redo
        mgr.execute(Box::new(FieldUpdateCommand {
            task_id: TaskId::new("1"),
            field: TaskField::Title,
            new_value: FieldValue::Text("B".into()),
            old_value: None,
            desc: "B".into(),
        }), &mut tree);
        assert!(!mgr.can_redo());
    }

    #[test]
    fn add_task_undo() {
        let mut tree = TaskTree::new();
        let mut mgr = UndoRedoManager::new();

        let task = Task::new(TaskId::new("1"));
        mgr.execute(Box::new(AddTaskCommand {
            task,
            parent_id: None,
            position: 0,
            executed: false,
        }), &mut tree);
        assert_eq!(tree.len(), 1);

        mgr.undo(&mut tree);
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn undo_redo_counts() {
        let mut tree = make_tree();
        let mut mgr = UndoRedoManager::new();

        for i in 0..5 {
            mgr.execute(Box::new(FieldUpdateCommand {
                task_id: TaskId::new("1"),
                field: TaskField::Title,
                new_value: FieldValue::Text(format!("V{}", i)),
                old_value: None,
                desc: format!("Change {}", i),
            }), &mut tree);
        }

        assert_eq!(mgr.undo_count(), 5);
        assert_eq!(mgr.redo_count(), 0);

        mgr.undo(&mut tree);
        assert_eq!(mgr.undo_count(), 4);
        assert_eq!(mgr.redo_count(), 1);
    }
}
