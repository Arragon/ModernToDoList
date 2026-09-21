//! Release notes generation (RD-M10-040 / INH-1123).
//!
//! Provides the release-notes template ([`ReleaseNotes::render_markdown`]) and
//! the filled-in 2.0.0 GA changelog ([`release_notes_2_0_0`]). The rendered
//! markdown is mirrored into `docs/release/RELEASE_NOTES_2.0.0.md`.

use serde::{Deserialize, Serialize};

/// A single changelog entry: short summary plus optional issue reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangelogEntry {
    pub summary: String,
    /// Linear issue key, e.g. `INH-1117`, when applicable.
    pub issue: Option<String>,
}

impl ChangelogEntry {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            issue: None,
        }
    }

    pub fn with_issue(summary: impl Into<String>, issue: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            issue: Some(issue.into()),
        }
    }
}

/// Structured release notes, rendered to the project's markdown template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotes {
    pub product: String,
    pub version: String,
    /// ISO date, e.g. `2026-01-15`.
    pub release_date: String,
    pub highlights: Vec<String>,
    pub new_features: Vec<ChangelogEntry>,
    pub improvements: Vec<ChangelogEntry>,
    pub fixes: Vec<ChangelogEntry>,
    pub breaking_changes: Vec<ChangelogEntry>,
    pub known_issues: Vec<ChangelogEntry>,
    /// Download/install section body (Portable ZIP instructions).
    pub installation: Vec<String>,
}

impl ReleaseNotes {
    /// An empty template with all sections present.
    pub fn template(version: &str, release_date: &str) -> Self {
        Self {
            product: "ModernToDoList".to_string(),
            version: version.to_string(),
            release_date: release_date.to_string(),
            highlights: Vec::new(),
            new_features: Vec::new(),
            improvements: Vec::new(),
            fixes: Vec::new(),
            breaking_changes: Vec::new(),
            known_issues: Vec::new(),
            installation: Vec::new(),
        }
    }

    fn render_section(
        md: &mut String,
        title: &str,
        entries: &[ChangelogEntry],
        placeholder: &str,
    ) {
        md.push_str(&format!("## {title}\n\n"));
        if entries.is_empty() {
            md.push_str(&format!("- {placeholder}\n"));
        } else {
            for e in entries {
                match &e.issue {
                    Some(issue) => md.push_str(&format!("- {} ({})\n", e.summary, issue)),
                    None => md.push_str(&format!("- {}\n", e.summary)),
                }
            }
        }
        md.push('\n');
    }

    /// Render the full markdown document.
    pub fn render_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!(
            "# {} {} — Release Notes\n\n",
            self.product, self.version
        ));
        md.push_str(&format!("**Release date:** {}\n\n", self.release_date));

        md.push_str("## Highlights\n\n");
        if self.highlights.is_empty() {
            md.push_str("- (none)\n");
        } else {
            for h in &self.highlights {
                md.push_str(&format!("- {h}\n"));
            }
        }
        md.push('\n');

        Self::render_section(&mut md, "New Features", &self.new_features, "None.");
        Self::render_section(&mut md, "Improvements", &self.improvements, "None.");
        Self::render_section(&mut md, "Fixes", &self.fixes, "None.");
        Self::render_section(
            &mut md,
            "Breaking Changes",
            &self.breaking_changes,
            "None — TDL XML files written by 1.x remain fully readable and byte-compatible.",
        );
        Self::render_section(&mut md, "Known Issues", &self.known_issues, "None.");

        md.push_str("## Installation (Portable)\n\n");
        if self.installation.is_empty() {
            md.push_str("- See `docs/release/MANUAL_UPDATE.md`.\n");
        } else {
            for line in &self.installation {
                md.push_str(&format!("- {line}\n"));
            }
        }
        md.push('\n');

        md
    }
}

