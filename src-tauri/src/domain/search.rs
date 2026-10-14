//! M9: Search domain model (RD-M9-008~013, RD-M9-031~034 support).
//!
//! Pure domain types for global search and the command palette:
//! - [`SearchResult`] — the DTO returned by every search path (FTS5 or LIKE fallback).
//! - [`SearchQuery`] — bounded, paginated query description with deterministic ordering.
//! - [`MatchedField`] — which searchable field produced the hit.
//! - CJK detection helpers — FTS5's unicode61 tokenizer does not segment
//!   Chinese/Japanese/Korean text, so CJK queries must fall back to
//!   LIKE-based substring matching.
//! - [`fuzzy_score`] — subsequence fuzzy matcher used by the Ctrl+K palette.
//! - [`extract_plain_text`] — minimal comment-to-plain-text extractor.
//!
//! INTEGRATION SEAM (M7): when `domain::rich_text::extract_plain_text` lands,
//! the local [`extract_plain_text`] here should delegate to it. Until then
//! this module ships its own working HTML-stripping extractor.

use serde::{Deserialize, Serialize};

use super::task::{CommentType, Task, TaskComment};
use super::types::TaskKey;

/// Maximum number of results a single search page may return.
pub const MAX_PAGE_SIZE: usize = 200;
/// Default page size when the caller does not specify one.
pub const DEFAULT_PAGE_SIZE: usize = 20;

/// Which searchable field produced a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedField {
    Title,
    Description,
    Tags,
    Participants,
    Attachments,
    Links,
}

impl MatchedField {
    /// Stable sort rank (lower = more relevant field).
    pub fn rank(self) -> u8 {
        match self {
            MatchedField::Title => 0,
            MatchedField::Tags => 1,
            MatchedField::Participants => 2,
            MatchedField::Links => 3,
            MatchedField::Attachments => 4,
            MatchedField::Description => 5,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MatchedField::Title => "title",
            MatchedField::Description => "description",
            MatchedField::Tags => "tags",
            MatchedField::Participants => "participants",
            MatchedField::Attachments => "attachments",
            MatchedField::Links => "links",
        }
    }
}

/// A single search hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    /// Globally unique task identity (document_id + task_id).
    pub task_key: TaskKey,
    /// The task title (verbatim from the index).
    pub title: String,
    /// Document context for result grouping/display.
    pub document_context: DocumentContext,
    /// Which field matched.
    pub matched_field: MatchedField,
    /// Short human-readable excerpt around the match, with `[[` `]]` marks.
    pub snippet: String,
    /// Relevance score — higher is better. Deterministic tie-breakers are
    /// (matched_field rank, document_id, task_id) applied by the search engine.
    pub score: f64,
}

/// Document context attached to every search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentContext {
    pub document_id: String,
    /// File name (or full path if the name is unknown).
    pub document_name: String,
}

/// A bounded, paginated search request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchQuery {
    /// Raw user query text.
    pub text: String,
    /// Optional document filter.
    pub document_id: Option<String>,
    /// Page size; clamped to `1..=MAX_PAGE_SIZE`.
    pub limit: usize,
    /// Number of rows to skip (pagination).
    pub offset: usize,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            document_id: None,
            limit: DEFAULT_PAGE_SIZE,
            offset: 0,
        }
    }

    pub fn with_document(mut self, document_id: impl Into<String>) -> Self {
        self.document_id = Some(document_id.into());
        self
    }

    pub fn with_page(mut self, limit: usize, offset: usize) -> Self {
        self.limit = limit;
        self.offset = offset;
        self
    }

    /// Effective (clamped) page size.
    pub fn bounded_limit(&self) -> usize {
        self.limit.clamp(1, MAX_PAGE_SIZE)
    }

    /// True when the query text is empty or whitespace-only.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// A page of search results with the total hit count for pagination UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchPage {
    pub results: Vec<SearchResult>,
    /// Total number of matches (before pagination).
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    /// Which engine produced the page ("fts5" or "like-fallback").
    pub backend: String,
}

