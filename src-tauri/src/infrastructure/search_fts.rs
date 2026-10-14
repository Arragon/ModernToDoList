//! M9: Full-text search infrastructure (RD-M9-001~013).
//!
//! Two cooperating stores, both DISPOSABLE derived caches rebuildable from
//! an XML scan (never a business source of truth):
//!
//! 1. `task_search_fts` — an FTS5 virtual table with UNINDEXED identity
//!    columns (document_id, task_id) and indexed columns title/body/tags/
//!    participants/attachments/links. Created by schema migration v2 and
//!    re-ensured at runtime by [`ensure_search_schema`].
//! 2. `task_search_plain` — a plain mirror table that powers the LIKE-based
//!    substring fallback. The fallback is MANDATORY for CJK queries because
//!    FTS5's unicode61 tokenizer does not segment Chinese/Japanese/Korean
//!    text (a whole run of Han characters becomes a single token).
//!
//! Backend selection at query time:
//! - query contains CJK → LIKE fallback (`like-fallback`)
//! - otherwise → FTS5 MATCH with bm25 ranking; if FTS5 is unavailable or the
//!   MATCH expression fails, degrade to the LIKE fallback.
//!
//! M6 INTEGRATION SEAM (participants / attachments / progress links):
//! those features are owned by another milestone. This module depends only on
//! [`TaskSearchExtras`] and the [`SearchExtrasProvider`] trait. The default
//! [`IndexTableExtrasProvider`] reads the shared index tables
//! (`task_tags`, `task_participants`, `attachments_index`,
//! `progress_links_index`) — as soon as M6 populates them, their text becomes
//! searchable with zero changes here.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;

use crate::domain::mappers::read_task;
use crate::domain::search::{
    classify_match, cjk_match_tokens, make_snippet, query_needs_cjk_fallback, DocumentContext,
    MatchedField, SearchPage, SearchQuery, SearchResult,
};
use crate::domain::task::Task;
use crate::domain::types::{DocumentId, TaskId, TaskKey};
use crate::domain::xml_parser::parse_xml;
use crate::domain::xml_tree::XmlNode;
use crate::infrastructure::schema;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("XML parse error: {0}")]
    XmlParse(String),

    #[error("File read error: {0}")]
    FileRead(String),
}

pub type SearchResultT<T> = Result<T, SearchError>;

/// Which engine answers queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchBackend {
    Fts5,
    PlainLike,
}

impl SearchBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchBackend::Fts5 => "fts5",
            SearchBackend::PlainLike => "like-fallback",
        }
    }
}

/// Probe whether this SQLite build supports FTS5.
///
/// Creates and drops a throw-away virtual table (works even on connections
/// where the migration somehow did not create `task_search_fts`).
pub fn fts5_available(conn: &Connection) -> bool {
    let probe = "fts5_probe_m9";
    let ok = conn
        .execute_batch(&format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS {probe} USING fts5(x)"
        ))
        .is_ok()
        && conn
            .execute(&format!("INSERT INTO {probe}(x) VALUES ('ok')"), [])
            .is_ok()
        && conn
            .query_row(
                &format!("SELECT count(*) FROM {probe} WHERE {probe} MATCH 'ok'"),
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|n| n == 1)
            .unwrap_or(false);
    let _ = conn.execute_batch(&format!("DROP TABLE IF EXISTS {probe}"));
    ok
}

/// Ensure the search schema exists and report the usable backend.
///
/// Migration v2 already creates both tables; this is the runtime guard for
/// in-memory test connections and for graceful degradation if FTS5 is ever
/// unavailable (in that case everything still works via `task_search_plain`).
pub fn ensure_search_schema(conn: &Connection) -> SearchBackend {
    // Plain table always works.
    let _ = conn.execute_batch(schema::CREATE_TASK_SEARCH_PLAIN);
    let _ = conn.execute_batch(schema::CREATE_IDX_SEARCH_PLAIN_TITLE);
    let _ = conn.execute_batch(schema::CREATE_IDX_SEARCH_PLAIN_DOC);

    match conn.execute_batch(schema::CREATE_TASK_SEARCH_FTS) {
        Ok(()) if fts5_available(conn) => SearchBackend::Fts5,
        _ => SearchBackend::PlainLike,
    }
}

// ── M6 integration seam ──────────────────────────────────────────────────────

