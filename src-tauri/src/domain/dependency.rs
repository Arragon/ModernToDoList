//! Task dependency domain model for M6 Task Relations (RD-M6-009~016).
//!
//! # Reference model
//!
//! `TaskRef` classifies a dependency target:
//! - `Local` — a task within the same document (native TDL `<DEPENDENCY>`
//!   with a `<TASKID>` child).
//! - `External` — a cross-document reference using `DocumentId + TaskId`.
//!   The document id is carried in a custom `MTDL_DOCUMENTID` attribute on
//!   the native `<DEPENDENCY>` element (Tier B fallback per delivery-plan
//!   risk #1: legacy TDL preserves unknown attributes, and the mapper's
//!   `raw_xml` preservation keeps the attribute byte-stable on re-write).
//! - `Unresolved` — a reference whose raw value cannot be interpreted
//!   (e.g. empty `TASKID`). Preserved as-is, never rewritten or dropped.
//!
//! # Graph safety
//!
//! `DependencyGraph` maintains forward AND reverse adjacency and enforces:
//! - source and target existence,
//! - self-reference rejection,
//! - cycle rejection via DFS reachability (adding `A -> B` is refused when
//!   `B` can already reach `A`), reporting the offending path.
//!
//! # Undo integration
//!
//! `AddDependencyCommand` / `RemoveDependencyCommand` implement the
//! existing `UndoableCommand` trait. Validation happens in the checked
//! constructor (`::new`) against the current tree state; `execute`/`undo`
//! mutate `Task::dependencies` so the standard XML serialization path
//! (`mappers::write_task`) persists the change.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::command::UndoableCommand;
use super::task::{Task, TaskDependency, TaskTree};
use super::types::{DocumentId, TaskId};
use super::xml_parser::{escape_xml, parse_xml};

/// Custom attribute on `<DEPENDENCY>` carrying the target document id for
/// cross-document (external) references.
pub const EXTERNAL_DOC_ATTR: &str = "MTDL_DOCUMENTID";

/// Errors raised by dependency validation and graph mutation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DependencyError {
    /// The source or target task does not exist in the document.
    #[error("task '{0}' does not exist")]
    TaskNotFound(String),
    /// A task cannot depend on itself.
    #[error("task '{0}' cannot depend on itself")]
    SelfReference(String),
    /// Adding the edge would close a cycle; `path` shows `target -> ... -> source`.
    #[error("dependency '{source_task}' -> '{target_task}' would create a cycle: {target_task} -> {path}")]
    WouldCreateCycle {
        /// The edge source.
        source_task: String,
        /// The edge target.
        target_task: String,
        /// The existing path from target back to source.
        path: String,
    },
    /// The dependency to remove does not exist.
    #[error("dependency from '{source_task}' to '{target_task}' does not exist")]
    NoSuchDependency {
        /// The edge source.
        source_task: String,
        /// The edge target.
        target_task: String,
    },
}

/// A classified dependency target reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskRef {
    /// A task in the same document.
    Local(TaskId),
    /// A task in another document of the workspace.
    External {
        /// The target document.
        document_id: DocumentId,
        /// The task within that document.
        task_id: TaskId,
    },
    /// A raw reference that could not be resolved; preserved verbatim.
    Unresolved(String),
}

impl TaskRef {
    /// The referenced task id, when known.
    pub fn task_id(&self) -> Option<&TaskId> {
        match self {
            TaskRef::Local(id) => Some(id),
            TaskRef::External { task_id, .. } => Some(task_id),
            TaskRef::Unresolved(_) => None,
        }
    }

    /// True for same-document references.
    pub fn is_local(&self) -> bool {
        matches!(self, TaskRef::Local(_))
    }

