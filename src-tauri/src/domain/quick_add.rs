//! M9: Quick Add parsing domain logic (RD-M9-035~041).
//!
//! Grammar: `Task title #tag @participant !priority due:2024-01-15`
//!
//! - `#tag`          — adds a tag/category
//! - `@participant`  — adds a participant (allocated-to)
//! - `!priority`     — `!0`..`!10` or named (`!none !low !medium !high !vhigh`)
//! - `due:YYYY-MM-DD` / `start:YYYY-MM-DD` — dates (`due:today` / `due:tomorrow`
//!   also accepted, resolved against an injected "now" for deterministic tests)
//! - `#"multi word"` / `@"John Doe"` — quoted forms for values with spaces
//!
//! SAFE FALLBACK (hard requirement): any token that looks special but cannot
//! be parsed (bad priority word, invalid date, bare `#`/`@`/`!`, an email
//! address containing `@`) stays verbatim in the title. User text is never
//! lost.

use chrono::{Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use super::smart_view::naive_date_to_ole;
use super::task::{Task, TaskCategory, TaskPriority};
use super::types::TaskId;

/// The parsed result of a quick-add input line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickAddDraft {
    /// Remaining title text (never loses unrecognized tokens).
    pub title: String,
    /// Tags in first-occurrence order, deduplicated.
    pub tags: Vec<String>,
    /// Participants in first-occurrence order, deduplicated.
    pub participants: Vec<String>,
    /// Parsed priority on the TDL 0-10 scale.
    pub priority: Option<u8>,
    /// Parsed `due:` date.
    pub due_date: Option<NaiveDate>,
    /// Parsed `start:` date.
    pub start_date: Option<NaiveDate>,
    /// Tokens that looked special but failed to parse and were therefore
    /// kept in the title (surfaced for UI hints/tests).
    pub rejected_tokens: Vec<String>,
}

impl QuickAddDraft {
    /// True when nothing usable was typed.
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty()
    }
}

/// Where the new task should be inserted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickAddTarget {
    pub document_id: String,
    /// Selected parent task; `None` means insert at root level.
    pub parent_task_id: Option<String>,
}

impl QuickAddTarget {
    pub fn root(document_id: impl Into<String>) -> Self {
        Self { document_id: document_id.into(), parent_task_id: None }
    }

    pub fn under_parent(document_id: impl Into<String>, parent_task_id: impl Into<String>) -> Self {
        Self { document_id: document_id.into(), parent_task_id: Some(parent_task_id.into()) }
    }
}

/// Parse a quick-add input line against the current local date.
pub fn parse_quick_add(input: &str) -> QuickAddDraft {
    parse_quick_add_with_now(input, Local::now().date_naive())
}

/// Parse a quick-add input line with an injected "now" (deterministic tests).
pub fn parse_quick_add_with_now(input: &str, now: NaiveDate) -> QuickAddDraft {
    let mut draft = QuickAddDraft {
        title: String::new(),
        tags: Vec::new(),
        participants: Vec::new(),
        priority: None,
        due_date: None,
        start_date: None,
        rejected_tokens: Vec::new(),
    };
    let mut title_tokens: Vec<String> = Vec::new();

    for raw in tokenize(input) {
        let token = raw.as_str();
        if token.is_empty() {
            continue; // empty tokens (e.g. from `#""`) are dropped, not lost:
                      // they carry no user-visible text
        }
        match classify(token, now) {
            TokenKind::Tag(name) => push_unique(&mut draft.tags, name),
            TokenKind::Participant(name) => push_unique(&mut draft.participants, name),
            TokenKind::Priority(value) => draft.priority = Some(value),
            TokenKind::Due(date) => draft.due_date = Some(date),
            TokenKind::Start(date) => draft.start_date = Some(date),
            TokenKind::Rejected => {
                draft.rejected_tokens.push(token.to_string());
                title_tokens.push(token.to_string());
            }
            TokenKind::Title => title_tokens.push(token.to_string()),
        }
    }

    draft.title = title_tokens.join(" ");
    draft
}

fn push_unique(vec: &mut Vec<String>, value: String) {
    if !vec.iter().any(|v| v.eq_ignore_ascii_case(&value)) {
        vec.push(value);
    }
}

#[derive(Debug, PartialEq)]
enum TokenKind {
    Tag(String),
    Participant(String),
    Priority(u8),
    Due(NaiveDate),
    Start(NaiveDate),
    /// Looked special but invalid — safe fallback keeps it in the title.
    Rejected,
    Title,
}

