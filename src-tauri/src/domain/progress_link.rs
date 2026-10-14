//! Progress Link domain model for M6 Task Relations (RD-M6-023~026).
//!
//! A progress link points a task at an external tracker (GitHub PR/issue,
//! Linear issue, Jira issue, or any generic http/https resource).
//!
//! # XML storage (Tier B compatibility decision)
//!
//! Per the delivery plan risk #1, the M0 Tier B compatibility audit has not
//! formally approved a storage mechanism for M6 extension data. The
//! documented fallback is used: a custom XML attribute on the `TASK`
//! element (`MTDL_PROGRESS_LINKS`) carrying a JSON array, combined with the
//! existing `unknown_attrs` preservation machinery. Consequences:
//!
//! - Legacy TDL treats the attribute as unknown data and preserves it.
//! - ModernToDoList parses it into typed `ProgressLink`s; if the value is
//!   not valid link JSON it is preserved verbatim in `Task::unknown_attrs`
//!   (never dropped, never rewritten).
//! - The attribute is only written when the task actually has progress
//!   links, so documents without M6 data are byte-identical to before.
//!
//! # URL security
//!
//! `validate_url` is a security control: ONLY `http` and `https` schemes
//! are accepted. `javascript:`, `data:`, `vbscript:`, `file:` and every
//! other scheme are rejected, including obfuscated variants (case tricks,
//! embedded tabs/newlines/control characters such as `java\tscript:`).

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::task::Task;

/// Name of the custom TASK attribute that stores progress links (Tier B
/// fallback mechanism, see module docs).
pub const PROGRESS_LINKS_ATTR: &str = "MTDL_PROGRESS_LINKS";

/// Errors produced by URL validation. This is a security-relevant type:
/// anything not explicitly allowed is rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UrlValidationError {
    /// The URL is empty (after trimming surrounding whitespace).
    #[error("URL is empty")]
    Empty,
    /// The URL contains ASCII control characters or interior whitespace.
    /// This catches obfuscation such as `java\tscript:` or embedded
    /// newlines, which browsers may strip before resolving the scheme.
    #[error("URL contains control characters or whitespace")]
    ControlOrWhitespace,
    /// The URL has no `scheme:` prefix at all.
    #[error("URL is missing a scheme (only http and https are allowed)")]
    MissingScheme,
    /// The scheme is present but not `http`/`https` (e.g. `javascript`,
    /// `data`, `vbscript`, `file`).
    #[error("unsupported URL scheme '{0}': only http and https are allowed")]
    UnsupportedScheme(String),
    /// The scheme is http/https but not followed by `//authority`.
    #[error("URL is missing the '//' authority separator")]
    MalformedAuthority,
    /// The authority (host) part is empty, e.g. `http:///path`.
    #[error("URL has an empty host")]
    EmptyHost,
}

/// Validates a URL, allowing ONLY the `http` and `https` schemes.
///
/// Returns the normalized (surrounding-whitespace-trimmed) URL on success.
///
/// Validation order matters: control characters and interior whitespace
/// are rejected BEFORE scheme parsing, so obfuscated payloads like
/// `java\tscript:alert(1)` or `java\nscript:...` can never reach the
/// scheme check. The scheme comparison is case-insensitive (`HTTPS://`
/// is fine) but exact (`httpx` is not).
pub fn validate_url(raw: &str) -> Result<String, UrlValidationError> {
    // 1. Trim surrounding ASCII whitespace only.
    let trimmed = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    if trimmed.is_empty() {
        return Err(UrlValidationError::Empty);
    }

    // 2. Reject ANY control character (C0 range + DEL) or ANY whitespace
    //    inside the URL. Legitimate http(s) URLs never require raw control
    //    chars or spaces; browsers strip them, which is exactly the
    //    `java\tscript:` bypass we must not allow.
    if trimmed
        .chars()
        .any(|c| c.is_ascii_control() || c.is_whitespace())
    {
        return Err(UrlValidationError::ControlOrWhitespace);
    }

    // 3. Scheme check (case-insensitive, exact match).
    let (scheme, rest) = match trimmed.split_once(':') {
        Some(pair) => pair,
        None => return Err(UrlValidationError::MissingScheme),
    };
    let scheme_lower = scheme.to_ascii_lowercase();
    if scheme_lower != "http" && scheme_lower != "https" {
        return Err(UrlValidationError::UnsupportedScheme(scheme.to_string()));
    }

    // 4. Authority must be present and non-empty.
    let authority_and_rest = rest
        .strip_prefix("//")
        .ok_or(UrlValidationError::MalformedAuthority)?;
    let host_end = authority_and_rest
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(authority_and_rest.len());
    let authority = &authority_and_rest[..host_end];
    // userinfo@ is legal; the part after the last '@' is the host.
    let host = match authority.rsplit_once('@') {
        Some((_, h)) => h,
        None => authority,
    };
    // Strip optional port for the emptiness check.
    let host_only = match host.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() => h,
        _ => host,
    };
    if host_only.is_empty() {
        return Err(UrlValidationError::EmptyHost);
    }

    Ok(trimmed.to_string())
}

