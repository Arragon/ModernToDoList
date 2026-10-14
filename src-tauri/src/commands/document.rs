//! M2 IPC commands: document read/write, validation, and ID allocation.
//!
//! These commands bridge the domain layer to Tauri's invoke system,
//! allowing the frontend to perform document operations through the
//! lossless XML processing pipeline.

use serde::Serialize;
use std::path::Path;

use crate::domain::id_allocator::TaskIdAllocator;
use crate::domain::mappers::{read_document_metadata, read_task};
use crate::domain::task::{Task, TaskTree};
use crate::domain::types::TaskId;
use crate::domain::validator::{validate_document, validate_task_tree, ValidationError};
use crate::domain::xml_parser::parse_xml;
use crate::domain::xml_serializer::serialize_xml;

// ── Response DTOs ─────────────────────────────────────────────────────────────

/// Response from `read_and_parse_document`.
#[derive(Debug, Clone, Serialize)]
pub struct ReadDocumentResponse {
    /// The parsed XML content (UTF-8 string of the full XML tree).
    pub xml_content: String,
    /// Document metadata (project name, next unique ID, etc.).
    pub metadata: DocumentMetadataDto,
    /// All tasks extracted from the document.
    pub tasks: Vec<TaskDto>,
    /// Encoding label detected from the file (e.g. "UTF-8", "UTF-16LE").
    pub encoding: String,
    /// Number of tasks in the document.
    pub task_count: usize,
}

/// Document metadata returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentMetadataDto {
    pub project_name: Option<String>,
    pub filename: Option<String>,
    pub next_unique_id: u64,
    pub file_version: Option<String>,
    pub app_ver: Option<String>,
    pub file_format: Option<String>,
}

/// A single task returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct TaskDto {
    pub id: String,
    pub title: String,
    pub priority: u8,
    pub risk: u8,
    pub percent_done: u8,
    pub status: String,
    pub children: Vec<String>,
}

/// Response from `validate_document_cmd`.
#[derive(Debug, Clone, Serialize)]
pub struct ValidateDocumentResponse {
    /// Whether the document is valid (no errors).
    pub valid: bool,
    /// List of validation errors (empty if valid).
    pub errors: Vec<ValidationErrorDto>,
}

/// A single validation error.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationErrorDto {
    pub kind: String,
    pub message: String,
}