fn classify(token: &str, now: NaiveDate) -> TokenKind {
    let bytes = token.as_bytes();

    // due: / start: prefixes (case-insensitive)
    if let Some(rest) = strip_prefix_ci(token, "due:") {
        return match parse_date_token(rest, now) {
            Some(d) => TokenKind::Due(d),
            None => TokenKind::Rejected,
        };
    }
    if let Some(rest) = strip_prefix_ci(token, "start:") {
        return match parse_date_token(rest, now) {
            Some(d) => TokenKind::Start(d),
            None => TokenKind::Rejected,
        };
    }

    match bytes[0] {
        b'#' => {
            let name = &token[1..];
            match normalize_quoted_name(name) {
                Some(n) => TokenKind::Tag(n),
                None => TokenKind::Rejected,
            }
        }
        b'@' => {
            let name = &token[1..];
            // Reject email-like values ("@a@b") — an email such as
            // "john@example.com" never starts with '@' so it is a Title
            // token anyway; this guards pasted "@john@example.com".
            match normalize_quoted_name(name) {
                Some(n) if !n.contains('@') => TokenKind::Participant(n),
                _ => TokenKind::Rejected,
            }
        }
        b'!' => {
            match parse_priority(&token[1..]) {
                Some(p) => TokenKind::Priority(p),
                None => TokenKind::Rejected,
            }
        }
        _ => TokenKind::Title,
    }
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    // `get` is char-boundary safe: returns None when the byte slice would
    // split a multi-byte character (e.g. CJK input).
    let head = s.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        s.get(prefix.len()..)
    } else {
        None
    }
}

/// Valid tag/participant name after normalization: non-empty, no control
/// chars, no quote chars. CJK names are fine (char-based check).
///
/// Internal whitespace is ONLY possible via the quoted form (`#"q4 plan"`):
/// the tokenizer keeps quoted whitespace inside one token, and unquoted
/// input can never produce a prefixed token containing spaces. Trimming
/// keeps the name clean.
fn normalize_quoted_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().any(|c| c.is_control() || c == '"' || c == '\'') {
        return None;
    }
    Some(trimmed.to_string())
}

fn parse_priority(word: &str) -> Option<u8> {
    if let Ok(n) = word.parse::<u8>() {
        return if n <= 10 { Some(n) } else { None };
    }
    Some(match word.to_ascii_lowercase().as_str() {
        "none" => 0,
        "low" => 3,
        "medium" | "med" | "normal" => 5,
        "high" => 7,
        "vhigh" | "veryhigh" | "very-high" | "urgent" => 10,
        _ => return None,
    })
}

fn parse_date_token(value: &str, now: NaiveDate) -> Option<NaiveDate> {
    match value.to_ascii_lowercase().as_str() {
        "today" => return Some(now),
        "tomorrow" | "tmr" => return now.checked_add_signed(Duration::days(1)),
        _ => {}
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

/// Whitespace tokenizer with double-quote support. Quotes are consumed;
/// an unterminated quote consumes the remainder (text is never lost).
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_content = false; // distinguishes "" from nothing

    for c in input.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_content = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_content || !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                has_content = false;
            }
            c => {
                current.push(c);
                has_content = true;
            }
        }
    }
    if has_content || !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Build a domain [`Task`] from a parsed draft, ready to be wrapped in an
