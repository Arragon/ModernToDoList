//! XML-to-SQLite indexer for ModernToDoList 2.0
//!
//! Reads parsed task documents and populates the SQLite index tables
//! (task_index, task_tags, task_participants, task_dependencies).

use rusqlite::Connection;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use thiserror::Error;

use crate::domain::fingerprint::FileFingerprint;
use crate::domain::mappers::read_task;
use crate::domain::task::Task;
use crate::domain::xml_parser::parse_xml;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("XML parse error: {0}")]
    XmlParse(String),

    #[error("File read error: {0}")]
    FileRead(String),

    #[error("Index operation cancelled")]
    Cancelled,
}

pub type IndexResult<T> = Result<T, IndexError>;

/// Progress information for index operations.
#[derive(Debug, Clone)]
pub struct IndexProgress {
    pub total_files: usize,
    pub processed_files: usize,
    pub total_tasks: usize,
    pub current_file: String,
}

/// Index a single XML/TDL document into the SQLite database.
///
/// Clears existing task data for the given document_id and re-indexes
/// all tasks from the XML file.
pub fn index_document(
    conn: &Connection,
    document_id: &str,
    file_path: &Path,
) -> IndexResult<usize> {
    let content = std::fs::read(file_path)
        .map_err(|e| IndexError::FileRead(format!("{}: {}", file_path.display(), e)))?;

    let xml_doc = parse_xml(&content)
        .map_err(|e| IndexError::XmlParse(format!("{}: {}", file_path.display(), e)))?;

    // Clear existing task data for this document
    conn.execute("DELETE FROM task_index WHERE document_id = ?1", [document_id])?;
    conn.execute("DELETE FROM task_tags WHERE document_id = ?1", [document_id])?;
    conn.execute("DELETE FROM task_participants WHERE document_id = ?1", [document_id])?;
    conn.execute("DELETE FROM task_dependencies WHERE document_id = ?1", [document_id])?;

    // Extract tasks from the XML tree
    let tasks = extract_tasks_from_xml(&xml_doc);
    let mut task_count = 0;

    for task in &tasks {
        insert_task_index(conn, document_id, task)?;
        task_count += 1;
    }

    Ok(task_count)
}

/// Extract all tasks from a parsed XML document.
fn extract_tasks_from_xml(xml_doc: &crate::domain::xml_tree::XmlDocument) -> Vec<Task> {
    let mut tasks = Vec::new();
    collect_tasks_from_element(&xml_doc.root, &mut tasks);
    tasks
}

fn collect_tasks_from_element(
    element: &crate::domain::xml_tree::XmlElement,
    tasks: &mut Vec<Task>,
) {
    if element.tag == "TASK" {
        let task = read_task(element);
        tasks.push(task);
    }
    // Recurse into child elements
    for node in &element.children {
        if let crate::domain::xml_tree::XmlNode::Element(child) = node {
            collect_tasks_from_element(child, tasks);
        }
    }
}

/// Insert a single task into the task_index table and related tables.
fn insert_task_index(conn: &Connection, document_id: &str, task: &crate::domain::task::Task) -> IndexResult<()> {
    let status_str = format!("{:?}", task.status());

    conn.execute(
        "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, risk, start_date, due_date, completed_date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            task.id.to_string(),
            document_id,
            task.title,
            task.priority.value() as i32,
            status_str,
            task.percent_done,
            task.risk as i32,
            task.start_date,
            task.due_date,
            task.completion_date.map(|d| d.to_string()),
        ],
    )?;

    // Insert tags
    for cat in &task.categories {
        conn.execute(
            "INSERT OR IGNORE INTO task_tags (task_key, document_id, tag) VALUES (?1, ?2, ?3)",
            rusqlite::params![task.id.to_string(), document_id, cat.name],
        )?;
    }

    // Insert participants
    for participant in &task.allocated_to {
        conn.execute(
            "INSERT OR IGNORE INTO task_participants (task_key, document_id, participant, role) VALUES (?1, ?2, ?3, 'allocated_to')",
            rusqlite::params![task.id.to_string(), document_id, participant],
        )?;
    }
    if let Some(ref allocated_by) = task.allocated_by {
        conn.execute(
            "INSERT OR IGNORE INTO task_participants (task_key, document_id, participant, role) VALUES (?1, ?2, ?3, 'allocated_by')",
            rusqlite::params![task.id.to_string(), document_id, allocated_by],
        )?;
    }

    // Insert dependencies
    for dep in &task.dependencies {
        conn.execute(
            "INSERT OR IGNORE INTO task_dependencies (task_key, document_id, depends_on_key, dep_type, raw_ref) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                task.id.to_string(),
                document_id,
                dep.task_id,
                dep.dependency_type.to_string(),
                dep.raw_xml,
            ],
        )?;
    }

    Ok(())
}