// ── CJK detection (RD-M9-010~012) ────────────────────────────────────────────

/// Returns true for characters in the common CJK Unicode blocks.
pub fn is_cjk_char(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF    // CJK Unified Ideographs
        | 0x3400..=0x4DBF  // CJK Unified Ideographs Extension A
        | 0xF900..=0xFAFF  // CJK Compatibility Ideographs
        | 0x3000..=0x303F  // CJK Symbols and Punctuation
        | 0xFF00..=0xFFEF  // Halfwidth and Fullwidth Forms
        | 0x3040..=0x30FF  // Hiragana + Katakana
        | 0xAC00..=0xD7AF  // Hangul Syllables
    )
}

/// Returns true when the query contains any CJK character, meaning the FTS5
/// unicode61 tokenizer cannot be trusted and the LIKE substring fallback
/// must be used.
pub fn query_needs_cjk_fallback(query: &str) -> bool {
    query.chars().any(is_cjk_char)
}

/// Split a query into match tokens: maximal runs of CJK characters become
/// one token each (no whitespace in CJK queries, but the user may type
/// "任务 待办"), and runs of non-CJK characters are split on whitespace.
/// All tokens must match (AND semantics) for the fallback path.
pub fn cjk_match_tokens(query: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_is_cjk: Option<bool> = None;

    for c in query.chars() {
        let cjk = is_cjk_char(c);
        if c.is_whitespace() {
            flush_token(&mut current, &mut tokens);
            current_is_cjk = None;
            continue;
        }
        match current_is_cjk {
            Some(prev) if prev == cjk => current.push(c),
            _ => {
                flush_token(&mut current, &mut tokens);
                current.push(c);
                current_is_cjk = Some(cjk);
            }
        }
    }
    flush_token(&mut current, &mut tokens);
    tokens
}

fn flush_token(current: &mut String, tokens: &mut Vec<String>) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