/// `AddTaskCommand` by the session layer (M3 seam — see commands/quick_add.rs).
pub fn build_task(draft: &QuickAddDraft, id: TaskId) -> Task {
    let mut task = Task::new(id);
    task.title = draft.title.clone();
    if let Some(p) = draft.priority {
        task.priority = TaskPriority::new(p);
    }
    for tag in &draft.tags {
        task.categories.push(TaskCategory { name: tag.clone() });
    }
    task.allocated_to = draft.participants.clone();
    if let Some(due) = draft.due_date {
        task.due_date = Some(naive_date_to_ole(due));
        task.due_date_string = Some(due.format("%Y-%m-%d").to_string());
    }
    if let Some(start) = draft.start_date {
        task.start_date = Some(naive_date_to_ole(start));
        task.start_date_string = Some(start.format("%Y-%m-%d").to_string());
    }
    task
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 1, 10).unwrap()
    }

    #[test]
    fn full_grammar() {
        let d = parse_quick_add_with_now(
            "Buy groceries #shopping @Alice !high due:2024-01-15",
            now(),
        );
        assert_eq!(d.title, "Buy groceries");
        assert_eq!(d.tags, vec!["shopping"]);
        assert_eq!(d.participants, vec!["Alice"]);
        assert_eq!(d.priority, Some(7));
        assert_eq!(d.due_date, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));
        assert!(d.rejected_tokens.is_empty());
    }

    #[test]
    fn title_only() {
        let d = parse_quick_add_with_now("Just a plain title", now());
        assert_eq!(d.title, "Just a plain title");
        assert!(d.tags.is_empty() && d.participants.is_empty());
        assert!(d.priority.is_none() && d.due_date.is_none());
    }

    #[test]
    fn cjk_title_preserved() {
        let d = parse_quick_add_with_now("购买清单 #购物 @张三 !high", now());
        assert_eq!(d.title, "购买清单");
        assert_eq!(d.tags, vec!["购物"]);
        assert_eq!(d.participants, vec!["张三"]);
    }

    #[test]
    fn mixed_cjk_english_title() {
        let d = parse_quick_add_with_now("修复 login 页面 bug #前端", now());
        assert_eq!(d.title, "修复 login 页面 bug");
        assert_eq!(d.tags, vec!["前端"]);
    }

    #[test]
    fn email_is_not_a_participant() {
        let d = parse_quick_add_with_now("Email john@example.com about the report", now());
        assert_eq!(d.title, "Email john@example.com about the report");
        assert!(d.participants.is_empty());
    }

    #[test]
    fn malformed_at_email_is_rejected_to_title() {
        let d = parse_quick_add_with_now("Contact @john@example.com now", now());
        assert_eq!(d.title, "Contact @john@example.com now");
        assert!(d.participants.is_empty());
        assert_eq!(d.rejected_tokens, vec!["@john@example.com"]);
    }

    #[test]
    fn unrecognized_priority_stays_in_title() {
        let d = parse_quick_add_with_now("Fix build !whenever due:tomorrow", now());
        assert_eq!(d.title, "Fix build !whenever");
        assert!(d.priority.is_none());
        assert_eq!(d.due_date, Some(NaiveDate::from_ymd_opt(2024, 1, 11).unwrap()));
        assert_eq!(d.rejected_tokens, vec!["!whenever"]);
    }

    #[test]
    fn invalid_date_stays_in_title() {
        let d = parse_quick_add_with_now("Party due:2024-13-99", now());
        assert_eq!(d.title, "Party due:2024-13-99");
        assert!(d.due_date.is_none());
    }

    #[test]
    fn bare_specials_stay_in_title() {
        let d = parse_quick_add_with_now("What # @ ! means this", now());
        assert_eq!(d.title, "What # @ ! means this");
        assert!(d.tags.is_empty() && d.participants.is_empty() && d.priority.is_none());
    }

    #[test]
    fn empty_and_whitespace_input() {
        let d = parse_quick_add_with_now("", now());
        assert!(d.is_empty());
        let d = parse_quick_add_with_now("   \t  ", now());
        assert!(d.is_empty());
    }

    #[test]
    fn duplicate_tags_and_participants_dedup() {
        let d = parse_quick_add_with_now("Task #work #Work #work @bob @Bob", now());
        assert_eq!(d.tags, vec!["work"]);
        assert_eq!(d.participants, vec!["bob"]);
    }

    #[test]
    fn quoted_tag_and_participant() {
        let d = parse_quick_add_with_now(r#"Review #"q4 plan" @"John Doe" !5"#, now());
        assert_eq!(d.title, "Review");
        assert_eq!(d.tags, vec!["q4 plan"]);
        assert_eq!(d.participants, vec!["John Doe"]);
        assert_eq!(d.priority, Some(5));
    }

    #[test]
    fn priority_numeric_and_named() {
        assert_eq!(parse_quick_add_with_now("a !0", now()).priority, Some(0));
        assert_eq!(parse_quick_add_with_now("a !10", now()).priority, Some(10));
        assert_eq!(parse_quick_add_with_now("a !low", now()).priority, Some(3));
        assert_eq!(parse_quick_add_with_now("a !medium", now()).priority, Some(5));
        assert_eq!(parse_quick_add_with_now("a !high", now()).priority, Some(7));
        assert_eq!(parse_quick_add_with_now("a !vhigh", now()).priority, Some(10));
        // 11 is out of range -> rejected into title
        let d = parse_quick_add_with_now("a !11", now());
        assert!(d.priority.is_none());
        assert_eq!(d.title, "a !11");
    }

    #[test]
    fn relative_dates() {
        assert_eq!(parse_quick_add_with_now("a due:today", now()).due_date, Some(now()));
        assert_eq!(
            parse_quick_add_with_now("a start:tomorrow", now()).start_date,
            Some(NaiveDate::from_ymd_opt(2024, 1, 11).unwrap())
        );
    }

    #[test]
    fn build_task_maps_all_fields() {
        let d = parse_quick_add_with_now(
            "Ship release #m9 @QA !high due:2024-02-01 start:2024-01-20",
            now(),
        );
        let task = build_task(&d, TaskId::new("99"));
        assert_eq!(task.title, "Ship release");
        assert_eq!(task.priority.value(), 7);
        assert_eq!(task.categories.len(), 1);
        assert_eq!(task.categories[0].name, "m9");
        assert_eq!(task.allocated_to, vec!["QA".to_string()]);
        assert_eq!(task.due_date, Some(naive_date_to_ole(NaiveDate::from_ymd_opt(2024, 2, 1).unwrap())));
        assert_eq!(task.due_date_string.as_deref(), Some("2024-02-01"));
        assert_eq!(task.start_date_string.as_deref(), Some("2024-01-20"));
    }

    #[test]
    fn target_defaults() {
        assert_eq!(QuickAddTarget::root("doc-1").parent_task_id, None);
        assert_eq!(
            QuickAddTarget::under_parent("doc-1", "5").parent_task_id.as_deref(),
            Some("5")
        );
    }

    #[test]
    fn tokenize_empty_quotes_do_not_lose_neighbors() {
        let toks = tokenize(r#"a "" b"#);
        assert_eq!(toks, vec!["a", "", "b"]);
    }
}