/// Response from `allocate_task_id`.
#[derive(Debug, Clone, Serialize)]
pub struct AllocateTaskIdResponse {
    /// The newly allocated task ID.
    pub id: String,
    /// The updated NEXTUNIQUEID value (for saving back to XML).
    pub next_unique_id: u64,
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Reads a TDL file, detects encoding, parses XML, and returns the task tree + metadata.
///
/// This is the primary entry point for loading a document. It performs:
/// 1. File I/O (read raw bytes)
/// 2. BOM detection and encoding identification
/// 3. XML parsing into the lossless tree
/// 4. Task extraction from the XML tree
/// 5. Metadata extraction
#[tauri::command]
pub fn read_and_parse_document(path: String) -> Result<ReadDocumentResponse, String> {
    let file_path = Path::new(&path);

    // Read file bytes
    let bytes = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

    // Parse XML (handles BOM detection, encoding, etc.)
    let doc = parse_xml(&bytes)
        .map_err(|e| format!("Failed to parse XML: {}", e))?;

    // Extract metadata
    let metadata = read_document_metadata(&doc.root, doc.meta.clone());

    // Extract tasks from XML tree
    let mut tree = TaskTree::new();
    extract_tasks_recursive(&doc.root, &mut tree);

    // Build encoding label
    let encoding = doc.meta.encoding.iana_label().to_string();

    // Build task DTOs
    let tasks: Vec<TaskDto> = tree.iter().map(task_to_dto).collect();
    let task_count = tasks.len();

    // Serialize the full XML document to a UTF-8 string for the frontend
    let xml_bytes = serialize_xml(&doc);
    let xml_content = String::from_utf8_lossy(&xml_bytes).to_string();

    Ok(ReadDocumentResponse {
        xml_content,
        metadata: metadata_to_dto(&metadata),
        tasks,
        encoding,
        task_count,
    })
}

/// Serializes an XML document back to a file with the specified encoding.
///
/// The `xml_content` is the full XML string. The command parses it,
/// applies encoding metadata, and writes the result to `path`.
#[tauri::command]
pub fn serialize_and_write_document(
    xml_content: String,
    path: String,
    encoding: Option<String>,
) -> Result<(), String> {
    let file_path = Path::new(&path);

    // Parse the XML content
    let mut doc = parse_xml(xml_content.as_bytes())
        .map_err(|e| format!("Failed to parse XML content: {}", e))?;

    // Override encoding if specified
    if let Some(enc_str) = encoding {
        if let Some(enc) = resolve_encoding_label(&enc_str) {
            doc.meta.encoding = enc;
        }
    }

    // Serialize to bytes
    let bytes = serialize_xml(&doc);

    // Write to file
    std::fs::write(file_path, &bytes)
        .map_err(|e| format!("Failed to write file '{}': {}", path, e))?;

    Ok(())
}

/// Parses and returns document metadata without extracting tasks.
///
/// Lighter-weight than `read_and_parse_document` when only metadata is needed.
#[tauri::command]
pub fn get_document_metadata(path: String) -> Result<DocumentMetadataDto, String> {
    let file_path = Path::new(&path);

    let bytes = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

    let doc = parse_xml(&bytes)
        .map_err(|e| format!("Failed to parse XML: {}", e))?;

    let metadata = read_document_metadata(&doc.root, doc.meta);

    Ok(metadata_to_dto(&metadata))
}

/// Runs semantic validation on a parsed document.
///
/// Checks for ID uniqueness, orphaned references, and structural constraints.
#[tauri::command]
pub fn validate_document_cmd(path: String) -> Result<ValidateDocumentResponse, String> {
    let file_path = Path::new(&path);

    let bytes = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

    let doc = parse_xml(&bytes)
        .map_err(|e| format!("Failed to parse XML: {}", e))?;

    // Validate document structure
    let doc_errors = validate_document(&doc);

    // Extract tasks and validate task tree
    let mut tree = TaskTree::new();
    extract_tasks_recursive(&doc.root, &mut tree);
    let tree_errors = validate_task_tree(&tree);

    // Combine errors
    let mut all_errors = doc_errors;
    all_errors.extend(tree_errors);

    let errors: Vec<ValidationErrorDto> = all_errors
        .iter()
        .map(validation_error_to_dto)
        .collect();

    Ok(ValidateDocumentResponse {
        valid: errors.is_empty(),
        errors,
    })
}

/// Allocates the next collision-free task ID given a set of existing IDs.
///
/// Uses the domain's `TaskIdAllocator` to find the next available ID.
#[tauri::command]
pub fn allocate_task_id(
    existing_ids: Vec<String>,
    next_unique_id: u64,
) -> Result<AllocateTaskIdResponse, String> {
    // Build a temporary TaskTree to feed the allocator
    let mut tree = TaskTree::new();
    for id_str in &existing_ids {
        let task = Task::new(TaskId::new(id_str.as_str()));
        tree.add_task(task);
    }

    let mut allocator = TaskIdAllocator::new(&tree, next_unique_id);
    let new_id = allocator.allocate();
    let new_next_unique_id = allocator.next_unique_id();

    Ok(AllocateTaskIdResponse {
        id: new_id.as_str().to_string(),
        next_unique_id: new_next_unique_id,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Recursively extracts Task objects from the XML tree.
fn extract_tasks_recursive(elem: &crate::domain::xml_tree::XmlElement, tree: &mut TaskTree) {
    for child in elem.child_elements() {
        if child.tag == "TASK" {
            let task = read_task(child);
            let id = task.id.clone();
            tree.add_task(task);

            // Check if this is a root-level task (child of TODOLIST)
            if elem.tag == "TODOLIST" {
                tree.add_root_id(id);
            }

            // Recurse into nested tasks
            extract_tasks_recursive(child, tree);
        } else {
            // Recurse into non-TASK elements to find nested tasks
            extract_tasks_recursive(child, tree);
        }
    }
}

/// Converts a domain Task to a TaskDto for IPC.
fn task_to_dto(task: &Task) -> TaskDto {
    TaskDto {
        id: task.id.as_str().to_string(),
        title: task.title.clone(),
        priority: task.priority.value(),
        risk: task.risk,
        percent_done: task.percent_done,
        status: format!("{:?}", task.status()),
        children: task.children.iter().map(|c| c.as_str().to_string()).collect(),
    }
}

/// Converts domain DocumentMetadata to a DTO.
fn metadata_to_dto(m: &crate::domain::task::DocumentMetadata) -> DocumentMetadataDto {
    DocumentMetadataDto {
        project_name: m.project_name.clone(),
        filename: m.filename.clone(),
        next_unique_id: m.next_unique_id,
        file_version: m.file_version.clone(),
        app_ver: m.app_ver.clone(),
        file_format: m.file_format.clone(),
    }
}

/// Converts a ValidationError to a DTO.
fn validation_error_to_dto(e: &ValidationError) -> ValidationErrorDto {
    match e {
        ValidationError::DuplicateTaskId(id) => ValidationErrorDto {
            kind: "DuplicateTaskId".into(),
            message: format!("Duplicate task ID: {}", id),
        },
        ValidationError::OrphanedChildRef { parent_id, child_id } => ValidationErrorDto {
            kind: "OrphanedChildRef".into(),
            message: format!("Task {} references non-existent child {}", parent_id, child_id),
        },
        ValidationError::OrphanedDependency { task_id, dep_task_id } => ValidationErrorDto {
            kind: "OrphanedDependency".into(),
            message: format!("Task {} depends on non-existent task {}", task_id, dep_task_id),
        },
        ValidationError::InvalidRootElement(tag) => ValidationErrorDto {
            kind: "InvalidRootElement".into(),
            message: format!("Expected TODOLIST root, got <{}>", tag),
        },
        ValidationError::InvalidNextUniqueId(msg) => ValidationErrorDto {
            kind: "InvalidNextUniqueId".into(),
            message: format!("Invalid NEXTUNIQUEID: {}", msg),
        },
        ValidationError::MissingRequiredAttr { task_id, attr } => ValidationErrorDto {
            kind: "MissingRequiredAttr".into(),
            message: format!("Task {} missing required attribute {}", task_id, attr),
        },
    }
}

/// Resolves an encoding label string to an XmlEncoding variant.
fn resolve_encoding_label(label: &str) -> Option<crate::domain::encoding::XmlEncoding> {
    use crate::domain::encoding::XmlEncoding;
    match label.to_uppercase().as_str() {
        "UTF-8" | "UTF8" => Some(XmlEncoding::Utf8),
        "UTF-8-BOM" | "UTF8BOM" => Some(XmlEncoding::Utf8Bom),
        "UTF-16LE" | "UTF16LE" => Some(XmlEncoding::Utf16Le),
        "UTF-16BE" | "UTF16BE" => Some(XmlEncoding::Utf16Be),
        _ => None,
    }
}