/// The external provider of a progress link, detected from the URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkProvider {
    /// github.com (including gist. and www. subdomains).
    Github,
    /// linear.app (including app.linear.app).
    Linear,
    /// Jira (*.atlassian.net or hosts starting with `jira`).
    Jira,
    /// Any other validated http/https URL.
    Generic,
}

impl LinkProvider {
    /// Stable lowercase name used in the index and JSON storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            LinkProvider::Github => "github",
            LinkProvider::Linear => "linear",
            LinkProvider::Jira => "jira",
            LinkProvider::Generic => "generic",
        }
    }

    /// Parses a stored provider name; unknown values map to `Generic`.
    pub fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "github" => LinkProvider::Github,
            "linear" => LinkProvider::Linear,
            "jira" => LinkProvider::Jira,
            _ => LinkProvider::Generic,
        }
    }

    /// Detects the provider from a URL pattern.
    ///
    /// The URL is assumed to have passed `validate_url` (http/https only).
    /// Detection is host-suffix based to avoid substring false positives
    /// (e.g. `https://evil.com/?x=github.com` must NOT detect as GitHub).
    pub fn detect(url: &str) -> Self {
        let Some(host) = url_host(url) else {
            return LinkProvider::Generic;
        };
        let host = host.to_ascii_lowercase();
        if host == "github.com" || host.ends_with(".github.com") {
            return LinkProvider::Github;
        }
        if host == "linear.app" || host.ends_with(".linear.app") {
            return LinkProvider::Linear;
        }
        if host.ends_with(".atlassian.net") || host.starts_with("jira.") || host == "jira" {
            return LinkProvider::Jira;
        }
        LinkProvider::Generic
    }
}

/// Extracts the lowercased host (no port, no userinfo) from an http/https URL.
fn url_host(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://").map(|p| p.1)?;
    let end = after_scheme
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..end];
    let without_user = match authority.rsplit_once('@') {
        Some((_, h)) => h,
        None => authority,
    };
    let host = match without_user.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() => h,
        _ => without_user,
    };
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// A progress link attached to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressLink {
    /// Stable unique id (UUID string) within the task.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Validated http/https URL.
    pub url: String,
    /// Provider detected from the URL.
    pub provider: LinkProvider,
}

impl ProgressLink {
    /// Creates a validated progress link. The provider is detected from
    /// the URL; the URL must pass `validate_url`.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        url: &str,
    ) -> Result<Self, UrlValidationError> {
        let url = validate_url(url)?;
        let provider = LinkProvider::detect(&url);
        Ok(Self {
            id: id.into(),
            label: label.into(),
            url,
            provider,
        })
    }

    /// Creates a validated progress link with a fresh UUID id.
    pub fn new_auto_id(label: impl Into<String>, url: &str) -> Result<Self, UrlValidationError> {
        Self::new(uuid::Uuid::new_v4().to_string(), label, url)
    }
}

/// Minimal JSON form stored in the custom XML attribute. `provider` is
/// NOT persisted — it is always re-derived from the URL so the stored
/// form stays small and detection rules can evolve.
#[derive(Serialize, Deserialize)]
struct StoredLink {
    id: String,
    label: String,
    url: String,
}

/// Serializes links to the `MTDL_PROGRESS_LINKS` attribute value.
pub fn links_to_attr_value(links: &[ProgressLink]) -> String {
    let stored: Vec<StoredLink> = links
        .iter()
        .map(|l| StoredLink {
            id: l.id.clone(),
            label: l.label.clone(),
            url: l.url.clone(),
        })
        .collect();
    serde_json::to_string(&stored).unwrap_or_else(|_| "[]".to_string())
}