/// Extra searchable text contributed by M6 features (tags, participants,
/// attachment display names, progress-link labels).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskSearchExtras {
    pub tags: Vec<String>,
    pub participants: Vec<String>,
    pub attachment_names: Vec<String>,
    pub progress_link_labels: Vec<String>,
}

/// Supplies [`TaskSearchExtras`] per task. Implement this trait on M6 types
/// to feed search without search depending on them.
pub trait SearchExtrasProvider {
    fn extras_for(
        &self,
        conn: &Connection,
        document_id: &str,
        task_id: &str,
    ) -> TaskSearchExtras;
}

/// Default provider: reads the shared index tables. Works for tags and
/// allocated-to participants today; picks up M6 attachment/link/participant
/// rows automatically once those agents populate their tables.
pub struct IndexTableExtrasProvider;

impl SearchExtrasProvider for IndexTableExtrasProvider {
    fn extras_for(
        &self,
        conn: &Connection,
        document_id: &str,
        task_id: &str,
    ) -> TaskSearchExtras {
        let strings = |sql: &str| -> Vec<String> {
            let mut stmt = match conn.prepare(sql) {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            };
            stmt.query_map([document_id, task_id], |row| row.get::<_, String>(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        };
        TaskSearchExtras {
            tags: strings(
                "SELECT tag FROM task_tags WHERE document_id = ?1 AND task_key = ?2 ORDER BY tag",
            ),
            participants: strings(
                "SELECT DISTINCT participant FROM task_participants \
                 WHERE document_id = ?1 AND task_key = ?2 ORDER BY participant",
            ),
            attachment_names: strings(
                "SELECT file_name FROM attachments_index \
                 WHERE document_id = ?1 AND task_key = ?2 ORDER BY file_name",
            ),
            progress_link_labels: strings(
                "SELECT COALESCE(provider, '') || ' ' || url FROM progress_links_index \
                 WHERE document_id = ?1 AND task_key = ?2 ORDER BY url",
            ),
        }
    }
}

// ── Index writes ─────────────────────────────────────────────────────────────

/// One task's full searchable payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchDocument {
    pub document_id: String,
    pub task_id: String,
    pub title: String,
    pub body: String,
    pub tags: String,
    pub participants: String,
    pub attachments: String,
    pub links: String,
}

impl SearchDocument {
    /// Build from a domain task plus M6 extras.
    pub fn from_task(document_id: &str, task: &Task, extras: &TaskSearchExtras) -> Self {
        let join = |v: &[String]| v.join(" ");
        Self {
            document_id: document_id.to_string(),
            task_id: task.id.to_string(),
            title: task.title.clone(),
            body: crate::domain::search::task_description_text(task),
            tags: join(&extras.tags),
            participants: join(&extras.participants),
            attachments: join(&extras.attachment_names),
            links: join(&extras.progress_link_labels),
        }
    }
}

/// Upsert one task into both search stores (incremental update entry point).
///
/// Call this on every task mutation (title/description/tags/participants/
/// attachments/links change) — RD-M9-013.
pub fn update_task_search(conn: &Connection, doc: &SearchDocument) -> SearchResultT<()> {
    conn.execute(
        "INSERT OR REPLACE INTO task_search_plain \
         (document_id, task_id, title, body, tags, participants, attachments, links) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            doc.document_id, doc.task_id, doc.title, doc.body,
            doc.tags, doc.participants, doc.attachments, doc.links
        ],
    )?;

    if fts_table_exists(conn) {
        conn.execute(
            "DELETE FROM task_search_fts WHERE document_id = ?1 AND task_id = ?2",
            rusqlite::params![doc.document_id, doc.task_id],
        )?;
        conn.execute(
            "INSERT INTO task_search_fts \
             (document_id, task_id, title, body, tags, participants, attachments, links) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                doc.document_id, doc.task_id, doc.title, doc.body,
                doc.tags, doc.participants, doc.attachments, doc.links
            ],
        )?;
    }
    Ok(())
}

/// Convenience for task mutations: build the payload from a domain task.
pub fn on_task_mutated(
    conn: &Connection,
    document_id: &str,
    task: &Task,
    provider: &dyn SearchExtrasProvider,
) -> SearchResultT<()> {
    let extras = provider.extras_for(conn, document_id, task.id.as_str());
    let doc = SearchDocument::from_task(document_id, task, &extras);
    update_task_search(conn, &doc)
}