    /// Index classification name (`local` / `external` / `unresolved`).
    pub fn kind_name(&self) -> &'static str {
        match self {
            TaskRef::Local(_) => "local",
            TaskRef::External { .. } => "external",
            TaskRef::Unresolved(_) => "unresolved",
        }
    }

    /// Classifies a parsed `TaskDependency`.
    pub fn from_dependency(dep: &TaskDependency) -> Self {
        let tid = dep.task_id.trim();
        if tid.is_empty() {
            return TaskRef::Unresolved(dep.task_id.clone());
        }
        if let Some(raw) = dep.raw_xml.as_deref() {
            if let Some(doc) = external_document_attr(raw) {
                if !doc.trim().is_empty() {
                    return TaskRef::External {
                        document_id: DocumentId::new(doc),
                        task_id: TaskId::new(tid),
                    };
                }
            }
        }
        TaskRef::Local(TaskId::new(tid))
    }

    /// Converts back into the XML-facing `TaskDependency` representation.
    /// External refs serialize as a native `<DEPENDENCY>` element carrying
    /// the custom `MTDL_DOCUMENTID` attribute (stored in `raw_xml` so the
    /// mapper writes it back losslessly).
    pub fn to_dependency(&self, dependency_type: u8) -> TaskDependency {
        match self {
            TaskRef::Local(id) => TaskDependency {
                task_id: id.as_str().to_string(),
                dependency_type,
                raw_xml: None,
            },
            TaskRef::External {
                document_id,
                task_id,
            } => {
                let raw = format!(
                    "<DEPENDENCY {}=\"{}\"><TASKID>{}</TASKID><DEPENDENCYTYPE>{}</DEPENDENCYTYPE></DEPENDENCY>",
                    EXTERNAL_DOC_ATTR,
                    escape_xml(document_id.as_str()),
                    escape_xml(task_id.as_str()),
                    dependency_type
                );
                TaskDependency {
                    task_id: task_id.as_str().to_string(),
                    dependency_type,
                    raw_xml: Some(raw),
                }
            }
            TaskRef::Unresolved(raw_id) => TaskDependency {
                task_id: raw_id.clone(),
                dependency_type,
                raw_xml: None,
            },
        }
    }
}

/// Extracts the `MTDL_DOCUMENTID` attribute from a serialized
/// `<DEPENDENCY>` element (stored in `TaskDependency::raw_xml`).
fn external_document_attr(raw_xml: &str) -> Option<String> {
    let wrapped = format!("<MTDLRAW>{}</MTDLRAW>", raw_xml);
    let doc = parse_xml(wrapped.as_bytes()).ok()?;
    let dep = doc.root.first_child_by_tag("DEPENDENCY")?;
    dep.get_attr(EXTERNAL_DOC_ATTR).map(|s| s.to_string())
}

/// An in-memory dependency graph with forward and reverse adjacency.
///
/// Only `Local` edges participate in cycle detection; `External` edges are
/// tracked separately (cycle analysis across documents is out of scope for
/// a single-document graph) and `Unresolved` refs are ignored.
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    /// source -> targets it depends on.
    forward: HashMap<TaskId, Vec<TaskId>>,
    /// target -> sources that depend on it ("target blocks source").
    reverse: HashMap<TaskId, Vec<TaskId>>,
    /// (source task, target document, target task).
    external: Vec<(TaskId, DocumentId, TaskId)>,
    /// All known task ids.
    nodes: HashSet<TaskId>,
}

impl DependencyGraph {
    /// Creates an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the graph from a task tree.
    pub fn from_tree(tree: &TaskTree) -> Self {
        let mut g = Self::new();
        for task in tree.iter() {
            g.nodes.insert(task.id.clone());
        }
        for task in tree.iter() {
            for dep in &task.dependencies {
                match TaskRef::from_dependency(dep) {
                    TaskRef::Local(target) => {
                        g.forward
                            .entry(task.id.clone())
                            .or_default()
                            .push(target.clone());
                        g.reverse
                            .entry(target)
                            .or_default()
                            .push(task.id.clone());
                    }
                    TaskRef::External {
                        document_id,
                        task_id,
                    } => {
                        g.external
                            .push((task.id.clone(), document_id, task_id));
                    }
                    TaskRef::Unresolved(_) => {}
                }
            }
        }
        g
    }

    /// Registers a node (task) without edges.
    pub fn add_node(&mut self, id: &TaskId) {
        self.nodes.insert(id.clone());
    }

    /// True if the id is a known task.
    pub fn contains(&self, id: &TaskId) -> bool {
        self.nodes.contains(id)
    }