/// Build a snippet: a window around the first occurrence of any token,
/// with the matched span wrapped in `[[` `]]`.
pub fn make_snippet(text: &str, tokens: &[String], radius: usize) -> String {
    let lower = text.to_lowercase();
    let mut best: Option<(usize, usize)> = None;
    for tok in tokens {
        if tok.is_empty() {
            continue;
        }
        if let Some(pos) = lower.find(&tok.to_lowercase()) {
            if best.map_or(true, |(b, _)| pos < b) {
                best = Some((pos, tok.chars().count()));
            }
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let Some((byte_pos, tok_chars)) = best else {
        return chars.iter().take(2 * radius).collect();
    };

    let before = text[..byte_pos].chars().count();
    let start = before.saturating_sub(radius);
    let end = (before + tok_chars + radius).min(chars.len());

    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    for i in start..end {
        if i == before {
            out.push_str("[[");
        }
        if i == before + tok_chars {
            out.push_str("]]");
        }
        out.push(chars[i]);
    }
    if before + tok_chars >= end {
        out.push_str("]]");
    }
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// Choose the best matched field for a hit given the tokens and field texts.
pub fn classify_match(
    tokens: &[String],
    title: &str,
    body: &str,
    tags: &str,
    participants: &str,
    attachments: &str,
    links: &str,
) -> MatchedField {
    let hits = |field: &str| {
        let lower = field.to_lowercase();
        tokens.iter().any(|t| lower.contains(&t.to_lowercase()))
    };
    if hits(title) {
        MatchedField::Title
    } else if hits(tags) {
        MatchedField::Tags
    } else if hits(participants) {
        MatchedField::Participants
    } else if hits(links) {
        MatchedField::Links
    } else if hits(attachments) {
        MatchedField::Attachments
    } else if hits(body) {
        MatchedField::Description
    } else {
        MatchedField::Description
    }
}

// ── Plain text extraction (M7 seam) ──────────────────────────────────────────

/// Extract searchable plain text from a task comment.
///
/// - `PLAIN_TEXT`: content as-is.
/// - `HTML`: tags stripped, entities decoded, block elements → newlines.
/// - Unknown/RTF: empty string (not searchable, never mis-parsed).
///
/// SEAM: M7 (INH-1068, RD-M7-028) will provide the canonical extractor in
/// `domain::rich_text`; swap this body for a delegation when it lands.
pub fn extract_plain_text(comment: &TaskComment) -> String {
    match comment.comment_type {
        CommentType::Plain => comment.content.clone(),
        CommentType::Html => strip_html(&comment.content),
        CommentType::Unknown(_) => String::new(),
    }
}

/// Extract searchable plain text from a whole task (description part only).
pub fn task_description_text(task: &Task) -> String {
    task.comments.as_ref().map(extract_plain_text).unwrap_or_default()
}

/// Minimal, dependency-free HTML → plain text conversion.
pub fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let lower = html.to_lowercase();
    let mut script_depth = 0usize;

    // Very small state machine; handles <script>/<style> removal and
    // block-level line breaks. Entities are decoded afterwards.
    let bytes: Vec<char> = html.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_tag {
            if c == '>' {
                in_tag = false;
                // Look back at the tag we just closed. `i` is an index into the
                // char vector, so the tag must be reconstructed in char space —
                // byte-slicing `html`/`lower` with it panicked on any multibyte
                // text (e.g. an HTML comment containing CJK before a tag's `>`),
                // which took down the whole search-index rebuild.
                let tag_end = i;
                let tag_start = bytes[..tag_end].iter().rposition(|&ch| ch == '<').unwrap_or(0);
                let tag: String = bytes[tag_start..=tag_end].iter().collect::<String>().to_lowercase();
                if tag.starts_with("<script") {
                    script_depth += 1;
                    in_script = true;
                } else if tag.starts_with("</script") {
                    script_depth = script_depth.saturating_sub(1);
                    in_script = script_depth > 0;
                } else if tag.starts_with("<style") {
                    script_depth += 1;
                    in_script = true;
                } else if tag.starts_with("</style") {
                    script_depth = script_depth.saturating_sub(1);
                    in_script = script_depth > 0;
                } else if is_block_tag(&tag) && !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            i += 1;
            continue;
        }
        if c == '<' {
            in_tag = true;
            i += 1;
            continue;
        }
        if !in_script {
            out.push(c);
        }
        i += 1;
    }
    decode_entities(&out)
}

fn is_block_tag(tag: &str) -> bool {
    const BLOCKS: [&str; 12] = [
        "<p", "<div", "<br", "<li", "<ul", "<ol", "<h1", "<h2", "<h3", "<h4", "<h5", "<h6",
    ];
    BLOCKS.iter().any(|b| tag.starts_with(b))
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

// ── Fuzzy matching for the Ctrl+K palette (RD-M9-032) ────────────────────────

/// Subsequence fuzzy match score. Returns `None` when `query` is not a
/// subsequence of `text` (case-insensitive).
///
/// Scoring rewards, in order: exact/prefix matches, word-boundary matches,
/// consecutive runs, and shorter texts. Scores are positive; higher = better.
pub fn fuzzy_score(query: &str, text: &str) -> Option<f64> {
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0.0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();

    // Exact substring bonus path.
    let text_lower = text.to_lowercase();
    let query_lower: String = q.iter().collect();
    if let Some(pos) = text_lower.find(&query_lower) {
        let mut score = 100.0;
        if pos == 0 {
            score += 50.0; // prefix match
        } else if text[..pos].chars().next_back().map_or(false, |c| is_word_boundary(c)) {
            score += 25.0; // word-boundary match
        }
        score -= (pos as f64) * 0.1;
        score -= (t.len() as f64) * 0.01;
        return Some(score);
    }

    // Subsequence path.
    let mut score = 0.0;
    let mut ti = 0usize;
    let mut last_match: Option<usize> = None;
    for qc in &q {
        let mut found = None;
        while ti < t.len() {
            if t[ti] == *qc {
                found = Some(ti);
                ti += 1;
                break;
            }
            ti += 1;
        }
        let idx = found?; // not a subsequence
        score += 1.0;
        if idx > 0 && last_match == Some(idx - 1) {
            score += 3.0; // consecutive run
        }
        if idx == 0 || is_word_boundary(t[idx - 1]) {
            score += 4.0; // word boundary
        }
        last_match = Some(idx);
    }
    score -= (t.len() as f64) * 0.01;
    Some(score.max(0.1))
}

fn is_word_boundary(c: char) -> bool {
    c == ' ' || c == '_' || c == '-' || c == '.' || c == ':' || c == '/'
}

// ── Command Registry (RD-M9-031) ─────────────────────────────────────────────
//
// Lives in the domain layer so it is reachable from integration tests
// (the `commands` module is crate-private) and free of IO.

/// Palette categories.
pub mod category {
    pub const WORKSPACE: &str = "workspace";
    pub const TASK: &str = "task";
    pub const EDIT: &str = "edit";
    pub const FILE: &str = "file";
    pub const SEARCH: &str = "search";
    pub const VIEW: &str = "view";
}

/// A registered application command (data mirror of the frontend registry).
///
/// Serialize-only: the `&'static str` fields are a compile-time data table,
/// so the type is produced by the backend and consumed by the frontend.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub shortcut: Option<&'static str>,
    pub category: &'static str,
}

/// The canonical command list. Mirrors `src/app/commands.ts` one-for-one
/// (workspace.open/close/scanAndIndex/rebuildIndex, task.addRoot/delete,
/// edit.undo/redo, file.save) and adds the M9 palette/search/view commands.
pub fn command_registry() -> Vec<CommandDescriptor> {
    vec![
        CommandDescriptor { id: "workspace.open", label: "Open Workspace", icon: "fa-folder-open", shortcut: None, category: category::WORKSPACE },
        CommandDescriptor { id: "workspace.close", label: "Close Workspace", icon: "fa-folder-minus", shortcut: None, category: category::WORKSPACE },
        CommandDescriptor { id: "workspace.scanAndIndex", label: "Scan & Index", icon: "fa-magnifying-glass", shortcut: None, category: category::WORKSPACE },
        CommandDescriptor { id: "workspace.rebuildIndex", label: "Rebuild Index", icon: "fa-arrows-rotate", shortcut: None, category: category::WORKSPACE },
        CommandDescriptor { id: "task.addRoot", label: "Add Root Task", icon: "fa-plus", shortcut: Some("Ctrl+N"), category: category::TASK },
        CommandDescriptor { id: "task.quickAdd", label: "Quick Add Task", icon: "fa-bolt", shortcut: Some("Ctrl+Shift+N"), category: category::TASK },
        CommandDescriptor { id: "task.delete", label: "Delete Task", icon: "fa-trash", shortcut: Some("Delete"), category: category::TASK },
        CommandDescriptor { id: "edit.undo", label: "Undo", icon: "fa-rotate-left", shortcut: Some("Ctrl+Z"), category: category::EDIT },
        CommandDescriptor { id: "edit.redo", label: "Redo", icon: "fa-rotate-right", shortcut: Some("Ctrl+Y"), category: category::EDIT },
        CommandDescriptor { id: "file.save", label: "Save", icon: "fa-floppy-disk", shortcut: Some("Ctrl+S"), category: category::FILE },
        CommandDescriptor { id: "search.open", label: "Global Search", icon: "fa-magnifying-glass", shortcut: Some("Ctrl+K"), category: category::SEARCH },
        CommandDescriptor { id: "search.rebuildIndex", label: "Rebuild Search Index", icon: "fa-arrows-rotate", shortcut: None, category: category::SEARCH },
        CommandDescriptor { id: "view.today", label: "Today", icon: "fa-calendar-day", shortcut: None, category: category::VIEW },
        CommandDescriptor { id: "view.upcoming", label: "Upcoming", icon: "fa-calendar-week", shortcut: None, category: category::VIEW },
        CommandDescriptor { id: "view.overdue", label: "Overdue", icon: "fa-triangle-exclamation", shortcut: None, category: category::VIEW },
        CommandDescriptor { id: "view.unscheduled", label: "Unscheduled", icon: "fa-calendar", shortcut: None, category: category::VIEW },
        CommandDescriptor { id: "view.completed", label: "Completed", icon: "fa-circle-check", shortcut: None, category: category::VIEW },
        CommandDescriptor { id: "view.flagged", label: "Flagged", icon: "fa-flag", shortcut: None, category: category::VIEW },
    ]
}

/// A command registry conflict: two commands bound to the same shortcut.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShortcutConflict {
    pub shortcut: String,
    pub command_ids: Vec<String>,
}

/// Detect duplicate shortcut bindings inside the registry (QA-M9-012).
/// Comparison is case-insensitive on the accelerator string.
pub fn find_shortcut_conflicts(registry: &[CommandDescriptor]) -> Vec<ShortcutConflict> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for cmd in registry {
        if let Some(sc) = cmd.shortcut {
            map.entry(sc.to_lowercase()).or_default().push(cmd.id.to_string());
        }
    }
    map.into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|(shortcut, command_ids)| ShortcutConflict { shortcut, command_ids })
        .collect()
}

