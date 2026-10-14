//! Document-aware Task ID allocator with collision detection.
//!
//! The TDL format uses integer task IDs. The allocator ensures:
//! - New IDs are unique within a document
//! - IDs are allocated sequentially from NEXTUNIQUEID
//! - Collision detection prevents reuse of existing IDs

use std::collections::HashSet;

use super::task::TaskTree;
use super::types::TaskId;

/// Allocates unique task IDs within a document.
///
/// Tracks the next available ID and all existing IDs to prevent collisions.
#[derive(Debug, Clone)]
pub struct TaskIdAllocator {
    /// The next ID to allocate (from NEXTUNIQUEID).
    next_id: u64,
    /// Set of all existing IDs in the document.
    existing: HashSet<String>,
}

impl TaskIdAllocator {
    /// Creates a new allocator from a TaskTree and the document's NEXTUNIQUEID.
    pub fn new(tree: &TaskTree, next_unique_id: u64) -> Self {
        let mut existing = HashSet::new();
        for task in tree.iter() {
            existing.insert(task.id.as_str().to_string());
        }
        Self {
            next_id: next_unique_id,
            existing,
        }
    }

    /// Allocates the next unique task ID.
    ///
    /// Returns a new TaskId that is guaranteed not to collide with any
    /// existing ID in the document.
    pub fn allocate(&mut self) -> TaskId {
        loop {
            let id = self.next_id;
            self.next_id += 1;
            let id_str = id.to_string();
            if !self.existing.contains(&id_str) {
                self.existing.insert(id_str.clone());
                return TaskId::new(id_str);
            }
            // If collision, keep incrementing
        }
    }

    /// Returns the current NEXTUNIQUEID value (for saving back to XML).
    pub fn next_unique_id(&self) -> u64 {
        self.next_id
    }

    /// Registers an existing ID (e.g., when adding an externally-created task).
    pub fn register(&mut self, id: &TaskId) {
        self.existing.insert(id.as_str().to_string());
    }

    /// Returns true if the given ID already exists.
    pub fn exists(&self, id: &TaskId) -> bool {
        self.existing.contains(id.as_str())
    }

    /// Returns the number of existing IDs.
    pub fn count(&self) -> usize {
        self.existing.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::task::{Task, TaskTree};

    #[test]
    fn allocate_sequential() {
        let tree = TaskTree::new();
        let mut alloc = TaskIdAllocator::new(&tree, 1);
        assert_eq!(alloc.allocate().as_str(), "1");
        assert_eq!(alloc.allocate().as_str(), "2");
        assert_eq!(alloc.allocate().as_str(), "3");
        assert_eq!(alloc.next_unique_id(), 4);
    }

    #[test]
    fn allocate_skips_existing() {
        let mut tree = TaskTree::new();
        tree.add_task(Task::new(TaskId::new("5")));
        let mut alloc = TaskIdAllocator::new(&tree, 1);
        // Should skip 5 since it exists
        let _id1 = alloc.allocate(); // 1
        let _id2 = alloc.allocate(); // 2
        let _id3 = alloc.allocate(); // 3
        let _id4 = alloc.allocate(); // 4
        let id5 = alloc.allocate(); // 5 would collide, skip to 6
        assert_eq!(id5.as_str(), "6");
    }

    #[test]
    fn register_prevents_collision() {
        let tree = TaskTree::new();
        let mut alloc = TaskIdAllocator::new(&tree, 1);
        alloc.register(&TaskId::new("3"));
        let _id1 = alloc.allocate(); // 1
        let _id2 = alloc.allocate(); // 2
        let id3 = alloc.allocate(); // 3 collides, skip to 4
        assert_eq!(id3.as_str(), "4");
    }

    #[test]
    fn exists_check() {
        let mut tree = TaskTree::new();
        tree.add_task(Task::new(TaskId::new("42")));
        let alloc = TaskIdAllocator::new(&tree, 1);
        assert!(alloc.exists(&TaskId::new("42")));
        assert!(!alloc.exists(&TaskId::new("99")));
    }
}