    /// Tasks that `id` depends on (forward lookup).
    pub fn dependencies_of(&self, id: &TaskId) -> &[TaskId] {
        self.forward
            .get(id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Tasks that depend on `id` (reverse lookup: `id` blocks them).
    pub fn dependents_of(&self, id: &TaskId) -> &[TaskId] {
        self.reverse
            .get(id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Cross-document edges.
    pub fn external_refs(&self) -> &[(TaskId, DocumentId, TaskId)] {
        &self.external
    }

    /// Adds an edge `source depends-on target`.
    ///
    /// Ok(true) = edge added; Ok(false) = edge already existed
    /// (idempotent); Err = rejected (unknown task, self-reference, or the
    /// edge would create a cycle).
    pub fn add_dependency(
        &mut self,
        source: &TaskId,
        target: &TaskId,
    ) -> Result<bool, DependencyError> {
        self.validate_edge(source, target)?;
        let fwd = self.forward.entry(source.clone()).or_default();
        if fwd.iter().any(|t| t == target) {
            return Ok(false);
        }
        fwd.push(target.clone());
        self.reverse
            .entry(target.clone())
            .or_default()
            .push(source.clone());
        Ok(true)
    }

    /// Removes an edge. Ok(true) = removed, Ok(false) = not present.
    pub fn remove_dependency(
        &mut self,
        source: &TaskId,
        target: &TaskId,
    ) -> Result<bool, DependencyError> {
        self.check_nodes(source, target)?;
        let mut removed = false;
        if let Some(fwd) = self.forward.get_mut(source) {
            if let Some(pos) = fwd.iter().position(|t| t == target) {
                fwd.remove(pos);
                removed = true;
            }
        }
        if removed {
            if let Some(rev) = self.reverse.get_mut(target) {
                if let Some(pos) = rev.iter().position(|s| s == source) {
                    rev.remove(pos);
                }
            }
        }
        Ok(removed)
    }

    /// Validates an edge without mutating: existence, self-reference, cycle.
    pub fn validate_edge(
        &self,
        source: &TaskId,
        target: &TaskId,
    ) -> Result<(), DependencyError> {
        self.check_nodes(source, target)?;
        if source == target {
            return Err(DependencyError::SelfReference(source.to_string()));
        }
        // Adding source->target creates a cycle iff target can already
        // reach source (target -> ... -> source).
        if let Some(path) = self.find_path(target, source) {
            let path_str = path
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(DependencyError::WouldCreateCycle {
                source_task: source.to_string(),
                target_task: target.to_string(),
                path: path_str,
            });
        }
        Ok(())
    }

    fn check_nodes(
        &self,
        source: &TaskId,
        target: &TaskId,
    ) -> Result<(), DependencyError> {
        if !self.nodes.contains(source) {
            return Err(DependencyError::TaskNotFound(source.to_string()));
        }
        if !self.nodes.contains(target) {
            return Err(DependencyError::TaskNotFound(target.to_string()));
        }
        Ok(())
    }

    /// DFS path from `from` to `to` over forward edges (exclusive of the
    /// `from` node, inclusive of `to`), or None if unreachable.
    pub fn find_path(&self, from: &TaskId, to: &TaskId) -> Option<Vec<TaskId>> {
        let mut visited = HashSet::new();
        let mut path = Vec::new();
        self.dfs(from, to, &mut visited, &mut path)
    }

    fn dfs(
        &self,
        current: &TaskId,
        goal: &TaskId,
        visited: &mut HashSet<TaskId>,
        path: &mut Vec<TaskId>,
    ) -> Option<Vec<TaskId>> {
        if !visited.insert(current.clone()) {
            return None;
        }
        for next in self.dependencies_of(current) {
            path.push(next.clone());
            if next == goal {
                return Some(path.clone());
            }
            if let Some(found) = self.dfs(next, goal, visited, path) {
                return Some(found);
            }
            path.pop();
        }
        None
    }
}

/// Checked constructor helpers shared by the undoable commands.
fn validate_against_tree(
    tree: &TaskTree,
    source: &TaskId,
    target: &TaskId,
) -> Result<(), DependencyError> {
    let graph = DependencyGraph::from_tree(tree);
    graph.validate_edge(source, target)
}

/// Undoable command: `source` gains a dependency on `target`.
#[derive(Debug, Clone)]
pub struct AddDependencyCommand {
    /// The depending task.
    pub source: TaskId,
    /// The task depended upon.
    pub target: TaskId,
    /// Native DEPENDENCYTYPE value (0 = Finish-to-Start by convention).
    pub dependency_type: u8,
    /// Whether the command has been executed.
    pub executed: bool,
}

impl AddDependencyCommand {
    /// Validates against the current tree (existence, self-reference,
    /// cycles) and builds the command. This is the ONLY supported way to
    /// create the command — an unvalidated edge could corrupt the graph.
    pub fn new(
        tree: &TaskTree,
        source: TaskId,
        target: TaskId,
        dependency_type: u8,
    ) -> Result<Self, DependencyError> {
        validate_against_tree(tree, &source, &target)?;
        Ok(Self {
            source,
            target,
            dependency_type,
            executed: false,
        })
    }
}

impl UndoableCommand for AddDependencyCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        if let Some(task) = tree.get_mut(&self.source) {
            let target_str = self.target.as_str();
            if !task.dependencies.iter().any(|d| d.task_id == target_str) {
                task.dependencies.push(
                    TaskRef::Local(self.target.clone()).to_dependency(self.dependency_type),
                );
            }
        }
        self.executed = true;
        format!("Add dependency {} -> {}", self.source, self.target)
    }

    fn undo(&mut self, tree: &mut TaskTree) {
        if let Some(task) = tree.get_mut(&self.source) {
            let target_str = self.target.as_str();
            task.dependencies.retain(|d| d.task_id != target_str);
        }
        self.executed = false;
    }

    fn redo(&mut self, tree: &mut TaskTree) {
        self.execute(tree);
    }

    fn description(&self) -> &str {
        "Add dependency"
    }
}

/// Undoable command: remove the dependency `source -> target`.
#[derive(Debug, Clone)]
pub struct RemoveDependencyCommand {
    /// The depending task.
    pub source: TaskId,
    /// The task depended upon.
    pub target: TaskId,
    /// Snapshot of the removed dependency and its original position.
    pub removed: Option<(usize, TaskDependency)>,
    /// Whether the command has been executed.
    pub executed: bool,
}

impl RemoveDependencyCommand {
    /// Validates that both tasks exist and that the edge is present.
    pub fn new(
        tree: &TaskTree,
        source: TaskId,
        target: TaskId,
    ) -> Result<Self, DependencyError> {
        if tree.get(&source).is_none() {
            return Err(DependencyError::TaskNotFound(source.to_string()));
        }
        if tree.get(&target).is_none() {
            return Err(DependencyError::TaskNotFound(target.to_string()));
        }
        let target_str = target.as_str();
        let exists = tree
            .get(&source)
            .map(|t| t.dependencies.iter().any(|d| d.task_id == target_str))
            .unwrap_or(false);
        if !exists {
            return Err(DependencyError::NoSuchDependency {
                source_task: source.to_string(),
                target_task: target.to_string(),
            });
        }
        Ok(Self {
            source,
            target,
            removed: None,
            executed: false,
        })
    }
}

impl UndoableCommand for RemoveDependencyCommand {
    fn execute(&mut self, tree: &mut TaskTree) -> String {
        if let Some(task) = tree.get_mut(&self.source) {
            let target_str = self.target.as_str();
            if let Some(pos) = task
                .dependencies
                .iter()
                .position(|d| d.task_id == target_str)
            {
                let dep = task.dependencies.remove(pos);
                self.removed = Some((pos, dep));
            }
        }
        self.executed = true;
        format!("Remove dependency {} -> {}", self.source, self.target)
    }

    fn undo(&mut self, tree: &mut TaskTree) {
        if let Some((pos, dep)) = self.removed.clone() {
            if let Some(task) = tree.get_mut(&self.source) {
                let pos = pos.min(task.dependencies.len());
                task.dependencies.insert(pos, dep);
            }
        }
        self.executed = false;
    }

    fn redo(&mut self, tree: &mut TaskTree) {
        self.execute(tree);
    }

    fn description(&self) -> &str {
        "Remove dependency"
    }
}

/// One row of the `task_dependencies` index table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyIndexRow {
    /// The depending task's key.
    pub task_key: String,
    /// The owning document id.
    pub document_id: String,
    /// The referenced task key. For external refs this is `docid:taskid`.
    pub depends_on_key: String,
    /// `local` / `external` / `unresolved`.
    pub dep_type: String,
    /// Raw reference detail (document id for external refs).
    pub raw_ref: Option<String>,
}

/// Computes the forward index rows for one task's dependencies.
pub fn index_rows(document_id: &str, task_key: &str, task: &Task) -> Vec<DependencyIndexRow> {
    task.dependencies
        .iter()
        .map(|dep| {
            let task_ref = TaskRef::from_dependency(dep);
            let (depends_on_key, raw_ref) = match &task_ref {
                TaskRef::Local(id) => (id.as_str().to_string(), None),
                TaskRef::External {
                    document_id: doc,
                    task_id,
                } => (
                    format!("{}:{}", doc, task_id),
                    Some(doc.as_str().to_string()),
                ),
                TaskRef::Unresolved(raw) => (raw.clone(), None),
            };
            DependencyIndexRow {
                task_key: task_key.to_string(),
                document_id: document_id.to_string(),
                depends_on_key,
                dep_type: task_ref.kind_name().to_string(),
                raw_ref,
            }
        })
        .collect()
}

/// Replaces the `task_dependencies` rows for one task in the index DB.
/// Reverse lookups are served by the `idx_task_dependencies_target` index
/// on `(document_id, depends_on_key)` — no duplicate storage needed.
pub fn populate_task_dependencies(
    conn: &rusqlite::Connection,
    document_id: &str,
    task_key: &str,
    task: &Task,
) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM task_dependencies WHERE document_id = ?1 AND task_key = ?2",
        rusqlite::params![document_id, task_key],
    )?;
    let rows = index_rows(document_id, task_key, task);
    for row in &rows {
        conn.execute(
            "INSERT OR REPLACE INTO task_dependencies \
             (task_key, document_id, depends_on_key, dep_type, raw_ref) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                row.task_key,
                row.document_id,
                row.depends_on_key,
                row.dep_type,
                row.raw_ref
            ],
        )?;
    }
    Ok(rows.len())
}