/// A command hit in the Ctrl+K palette.
#[derive(Debug, Clone, Serialize)]
pub struct CommandHit {
    pub command: CommandDescriptor,
    pub score: f64,
}

/// Fuzzy-search the command registry over label AND id (RD-M9-032).
/// Deterministic ordering: score desc, then id asc. Empty query lists all
/// commands in registry order (score 0).
pub fn search_commands(query: &str, limit: usize) -> Vec<CommandHit> {
    let registry = command_registry();
    let mut hits: Vec<CommandHit> = if query.trim().is_empty() {
        registry
            .into_iter()
            .map(|command| CommandHit { command, score: 0.0 })
            .collect()
    } else {
        registry
            .into_iter()
            .filter_map(|command| {
                let by_label = fuzzy_score(query, command.label);
                let by_id = fuzzy_score(query, &command.id.replace('.', " "));
                let score = match (by_label, by_id) {
                    (Some(a), Some(b)) => a.max(b),
                    (a, b) => a.or(b)?,
                };
                Some(CommandHit { command, score })
            })
            .collect()
    };
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.command.id.cmp(b.command.id))
    });
    hits.truncate(limit.clamp(1, 50));
    hits
}

// ── Task jump resolution (RD-M9-033) ─────────────────────────────────────────

/// Find the ancestor task-ID chain (root → parent) for a task by walking the
/// XML source of truth. Works even when the disposable index has no parent
/// links; returns `None` when the XML is invalid or the task is missing.
pub fn resolve_ancestors(xml_bytes: &[u8], task_id: &str) -> Option<Vec<String>> {
    let doc = super::xml_parser::parse_xml(xml_bytes).ok()?;
    let mut path: Vec<String> = Vec::new();
    walk_for_task(&doc.root, task_id, &mut path)
}