/// Remove one task from both search stores.
pub fn remove_task_search(conn: &Connection, document_id: &str, task_id: &str) -> SearchResultT<()> {
    conn.execute(
        "DELETE FROM task_search_plain WHERE document_id = ?1 AND task_id = ?2",
        rusqlite::params![document_id, task_id],
    )?;
    if fts_table_exists(conn) {
        conn.execute(
            "DELETE FROM task_search_fts WHERE document_id = ?1 AND task_id = ?2",
            rusqlite::params![document_id, task_id],
        )?;
    }
    Ok(())
}

/// Remove every row belonging to a document from both search stores.
pub fn remove_document_search(conn: &Connection, document_id: &str) -> SearchResultT<()> {
    conn.execute(
        "DELETE FROM task_search_plain WHERE document_id = ?1",
        [document_id],
    )?;
    if fts_table_exists(conn) {
        conn.execute(
            "DELETE FROM task_search_fts WHERE document_id = ?1",
            [document_id],
        )?;
    }
    Ok(())
}

fn fts_table_exists(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'task_search_fts'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

// ── Full rebuild from XML scan (disposability proof) ────────────────────────

/// Rebuild the search index for the given documents straight from their XML
/// files. The search stores are derived data: after `clear_search_index` +
/// this function, search behaves identically.
pub fn rebuild_search_index(
    conn: &Connection,
    documents: &[(String, std::path::PathBuf)],
    provider: &dyn SearchExtrasProvider,
) -> SearchResultT<usize> {
    clear_search_index(conn)?;
    let mut total = 0usize;
    for (document_id, path) in documents {
        total += index_document_search(conn, document_id, path, provider)?;
    }
    Ok(total)
}

/// Rebuild the search rows for a single document from its XML file.
pub fn index_document_search(
    conn: &Connection,
    document_id: &str,
    file_path: &Path,
    provider: &dyn SearchExtrasProvider,
) -> SearchResultT<usize> {
    let content = std::fs::read(file_path)
        .map_err(|e| SearchError::FileRead(format!("{}: {}", file_path.display(), e)))?;
    let xml_doc = parse_xml(&content)
        .map_err(|e| SearchError::XmlParse(format!("{}: {}", file_path.display(), e)))?;

    remove_document_search(conn, document_id)?;

    let mut tasks = Vec::new();
    collect_tasks(&xml_doc.root, &mut tasks);

    let mut count = 0;
    for task in &tasks {
        let extras = provider.extras_for(conn, document_id, task.id.as_str());
        let doc = SearchDocument::from_task(document_id, task, &extras);
        update_task_search(conn, &doc)?;
        count += 1;
    }
    Ok(count)
}

fn collect_tasks(element: &crate::domain::xml_tree::XmlElement, out: &mut Vec<Task>) {
    if element.tag == "TASK" {
        out.push(read_task(element));
    }
    for node in &element.children {
        if let XmlNode::Element(child) = node {
            collect_tasks(child, out);
        }
    }
}

/// Delete all rows from both search stores (they are pure derived data).
pub fn clear_search_index(conn: &Connection) -> SearchResultT<()> {
    conn.execute_batch("DELETE FROM task_search_plain")?;
    if fts_table_exists(conn) {
        conn.execute_batch("DELETE FROM task_search_fts")?;
    }
    Ok(())
}

// ── Query path ───────────────────────────────────────────────────────────────

/// Execute a bounded, paginated, deterministically ordered search.
pub fn search(conn: &Connection, query: &SearchQuery) -> SearchResultT<SearchPage> {
    let backend = if fts_table_exists(conn) {
        SearchBackend::Fts5
    } else {
        SearchBackend::PlainLike
    };
    let limit = query.bounded_limit();

    if query.is_empty() {
        return Ok(SearchPage {
            results: Vec::new(),
            total: 0,
            limit,
            offset: query.offset,
            backend: backend.as_str().to_string(),
        });
    }

    // CJK queries ALWAYS take the LIKE fallback: unicode61 does not
    // segment Han characters, so FTS5 MATCH would silently return nothing.
    let use_fts = backend == SearchBackend::Fts5 && !query_needs_cjk_fallback(&query.text);

    let page = if use_fts {
        match search_fts5(conn, query, limit) {
            Ok(page) => page,
            Err(e) => {
                // Malformed MATCH expression or engine issue → degrade.
                log::warn!("FTS5 search failed ({}), degrading to LIKE fallback", e);
                search_like(conn, query, limit)?
            }
        }
    } else {
        search_like(conn, query, limit)?
    };
    Ok(page)
}

fn file_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// FTS5 MATCH path with bm25 ranking and snippet() excerpts.
///
/// NOTE: the FTS table is referenced by its full name (never aliased) because
/// FTS5 auxiliary functions (bm25, snippet) take the table name as first arg.
fn search_fts5(conn: &Connection, query: &SearchQuery, limit: usize) -> SearchResultT<SearchPage> {
    let match_expr = build_fts_match(&query.text);
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(match_expr.clone()));
    let mut where_extra = String::new();
    if let Some(ref doc) = query.document_id {
        where_extra.push_str(" AND task_search_fts.document_id = ?2");
        params.push(Box::new(doc.clone()));
    }

    // Total count (before pagination).
    let count_sql = format!(
        "SELECT count(*) FROM task_search_fts WHERE task_search_fts MATCH ?1{where_extra}"
    );
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let total: usize = conn.query_row(&count_sql, params_ref.as_slice(), |r| {
        r.get::<_, i64>(0)
    })? as usize;

    let sql = format!(
        "SELECT task_search_fts.document_id, task_search_fts.task_id, task_search_fts.title, \
                task_search_fts.body, task_search_fts.tags, task_search_fts.participants, \
                task_search_fts.attachments, task_search_fts.links, \
                COALESCE((SELECT d.file_path FROM documents d \
                          WHERE d.id = task_search_fts.document_id), \
                         task_search_fts.document_id) AS doc_name, \
                bm25(task_search_fts) AS rank, \
                snippet(task_search_fts, 2, '[[', ']]', '…', 10), \
                snippet(task_search_fts, 3, '[[', ']]', '…', 10) \
         FROM task_search_fts \
         WHERE task_search_fts MATCH ?1{where_extra} \
         ORDER BY rank ASC, task_search_fts.document_id ASC, task_search_fts.task_id ASC \
         LIMIT ?{lim_pos} OFFSET ?{off_pos}",
        lim_pos = params.len() + 1,
        off_pos = params.len() + 2,
    );
    params.push(Box::new(limit as i64));
    params.push(Box::new(query.offset as i64));

    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let tokens: Vec<String> = query
        .text
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        let document_id: String = row.get(0)?;
        let task_id: String = row.get(1)?;
        let title: String = row.get(2)?;
        let body: String = row.get(3)?;
        let tags: String = row.get(4)?;
        let participants: String = row.get(5)?;
        let attachments: String = row.get(6)?;
        let links: String = row.get(7)?;
        let doc_path: String = row.get(8)?;
        let rank: f64 = row.get(9)?;
        let title_snippet: String = row.get(10)?;
        let body_snippet: String = row.get(11)?;

        let matched_field = classify_match(
            &tokens, &title, &body, &tags, &participants, &attachments, &links,
        );
        let snippet = if matched_field == MatchedField::Title
            && title_snippet.contains("[[")
        {
            title_snippet
        } else if body_snippet.contains("[[") {
            body_snippet
        } else if title_snippet.contains("[[") {
            title_snippet
        } else {
            make_snippet(&title, &tokens, 24)
        };

        Ok(SearchResult {
            task_key: TaskKey::new(DocumentId::new(&document_id), TaskId::new(&task_id)),
            title,
            document_context: DocumentContext {
                document_id: document_id.clone(),
                document_name: file_name_from_path(&doc_path),
            },
            matched_field,
            snippet,
            // bm25 is "lower is better"; negate so higher = better everywhere.
            score: -rank,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(SearchPage {
        results,
        total,
        limit,
        offset: query.offset,
        backend: SearchBackend::Fts5.as_str().to_string(),
    })
}

/// Convert user text into a safe FTS5 MATCH expression: every whitespace
/// token becomes a quoted phrase (embedded quotes doubled), AND-combined.
fn build_fts_match(text: &str) -> String {
    text.split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn escape_like(token: &str) -> String {
    token
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// LIKE substring fallback (CJK queries and non-FTS builds).
///
/// Every token must appear (AND) in at least one searchable field (OR).
/// Scoring: title 3.0 > tags/participants 2.0 > attachments/links 1.5 >
/// body 1.0, plus 0.5 per extra field hit. Ordering is fully deterministic:
/// score desc, matched-field rank, document_id, task_id.
fn search_like(conn: &Connection, query: &SearchQuery, limit: usize) -> SearchResultT<SearchPage> {
    let tokens = if query_needs_cjk_fallback(&query.text) {
        cjk_match_tokens(&query.text)
    } else {
        query
            .text
            .split_whitespace()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
    };
    if tokens.is_empty() {
        return Ok(SearchPage {
            results: Vec::new(),
            total: 0,
            limit,
            offset: query.offset,
            backend: SearchBackend::PlainLike.as_str().to_string(),
        });
    }

    let mut sql = String::from(
        "SELECT s.document_id, s.task_id, s.title, s.body, s.tags, s.participants, \
                s.attachments, s.links, \
                COALESCE((SELECT d.file_path FROM documents d WHERE d.id = s.document_id), \
                         s.document_id) \
         FROM task_search_plain s WHERE 1=1",
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(ref doc) = query.document_id {
        params.push(Box::new(doc.clone()));
        sql.push_str(&format!(" AND s.document_id = ?{}", params.len()));
    }
    for token in &tokens {
        let pattern = format!("%{}%", escape_like(token));
        params.push(Box::new(pattern.clone()));
        let p = params.len();
        params.push(Box::new(pattern.clone()));
        let p2 = params.len();
        params.push(Box::new(pattern.clone()));
        let p3 = params.len();
        params.push(Box::new(pattern.clone()));
        let p4 = params.len();
        params.push(Box::new(pattern.clone()));
        let p5 = params.len();
        params.push(Box::new(pattern));
        let p6 = params.len();
        sql.push_str(&format!(
            " AND (s.title LIKE ?{p} ESCAPE '\\' OR s.body LIKE ?{p2} ESCAPE '\\' \
              OR s.tags LIKE ?{p3} ESCAPE '\\' OR s.participants LIKE ?{p4} ESCAPE '\\' \
              OR s.attachments LIKE ?{p5} ESCAPE '\\' OR s.links LIKE ?{p6} ESCAPE '\\')"
        ));
    }

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?, // document_id
            row.get::<_, String>(1)?, // task_id
            row.get::<_, String>(2)?, // title
            row.get::<_, String>(3)?, // body
            row.get::<_, String>(4)?, // tags
            row.get::<_, String>(5)?, // participants
            row.get::<_, String>(6)?, // attachments
            row.get::<_, String>(7)?, // links
            row.get::<_, String>(8)?, // doc path
        ))
    })?;

    let lower_tokens: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();
    let mut scored: Vec<SearchResult> = Vec::new();
    for row in rows {
        let (document_id, task_id, title, body, tags, participants, attachments, links, doc_path) =
            row?;
        let contains_any = |field: &str| {
            let lower = field.to_lowercase();
            lower_tokens.iter().all(|t| lower.contains(t))
        };
        let mut score = 0.0f64;
        if contains_any(&title) {
            score += 3.0;
        }
        if contains_any(&tags) || contains_any(&participants) {
            score += 2.0;
        }
        if contains_any(&attachments) || contains_any(&links) {
            score += 1.5;
        }
        if contains_any(&body) {
            score += 1.0;
        }
        if score == 0.0 {
            // Every token matched SOME field (the SQL WHERE guarantees it),
            // but no single field contains all tokens — base relevance.
            score = 0.5;
        }

        let matched_field = classify_match(
            &tokens, &title, &body, &tags, &participants, &attachments, &links,
        );
        let source = match matched_field {
            MatchedField::Title => &title,
            MatchedField::Description => &body,
            MatchedField::Tags => &tags,
            MatchedField::Participants => &participants,
            MatchedField::Attachments => &attachments,
            MatchedField::Links => &links,
        };
        let snippet = make_snippet(source, &tokens, 24);

        scored.push(SearchResult {
            task_key: TaskKey::new(DocumentId::new(&document_id), TaskId::new(&task_id)),
            title,
            document_context: DocumentContext {
                document_id,
                document_name: file_name_from_path(&doc_path),
            },
            matched_field,
            snippet,
            score,
        });
    }

    let total = scored.len();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.matched_field.rank().cmp(&b.matched_field.rank()))
            .then_with(|| {
                a.task_key
                    .document_id
                    .as_str()
                    .cmp(b.task_key.document_id.as_str())
            })
            .then_with(|| a.task_key.task_id.as_str().cmp(b.task_key.task_id.as_str()))
    });

    let results: Vec<SearchResult> = scored
        .into_iter()
        .skip(query.offset)
        .take(limit)
        .collect();

    Ok(SearchPage {
        results,
        total,
        limit,
        offset: query.offset,
        backend: SearchBackend::PlainLike.as_str().to_string(),
    })
}

