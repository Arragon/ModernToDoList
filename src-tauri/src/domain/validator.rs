//! Semantic XML/document validator and structured error model.
//!
//! Validates the parsed XML document structure and domain model integrity.
//! This goes beyond XML well-formedness (handled by the parser) to check
//! semantic constraints like ID uniqueness, required fields, etc.

use std::collections::HashSet;

use super::task::TaskTree;
use super::xml_tree::XmlDocument;

/// Structured validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// A task ID is duplicated within the document.
    DuplicateTaskId(String),
    /// A task references a child ID that doesn't exist.
    OrphanedChildRef { parent_id: String, child_id: String },
    /// A dependency references a task ID that doesn't exist.
    OrphanedDependency { task_id: String, dep_task_id: String },
    /// The root element is not a TODOLIST.
    InvalidRootElement(String),
    /// NEXTUNIQUEID is missing or invalid.
    InvalidNextUniqueId(String),
    /// A required attribute is missing from a task.
    MissingRequiredAttr { task_id: String, attr: String },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateTaskId(id) => write!(f, "duplicate task ID: {}", id),
            Self::OrphanedChildRef { parent_id, child_id } => {
                write!(f, "task {} references non-existent child {}", parent_id, child_id)
            }
            Self::OrphanedDependency { task_id, dep_task_id } => {
                write!(f, "task {} depends on non-existent task {}", task_id, dep_task_id)
            }
            Self::InvalidRootElement(tag) => write!(f, "expected TODOLIST root, got <{}>", tag),
            Self::InvalidNextUniqueId(msg) => write!(f, "invalid NEXTUNIQUEID: {}", msg),
            Self::MissingRequiredAttr { task_id, attr } => {
                write!(f, "task {} missing required attribute {}", task_id, attr)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Validates a parsed XML document for semantic correctness.
pub fn validate_document(doc: &XmlDocument) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Check root element
    if doc.root.tag != "TODOLIST" {
        errors.push(ValidationError::InvalidRootElement(doc.root.tag.clone()));
    }

    // Check NEXTUNIQUEID
    if let Some(nuid) = doc.root.get_attr("NEXTUNIQUEID") {
        if nuid.parse::<u64>().is_err() {
            errors.push(ValidationError::InvalidNextUniqueId(format!(
                "cannot parse '{}' as integer",
                nuid
            )));
        }
    } else {
        errors.push(ValidationError::InvalidNextUniqueId("missing".into()));
    }

    errors
}

/// Validates a TaskTree for semantic correctness.
pub fn validate_task_tree(tree: &TaskTree) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let all_ids: HashSet<&str> = tree.iter().map(|t| t.id.as_str()).collect();

    // Check for duplicate IDs (already prevented by HashMap, but check children refs)
    for task in tree.iter() {
        // Check child references
        for child_id in &task.children {
            if !all_ids.contains(child_id.as_str()) {
                errors.push(ValidationError::OrphanedChildRef {
                    parent_id: task.id.as_str().to_string(),
                    child_id: child_id.as_str().to_string(),
                });
            }
        }

        // Check dependency references
        for dep in &task.dependencies {
            if !dep.task_id.is_empty() && !all_ids.contains(dep.task_id.as_str()) {
                errors.push(ValidationError::OrphanedDependency {
                    task_id: task.id.as_str().to_string(),
                    dep_task_id: dep.task_id.clone(),
                });
            }
        }

        // Check required attributes
        if task.title.is_empty() {
            // Title is not strictly required but worth noting
        }
    }

    errors
}

/// Structured XML/encoding parse error model (RD-M2-028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlCoreError {
    /// The input encoding is not supported.
    UnsupportedEncoding(String),
    /// The XML declaration is malformed.
    MalformedDeclaration(String),
    /// The XML content is not well-formed.
    NotWellFormed(String),
    /// The document structure is invalid (semantic error).
    InvalidStructure(Vec<ValidationError>),
    /// An I/O error occurred.
    IoError(String),
}

impl std::fmt::Display for XmlCoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedEncoding(e) => write!(f, "unsupported encoding: {}", e),
            Self::MalformedDeclaration(m) => write!(f, "malformed XML declaration: {}", m),
            Self::NotWellFormed(m) => write!(f, "XML not well-formed: {}", m),
            Self::InvalidStructure(errs) => {
                write!(f, "invalid structure ({} errors):", errs.len())?;
                for e in errs {
                    write!(f, "\n  - {}", e)?;
                }
                Ok(())
            }
            Self::IoError(m) => write!(f, "I/O error: {}", m),
        }
    }
}

impl std::error::Error for XmlCoreError {}

impl From<super::xml_parser::XmlParseError> for XmlCoreError {
    fn from(e: super::xml_parser::XmlParseError) -> Self {
        XmlCoreError::NotWellFormed(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::TaskId;
    use crate::domain::encoding::XmlEncodingMeta;
    use crate::domain::xml_tree::XmlElement;

    #[test]
    fn validate_good_document() {
        let mut root = XmlElement::new("TODOLIST");
        root.set_attr("NEXTUNIQUEID", "10");
        let doc = XmlDocument::new(XmlEncodingMeta::default(), root);
        let errors = validate_document(&doc);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_wrong_root() {
        let root = XmlElement::new("WRONG");
        let doc = XmlDocument::new(XmlEncodingMeta::default(), root);
        let errors = validate_document(&doc);
        assert!(errors.iter().any(|e| matches!(e, ValidationError::InvalidRootElement(_))));
    }

    #[test]
    fn validate_missing_next_unique_id() {
        let root = XmlElement::new("TODOLIST");
        let doc = XmlDocument::new(XmlEncodingMeta::default(), root);
        let errors = validate_document(&doc);
        assert!(errors.iter().any(|e| matches!(e, ValidationError::InvalidNextUniqueId(_))));
    }

    #[test]
    fn validate_orphaned_child_ref() {
        use crate::domain::task::{Task, TaskTree};
        let mut tree = TaskTree::new();
        let mut task = Task::new(TaskId::new("1"));
        task.children.push(TaskId::new("99")); // doesn't exist
        tree.add_task(task);
        let errors = validate_task_tree(&tree);
        assert!(errors.iter().any(|e| matches!(e, ValidationError::OrphanedChildRef { .. })));
    }

    #[test]
    fn validate_orphaned_dependency() {
        use crate::domain::task::{Task, TaskDependency, TaskTree};
        let mut tree = TaskTree::new();
        let mut task = Task::new(TaskId::new("1"));
        task.dependencies.push(TaskDependency {
            task_id: "99".into(),
            dependency_type: 0,
            raw_xml: None,
        });
        tree.add_task(task);
        let errors = validate_task_tree(&tree);
        assert!(errors.iter().any(|e| matches!(e, ValidationError::OrphanedDependency { .. })));
    }
}