/// Parses links from the `MTDL_PROGRESS_LINKS` attribute value.
///
/// Returns `None` when the value is not a valid link array or when ANY
/// stored URL fails re-validation — in that case the caller (the mapper)
/// preserves the raw attribute in `unknown_attrs` instead of dropping it.
pub fn links_from_attr_value(value: &str) -> Option<Vec<ProgressLink>> {
    let stored: Vec<StoredLink> = serde_json::from_str(value).ok()?;
    let mut links = Vec::with_capacity(stored.len());
    for s in stored {
        let url = validate_url(&s.url).ok()?;
        links.push(ProgressLink {
            id: s.id,
            label: s.label,
            provider: LinkProvider::detect(&url),
            url,
        });
    }
    Some(links)
}

/// Adds a progress link to a task. Returns false if the id already exists.
pub fn add_progress_link(task: &mut Task, link: ProgressLink) -> bool {
    if task.progress_links.iter().any(|l| l.id == link.id) {
        return false;
    }
    task.progress_links.push(link);
    true
}

/// Removes a progress link by id, returning the removed link.
pub fn remove_progress_link(task: &mut Task, id: &str) -> Option<ProgressLink> {
    let pos = task.progress_links.iter().position(|l| l.id == id)?;
    Some(task.progress_links.remove(pos))
}

/// One row of the `progress_links_index` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressLinkIndexRow {
    /// Link id (primary key of the index table).
    pub id: String,
    /// The task key within the document.
    pub task_key: String,
    /// The owning document id.
    pub document_id: String,
    /// The validated URL.
    pub url: String,
    /// Detected provider name.
    pub provider: String,
}

/// Computes the `progress_links_index` rows for one task.
pub fn index_rows(
    document_id: &str,
    task_key: &str,
    links: &[ProgressLink],
) -> Vec<ProgressLinkIndexRow> {
    links
        .iter()
        .map(|l| ProgressLinkIndexRow {
            id: l.id.clone(),
            task_key: task_key.to_string(),
            document_id: document_id.to_string(),
            url: l.url.clone(),
            provider: l.provider.as_str().to_string(),
        })
        .collect()
}