/// Number of rows currently in the search index (both stores agree by design).
pub fn search_index_task_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM task_search_plain", [], |r| r.get(0))
        .unwrap_or(-1)
}

/// Fetch a document's display name for context (used by callers that build
/// results outside of `search`).
pub fn document_display_name(conn: &Connection, document_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT file_path FROM documents WHERE id = ?1",
        [document_id],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .map(|p| file_name_from_path(&p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::migration;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migration::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES ('ws1', 'WS', '/tmp')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path) VALUES ('doc1', 'ws1', 'C:/data/项目清单.tdl')",
            [],
        )
        .unwrap();
        conn
    }

    fn seed(conn: &Connection) {
        let extras = TaskSearchExtras::default();
        let mut t1 = Task::new(TaskId::new("1"));
        t1.title = "Buy milk and eggs".into();
        index_one(conn, &t1, &TaskSearchExtras {
            tags: vec!["shopping".into()],
            participants: vec!["Alice".into()],
            attachment_names: vec!["receipt.pdf".into()],
            progress_link_labels: vec!["GitHub PR 42".into()],
            ..extras.clone()
        });
        let mut t2 = Task::new(TaskId::new("2"));
        t2.title = "购买清单：牛奶和鸡蛋".into();
        index_one(conn, &t2, &TaskSearchExtras {
            tags: vec!["购物".into()],
            ..extras.clone()
        });
        let mut t3 = Task::new(TaskId::new("3"));
        t3.title = "Write release notes".into();
        let mut t3c = t3.clone();
        t3c.comments = Some(crate::domain::task::TaskComment {
            comment_type: crate::domain::task::CommentType::Plain,
            content: "Remember the 待办事项 list from 购物清单".into(),
        });
        index_one(conn, &t3c, &extras);
    }

    fn index_one(conn: &Connection, task: &Task, extras: &TaskSearchExtras) {
        let doc = SearchDocument::from_task("doc1", task, extras);
        update_task_search(conn, &doc).unwrap();
    }

    #[test]
    fn fts5_is_available_in_bundled_sqlite() {
        let conn = setup();
        assert!(
            fts5_available(&conn),
            "bundled rusqlite 0.32 must compile with SQLITE_ENABLE_FTS5"
        );
        assert_eq!(ensure_search_schema(&conn), SearchBackend::Fts5);
    }

    #[test]
    fn migration_creates_fts_table() {
        let conn = setup();
        assert!(fts_table_exists(&conn));
    }

    #[test]
    fn fts5_search_english() {
        let conn = setup();
        seed(&conn);
        let page = search(&conn, &SearchQuery::new("milk")).unwrap();
        assert_eq!(page.backend, "fts5");
        assert_eq!(page.total, 1);
        assert_eq!(page.results[0].task_key.task_id.as_str(), "1");
        assert_eq!(page.results[0].matched_field, MatchedField::Title);
        assert!(page.results[0].snippet.contains("[[milk]]"));
        assert_eq!(
            page.results[0].document_context.document_name,
            "项目清单.tdl"
        );
    }

    #[test]
    fn plain_fts5_fails_on_cjk_but_fallback_finds_it() {
        let conn = setup();
        seed(&conn);

        // PROOF: a raw FTS5 MATCH for a CJK substring returns nothing,
        // because unicode61 indexed the whole run as one token.
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM task_search_fts WHERE task_search_fts MATCH '\"购买\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "plain FTS5 should NOT find the CJK substring");

        // The LIKE fallback must find it.
        let page = search(&conn, &SearchQuery::new("购买")).unwrap();
        assert_eq!(page.backend, "like-fallback");
        assert!(page.total >= 1);
        assert!(page.results.iter().any(|r| r.task_key.task_id.as_str() == "2"));
        assert!(page.results.iter().any(|r| r.snippet.contains("[[购买]]")));
    }

    #[test]
    fn cjk_search_terms() {
        let conn = setup();
        seed(&conn);

        // "购物清单" appears in task 3's description → fallback finds it.
        let page = search(&conn, &SearchQuery::new("购物清单")).unwrap();
        assert_eq!(page.backend, "like-fallback");
        assert_eq!(page.total, 1);
        assert_eq!(page.results[0].task_key.task_id.as_str(), "3");
        assert_eq!(page.results[0].matched_field, MatchedField::Description);

        // "待办事项" also lives in that description.
        let page = search(&conn, &SearchQuery::new("待办事项")).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.results[0].task_key.task_id.as_str(), "3");

        // Title match for the exact title phrase of task 2.
        let page = search(&conn, &SearchQuery::new("购买清单")).unwrap();
        assert!(page.results.iter().any(|r| {
            r.task_key.task_id.as_str() == "2" && r.matched_field == MatchedField::Title
        }));

        // Terms that are in NO document.
        assert_eq!(search(&conn, &SearchQuery::new("任务")).unwrap().total, 0);
        assert_eq!(search(&conn, &SearchQuery::new("不存在的词")).unwrap().total, 0);
    }

    #[test]
    fn mixed_cjk_english_query() {
        let conn = setup();
        seed(&conn);
        let page = search(&conn, &SearchQuery::new("release 待办事项")).unwrap();
        assert_eq!(page.backend, "like-fallback");
        assert_eq!(page.total, 1);
        assert_eq!(page.results[0].task_key.task_id.as_str(), "3");
    }

    #[test]
    fn search_by_tag_participant_attachment_link() {
        let conn = setup();
        seed(&conn);
        assert_eq!(search(&conn, &SearchQuery::new("shopping")).unwrap().total, 1);
        assert_eq!(search(&conn, &SearchQuery::new("Alice")).unwrap().total, 1);
        let r = &search(&conn, &SearchQuery::new("receipt.pdf")).unwrap().results[0];
        assert_eq!(r.matched_field, MatchedField::Attachments);
        let r = &search(&conn, &SearchQuery::new("GitHub")).unwrap().results[0];
        assert_eq!(r.matched_field, MatchedField::Links);
    }

    #[test]
    fn pagination_is_bounded_and_deterministic() {
        let conn = setup();
        for i in 0..10 {
            let mut t = Task::new(TaskId::new(format!("t{i}")));
            t.title = format!("common word task {i}");
            index_one(&conn, &t, &TaskSearchExtras::default());
        }
        let q = SearchQuery::new("common").with_page(3, 0);
        let p1 = search(&conn, &q).unwrap();
        assert_eq!(p1.results.len(), 3);
        assert_eq!(p1.total, 10);

        let p2 = search(&conn, &SearchQuery::new("common").with_page(3, 3)).unwrap();
        assert_ne!(
            p1.results[0].task_key, p2.results[0].task_key,
            "pages must not overlap"
        );

        // Deterministic: same query twice → identical order.
        let again = search(&conn, &q).unwrap();
        assert_eq!(p1.results, again.results);

        // Hard bound.
        let huge = search(&conn, &SearchQuery::new("common").with_page(99999, 0)).unwrap();
        assert!(huge.results.len() <= crate::domain::search::MAX_PAGE_SIZE);
    }

    #[test]
    fn incremental_update_and_delete() {
        let conn = setup();
        seed(&conn);
        assert_eq!(search(&conn, &SearchQuery::new("milk")).unwrap().total, 1);

        // Mutate task 1: title no longer contains "milk".
        let mut t1 = Task::new(TaskId::new("1"));
        t1.title = "Buy bread".into();
        index_one(&conn, &t1, &TaskSearchExtras::default());
        assert_eq!(search(&conn, &SearchQuery::new("milk")).unwrap().total, 0);
        assert_eq!(search(&conn, &SearchQuery::new("bread")).unwrap().total, 1);

        // Delete task.
        remove_task_search(&conn, "doc1", "1").unwrap();
        assert_eq!(search(&conn, &SearchQuery::new("bread")).unwrap().total, 0);
        let plain: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_search_plain WHERE task_id='1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_search_fts WHERE task_id='1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((plain, fts), (0, 0));
    }

    #[test]
    fn rebuild_from_xml_scan_restores_search() {
        let dir = std::env::temp_dir().join(format!("mtdl_fts_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.xml");
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="utf-8"?>
<TDL>
  <TASK ID="1" TITLE="Rebuildable search alpha">
    <CATEGORY>work</CATEGORY>
    <COMMENTS>the quick brown fox</COMMENTS>
  </TASK>
  <TASK ID="2" TITLE="重建索引测试"/>
</TDL>"#,
        )
        .unwrap();

        let conn = setup();
        conn.execute(
            "UPDATE documents SET file_path = ?1 WHERE id = 'doc1'",
            [path.to_string_lossy().as_ref()],
        )
        .unwrap();

        let docs = vec![("doc1".to_string(), path.clone())];
        let n = rebuild_search_index(&conn, &docs, &IndexTableExtrasProvider).unwrap();
        assert_eq!(n, 2);

        let page = search(&conn, &SearchQuery::new("brown")).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.results[0].matched_field, MatchedField::Description);

        // Prove disposability: wipe everything, rebuild, identical results.
        clear_search_index(&conn).unwrap();
        assert_eq!(search(&conn, &SearchQuery::new("brown")).unwrap().total, 0);
        rebuild_search_index(&conn, &docs, &IndexTableExtrasProvider).unwrap();
        let page2 = search(&conn, &SearchQuery::new("brown")).unwrap();
        assert_eq!(page2.results, page.results);

        // CJK title survives rebuild too.
        assert!(search(&conn, &SearchQuery::new("重建")).unwrap().total >= 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn like_fallback_works_without_fts_table() {
        // Simulate a build where FTS5 is unavailable: drop the virtual table.
        let conn = setup();
        seed(&conn);
        conn.execute_batch("DROP TABLE task_search_fts").unwrap();
        let page = search(&conn, &SearchQuery::new("milk")).unwrap();
        assert_eq!(page.backend, "like-fallback");
        assert_eq!(page.total, 1);
        assert_eq!(page.results[0].task_key.task_id.as_str(), "1");
    }

    #[test]
    fn empty_query_returns_empty_page() {
        let conn = setup();
        seed(&conn);
        let page = search(&conn, &SearchQuery::new("   ")).unwrap();
        assert!(page.results.is_empty());
        assert_eq!(page.total, 0);
    }

    #[test]
    fn document_filter_scopes_results() {
        let conn = setup();
        seed(&conn);
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path) VALUES ('doc2', 'ws1', 'other.tdl')",
            [],
        )
        .unwrap();
        let mut t = Task::new(TaskId::new("9"));
        t.title = "Buy milk in other doc".into();
        let doc = SearchDocument::from_task("doc2", &t, &TaskSearchExtras::default());
        update_task_search(&conn, &doc).unwrap();

        let all = search(&conn, &SearchQuery::new("milk")).unwrap();
        assert_eq!(all.total, 2);
        let scoped = search(
            &conn,
            &SearchQuery::new("milk").with_document("doc2"),
        )
        .unwrap();
        assert_eq!(scoped.total, 1);
        assert_eq!(scoped.results[0].task_key.document_id.as_str(), "doc2");
    }

    #[test]
    fn extras_provider_reads_index_tables() {
        let conn = setup();
        conn.execute(
            "INSERT INTO task_index (task_key, document_id, title) VALUES ('5', 'doc1', 'x')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_tags (task_key, document_id, tag) VALUES ('5', 'doc1', 'alpha')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_participants (task_key, document_id, participant, role) \
             VALUES ('5', 'doc1', 'Bob', 'allocated_to')",
            [],
        )
        .unwrap();
        let extras = IndexTableExtrasProvider.extras_for(&conn, "doc1", "5");
        assert_eq!(extras.tags, vec!["alpha"]);
        assert_eq!(extras.participants, vec!["Bob"]);
    }

    #[test]
    fn fts_match_expression_is_safe() {
        // Quotes and special chars must not produce a syntax error.
        assert_eq!(build_fts_match(r#"say "hi" NEAR/x"#), r#""say" """hi""" "NEAR/x""#);
        let conn = setup();
        seed(&conn);
        let page = search(&conn, &SearchQuery::new(r#""quoted" and*"#)).unwrap();
        // Must not error out; degrades gracefully if MATCH fails.
        assert!(page.total == 0 || !page.results.is_empty());
    }
}