/// Rebuild the entire index for a workspace.
///
/// Scans all registered documents and re-indexes them.
/// Returns the total number of tasks indexed.
pub fn rebuild_index(
    conn: &Connection,
    documents: &[(String, std::path::PathBuf)], // (doc_id, abs_path)
    cancel_flag: Option<Arc<AtomicBool>>,
    progress_callback: Option<&dyn Fn(IndexProgress)>,
) -> IndexResult<usize> {
    let total = documents.len();
    let mut total_tasks = 0;

    for (i, (doc_id, path)) in documents.iter().enumerate() {
        // Check cancellation
        if let Some(ref flag) = cancel_flag {
            if flag.load(Ordering::Relaxed) {
                return Err(IndexError::Cancelled);
            }
        }

        let file_name = path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        if let Some(cb) = progress_callback {
            cb(IndexProgress {
                total_files: total,
                processed_files: i,
                total_tasks,
                current_file: file_name.clone(),
            });
        }

        match index_document(conn, doc_id, path) {
            Ok(count) => total_tasks += count,
            Err(e) => {
                log::warn!("Failed to index {}: {}", path.display(), e);
                // Continue indexing other files
            }
        }
    }

    if let Some(cb) = progress_callback {
        cb(IndexProgress {
            total_files: total,
            processed_files: total,
            total_tasks,
            current_file: String::new(),
        });
    }

    Ok(total_tasks)
}

/// Compute and return the fingerprint for a file.
pub fn compute_fingerprint(path: &Path) -> IndexResult<String> {
    let fp = FileFingerprint::from_file(path)
        .map_err(|e| IndexError::FileRead(format!("{}: {}", path.display(), e)))?;
    Ok(fp.hash.clone())
}