/// The filled-in ModernToDoList 2.0.0 GA changelog.
pub fn release_notes_2_0_0() -> ReleaseNotes {
    let mut n = ReleaseNotes::template("2.0.0", "2026-01-15");

    n.highlights = vec![
        "Portable Windows app: unzip and run, no installer, no admin rights, no network required".to_string(),
        "Strict byte-level compatibility with the legacy TDL XML format (M0 audit verified)".to_string(),
        "SQLite-derived search index with delete-and-rebuild recovery — XML files stay the single source of truth".to_string(),
        "Reproducible release ZIP with published SHA-256 (RD-M10-024)".to_string(),
    ];

    n.new_features = vec![
        ChangelogEntry::with_issue("Task relations: participants, dependencies with cycle detection, progress links, attachments", "INH-1042..INH-1054"),
        ChangelogEntry::with_issue("Rich text editor with HTML whitelist sanitizer and comments-type preservation", "INH-1061..INH-1067"),
        ChangelogEntry::with_issue("Multi-document workspace: cross-document copy/move transactions, file library, trash", "INH-1075..INH-1094"),
        ChangelogEntry::with_issue("Productivity: FTS5 global search with CJK fallback, smart views, saved views, command palette, quick add", "INH-1095..INH-1109"),
        ChangelogEntry::with_issue("Versioned Data directory migration with index-rebuild fallback", "INH-1118"),
        ChangelogEntry::with_issue("Crash diagnostics with privacy-safe bug-report export (path hashes only)", "INH-1122"),
    ];

    n.improvements = vec![
        ChangelogEntry::with_issue("Portable path hardening: Unicode paths, >260-char paths, removable-drive letter changes, read-only EXE directory fallback", "INH-1121"),
        ChangelogEntry::with_issue("WebView2 runtime detection with structured errors and offline fixed-version policy", "INH-1119"),
        ChangelogEntry::with_issue("Antivirus false-positive investigation process, mitigation checklist and code-signing policy", "INH-1120"),
        ChangelogEntry::with_issue("Third-party license notices generated from Cargo.lock and package-lock.json", "INH-1123"),
        ChangelogEntry::new("EXE version metadata (file version, product version, company) embedded via tauri.conf.json"),
    ];

    n.fixes = vec![
        ChangelogEntry::new("Atomic save: kill-before/mid-write recovery never truncates the source XML"),
        ChangelogEntry::new("Watcher fingerprint checks reject stale external-edit conflicts"),
        ChangelogEntry::new("Undo/redo coalescing for description edits no longer serializes per keystroke"),
    ];

    n.breaking_changes = vec![];

    n.known_issues = vec![
        ChangelogEntry::new("FTS5 unicode61 tokenizer does not segment Chinese; search falls back to LIKE substring matching for CJK queries"),
        ChangelogEntry::new("Releases are not code-signed yet; SmartScreen may warn on first run (see docs/release/ANTIVIRUS_FALSE_POSITIVES.md)"),
    ];

    n.installation = vec![
        "Download `ModernToDoList-2.0.0-Portable-win-x64.zip` and verify its SHA-256 against the `.sha256` sidecar file".to_string(),
        "Extract to a writable folder (or a USB drive) and run `ModernToDoList.exe`".to_string(),
        "All user data lives in the `Data/` folder next to the EXE; upgrading preserves it (see docs/release/MANUAL_UPDATE.md)".to_string(),
        "Requires the Microsoft Edge WebView2 runtime — usually preinstalled on Windows 10/11 (see docs/release/WEBVIEW2_POLICY.md)".to_string(),
    ];

    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_renders_all_sections() {
        let md = ReleaseNotes::template("9.9.9", "2026-12-31").render_markdown();
        for section in [
            "# ModernToDoList 9.9.9 — Release Notes",
            "## Highlights",
            "## New Features",
            "## Improvements",
            "## Fixes",
            "## Breaking Changes",
            "## Known Issues",
            "## Installation (Portable)",
        ] {
            assert!(md.contains(section), "missing section: {section}");
        }
    }

    #[test]
    fn v2_notes_contain_version_and_zip_name() {
        let notes = release_notes_2_0_0();
        assert_eq!(notes.version, "2.0.0");
        assert!(!notes.highlights.is_empty());
        assert!(!notes.new_features.is_empty());
        assert!(!notes.installation.is_empty());
        let md = notes.render_markdown();
        assert!(md.contains("ModernToDoList-2.0.0-Portable-win-x64.zip"));
        assert!(md.contains("(INH-1118)"));
    }

    #[test]
    fn entries_render_with_issue_refs() {
        let mut n = ReleaseNotes::template("1.2.3", "2026-01-01");
        n.fixes.push(ChangelogEntry::with_issue("fixed X", "INH-0001"));
        n.fixes.push(ChangelogEntry::new("fixed Y"));
        let md = n.render_markdown();
        assert!(md.contains("- fixed X (INH-0001)"));
        assert!(md.contains("- fixed Y"));
    }

    #[test]
    fn notes_roundtrip_through_json() {
        let n = release_notes_2_0_0();
        let back: ReleaseNotes = serde_json::from_str(&serde_json::to_string(&n).unwrap()).unwrap();
        assert_eq!(n, back);
    }
}