fn walk_for_task(
    element: &super::xml_tree::XmlElement,
    task_id: &str,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    if element.tag == "TASK" {
        let id = element.get_attr("ID")?;
        if id == task_id {
            return Some(path.clone());
        }
        path.push(id.to_string());
        for node in &element.children {
            if let super::xml_tree::XmlNode::Element(child) = node {
                if let Some(found) = walk_for_task(child, task_id, path) {
                    return Some(found);
                }
            }
        }
        path.pop();
        None
    } else {
        for node in &element.children {
            if let super::xml_tree::XmlNode::Element(child) = node {
                if let Some(found) = walk_for_task(child, task_id, path) {
                    return Some(found);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_detection_chinese() {
        assert!(query_needs_cjk_fallback("任务"));
        assert!(query_needs_cjk_fallback("待办事项"));
        assert!(query_needs_cjk_fallback("购物清单"));
        assert!(query_needs_cjk_fallback("发布 release 说明"));
        assert!(!query_needs_cjk_fallback("release notes"));
        assert!(!query_needs_cjk_fallback(""));
    }

    #[test]
    fn cjk_detection_japanese_korean() {
        assert!(query_needs_cjk_fallback("タスク"));
        assert!(query_needs_cjk_fallback("할일"));
    }

    #[test]
    fn match_tokens_mixed_query() {
        // CJK runs and ASCII runs become separate tokens (AND semantics).
        let toks = cjk_match_tokens("任务 release 说明v2");
        assert_eq!(toks, vec!["任务", "release", "说明", "v2"]);
    }

    #[test]
    fn match_tokens_pure_cjk_no_space() {
        assert_eq!(cjk_match_tokens("购物清单"), vec!["购物清单"]);
    }

    #[test]
    fn snippet_marks_match() {
        let text = "这是一个很长的中文任务描述，里面包含购物清单几个字，用于测试片段生成。";
        let snippet = make_snippet(text, &["购物清单".to_string()], 8);
        assert!(snippet.contains("[[购物清单]]"), "snippet was: {snippet}");
    }

    #[test]
    fn snippet_ascii() {
        let s = make_snippet("Buy milk and eggs", &["milk".to_string()], 10);
        assert!(s.contains("[[milk]]"));
    }

    #[test]
    fn classify_match_prefers_title() {
        let f = classify_match(&["task".to_string()], "My Task", "task body", "", "", "", "");
        assert_eq!(f, MatchedField::Title);
        let f = classify_match(&["body".to_string()], "title", "body text", "", "", "", "");
        assert_eq!(f, MatchedField::Description);
        let f = classify_match(&["work".to_string()], "t", "b", "work", "", "", "");
        assert_eq!(f, MatchedField::Tags);
    }

    #[test]
    fn strip_html_basic() {
        let html = "<p>Hello <strong>world</strong></p><p>Second line</p>";
        let text = strip_html(html);
        assert!(text.contains("Hello world"));
        assert!(text.contains("Second line"));
        assert!(!text.contains('<'));
    }

    #[test]
    fn strip_html_removes_script_and_entities() {
        let html = "<div>a&amp;b<script>alert('x')</script>c</div>";
        let text = strip_html(html);
        assert!(text.contains("a&b"));
        assert!(text.contains('c'));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn extract_plain_text_by_type() {
        let plain = TaskComment { comment_type: CommentType::Plain, content: "raw 中文".into() };
        assert_eq!(extract_plain_text(&plain), "raw 中文");
        let html = TaskComment { comment_type: CommentType::Html, content: "<p>hi</p>".into() };
        assert_eq!(extract_plain_text(&html).trim(), "hi");
        let unknown = TaskComment { comment_type: CommentType::Unknown("RTF".into()), content: "binary".into() };
        assert_eq!(extract_plain_text(&unknown), "");
    }

    #[test]
    fn fuzzy_exact_prefix_scores_highest() {
        let s_prefix = fuzzy_score("open", "Open Workspace").unwrap();
        let s_sub = fuzzy_score("ow", "Open Workspace").unwrap();
        assert!(s_prefix > s_sub);
    }

    #[test]
    fn fuzzy_subsequence_and_miss() {
        assert!(fuzzy_score("wks", "Open Workspace").is_some());
        assert!(fuzzy_score("xyz", "Open Workspace").is_none());
        assert_eq!(fuzzy_score("", "anything"), Some(0.0));
    }

    #[test]
    fn fuzzy_cjk() {
        assert!(fuzzy_score("工作", "打开工作区").is_some());
    }

    #[test]
    fn query_bounds_clamped() {
        let q = SearchQuery::new("x").with_page(10_000, 5);
        assert_eq!(q.bounded_limit(), MAX_PAGE_SIZE);
        assert!(SearchQuery::new("  ").is_empty());
    }

    #[test]
    fn registry_mirrors_frontend_commands() {
        let ids: Vec<&str> = command_registry().iter().map(|c| c.id).collect();
        // Exact mirror of src/app/commands.ts registrations.
        for expected in [
            "workspace.open", "workspace.close", "workspace.scanAndIndex",
            "workspace.rebuildIndex", "task.addRoot", "task.delete",
            "edit.undo", "edit.redo", "file.save",
        ] {
            assert!(ids.contains(&expected), "missing frontend command {expected}");
        }
        // M9 additions
        for expected in ["search.open", "task.quickAdd", "view.today"] {
            assert!(ids.contains(&expected));
        }
        // IDs unique
        let mut sorted = ids.clone();
        sorted.sort();
        let n = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), n);
    }

    #[test]
    fn canonical_registry_has_no_shortcut_conflicts() {
        assert!(find_shortcut_conflicts(&command_registry()).is_empty());
    }

    #[test]
    fn shortcut_conflict_detected() {
        let reg = vec![
            CommandDescriptor { id: "a.one", label: "One", icon: "i", shortcut: Some("Ctrl+P"), category: category::EDIT },
            CommandDescriptor { id: "a.two", label: "Two", icon: "i", shortcut: Some("ctrl+p"), category: category::EDIT },
            CommandDescriptor { id: "a.three", label: "Three", icon: "i", shortcut: Some("Ctrl+Q"), category: category::EDIT },
        ];
        let conflicts = find_shortcut_conflicts(&reg);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].shortcut, "ctrl+p");
        assert_eq!(conflicts[0].command_ids, vec!["a.one", "a.two"]);
    }

    #[test]
    fn palette_fuzzy_search_orders_by_relevance() {
        let hits = search_commands("undo", 10);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].command.id, "edit.undo");
        let hits = search_commands("open work", 10);
        assert_eq!(hits[0].command.id, "workspace.open");
        let hits = search_commands("save", 10);
        assert_eq!(hits[0].command.id, "file.save");
    }

    #[test]
    fn palette_empty_query_lists_all_bounded() {
        let all = search_commands("", 50);
        assert_eq!(all.len(), command_registry().len());
        let bounded = search_commands("", 3);
        assert_eq!(bounded.len(), 3);
    }

    #[test]
    fn palette_deterministic() {
        let a = search_commands("ta", 10);
        let b = search_commands("ta", 10);
        assert_eq!(
            a.iter().map(|h| h.command.id).collect::<Vec<_>>(),
            b.iter().map(|h| h.command.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn jump_ancestors_from_xml() {
        let xml = br#"<?xml version="1.0" encoding="utf-8"?>
<TDL>
  <TASK ID="1" TITLE="Root">
    <TASK ID="2" TITLE="Mid">
      <TASK ID="3" TITLE="Leaf"/>
    </TASK>
  </TASK>
  <TASK ID="9" TITLE="Other"/>
</TDL>"#;
        assert_eq!(resolve_ancestors(xml, "3"), Some(vec!["1".into(), "2".into()]));
        assert_eq!(resolve_ancestors(xml, "1"), Some(vec![]));
        assert_eq!(resolve_ancestors(xml, "9"), Some(vec![]));
        assert_eq!(resolve_ancestors(xml, "404"), None);
        assert_eq!(resolve_ancestors(b"<broken", "1"), None);
    }
}