/// Clear all index data for a workspace (documents + tasks + tags + participants + dependencies).
pub fn clear_workspace_index(conn: &Connection, workspace_id: &str) -> IndexResult<()> {
    // Get all document IDs for this workspace
    let mut stmt = conn.prepare("SELECT id FROM documents WHERE workspace_id = ?1")?;
    let doc_ids: Vec<String> = stmt
        .query_map([workspace_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    for doc_id in &doc_ids {
        conn.execute("DELETE FROM task_dependencies WHERE document_id = ?1", [doc_id])?;
        conn.execute("DELETE FROM task_participants WHERE document_id = ?1", [doc_id])?;
        conn.execute("DELETE FROM task_tags WHERE document_id = ?1", [doc_id])?;
        conn.execute("DELETE FROM task_index WHERE document_id = ?1", [doc_id])?;
    }

    conn.execute("DELETE FROM documents WHERE workspace_id = ?1", [workspace_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::migration;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migration::run_migrations(&conn).unwrap();

        // Insert test workspace and document
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES ('ws-test', 'Test', '/tmp')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path, doc_type) VALUES ('doc-test', 'ws-test', 'test.xml', 'managed')",
            [],
        ).unwrap();

        conn
    }

    fn write_test_xml(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("test.xml");
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<TDL>
    <TASK ID="1" TITLE="Task One" PERCENTDONE="50" PRIORITY="2">
        <TASK ID="2" TITLE="Subtask" PERCENTDONE="0"/>
    </TASK>
    <TASK ID="3" TITLE="Task Two" PERCENTDONE="100"/>
</TDL>"#;
        std::fs::write(&path, xml).unwrap();
        path
    }

    #[test]
    fn index_document_counts_tasks() {
        let dir = std::env::temp_dir().join(format!("mtdl_idx_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let xml_path = write_test_xml(&dir);

        let conn = setup_test_db();
        let count = index_document(&conn, "doc-test", &xml_path).unwrap();
        assert_eq!(count, 3, "should index 3 tasks");

        // Verify task_index
        let task_count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM task_index WHERE document_id = 'doc-test'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(task_count, 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn index_document_clears_before_reindex() {
        let dir = std::env::temp_dir().join(format!("mtdl_idx2_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let xml_path = write_test_xml(&dir);

        let conn = setup_test_db();

        // Index twice
        index_document(&conn, "doc-test", &xml_path).unwrap();
        let count = index_document(&conn, "doc-test", &xml_path).unwrap();

        // Should still be 3, not 6
        assert_eq!(count, 3);
        let task_count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM task_index WHERE document_id = 'doc-test'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(task_count, 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn index_document_with_tags() {
        let dir = std::env::temp_dir().join(format!("mtdl_idx3_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tags.xml");
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<TDL>
    <TASK ID="1" TITLE="Tagged Task">
        <CATEGORY>work</CATEGORY>
        <CATEGORY>urgent</CATEGORY>
    </TASK>
</TDL>"#;
        std::fs::write(&path, xml).unwrap();

        let conn = setup_test_db();
        index_document(&conn, "doc-test", &path).unwrap();

        let tag_count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM task_tags WHERE document_id = 'doc-test'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(tag_count >= 1, "should have at least 1 tag");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rebuild_index_with_cancel() {
        let conn = setup_test_db();
        let cancel = Arc::new(AtomicBool::new(true)); // Already cancelled

        let docs = vec![
            ("doc-1".to_string(), Path::new("/fake/file1.xml").to_path_buf()),
        ];

        let result = rebuild_index(&conn, &docs, Some(cancel), None);
        assert!(matches!(result, Err(IndexError::Cancelled)));
    }

    #[test]
    fn rebuild_index_reports_progress() {
        let dir = std::env::temp_dir().join(format!("mtdl_idx4_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let xml_path = write_test_xml(&dir);

        let conn = setup_test_db();
        let docs = vec![("doc-test".to_string(), xml_path)];

        use std::sync::Mutex;
        let progress_log = Arc::new(Mutex::new(Vec::new()));
        let log_clone = progress_log.clone();

        let total = rebuild_index(&conn, &docs, None, Some(&move |p: IndexProgress| {
            log_clone.lock().unwrap().push(p.processed_files);
        })).unwrap();

        assert_eq!(total, 3);
        let log = progress_log.lock().unwrap();
        assert!(!log.is_empty(), "should have progress reports");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_clear_workspace_index() {
        let dir = std::env::temp_dir().join(format!("mtdl_idx5_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let xml_path = write_test_xml(&dir);

        let conn = setup_test_db();
        index_document(&conn, "doc-test", &xml_path).unwrap();

        // Verify tasks exist
        let count_before: u32 = conn.query_row(
            "SELECT COUNT(*) FROM task_index", [], |row| row.get(0)
        ).unwrap();
        assert!(count_before > 0);

        // Clear
        clear_workspace_index(&conn, "ws-test").unwrap();

        // Verify cleared
        let count_after: u32 = conn.query_row(
            "SELECT COUNT(*) FROM task_index", [], |row| row.get(0)
        ).unwrap();
        assert_eq!(count_after, 0);

        let doc_count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE workspace_id = 'ws-test'", [], |row| row.get(0)
        ).unwrap();
        assert_eq!(doc_count, 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compute_fingerprint_works() {
        let dir = std::env::temp_dir().join(format!("mtdl_fp_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.txt");
        std::fs::write(&path, "hello world").unwrap();

        let fp = compute_fingerprint(&path).unwrap();
        assert!(!fp.is_empty());
        assert!(fp.len() > 10, "fingerprint should be a hex string");

        std::fs::remove_dir_all(&dir).ok();
    }
}