/// Reverse lookup: which tasks in `document_id` depend on `task_key`?
/// Returns `(task_key, dep_type)` pairs.
pub fn reverse_lookup(
    conn: &rusqlite::Connection,
    document_id: &str,
    task_key: &str,
) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT task_key, dep_type FROM task_dependencies \
         WHERE document_id = ?1 AND depends_on_key = ?2 ORDER BY task_key",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![document_id, task_key], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::command::UndoRedoManager;

    fn make_tree(ids: &[&str]) -> TaskTree {
        let mut tree = TaskTree::new();
        for id in ids {
            let mut t = Task::new(TaskId::new(*id));
            t.title = format!("Task {id}");
            tree.add_task(t);
            tree.add_root_id(TaskId::new(*id));
        }
        tree
    }

    #[test]
    fn task_ref_classification() {
        let local = TaskDependency {
            task_id: "42".into(),
            dependency_type: 0,
            raw_xml: None,
        };
        assert_eq!(
            TaskRef::from_dependency(&local),
            TaskRef::Local(TaskId::new("42"))
        );

        let external = TaskRef::External {
            document_id: DocumentId::new("doc-2"),
            task_id: TaskId::new("7"),
        }
        .to_dependency(1);
        assert_eq!(external.dependency_type, 1);
        assert_eq!(
            TaskRef::from_dependency(&external),
            TaskRef::External {
                document_id: DocumentId::new("doc-2"),
                task_id: TaskId::new("7"),
            }
        );

        let unresolved = TaskDependency {
            task_id: "".into(),
            dependency_type: 0,
            raw_xml: None,
        };
        assert!(matches!(
            TaskRef::from_dependency(&unresolved),
            TaskRef::Unresolved(_)
        ));
    }

    #[test]
    fn graph_forward_and_reverse() {
        let mut g = DependencyGraph::new();
        g.add_node(&TaskId::new("1"));
        g.add_node(&TaskId::new("2"));
        g.add_node(&TaskId::new("3"));
        assert!(g.add_dependency(&TaskId::new("1"), &TaskId::new("2")).unwrap());
        assert!(g.add_dependency(&TaskId::new("3"), &TaskId::new("2")).unwrap());
        // Idempotent
        assert!(!g.add_dependency(&TaskId::new("1"), &TaskId::new("2")).unwrap());
        assert_eq!(
            g.dependencies_of(&TaskId::new("1")),
            &[TaskId::new("2")]
        );
        assert_eq!(
            g.dependents_of(&TaskId::new("2")).len(),
            2,
            "reverse lookup sees both dependents"
        );
        assert!(g
            .remove_dependency(&TaskId::new("1"), &TaskId::new("2"))
            .unwrap());
        assert!(!g
            .remove_dependency(&TaskId::new("1"), &TaskId::new("2"))
            .unwrap());
        assert_eq!(g.dependents_of(&TaskId::new("2")).len(), 1);
    }

    #[test]
    fn graph_rejects_self_unknown_and_cycles() {
        let mut g = DependencyGraph::new();
        for id in ["a", "b", "c"] {
            g.add_node(&TaskId::new(id));
        }
        assert_eq!(
            g.add_dependency(&TaskId::new("a"), &TaskId::new("a")),
            Err(DependencyError::SelfReference("a".into()))
        );
        assert_eq!(
            g.add_dependency(&TaskId::new("a"), &TaskId::new("zz")),
            Err(DependencyError::TaskNotFound("zz".into()))
        );
        g.add_dependency(&TaskId::new("a"), &TaskId::new("b")).unwrap();
        g.add_dependency(&TaskId::new("b"), &TaskId::new("c")).unwrap();
        let err = g
            .add_dependency(&TaskId::new("c"), &TaskId::new("a"))
            .unwrap_err();
        match err {
            DependencyError::WouldCreateCycle { ref path, .. } => {
                // find_path excludes the start node: a -> (b -> c)
                assert!(path.contains("b -> c"), "path was: {path}");
                assert!(err.to_string().contains("a -> b -> c"));
            }
            other => panic!("expected cycle error, got {other:?}"),
        }
        // Direct 2-cycle also rejected
        assert!(matches!(
            g.add_dependency(&TaskId::new("b"), &TaskId::new("a")),
            Err(DependencyError::WouldCreateCycle { .. })
        ));
        // Diamond (non-cycle) is fine: a->b, a->c, b->d, c->d
        let mut d = DependencyGraph::new();
        for id in ["a", "b", "c", "dd"] {
            d.add_node(&TaskId::new(id));
        }
        d.add_dependency(&TaskId::new("a"), &TaskId::new("b")).unwrap();
        d.add_dependency(&TaskId::new("a"), &TaskId::new("c")).unwrap();
        d.add_dependency(&TaskId::new("b"), &TaskId::new("dd")).unwrap();
        assert!(d
            .add_dependency(&TaskId::new("c"), &TaskId::new("dd"))
            .unwrap());
    }

    #[test]
    fn graph_from_tree_classifies_edges() {
        let mut tree = make_tree(&["1", "2", "3"]);
        tree.get_mut(&TaskId::new("1")).unwrap().dependencies = vec![
            TaskRef::Local(TaskId::new("2")).to_dependency(0),
            TaskRef::External {
                document_id: DocumentId::new("doc-x"),
                task_id: TaskId::new("9"),
            }
            .to_dependency(0),
            TaskDependency {
                task_id: "".into(),
                dependency_type: 0,
                raw_xml: None,
            },
        ];
        let g = DependencyGraph::from_tree(&tree);
        assert_eq!(g.dependencies_of(&TaskId::new("1")), &[TaskId::new("2")]);
        assert_eq!(g.dependents_of(&TaskId::new("2")), &[TaskId::new("1")]);
        assert_eq!(g.external_refs().len(), 1);
        // Cycle check still works on tree-derived graphs.
        assert!(matches!(
            g.validate_edge(&TaskId::new("2"), &TaskId::new("1")),
            Err(DependencyError::WouldCreateCycle { .. })
        ));
    }

    #[test]
    fn add_command_validated_construction() {
        let tree = make_tree(&["1", "2"]);
        assert!(AddDependencyCommand::new(
            &tree,
            TaskId::new("1"),
            TaskId::new("2"),
            0
        )
        .is_ok());
        assert_eq!(
            AddDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("1"), 0)
                .unwrap_err(),
            DependencyError::SelfReference("1".into())
        );
        assert!(matches!(
            AddDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("99"), 0),
            Err(DependencyError::TaskNotFound(_))
        ));
    }

    #[test]
    fn commands_integrate_with_undo_manager() {
        let mut tree = make_tree(&["1", "2", "3"]);
        let mut mgr = UndoRedoManager::new();

        let cmd = AddDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("2"), 0).unwrap();
        mgr.execute(Box::new(cmd), &mut tree);
        assert_eq!(tree.get(&TaskId::new("1")).unwrap().dependencies.len(), 1);

        // Now 2->1 would be a cycle; validated constructor rejects it.
        assert!(matches!(
            AddDependencyCommand::new(&tree, TaskId::new("2"), TaskId::new("1"), 0),
            Err(DependencyError::WouldCreateCycle { .. })
        ));

        mgr.undo(&mut tree);
        assert!(tree.get(&TaskId::new("1")).unwrap().dependencies.is_empty());
        // After undo the reverse edge is legal again.
        assert!(AddDependencyCommand::new(&tree, TaskId::new("2"), TaskId::new("1"), 0).is_ok());

        mgr.redo(&mut tree);
        assert_eq!(tree.get(&TaskId::new("1")).unwrap().dependencies.len(), 1);

        // Remove command round-trips through undo, restoring position.
        let rm = RemoveDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("2")).unwrap();
        mgr.execute(Box::new(rm), &mut tree);
        assert!(tree.get(&TaskId::new("1")).unwrap().dependencies.is_empty());
        mgr.undo(&mut tree);
        let deps = &tree.get(&TaskId::new("1")).unwrap().dependencies;
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].task_id, "2");
    }

    #[test]
    fn remove_command_requires_existing_edge() {
        let tree = make_tree(&["1", "2"]);
        assert!(matches!(
            RemoveDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("2")),
            Err(DependencyError::NoSuchDependency { .. })
        ));
        assert!(matches!(
            RemoveDependencyCommand::new(&tree, TaskId::new("1"), TaskId::new("77")),
            Err(DependencyError::TaskNotFound(_))
        ));
    }

    #[test]
    fn index_rows_classify_and_populate() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE task_dependencies (
                task_key TEXT NOT NULL, document_id TEXT NOT NULL,
                depends_on_key TEXT NOT NULL, dep_type TEXT NOT NULL DEFAULT 'local',
                raw_ref TEXT, PRIMARY KEY (task_key, document_id, depends_on_key));",
        )
        .unwrap();

        let mut t = Task::new(TaskId::new("1"));
        t.dependencies = vec![
            TaskRef::Local(TaskId::new("2")).to_dependency(0),
            TaskRef::External {
                document_id: DocumentId::new("doc-x"),
                task_id: TaskId::new("9"),
            }
            .to_dependency(0),
        ];
        let rows = index_rows("doc-1", "1", &t);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].dep_type, "local");
        assert_eq!(rows[0].depends_on_key, "2");
        assert_eq!(rows[1].dep_type, "external");
        assert_eq!(rows[1].depends_on_key, "doc-x:9");
        assert_eq!(rows[1].raw_ref.as_deref(), Some("doc-x"));

        let n = populate_task_dependencies(&conn, "doc-1", "1", &t).unwrap();
        assert_eq!(n, 2);
        populate_task_dependencies(&conn, "doc-1", "1", &t).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_dependencies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "populate replaces instead of duplicating");

        // Reverse lookup: who depends on task 2?
        let rev = reverse_lookup(&conn, "doc-1", "2").unwrap();
        assert_eq!(rev, vec![("1".to_string(), "local".to_string())]);
        let rev_ext = reverse_lookup(&conn, "doc-1", "doc-x:9").unwrap();
        assert_eq!(rev_ext.len(), 1);
    }
}