/// Replaces the `progress_links_index` rows for one task in the index DB.
/// Returns the number of rows inserted.
pub fn populate_progress_links_index(
    conn: &rusqlite::Connection,
    document_id: &str,
    task_key: &str,
    links: &[ProgressLink],
) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM progress_links_index WHERE document_id = ?1 AND task_key = ?2",
        rusqlite::params![document_id, task_key],
    )?;
    let rows = index_rows(document_id, task_key, links);
    for row in &rows {
        conn.execute(
            "INSERT OR REPLACE INTO progress_links_index (id, task_key, document_id, url, provider) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![row.id, row.task_key, row.document_id, row.url, row.provider],
        )?;
    }
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::TaskId;

    #[test]
    fn accepts_plain_http_and_https() {
        assert_eq!(
            validate_url("https://github.com/a/b/pull/1").unwrap(),
            "https://github.com/a/b/pull/1"
        );
        assert!(validate_url("http://example.com").is_ok());
        assert!(validate_url("HTTPS://Example.COM/Path?Q=1#F").is_ok());
        assert_eq!(
            validate_url("   https://example.com/x  ").unwrap(),
            "https://example.com/x"
        );
        assert!(validate_url("https://user:pw@example.com:8080/p").is_ok());
    }

    #[test]
    fn rejects_dangerous_schemes() {
        for bad in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/html;base64,PHNjcmlwdD4=",
            "DATA:,x",
            "vbscript:msgbox(1)",
            "file:///C:/Windows/system32",
            "ftp://example.com/file",
            "about:blank",
            "blob:https://example.com/uuid",
        ] {
            assert!(validate_url(bad).is_err(), "should reject: {bad}");
        }
    }

    #[test]
    fn rejects_obfuscated_javascript_variants() {
        // Embedded tab / newline / CR / NUL — the classic filter bypasses.
        for bad in [
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            "java\rscript:alert(1)",
            "javascript\u{0}:alert(1)",
            "  javascript\t:alert(1)  ",
            "ht\ttp://example.com",
            "https://exa mple.com",
            "https://example.com/pa th",
        ] {
            assert!(
                validate_url(bad).is_err(),
                "should reject obfuscated: {:?}",
                bad
            );
        }
    }

    #[test]
    fn rejects_malformed_urls() {
        assert_eq!(validate_url(""), Err(UrlValidationError::Empty));
        assert_eq!(validate_url("   "), Err(UrlValidationError::Empty));
        assert_eq!(
            validate_url("example.com/no-scheme"),
            Err(UrlValidationError::MissingScheme)
        );
        assert_eq!(
            validate_url("http:example.com"),
            Err(UrlValidationError::MalformedAuthority)
        );
        assert_eq!(
            validate_url("http:///path-only"),
            Err(UrlValidationError::EmptyHost)
        );
        assert_eq!(
            validate_url("httpx://example.com"),
            Err(UrlValidationError::UnsupportedScheme("httpx".into()))
        );
        assert_eq!(
            validate_url("https:/example.com"),
            Err(UrlValidationError::MalformedAuthority)
        );
    }

    #[test]
    fn provider_detection() {
        assert_eq!(
            LinkProvider::detect("https://github.com/org/repo/pull/12"),
            LinkProvider::Github
        );
        assert_eq!(
            LinkProvider::detect("https://gist.github.com/user/abc"),
            LinkProvider::Github
        );
        assert_eq!(
            LinkProvider::detect("https://linear.app/team/issue/INH-1048"),
            LinkProvider::Linear
        );
        assert_eq!(
            LinkProvider::detect("https://team.atlassian.net/browse/PROJ-1"),
            LinkProvider::Jira
        );
        assert_eq!(
            LinkProvider::detect("https://jira.internal.corp/browse/X-1"),
            LinkProvider::Jira
        );
        assert_eq!(
            LinkProvider::detect("https://example.com/task/1"),
            LinkProvider::Generic
        );
        // Substring in path/query must NOT trigger detection.
        assert_eq!(
            LinkProvider::detect("https://evil.com/?ref=github.com"),
            LinkProvider::Generic
        );
        assert_eq!(
            LinkProvider::detect("https://notgithub.com.evil.com/x"),
            LinkProvider::Generic
        );
    }

    #[test]
    fn progress_link_new_validates() {
        let ok = ProgressLink::new("id1", "PR #12", "https://github.com/a/b/pull/12").unwrap();
        assert_eq!(ok.provider, LinkProvider::Github);
        assert!(ProgressLink::new("id2", "bad", "javascript:alert(1)").is_err());
        assert!(ProgressLink::new_auto_id("lbl", "https://linear.app/x").is_ok());
    }

    #[test]
    fn attr_codec_roundtrip_and_revalidation() {
        let links = vec![
            ProgressLink::new("a", "GitHub PR", "https://github.com/o/r/pull/1").unwrap(),
            ProgressLink::new("b", "Tracker", "https://example.com/issue/9").unwrap(),
        ];
        let json = links_to_attr_value(&links);
        let back = links_from_attr_value(&json).unwrap();
        assert_eq!(back, links);

        // Corrupt JSON → None (mapper will preserve raw attr as unknown).
        assert!(links_from_attr_value("not json").is_none());
        assert!(links_from_attr_value("{}").is_none());
        // Stored URL that no longer validates (e.g. hand-edited XML) → None.
        assert!(links_from_attr_value(
            r#"[{"id":"x","label":"l","url":"javascript:alert(1)"}]"#
        )
        .is_none());
    }

    #[test]
    fn add_remove_on_task() {
        let mut t = Task::new(TaskId::new("1"));
        let link = ProgressLink::new("id-1", "L", "https://example.com").unwrap();
        assert!(add_progress_link(&mut t, link.clone()));
        assert!(!add_progress_link(&mut t, link.clone())); // duplicate id
        assert_eq!(t.progress_links.len(), 1);
        let removed = remove_progress_link(&mut t, "id-1").unwrap();
        assert_eq!(removed, link);
        assert!(remove_progress_link(&mut t, "id-1").is_none());
        assert!(t.progress_links.is_empty());
    }

    #[test]
    fn index_rows_and_populate() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE progress_links_index (
                id TEXT PRIMARY KEY, task_key TEXT NOT NULL, document_id TEXT NOT NULL,
                url TEXT NOT NULL, provider TEXT);",
        )
        .unwrap();
        let links = vec![
            ProgressLink::new("l1", "PR", "https://github.com/a/b/pull/1").unwrap(),
            ProgressLink::new("l2", "Issue", "https://example.com/i/2").unwrap(),
        ];
        let rows = index_rows("doc-9", "3", &links);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].provider, "github");
        assert_eq!(rows[1].provider, "generic");

        let n = populate_progress_links_index(&conn, "doc-9", "3", &links).unwrap();
        assert_eq!(n, 2);
        // Re-populate is a replace, not a duplicate.
        populate_progress_links_index(&conn, "doc-9", "3", &links).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM progress_links_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
