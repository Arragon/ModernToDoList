//! Rich-content services for M7 (RD-M7-019~028, INH-1065/1066/1067/1068).
//!
//! Three responsibilities live here:
//!
//! 1. **Search-text extraction** ([`extract_plain_text`], RD-M7-028) — turns a
//!    task's COMMENTS payload into indexable text without ever changing it.
//! 2. **Managed image assets** ([`ImageAssetStore`], RD-M7-019~025) — clipboard
//!    and drag-drop ingestion into `.assets/<doc>/images/<uuid>.<ext>` with
//!    BLAKE3 verification, document-relative references (never base64 inside
//!    the XML), orphan tracking and an explicit, user-confirmed garbage
//!    collection.
//! 3. **Coalesced commit / autosave semantics** ([`RichTextSession`],
//!    RD-M7-026~027) — a debounced editor → Core commit pipeline in which one
//!    description update is exactly one undoable command, and serialization
//!    never happens per keystroke. Everything is driven by an injectable
//!    [`Clock`] so the tests run on a virtual clock with no real sleeping.
//!
//! The module is self-contained: it depends only on `domain::comments` and
//! `domain::sanitizer`, so it can be wired into `Task` without touching the
//! types another milestone owns.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::comments::{self, CommentsType, TaskComments};
use super::sanitizer::{self, SanitizeError};

pub use super::comments::{CommentsConversion, CommentsField, EditorMode};

// ===========================================================================
// 1. Search text extraction (RD-M7-028)
// ===========================================================================

/// Extracts indexable plain text from a task's comments.
///
/// * `PLAIN_TEXT` — returned verbatim (no re-wrapping, no escaping).
/// * `HTML` — sanitized (so `<script>` text never reaches the index), then
///   stripped to text with block-level separators.
/// * `RTF` / unknown — empty. Use [`opaque_placeholder`] when the UI needs to
///   say *why* there is no text; the placeholder is deliberately **not**
///   returned here so it cannot pollute a full-text index.
///
/// CJK content is passed through untouched: no spaces are inserted between
/// Chinese characters, so `任务描述` stays searchable as one token.
pub fn extract_plain_text(comments: &TaskComments) -> String {
    match &comments.comments_type {
        CommentsType::PlainText => comments.content.clone(),
        CommentsType::Html => match sanitizer::sanitize_for_core(&comments.content) {
            Ok(outcome) => sanitizer::html_to_plain_text(&outcome.html),
            // Refusing to index is safer than indexing unsanitized markup.
            Err(_) => String::new(),
        },
        CommentsType::Rtf | CommentsType::Unknown(_) => String::new(),
    }
}

/// Convenience wrapper for the indexer: `None` yields an empty string.
pub fn extract_search_text(comments: Option<&TaskComments>) -> String {
    match comments {
        Some(comments) => extract_plain_text(comments),
        None => String::new(),
    }
}

/// Seam helper for callers that hold a raw `(COMMENTSTYPE, content)` pair rather
/// than a [`TaskComments`] value — notably `domain::search`, whose
/// `extract_plain_text(&TaskComment)` is documented to delegate here, and
/// `domain::mappers`, which reads the attribute straight off the XML element.
///
/// Passing `None` for `attr_value` means "the attribute was absent", which the
/// legacy format treats as `PLAIN_TEXT`.
pub fn extract_plain_text_by_attr(attr_value: Option<&str>, content: &str) -> String {
    extract_plain_text(&TaskComments::from_attr(attr_value, content))
}

/// A non-indexable, human-readable placeholder for payloads this application
/// refuses to interpret (RTF and unknown COMMENTSTYPE values).
pub fn opaque_placeholder(comments: &TaskComments) -> Option<String> {
    match &comments.comments_type {
        CommentsType::Rtf => Some("[RTF 内容 · 只读，未纳入搜索索引]".to_string()),
        CommentsType::Unknown(raw) => Some(format!(
            "[{} 内容 · 只读，未纳入搜索索引]",
            raw.chars().filter(|c| !c.is_control()).take(32).collect::<String>()
        )),
        _ => None,
    }
}

// ===========================================================================
// 2. Clocks (virtual time for deterministic debounce tests)
// ===========================================================================

/// Time source for the commit pipeline.
///
/// Production uses [`SystemClock`]; tests use [`VirtualClock`] so debounce and
/// autosave behaviour can be verified in microseconds without real sleeping.
pub trait Clock: std::fmt::Debug {
    /// Current time in milliseconds since an arbitrary epoch.
    fn now_ms(&self) -> u64;
    /// Advances the clock. Wall clocks ignore this.
    fn advance_ms(&mut self, _ms: u64) {}
}

/// Wall-clock time source.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        now_ms_system()
    }
}

/// Manually advanced clock for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualClock {
    now_ms: u64,
}

impl VirtualClock {
    /// A clock starting at `start_ms`.
    pub fn new(start_ms: u64) -> Self {
        Self { now_ms: start_ms }
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Clock for VirtualClock {
    fn now_ms(&self) -> u64 {
        self.now_ms
    }

    fn advance_ms(&mut self, ms: u64) {
        self.now_ms = self.now_ms.saturating_add(ms);
    }
}

/// Current wall-clock time in milliseconds since the Unix epoch.
pub fn now_ms_system() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ===========================================================================
// 3. Coalesced commits, undo and autosave gating (RD-M7-026~027)
// ===========================================================================

/// Lower bound of the commit debounce window required by RD-M7-026.
pub const MIN_DEBOUNCE_MS: u64 = 1_000;
/// Upper bound of the commit debounce window required by RD-M7-026.
pub const MAX_DEBOUNCE_MS: u64 = 2_000;
/// Default debounce: the middle of the specified 1-2s window.
pub const DEFAULT_DEBOUNCE_MS: u64 = 1_500;
/// Default autosave interval; production overrides this from the M3 session.
pub const DEFAULT_AUTOSAVE_INTERVAL_MS: u64 = 30_000;

/// Tuning for the rich-text commit pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichTextConfig {
    /// Quiet period after the last keystroke before a commit is due.
    pub debounce_ms: u64,
    /// Minimum distance between two document serializations. Must be taken from
    /// the existing M3 session autosave interval, never shortened per keystroke.
    pub autosave_interval_ms: u64,
    /// Largest accepted editor draft, in bytes.
    pub max_draft_bytes: usize,
    /// Depth of the Core undo history for description commits.
    pub max_undo_depth: usize,
}

impl Default for RichTextConfig {
    fn default() -> Self {
        Self {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            autosave_interval_ms: DEFAULT_AUTOSAVE_INTERVAL_MS,
            max_draft_bytes: sanitizer::DEFAULT_MAX_INPUT_BYTES,
            max_undo_depth: 100,
        }
    }
}

impl RichTextConfig {
    /// Builds a config, clamping the debounce into the specified 1-2s window.
    pub fn new(debounce_ms: u64, autosave_interval_ms: u64) -> Self {
        Self {
            debounce_ms: debounce_ms.clamp(MIN_DEBOUNCE_MS, MAX_DEBOUNCE_MS),
            autosave_interval_ms: if autosave_interval_ms == 0 {
                DEFAULT_AUTOSAVE_INTERVAL_MS
            } else {
                autosave_interval_ms
            },
            ..Default::default()
        }
    }

    /// Adopts the autosave interval of an open M3 document session.
    pub fn with_session_autosave_interval(mut self, interval_ms: u64) -> Self {
        if interval_ms > 0 {
            self.autosave_interval_ms = interval_ms;
        }
        self
    }
}

/// One Core-level undoable unit: a single description field update.
///
/// The invariant enforced by [`RichTextSession`] is
/// **one description update == exactly one `DescriptionCommit`**, no matter how
/// many keystrokes produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptionCommit {
    /// Task whose description changed.
    pub task_id: String,
    /// Value before the commit (`None` when the task had no comments yet).
    pub previous: Option<TaskComments>,
    /// Value after the commit.
    pub next: TaskComments,
    /// Clock reading when the editing burst started.
    pub editing_started_at_ms: u64,
    /// Clock reading at commit time.
    pub committed_at_ms: u64,
    /// How many keystrokes were coalesced into this commit.
    pub coalesced_keystrokes: u64,
    /// Human-readable description, matching `UndoableCommand::description`.
    pub label: String,
}

impl DescriptionCommit {
    /// Restores the pre-commit value into a store.
    pub fn apply_undo(&self, core: &mut HashMap<String, TaskComments>) {
        match &self.previous {
            Some(previous) => {
                core.insert(self.task_id.clone(), previous.clone());
            }
            None => {
                core.remove(&self.task_id);
            }
        }
    }

    /// Re-applies the post-commit value into a store.
    pub fn apply_redo(&self, core: &mut HashMap<String, TaskComments>) {
        core.insert(self.task_id.clone(), self.next.clone());
    }

    /// Every managed image file name referenced by either side of the commit.
    /// Used by asset GC so an image that only exists in Undo history is never
    /// collected (RD-M7-025).
    pub fn referenced_image_names(&self) -> HashSet<String> {
        let mut names = HashSet::new();
        if let Some(previous) = &self.previous {
            names.extend(collect_image_file_names(&previous.content));
        }
        names.extend(collect_image_file_names(&self.next.content));
        names
    }
}

/// Instrumentation counters proving the performance invariants (QA-M7-016).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichTextMetrics {
    /// Editor keystrokes / draft updates received.
    pub keystrokes: u64,
    /// Keystrokes that deliberately did **not** trigger a serialization.
    pub suppressed_serializations: u64,
    /// Coalesced commits pushed to Core.
    pub commits: u64,
    /// Undoable commands created. Must always equal `commits`.
    pub commands_pushed: u64,
    /// Draft updates dropped because the content was unchanged.
    pub noop_commits: u64,
    /// Draft updates refused (read-only COMMENTSTYPE, oversized draft).
    pub rejected_commits: u64,
    /// Undo operations performed.
    pub undos: u64,
    /// Redo operations performed.
    pub redos: u64,
    /// Document serializations performed.
    pub serializations: u64,
    /// Times an autosave was skipped because the M3 interval had not elapsed.
    pub autosaves_deferred: u64,
    /// Whole-document snapshots taken. Must stay at zero: M7 commits carry the
    /// description field only, never the document.
    pub document_snapshots: u64,
}

/// A pending, not-yet-committed editor draft.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingDraft {
    draft: String,
    first_edit_ms: u64,
    last_edit_ms: u64,
    keystrokes: u64,
}

/// Editor → Core commit pipeline for rich-text descriptions.
///
/// * `keystroke()` never commits and never serializes.
/// * `tick()` commits drafts whose debounce window has elapsed — one
///   [`DescriptionCommit`] per task, no matter how many keystrokes.
/// * `flush()` forces the pending drafts out (blur, explicit save, close).
/// * Autosave serialization is gated on the M3 session interval.
#[derive(Debug)]
pub struct RichTextSession<C: Clock> {
    clock: C,
    config: RichTextConfig,
    core: HashMap<String, TaskComments>,
    pending: HashMap<String, PendingDraft>,
    undo_stack: Vec<DescriptionCommit>,
    redo_stack: Vec<DescriptionCommit>,
    metrics: RichTextMetrics,
    last_autosave_ms: u64,
    last_serialized_commits: u64,
}

impl RichTextSession<SystemClock> {
    /// A session on wall-clock time.
    pub fn system(config: RichTextConfig) -> Self {
        Self::new(SystemClock, config)
    }
}

impl<C: Clock> RichTextSession<C> {
    /// Builds a session around a clock.
    pub fn new(clock: C, config: RichTextConfig) -> Self {
        let start = clock.now_ms();
        Self {
            clock,
            config,
            core: HashMap::new(),
            pending: HashMap::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            metrics: RichTextMetrics::default(),
            last_autosave_ms: start,
            last_serialized_commits: 0,
        }
    }

    /// Effective configuration.
    pub fn config(&self) -> &RichTextConfig {
        &self.config
    }

    /// Instrumentation counters.
    pub fn metrics(&self) -> RichTextMetrics {
        self.metrics
    }

    /// Current clock reading.
    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    /// Mutable clock access (used by tests to advance virtual time).
    pub fn clock_mut(&mut self) -> &mut C {
        &mut self.clock
    }

    /// Advances the clock (no-op for wall clocks).
    pub fn advance_ms(&mut self, ms: u64) {
        self.clock.advance_ms(ms);
    }

    /// Loads committed Core state, e.g. when a document is opened.
    pub fn load_core_state(&mut self, task_id: impl Into<String>, comments: TaskComments) {
        self.core.insert(task_id.into(), comments);
    }

    /// Committed Core state for a task.
    pub fn core_state(&self, task_id: &str) -> Option<&TaskComments> {
        self.core.get(task_id)
    }

    /// All committed Core state.
    pub fn core_states(&self) -> &HashMap<String, TaskComments> {
        &self.core
    }

    /// Number of pending undoable commands.
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// Number of pending redoable commands.
    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    /// The undo history, for asset-GC liveness checks.
    pub fn undo_history(&self) -> &[DescriptionCommit] {
        &self.undo_stack
    }

    /// The redo history, for asset-GC liveness checks.
    pub fn redo_history(&self) -> &[DescriptionCommit] {
        &self.redo_stack
    }

    /// True when a task has an uncommitted editor draft.
    pub fn has_pending(&self, task_id: &str) -> bool {
        self.pending.contains_key(task_id)
    }

    /// Records one editor keystroke / draft update.
    ///
    /// This is the hot path. It performs no sanitization, no Core mutation, no
    /// undo command and no serialization — it only replaces the pending draft
    /// and stamps the clock, which is what makes large-document editing cheap
    /// (QA-M7-016).
    pub fn keystroke(&mut self, task_id: &str, draft: &str) -> KeystrokeOutcome {
        let now = self.clock.now_ms();
        self.metrics.keystrokes += 1;
        // Explicitly accounted: this keystroke did NOT serialize the document.
        self.metrics.suppressed_serializations += 1;

        if draft.len() > self.config.max_draft_bytes {
            self.metrics.rejected_commits += 1;
            return KeystrokeOutcome::RejectedDraftTooLarge;
        }

        match self.pending.get_mut(task_id) {
            Some(existing) => {
                existing.draft = draft.to_string();
                existing.last_edit_ms = now;
                existing.keystrokes += 1;
            }
            None => {
                self.pending.insert(
                    task_id.to_string(),
                    PendingDraft {
                        draft: draft.to_string(),
                        first_edit_ms: now,
                        last_edit_ms: now,
                        keystrokes: 1,
                    },
                );
            }
        }
        KeystrokeOutcome::Buffered
    }

    /// Advances the pipeline: commits every draft whose debounce window has
    /// elapsed, then applies the autosave gate.
    ///
    /// Returns the commits produced by this call.
    pub fn tick(&mut self) -> Vec<DescriptionCommit> {
        let now = self.clock.now_ms();
        let due: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, draft)| now.saturating_sub(draft.last_edit_ms) >= self.config.debounce_ms)
            .map(|(task_id, _)| task_id.clone())
            .collect();

        let mut committed = Vec::with_capacity(due.len());
        for task_id in due {
            if let Some(commit) = self.commit_pending(&task_id) {
                committed.push(commit);
            }
        }
        self.apply_autosave_gate(now);
        committed
    }

    /// Forces every pending draft out immediately (blur, save, close, switch).
    pub fn flush(&mut self) -> Vec<DescriptionCommit> {
        let task_ids: Vec<String> = self.pending.keys().cloned().collect();
        let mut committed = Vec::with_capacity(task_ids.len());
        for task_id in task_ids {
            if let Some(commit) = self.commit_pending(&task_id) {
                committed.push(commit);
            }
        }
        let now = self.clock.now_ms();
        self.apply_autosave_gate(now);
        committed
    }

    /// Discards pending drafts without committing (e.g. the editor was closed
    /// without the user's changes being accepted).
    pub fn cancel_pending(&mut self) -> usize {
        let count = self.pending.len();
        self.pending.clear();
        count
    }

    /// Commits one pending draft, producing at most one undoable command.
    fn commit_pending(&mut self, task_id: &str) -> Option<DescriptionCommit> {
        let pending = self.pending.remove(task_id)?;
        let now = self.clock.now_ms();

        let comments_type = self
            .core
            .get(task_id)
            .map(|c| c.comments_type.clone())
            .unwrap_or(CommentsType::Html);

        // Reuses the comments write path, so a read-only COMMENTSTYPE (RTF /
        // unknown) can never be written through the rich-text editor.
        let next = match comments::encode_for_save(&comments_type, &pending.draft) {
            Ok(content) => TaskComments {
                comments_type,
                content,
            },
            Err(_) => {
                self.metrics.rejected_commits += 1;
                return None;
            }
        };

        let previous = self.core.get(task_id).cloned();
        if previous.as_ref().map(|p| p.content == next.content) == Some(true) {
            self.metrics.noop_commits += 1;
            return None;
        }

        let commit = DescriptionCommit {
            task_id: task_id.to_string(),
            previous,
            next: next.clone(),
            editing_started_at_ms: pending.first_edit_ms,
            committed_at_ms: now,
            coalesced_keystrokes: pending.keystrokes,
            label: "Update description".to_string(),
        };

        self.core.insert(task_id.to_string(), next);
        self.undo_stack.push(commit.clone());
        if self.undo_stack.len() > self.config.max_undo_depth {
            self.undo_stack.remove(0);
        }
        // A fresh mutation invalidates the redo branch, matching UndoRedoManager.
        self.redo_stack.clear();
        self.metrics.commits += 1;
        self.metrics.commands_pushed += 1;
        Some(commit)
    }

    /// Undoes exactly one description commit.
    pub fn undo(&mut self) -> Option<DescriptionCommit> {
        let commit = self.undo_stack.pop()?;
        commit.apply_undo(&mut self.core);
        self.metrics.undos += 1;
        self.redo_stack.push(commit.clone());
        Some(commit)
    }

    /// Redoes exactly one description commit.
    pub fn redo(&mut self) -> Option<DescriptionCommit> {
        let commit = self.redo_stack.pop()?;
        commit.apply_redo(&mut self.core);
        self.metrics.redos += 1;
        // Undoing a redo must be possible again.
        self.undo_stack.push(commit.clone());
        Some(commit)
    }

    /// Serializes the document **only** when the M3 autosave interval elapsed
    /// and there is something new to write.
    fn apply_autosave_gate(&mut self, now: u64) {
        let elapsed = now.saturating_sub(self.last_autosave_ms);
        let dirty = self.metrics.commits > self.serialized_commit_watermark();
        if !dirty {
            return;
        }
        if elapsed >= self.config.autosave_interval_ms {
            self.metrics.serializations += 1;
            self.last_autosave_ms = now;
            self.last_serialized_commits = self.metrics.commits;
        } else {
            self.metrics.autosaves_deferred += 1;
        }
    }

    fn serialized_commit_watermark(&self) -> u64 {
        self.last_serialized_commits
    }

    /// Watermark of `commits` at the last serialization.
    pub fn last_serialized_commits(&self) -> u64 {
        self.last_serialized_commits
    }

    /// Forces a serialization now (explicit user save), bypassing the interval.
    pub fn save_now(&mut self) -> u64 {
        self.flush();
        self.metrics.serializations += 1;
        self.last_autosave_ms = self.clock.now_ms();
        self.last_serialized_commits = self.metrics.commits;
        self.metrics.serializations
    }
}

/// What happened to a keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeystrokeOutcome {
    /// Buffered into the pending draft. No Core traffic.
    Buffered,
    /// Refused: the draft exceeded `max_draft_bytes`.
    RejectedDraftTooLarge,
}

// ===========================================================================
// 4. Managed image assets (RD-M7-019~025)
// ===========================================================================

/// Largest accepted image payload (16 MiB).
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// Directory name of the managed asset root, relative to the workspace.
pub const ASSETS_DIR_NAME: &str = ".assets";
/// Sub-directory holding managed images.
pub const IMAGES_DIR_NAME: &str = "images";
/// File name of the image orphan manifest.
pub const ORPHAN_MANIFEST_NAME: &str = "orphans.manifest";
/// File name of the image asset index.
pub const ASSET_INDEX_NAME: &str = "index.json";

/// Asset-store failures.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum AssetError {
    /// The document id is not usable as a path segment.
    #[error("invalid document id: {0}")]
    InvalidDocumentId(String),
    /// No bytes were supplied.
    #[error("image payload is empty")]
    EmptyPayload,
    /// Payload exceeded [`MAX_IMAGE_BYTES`].
    #[error("image payload too large: {size} bytes (limit {limit})")]
    PayloadTooLarge { size: usize, limit: usize },
    /// The bytes are not a supported raster image.
    #[error("unsupported or unrecognized image format")]
    UnsupportedImageFormat,
    /// The base64 payload was malformed.
    #[error("invalid base64 payload: {0}")]
    InvalidBase64(String),
    /// I/O failure (message only, so the type stays serializable).
    #[error("i/o error: {0}")]
    Io(String),
    /// BLAKE3 of the written file did not match the source bytes.
    #[error("hash verification failed: expected {expected}, read back {actual}")]
    VerificationFailed { expected: String, actual: String },
    /// The reference escapes the managed asset root.
    #[error("path traversal refused: {0}")]
    PathTraversal(String),
    /// Asset not found.
    #[error("asset not found: {0}")]
    NotFound(String),
    /// GC was invoked without explicit user confirmation.
    #[error("asset garbage collection requires explicit user confirmation")]
    ConfirmationRequired,
    /// The sanitizer refused the input.
    #[error("rich-text content rejected: {0}")]
    Rejected(String),
}

impl From<SanitizeError> for AssetError {
    fn from(err: SanitizeError) -> Self {
        AssetError::Rejected(err.to_string())
    }
}

/// Where an image came from. Recorded in the asset index for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestSource {
    /// Clipboard paste of raw image bytes.
    Clipboard,
    /// File drag-drop onto the editor.
    DragDrop,
    /// `<img src="data:...">` lifted out of pasted HTML.
    PastedHtml,
    /// Anything else (import job, test harness).
    Programmatic,
}

impl IngestSource {
    /// Stable machine-readable name.
    pub fn as_str(&self) -> &'static str {
        match self {
            IngestSource::Clipboard => "clipboard",
            IngestSource::DragDrop => "drag_drop",
            IngestSource::PastedHtml => "pasted_html",
            IngestSource::Programmatic => "programmatic",
        }
    }
}

/// One managed image on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestedAsset {
    /// Asset id (the file stem, a UUID).
    pub id: String,
    /// `<uuid>.<ext>` on disk.
    pub file_name: String,
    /// Absolute path.
    pub abs_path: PathBuf,
    /// Document-relative reference to embed in rich text.
    pub reference: String,
    /// BLAKE3 hex digest of the stored bytes.
    pub blake3: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Sniffed MIME type.
    pub mime: String,
    /// Sanitized original file name (display only).
    pub source_name: String,
    /// Ingestion channel.
    pub source: IngestSource,
    /// Ingestion timestamp in ms since the Unix epoch.
    pub ingested_at_ms: u64,
    /// True when an identical payload was already stored and was reused.
    pub deduplicated: bool,
}

impl IngestedAsset {
    /// The `<img>` element to splice into rich text.
    ///
    /// Both the reference and the alt text are escaped, and the alt text is
    /// passed through the sanitizer's text-attribute policy so a hostile
    /// clipboard payload cannot break out of the attribute.
    pub fn img_element(&self, alt: &str) -> String {
        format!(
            "<img alt=\"{}\" src=\"{}\">",
            escape_attribute(&sanitizer::sanitize_text_attr(alt)),
            escape_attribute(&self.reference)
        )
    }
}

fn escape_attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Registry of managed images, persisted as `images/index.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetIndex {
    /// Schema version.
    pub version: u32,
    /// Known assets.
    pub entries: Vec<IngestedAsset>,
}

impl AssetIndex {
    fn new() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }

    fn find_by_hash(&self, blake3: &str) -> Option<&IngestedAsset> {
        self.entries.iter().find(|a| a.blake3 == blake3)
    }

    fn find_by_name(&self, file_name: &str) -> Option<&IngestedAsset> {
        self.entries.iter().find(|a| a.file_name == file_name)
    }

    fn remove(&mut self, file_name: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|a| a.file_name != file_name);
        self.entries.len() != before
    }
}

/// An image that is no longer referenced by rich text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanRecord {
    /// `<uuid>.<ext>` on disk.
    pub file_name: String,
    /// BLAKE3 digest, when known.
    pub blake3: Option<String>,
    /// When the orphan mark was set (ms since epoch).
    pub marked_at_ms: u64,
    /// Why it became an orphan.
    pub reason: String,
}

/// Persisted orphan candidates, at `images/orphans.manifest`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanManifest {
    /// Schema version.
    pub version: u32,
    /// Orphan candidates. Deletion still requires an explicit GC run.
    pub entries: Vec<OrphanRecord>,
}

impl OrphanManifest {
    fn new() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }

    /// True when `file_name` is already marked.
    pub fn contains(&self, file_name: &str) -> bool {
        self.entries.iter().any(|e| e.file_name == file_name)
    }

    /// Marks an asset as an orphan candidate (idempotent).
    pub fn mark(&mut self, record: OrphanRecord) -> bool {
        if self.contains(&record.file_name) {
            return false;
        }
        self.entries.push(record);
        true
    }

    /// Clears the orphan mark, e.g. when Undo restores the reference.
    pub fn unmark(&mut self, file_name: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.file_name != file_name);
        self.entries.len() != before
    }
}

/// Everything that can hold a live reference to a managed image.
///
/// GC eligibility (RD-M7-025) requires **no** hit in any of the four corpora.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceSet {
    /// Serialized XML documents (the business source of truth).
    pub xml_documents: Vec<String>,
    /// In-memory rich-text payloads not yet committed to XML.
    pub rich_text: Vec<String>,
    /// Undo **and** redo history payloads.
    pub undo_history: Vec<String>,
    /// Recovery journal / backup files that may still be replayed.
    pub recovery_journal: Vec<String>,
}

impl ReferenceSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a serialized XML document.
    pub fn push_xml(&mut self, document: impl Into<String>) {
        self.xml_documents.push(document.into());
    }

    /// Adds a rich-text payload.
    pub fn push_rich_text(&mut self, html: impl Into<String>) {
        self.rich_text.push(html.into());
    }

    /// Adds the undo/redo history of a rich-text session.
    pub fn push_session_history(&mut self, session_commits: &[DescriptionCommit]) {
        for commit in session_commits {
            if let Some(previous) = &commit.previous {
                self.undo_history.push(previous.content.clone());
            }
            self.undo_history.push(commit.next.content.clone());
        }
    }

    /// Adds recovery-journal or `.bak` text.
    pub fn push_recovery_journal(&mut self, text: impl Into<String>) {
        self.recovery_journal.push(text.into());
    }

    /// Every managed image file name mentioned anywhere in the set.
    pub fn referenced_file_names(&self) -> HashSet<String> {
        let mut names = HashSet::new();
        for corpus in self.corpora() {
            for text in corpus {
                names.extend(collect_image_file_names(text));
            }
        }
        names
    }

    /// Which corpora mention `file_name`.
    pub fn corpora_containing(&self, file_name: &str) -> Vec<&'static str> {
        let mut hits = Vec::new();
        for (label, corpus) in self.labelled_corpora() {
            if corpus.iter().any(|text| text.contains(file_name)) {
                hits.push(label);
            }
        }
        hits
    }

    fn corpora(&self) -> [&[String]; 4] {
        [
            &self.xml_documents,
            &self.rich_text,
            &self.undo_history,
            &self.recovery_journal,
        ]
    }

    fn labelled_corpora(&self) -> [(&'static str, &Vec<String>); 4] {
        [
            ("xml", &self.xml_documents),
            ("rich_text", &self.rich_text),
            ("undo_history", &self.undo_history),
            ("recovery_journal", &self.recovery_journal),
        ]
    }
}

/// Scans arbitrary text for managed image file names.
///
/// The scan deliberately over-approximates: any `<stem>.<ext>` token with an
/// allowed image extension counts as a reference, wherever it appears. For a
/// destructive GC the safe error is keeping a file, never deleting one.
pub fn collect_image_file_names(text: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut start = 0usize;
    // `char_indices` plus a synthetic terminator, so every token is cut at a
    // real char boundary even when the corpus contains CJK text.
    let scan = text.char_indices().chain(std::iter::once((text.len(), ' ')));
    for (index, ch) in scan {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            continue;
        }
        let token = &text[start..index];
        start = index + ch.len_utf8();
        if token.is_empty() || token.len() > 128 {
            continue;
        }
        let Some((stem, ext)) = token.rsplit_once('.') else {
            continue;
        };
        if stem.is_empty()
            || stem.len() > 100
            || !stem
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            continue;
        }
        if sanitizer::ALLOWED_IMAGE_EXTENSIONS.contains(&ext) {
            names.insert(token.to_string());
        }
    }
    names
}

/// Why an orphan candidate was **not** collected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeepReason {
    /// Still referenced from serialized XML.
    ReferencedInXml,
    /// Still referenced from an open rich-text buffer.
    ReferencedInRichText,
    /// Still reachable through Undo/Redo history.
    ReferencedInUndoHistory,
    /// Still referenced by the recovery journal or a `.bak` file.
    ReferencedInRecoveryJournal,
    /// Younger than the grace period.
    WithinGracePeriod,
    /// Never marked as an orphan candidate.
    NotMarkedAsOrphan,
}

impl KeepReason {
    /// Stable machine-readable name.
    pub fn as_str(&self) -> &'static str {
        match self {
            KeepReason::ReferencedInXml => "referenced_in_xml",
            KeepReason::ReferencedInRichText => "referenced_in_rich_text",
            KeepReason::ReferencedInUndoHistory => "referenced_in_undo_history",
            KeepReason::ReferencedInRecoveryJournal => "referenced_in_recovery_journal",
            KeepReason::WithinGracePeriod => "within_grace_period",
            KeepReason::NotMarkedAsOrphan => "not_marked_as_orphan",
        }
    }
}

/// One asset evaluated during a GC run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcDecision {
    /// `<uuid>.<ext>`.
    pub file_name: String,
    /// Absolute path.
    pub abs_path: PathBuf,
    /// True when the file is eligible for deletion.
    pub collect: bool,
    /// Why it was kept, when it was not collected.
    pub keep_reason: Option<KeepReason>,
}

/// Outcome of an explicit GC run.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GcReport {
    /// Whether this was a dry run.
    pub dry_run: bool,
    /// File names actually deleted (empty for a dry run).
    pub deleted: Vec<String>,
    /// File names that would be deleted by a real run.
    pub eligible: Vec<String>,
    /// Per-file decisions.
    pub decisions: Vec<GcDecision>,
    /// Deletions that failed.
    pub errors: Vec<String>,
}

/// Knobs for an explicit GC run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcOptions {
    /// Must be true: GC never runs on its own.
    pub user_confirmed: bool,
    /// When true, evaluate and report but delete nothing.
    pub dry_run: bool,
    /// Minimum age of an orphan mark before deletion is allowed.
    pub grace_period_ms: u64,
    /// Require an explicit orphan mark (RD-M7-024) before considering deletion.
    pub require_orphan_mark: bool,
    /// Reference time in ms since the epoch.
    pub now_ms: u64,
}

impl Default for GcOptions {
    fn default() -> Self {
        Self {
            user_confirmed: false,
            dry_run: true,
            grace_period_ms: 7 * 24 * 3_600_000,
            require_orphan_mark: true,
            now_ms: 0,
        }
    }
}

/// Managed image storage for one document.
///
/// Layout: `<workspace>/.assets/<document-id>/images/<uuid>.<ext>` plus
/// `index.json` (asset registry) and `orphans.manifest` (orphan candidates).
#[derive(Debug, Clone)]
pub struct ImageAssetStore {
    workspace_root: PathBuf,
    document_id: String,
    images_dir: PathBuf,
    reference_depth: usize,
}

impl ImageAssetStore {
    /// Opens (creating on demand) the image store of one document.
    ///
    /// `reference_depth` is how many directory levels the XML document sits
    /// below the workspace root; it determines the `../` prefix count of the
    /// generated references.
    pub fn new(
        workspace_root: impl Into<PathBuf>,
        document_id: impl Into<String>,
        reference_depth: usize,
    ) -> Result<Self, AssetError> {
        let document_id = document_id.into();
        if !sanitizer::is_safe_path_segment(&document_id) {
            return Err(AssetError::InvalidDocumentId(document_id));
        }
        let workspace_root = workspace_root.into();
        let images_dir = workspace_root
            .join(ASSETS_DIR_NAME)
            .join(&document_id)
            .join(IMAGES_DIR_NAME);
        std::fs::create_dir_all(&images_dir).map_err(|e| AssetError::Io(e.to_string()))?;
        Ok(Self {
            workspace_root,
            document_id,
            images_dir,
            reference_depth: reference_depth.min(sanitizer::MAX_MANAGED_SRC_DEPTH),
        })
    }

    /// Derives the reference depth from the document's path.
    pub fn for_document(
        workspace_root: impl Into<PathBuf>,
        document_id: impl Into<String>,
        document_path: &Path,
    ) -> Result<Self, AssetError> {
        let workspace_root = workspace_root.into();
        let depth = document_path
            .parent()
            .and_then(|dir| dir.strip_prefix(&workspace_root).ok())
            .map(|rel| {
                rel.components()
                    .filter(|c| matches!(c, Component::Normal(_)))
                    .count()
            })
            .unwrap_or(0);
        Self::new(workspace_root, document_id, depth)
    }

    /// Document id this store serves.
    pub fn document_id(&self) -> &str {
        &self.document_id
    }

    /// Workspace root.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Absolute images directory.
    pub fn images_dir(&self) -> &Path {
        &self.images_dir
    }

    /// Number of `../` prefixes in generated references.
    pub fn reference_depth(&self) -> usize {
        self.reference_depth
    }

    /// Absolute path of the asset index.
    pub fn index_path(&self) -> PathBuf {
        self.images_dir.join(ASSET_INDEX_NAME)
    }

    /// Absolute path of the orphan manifest.
    pub fn orphan_manifest_path(&self) -> PathBuf {
        self.images_dir.join(ORPHAN_MANIFEST_NAME)
    }

    /// Builds the document-relative reference for a stored file name.
    pub fn reference_for(&self, file_name: &str) -> String {
        sanitizer::managed_image_reference(&self.document_id, file_name, self.reference_depth)
    }

    /// Ingests raw bytes with the wall clock as the timestamp.
    pub fn ingest_bytes(
        &self,
        bytes: &[u8],
        suggested_name: &str,
        source: IngestSource,
    ) -> Result<IngestedAsset, AssetError> {
        self.ingest_bytes_at(bytes, suggested_name, source, now_ms_system())
    }

    /// Ingests raw bytes (clipboard paste or drag-drop) into managed storage.
    ///
    /// Pipeline: validate → sniff format → hash → dedupe → write temp →
    /// atomic rename → re-read and verify BLAKE3 → update index. The source
    /// bytes are never modified and the suggested file name is only ever used
    /// as a sanitized display name.
    pub fn ingest_bytes_at(
        &self,
        bytes: &[u8],
        suggested_name: &str,
        source: IngestSource,
        now_ms: u64,
    ) -> Result<IngestedAsset, AssetError> {
        if bytes.is_empty() {
            return Err(AssetError::EmptyPayload);
        }
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(AssetError::PayloadTooLarge {
                size: bytes.len(),
                limit: MAX_IMAGE_BYTES,
            });
        }
        let (mime, ext) = sniff_image(bytes).ok_or(AssetError::UnsupportedImageFormat)?;
        let digest = blake3_hex(bytes);
        let source_name = sanitize_suggested_file_name(suggested_name);

        let mut index = self.load_index();
        if let Some(existing) = index.find_by_hash(&digest) {
            let mut reused = existing.clone();
            reused.deduplicated = true;
            reused.reference = self.reference_for(&reused.file_name);
            return Ok(reused);
        }

        let id = uuid::Uuid::new_v4().to_string();
        let file_name = format!("{id}.{ext}");
        let abs_path = self.images_dir.join(&file_name);
        let temp_path = self.images_dir.join(format!("{file_name}.part"));

        std::fs::write(&temp_path, bytes).map_err(|e| AssetError::Io(e.to_string()))?;
        std::fs::rename(&temp_path, &abs_path).map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            AssetError::Io(e.to_string())
        })?;

        // Verify what actually landed on disk.
        let read_back = std::fs::read(&abs_path).map_err(|e| AssetError::Io(e.to_string()))?;
        let actual = blake3_hex(&read_back);
        if actual != digest {
            let _ = std::fs::remove_file(&abs_path);
            return Err(AssetError::VerificationFailed {
                expected: digest,
                actual,
            });
        }

        let asset = IngestedAsset {
            id,
            file_name: file_name.clone(),
            abs_path: abs_path.clone(),
            reference: self.reference_for(&file_name),
            blake3: digest,
            size_bytes: bytes.len() as u64,
            mime: mime.to_string(),
            source_name,
            source,
            ingested_at_ms: now_ms,
            deduplicated: false,
        };
        index.entries.push(asset.clone());
        self.save_index(&index)?;
        Ok(asset)
    }

    /// Ingests a `data:` URI captured by the paste sanitizer.
    pub fn ingest_data_uri(
        &self,
        data_uri: &str,
        source: IngestSource,
    ) -> Result<IngestedAsset, AssetError> {
        let (mime, payload) =
            sanitizer::parse_data_uri(data_uri).ok_or(AssetError::UnsupportedImageFormat)?;
        let bytes = base64_decode(&payload)?;
        let asset = self.ingest_bytes(&bytes, &format!("pasted.{ext}", ext = mime_extension(&mime)), source)?;
        if asset.mime != mime {
            return Err(AssetError::UnsupportedImageFormat);
        }
        Ok(asset)
    }

    /// Sanitizes pasted HTML and externalizes every inline `data:` image into
    /// managed storage, rewriting the `<img>` to a relative reference.
    ///
    /// This is the mechanism that keeps base64 out of the XML (RD-M7-023).
    pub fn externalize_pasted_html(&self, html: &str) -> Result<ExternalizeReport, AssetError> {
        let outcome = sanitizer::sanitize_paste_import(html)?;
        let mut rewritten = outcome.html.clone();
        let mut report = ExternalizeReport {
            html: String::new(),
            ingested: Vec::new(),
            failures: Vec::new(),
            removals: outcome.removals.clone(),
        };

        for (index, inline) in outcome.inline_images.iter().enumerate() {
            let token = sanitizer::inline_image_placeholder(index);
            let data_uri = format!("data:{};base64,{}", inline.mime, inline.base64);
            match self.ingest_data_uri(&data_uri, IngestSource::PastedHtml) {
                Ok(asset) => {
                    rewritten = rewritten.replace(&token, &asset.reference);
                    report.ingested.push(asset);
                }
                Err(err) => {
                    // Leave an empty src: the commit sanitizer drops the <img>.
                    rewritten = rewritten.replace(&token, "");
                    report.failures.push(err.to_string());
                }
            }
        }

        // Final pass so the returned HTML is exactly what Core will store.
        report.html = sanitizer::sanitize_for_core(&rewritten)?.html;
        Ok(report)
    }

    /// Re-reads and re-hashes a stored asset (integrity verification).
    pub fn verify(&self, file_name: &str) -> Result<bool, AssetError> {
        let path = self.resolve_file_name(file_name)?;
        let index = self.load_index();
        let expected = index
            .find_by_name(file_name)
            .map(|a| a.blake3.clone())
            .ok_or_else(|| AssetError::NotFound(file_name.to_string()))?;
        let bytes = std::fs::read(&path).map_err(|e| AssetError::Io(e.to_string()))?;
        Ok(blake3_hex(&bytes) == expected)
    }

    /// Reads a stored asset's bytes.
    pub fn read(&self, file_name: &str) -> Result<Vec<u8>, AssetError> {
        let path = self.resolve_file_name(file_name)?;
        std::fs::read(&path).map_err(|e| AssetError::Io(e.to_string()))
    }

    /// Resolves a document-relative reference to an absolute path, refusing
    /// anything that escapes the managed images directory.
    pub fn resolve_reference(&self, reference: &str) -> Result<PathBuf, AssetError> {
        let validated = sanitizer::sanitize_image_src(reference, Some(&self.document_id))
            .map_err(|e| AssetError::PathTraversal(e.to_string()))?;
        let relative = strip_managed_prefix(&validated);
        let candidate = normalize_lexically(&self.workspace_root.join(relative));
        let root = normalize_lexically(&self.images_dir);
        if !candidate.starts_with(&root) {
            return Err(AssetError::PathTraversal(reference.to_string()));
        }
        Ok(candidate)
    }

    /// Resolves a bare `<uuid>.<ext>` file name inside the images directory.
    pub fn resolve_file_name(&self, file_name: &str) -> Result<PathBuf, AssetError> {
        sanitizer::validate_asset_file_name(file_name)
            .map_err(|e| AssetError::PathTraversal(e.to_string()))?;
        let candidate = normalize_lexically(&self.images_dir.join(file_name));
        let root = normalize_lexically(&self.images_dir);
        if candidate.parent() != Some(root.as_path()) {
            return Err(AssetError::PathTraversal(file_name.to_string()));
        }
        Ok(candidate)
    }

    /// Lists every managed image file currently on disk.
    pub fn list_files(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.images_dir) {
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if !file_type.is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".part")
                    || name == ASSET_INDEX_NAME
                    || name == ORPHAN_MANIFEST_NAME
                {
                    continue;
                }
                if sanitizer::validate_asset_file_name(&name).is_ok() {
                    names.push(name);
                }
            }
        }
        names.sort();
        names
    }

    /// Loads the asset index, tolerating a missing or corrupt file.
    pub fn load_index(&self) -> AssetIndex {
        match std::fs::read_to_string(self.index_path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| AssetIndex::new()),
            Err(_) => AssetIndex::new(),
        }
    }

    fn save_index(&self, index: &AssetIndex) -> Result<(), AssetError> {
        let text = serde_json::to_string_pretty(index).map_err(|e| AssetError::Io(e.to_string()))?;
        write_atomic(&self.index_path(), text.as_bytes())
    }

    /// Loads the orphan manifest, tolerating a missing or corrupt file.
    pub fn load_orphans(&self) -> OrphanManifest {
        match std::fs::read_to_string(self.orphan_manifest_path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| OrphanManifest::new()),
            Err(_) => OrphanManifest::new(),
        }
    }

    fn save_orphans(&self, manifest: &OrphanManifest) -> Result<(), AssetError> {
        let text =
            serde_json::to_string_pretty(manifest).map_err(|e| AssetError::Io(e.to_string()))?;
        write_atomic(&self.orphan_manifest_path(), text.as_bytes())
    }

    /// Marks an asset as an orphan candidate (RD-M7-024).
    ///
    /// Marking never deletes anything; deletion only happens in an explicit,
    /// user-confirmed [`ImageAssetStore::run_gc`].
    pub fn mark_orphan(&self, file_name: &str, reason: &str, now_ms: u64) -> Result<bool, AssetError> {
        let path = self.resolve_file_name(file_name)?;
        if !path.exists() {
            return Err(AssetError::NotFound(file_name.to_string()));
        }
        let mut manifest = self.load_orphans();
        if manifest.version == 0 {
            manifest.version = 1;
        }
        let blake3 = self
            .load_index()
            .find_by_name(file_name)
            .map(|a| a.blake3.clone());
        let added = manifest.mark(OrphanRecord {
            file_name: file_name.to_string(),
            blake3,
            marked_at_ms: now_ms,
            reason: reason.to_string(),
        });
        if added {
            self.save_orphans(&manifest)?;
        }
        Ok(added)
    }

    /// Clears an orphan mark, e.g. when Undo restores the reference.
    pub fn unmark_orphan(&self, file_name: &str) -> Result<bool, AssetError> {
        let mut manifest = self.load_orphans();
        let removed = manifest.unmark(file_name);
        if removed {
            self.save_orphans(&manifest)?;
        }
        Ok(removed)
    }

    /// Scans the reference set and marks every unreferenced asset as an orphan
    /// candidate. Non-destructive.
    pub fn detect_orphans(
        &self,
        references: &ReferenceSet,
        now_ms: u64,
    ) -> Result<Vec<String>, AssetError> {
        let live = references.referenced_file_names();
        let mut manifest = self.load_orphans();
        if manifest.version == 0 {
            manifest.version = 1;
        }
        let index = self.load_index();
        let mut newly_marked = Vec::new();
        let mut changed = false;
        for file_name in self.list_files() {
            if live.contains(&file_name) {
                // A restored reference cancels a stale mark.
                if manifest.unmark(&file_name) {
                    changed = true;
                }
                continue;
            }
            let blake3 = manifest
                .entries
                .iter()
                .find(|e| e.file_name == file_name)
                .and_then(|e| e.blake3.clone())
                .or_else(|| index.find_by_name(&file_name).map(|a| a.blake3.clone()));
            if manifest.mark(OrphanRecord {
                file_name: file_name.clone(),
                blake3,
                marked_at_ms: now_ms,
                reason: "no live reference found".to_string(),
            }) {
                changed = true;
                newly_marked.push(file_name);
            }
        }
        if changed {
            self.save_orphans(&manifest)?;
        }
        Ok(newly_marked)
    }

    /// Evaluates GC eligibility without deleting anything.
    pub fn gc_candidates(
        &self,
        references: &ReferenceSet,
        opts: &GcOptions,
    ) -> Vec<GcDecision> {
        let manifest = self.load_orphans();
        let index = self.load_index();
        let mut decisions = Vec::new();

        for file_name in self.list_files() {
            let abs_path = self.images_dir.join(&file_name);
            let keep = self.keep_reason(&file_name, references, &manifest, &index, opts);
            decisions.push(GcDecision {
                collect: keep.is_none(),
                keep_reason: keep,
                file_name,
                abs_path,
            });
        }
        decisions.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        decisions
    }

    fn keep_reason(
        &self,
        file_name: &str,
        references: &ReferenceSet,
        manifest: &OrphanManifest,
        index: &AssetIndex,
        opts: &GcOptions,
    ) -> Option<KeepReason> {
        for (label, corpus) in [
            (KeepReason::ReferencedInXml, &references.xml_documents),
            (KeepReason::ReferencedInRichText, &references.rich_text),
            (KeepReason::ReferencedInUndoHistory, &references.undo_history),
            (
                KeepReason::ReferencedInRecoveryJournal,
                &references.recovery_journal,
            ),
        ] {
            if corpus.iter().any(|text| text.contains(file_name)) {
                return Some(label);
            }
        }
        if opts.require_orphan_mark && !manifest.contains(file_name) {
            return Some(KeepReason::NotMarkedAsOrphan);
        }
        let marked_at = manifest
            .entries
            .iter()
            .find(|e| e.file_name == file_name)
            .map(|e| e.marked_at_ms)
            .or_else(|| {
                index
                    .find_by_name(file_name)
                    .map(|a| a.ingested_at_ms)
            })
            .unwrap_or(0);
        if opts.now_ms.saturating_sub(marked_at) < opts.grace_period_ms {
            return Some(KeepReason::WithinGracePeriod);
        }
        None
    }

    /// Runs garbage collection. **Explicit and user-triggered only.**
    ///
    /// Returns [`AssetError::ConfirmationRequired`] unless
    /// `opts.user_confirmed` is set; there is no code path in this module that
    /// calls `run_gc` on a timer, on save, or on document close.
    pub fn run_gc(
        &self,
        references: &ReferenceSet,
        opts: &GcOptions,
    ) -> Result<GcReport, AssetError> {
        if !opts.user_confirmed {
            return Err(AssetError::ConfirmationRequired);
        }
        let decisions = self.gc_candidates(references, opts);
        let mut report = GcReport {
            dry_run: opts.dry_run,
            ..Default::default()
        };

        for decision in decisions {
            if !decision.collect {
                report.decisions.push(decision);
                continue;
            }
            report.eligible.push(decision.file_name.clone());
            if opts.dry_run {
                report.decisions.push(decision);
                continue;
            }
            match std::fs::remove_file(&decision.abs_path) {
                Ok(()) => {
                    report.deleted.push(decision.file_name.clone());
                    report.decisions.push(decision);
                }
                Err(err) => report
                    .errors
                    .push(format!("{}: {}", decision.file_name, err)),
            }
        }

        if !opts.dry_run && !report.deleted.is_empty() {
            let mut manifest = self.load_orphans();
            let mut index = self.load_index();
            for file_name in &report.deleted {
                manifest.unmark(file_name);
                index.remove(file_name);
            }
            self.save_orphans(&manifest)?;
            self.save_index(&index)?;
        }
        Ok(report)
    }
}

/// Result of externalizing inline images from pasted HTML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalizeReport {
    /// Commit-clean HTML with managed relative references.
    pub html: String,
    /// Assets written to managed storage.
    pub ingested: Vec<IngestedAsset>,
    /// Payloads that could not be stored (the `<img>` is dropped).
    pub failures: Vec<String>,
    /// Sanitizer removals from the paste pass.
    pub removals: Vec<sanitizer::Removal>,
}

/// Strips the `../` prefixes and returns the `.assets/<doc>/images/<file>` part.
fn strip_managed_prefix(reference: &str) -> String {
    let mut rest = reference;
    while let Some(tail) = rest.strip_prefix("../") {
        rest = tail;
    }
    while let Some(tail) = rest.strip_prefix("./") {
        rest = tail;
    }
    rest.to_string()
}

/// Lexically resolves `.` and `..` without touching the filesystem.
pub fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Writes a small metadata file atomically (temp + rename).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AssetError> {
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes).map_err(|e| AssetError::Io(e.to_string()))?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        AssetError::Io(e.to_string())
    })
}

/// BLAKE3 hex digest.
pub fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Maps a sniffed MIME type onto the managed file extension.
pub fn mime_extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "bin",
    }
}

/// Detects the image format from magic bytes.
///
/// The sniffed type is authoritative: the suggested file name is never trusted
/// for the on-disk extension, so `evil.png` carrying JPEG bytes is stored as
/// `.jpg`, and an SVG or executable renamed to `.png` is refused outright.
pub fn sniff_image(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(("image/png", "png"));
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(("image/jpeg", "jpg"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(("image/gif", "gif"));
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(("image/webp", "webp"));
    }
    if bytes.starts_with(b"BM") {
        return Some(("image/bmp", "bmp"));
    }
    None
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes bytes as base64 (standard alphabet, with padding).
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(BASE64_ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64_ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_ALPHABET[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decodes base64, rejecting anything malformed.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, AssetError> {
    let cleaned: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if cleaned.is_empty() {
        return Err(AssetError::InvalidBase64("empty payload".to_string()));
    }
    let padding = cleaned.iter().rev().take_while(|b| **b == b'=').count();
    if padding > 2 {
        return Err(AssetError::InvalidBase64("too much padding".to_string()));
    }
    let body = &cleaned[..cleaned.len() - padding];
    if body.iter().any(|b| *b == b'=') {
        return Err(AssetError::InvalidBase64("padding inside payload".to_string()));
    }
    if body.len() % 4 == 1 {
        return Err(AssetError::InvalidBase64("invalid length".to_string()));
    }

    let mut out = Vec::with_capacity(body.len() / 4 * 3 + 3);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for byte in body {
        let value = match BASE64_ALPHABET.iter().position(|c| c == byte) {
            Some(position) => position as u32,
            None => {
                return Err(AssetError::InvalidBase64(format!(
                    "illegal character {:?}",
                    *byte as char
                )))
            }
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xFF) as u8);
        }
    }
    if out.is_empty() {
        return Err(AssetError::InvalidBase64("no data".to_string()));
    }
    Ok(out)
}

/// Reserved Windows device names, which may never be used as a file name.
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Characters Windows refuses in a file name.
const WINDOWS_FORBIDDEN_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Sanitizes a user- or clipboard-supplied file name for use as a *display*
/// name.
///
/// The on-disk name is always `<uuid>.<ext>`, so this function is about not
/// letting a hostile `suggested_name` reach the UI, the index or any path
/// operation. Directory separators, drive prefixes, traversal segments, control
/// characters, Windows-reserved device names and over-long names are all
/// neutralized; Unicode (including Chinese) file names are preserved.
pub fn sanitize_suggested_file_name(raw: &str) -> String {
    // Keep only the final path component, whatever separator was used.
    let mut name = raw;
    for separator in ['/', '\\'] {
        if let Some(index) = name.rfind(separator) {
            name = &name[index + 1..];
        }
    }
    // Drop a `C:` style drive prefix.
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        name = &name[2..];
    }

    let mut cleaned: String = name
        .chars()
        .filter(|c| !c.is_control())
        .filter(|c| !WINDOWS_FORBIDDEN_CHARS.contains(c))
        .collect();

    // Neutralize traversal and dot-only names.
    while cleaned.contains("..") {
        cleaned = cleaned.replace("..", ".");
    }
    cleaned = cleaned.trim_matches(|c| c == '.' || c == ' ').to_string();
    if cleaned.is_empty() {
        return "image".to_string();
    }

    // Neutralize reserved device names (`con`, `nul.txt`, `COM1.png`, ...).
    let stem = cleaned
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(&cleaned);
    if WINDOWS_RESERVED_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        cleaned = format!("_{cleaned}");
    }

    // Cap the length, preserving the extension.
    const MAX_LEN: usize = 120;
    if cleaned.chars().count() > MAX_LEN {
        let (stem, ext) = match cleaned.rsplit_once('.') {
            Some((stem, ext)) if ext.len() <= 8 => (stem.to_string(), Some(ext.to_string())),
            _ => (cleaned.clone(), None),
        };
        let budget = MAX_LEN - ext.as_ref().map(|e| e.len() + 1).unwrap_or(0);
        let short_stem: String = stem.chars().take(budget.max(1)).collect();
        cleaned = match ext {
            Some(ext) => format!("{short_stem}.{ext}"),
            None => short_stem,
        };
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real 1x1 transparent PNG.
    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn temp_store(name: &str) -> (tempfile::TempDir, ImageAssetStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = ImageAssetStore::new(dir.path(), format!("doc-{name}"), 1).expect("store");
        (dir, store)
    }

    // ---------------------------------------------------------------- text

    #[test]
    fn extract_plain_text_returns_plain_text_verbatim() {
        let comments = TaskComments::plain("第一行\n第二行 <not markup> & more");
        assert_eq!(extract_plain_text(&comments), "第一行\n第二行 <not markup> & more");
    }

    #[test]
    fn extract_plain_text_sanitizes_then_strips_html() {
        let comments = TaskComments::html(
            "<h1>标题</h1><p>正文 <strong>加粗</strong></p>\
             <ul><li>项目一</li><li>项目二</li></ul>\
             <script>var secret = 1;</script>",
        );
        let text = extract_plain_text(&comments);
        assert_eq!(text, "标题\n正文 加粗\n项目一\n项目二");
        assert!(!text.contains("secret"));
    }

    #[test]
    fn extract_plain_text_handles_cjk_without_inserting_spaces() {
        let comments = TaskComments::html("<p>这是一个<strong>中文</strong>测试</p><p>第二段</p>");
        let text = extract_plain_text(&comments);
        assert_eq!(text, "这是一个中文测试\n第二段");
        assert!(text.contains("中文测试"));
    }

    #[test]
    fn extract_plain_text_returns_empty_for_opaque_types() {
        let rtf = TaskComments {
            comments_type: CommentsType::Rtf,
            content: "{\\rtf1\\ansi 任务}".to_string(),
        };
        assert_eq!(extract_plain_text(&rtf), "");
        assert_eq!(
            opaque_placeholder(&rtf),
            Some("[RTF 内容 · 只读，未纳入搜索索引]".to_string())
        );

        let markdown = TaskComments {
            comments_type: CommentsType::Unknown("MARKDOWN".to_string()),
            content: "# 标题".to_string(),
        };
        assert_eq!(extract_plain_text(&markdown), "");
        assert!(opaque_placeholder(&markdown).unwrap().contains("MARKDOWN"));
        assert_eq!(opaque_placeholder(&TaskComments::plain("x")), None);
    }

    #[test]
    fn extract_search_text_tolerates_absence() {
        assert_eq!(extract_search_text(None), "");
        assert_eq!(
            extract_search_text(Some(&TaskComments::plain("abc"))),
            "abc"
        );
    }

    #[test]
    fn extract_plain_text_by_attr_matches_the_value_object() {
        // This is the seam `domain::search` delegates to.
        assert_eq!(
            extract_plain_text_by_attr(Some("PLAIN_TEXT"), "纯文本 a < b"),
            "纯文本 a < b"
        );
        assert_eq!(
            extract_plain_text_by_attr(Some("HTML"), "<p>标题</p><ul><li>一</li></ul>"),
            "标题\n一"
        );
        assert_eq!(extract_plain_text_by_attr(Some("RTF"), "{\\rtf1 x}"), "");
        assert_eq!(extract_plain_text_by_attr(Some("MARKDOWN"), "# x"), "");
        // Absent attribute means PLAIN_TEXT, as in the legacy format.
        assert_eq!(extract_plain_text_by_attr(None, "as-is"), "as-is");
        assert_eq!(
            extract_plain_text_by_attr(Some("HTML"), "<p>x</p>"),
            extract_plain_text(&TaskComments::html("<p>x</p>"))
        );
    }

    // -------------------------------------------------------- base64 / mime

    #[test]
    fn base64_roundtrips() {
        for payload in [
            &b"".to_vec(),
            &b"f".to_vec(),
            &b"fo".to_vec(),
            &b"foo".to_vec(),
            &b"foob".to_vec(),
            &b"fooba".to_vec(),
            &b"foobar".to_vec(),
            &PNG_1X1.to_vec(),
        ] {
            let encoded = base64_encode(payload);
            if payload.is_empty() {
                assert!(base64_decode(&encoded).is_err());
                continue;
            }
            assert_eq!(&base64_decode(&encoded).expect("decode"), payload);
        }
    }

    #[test]
    fn base64_rejects_garbage() {
        assert!(base64_decode("!!!!").is_err());
        assert!(base64_decode("QUJD=====").is_err());
        assert!(base64_decode("A").is_err());
        assert_eq!(base64_decode("QUJD").expect("ok"), b"ABC");
    }

    #[test]
    fn image_formats_are_sniffed_from_magic_bytes() {
        assert_eq!(sniff_image(PNG_1X1), Some(("image/png", "png")));
        assert_eq!(
            sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]),
            Some(("image/jpeg", "jpg"))
        );
        assert_eq!(sniff_image(b"GIF89a...."), Some(("image/gif", "gif")));
        assert_eq!(
            sniff_image(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some(("image/webp", "webp"))
        );
        assert_eq!(sniff_image(b"BM\x00\x00"), Some(("image/bmp", "bmp")));
        assert_eq!(sniff_image(b"<svg onload=alert(1)>"), None);
        assert_eq!(sniff_image(b"MZ\x90\x00"), None);
    }

    // ------------------------------------------------------- file names

    #[test]
    fn suggested_file_names_are_neutralized() {
        assert_eq!(
            sanitize_suggested_file_name("../../etc/passwd"),
            "passwd"
        );
        assert_eq!(
            sanitize_suggested_file_name("..\\..\\Windows\\system32\\evil.png"),
            "evil.png"
        );
        assert_eq!(sanitize_suggested_file_name("C:\\evil.png"), "evil.png");
        assert_eq!(sanitize_suggested_file_name("////"), "image");
        assert_eq!(sanitize_suggested_file_name(""), "image");
        assert_eq!(sanitize_suggested_file_name("..."), "image");
        assert_eq!(sanitize_suggested_file_name("con.png"), "_con.png");
        assert_eq!(sanitize_suggested_file_name("NUL"), "_NUL");
        assert_eq!(sanitize_suggested_file_name("a<b>c|d?e*f.png"), "abcdef.png");
        assert_eq!(
            sanitize_suggested_file_name("截图 2026-09-22 上午.png"),
            "截图 2026-09-22 上午.png"
        );
        let long = format!("{}.png", "x".repeat(400));
        let sanitized = sanitize_suggested_file_name(&long);
        assert!(sanitized.chars().count() <= 120);
        assert!(sanitized.ends_with(".png"));
    }

    // ------------------------------------------------------- asset store

    #[test]
    fn store_layout_and_reference_shape() {
        let (_dir, store) = temp_store("layout");
        assert!(store.images_dir().ends_with(".assets/doc-layout/images"));
        let asset = store
            .ingest_bytes(PNG_1X1, "shot.png", IngestSource::Clipboard)
            .expect("ingest");
        assert_eq!(
            asset.reference,
            format!("../.assets/doc-layout/images/{}", asset.file_name)
        );
        assert!(asset.abs_path.exists());
        assert!(asset.file_name.ends_with(".png"));
        assert_eq!(asset.mime, "image/png");
        assert_eq!(asset.size_bytes, PNG_1X1.len() as u64);
        assert!(!asset.deduplicated);
        assert_eq!(asset.blake3, blake3_hex(PNG_1X1));
        assert!(store.verify(&asset.file_name).expect("verify"));
    }

    #[test]
    fn document_id_must_be_a_safe_path_segment() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(ImageAssetStore::new(dir.path(), "../escape", 1).is_err());
        assert!(ImageAssetStore::new(dir.path(), "", 1).is_err());
        assert!(ImageAssetStore::new(dir.path(), "a/b", 1).is_err());
        assert!(ImageAssetStore::new(dir.path(), "doc-1", 1).is_ok());
    }

    #[test]
    fn reference_depth_follows_the_document_location() {
        let dir = tempfile::tempdir().expect("tempdir");
        let document = dir.path().join("lists").join("test.xml");
        let store =
            ImageAssetStore::for_document(dir.path(), "doc-1", &document).expect("store");
        assert_eq!(store.reference_depth(), 1);
        assert_eq!(
            store.reference_for("abc.png"),
            "../.assets/doc-1/images/abc.png"
        );
        let flat = dir.path().join("test.xml");
        let store2 = ImageAssetStore::for_document(dir.path(), "doc-1", &flat).expect("store");
        assert_eq!(store2.reference_depth(), 0);
        assert_eq!(
            store2.reference_for("abc.png"),
            ".assets/doc-1/images/abc.png"
        );
    }

    #[test]
    fn identical_payloads_are_deduplicated() {
        let (_dir, store) = temp_store("dedupe");
        let first = store
            .ingest_bytes(PNG_1X1, "a.png", IngestSource::Clipboard)
            .expect("first");
        let second = store
            .ingest_bytes(PNG_1X1, "b.png", IngestSource::DragDrop)
            .expect("second");
        assert!(second.deduplicated);
        assert_eq!(first.file_name, second.file_name);
        assert_eq!(store.list_files().len(), 1);
    }

    #[test]
    fn non_image_payloads_are_refused() {
        let (_dir, store) = temp_store("refuse");
        assert_eq!(
            store.ingest_bytes(b"MZ\x90\x00not an image", "x.png", IngestSource::DragDrop),
            Err(AssetError::UnsupportedImageFormat)
        );
        assert_eq!(
            store.ingest_bytes(b"", "x.png", IngestSource::Clipboard),
            Err(AssetError::EmptyPayload)
        );
        let svg = b"<svg xmlns='http://www.w3.org/2000/svg' onload='alert(1)'/>";
        assert_eq!(
            store.ingest_bytes(svg, "x.png", IngestSource::DragDrop),
            Err(AssetError::UnsupportedImageFormat)
        );
        assert!(store.list_files().is_empty());
    }

    #[test]
    fn extension_follows_the_bytes_not_the_suggested_name() {
        let (_dir, store) = temp_store("sniff");
        let jpeg = [vec![0xFF, 0xD8, 0xFF, 0xE0], vec![0u8; 32]].concat();
        let asset = store
            .ingest_bytes(&jpeg, "totally-a.png", IngestSource::DragDrop)
            .expect("ingest");
        assert!(asset.file_name.ends_with(".jpg"));
        assert_eq!(asset.mime, "image/jpeg");
        assert_eq!(asset.source_name, "totally-a.png");
    }

    #[test]
    fn reference_resolution_refuses_traversal() {
        let (_dir, store) = temp_store("resolve");
        let asset = store
            .ingest_bytes(PNG_1X1, "s.png", IngestSource::Clipboard)
            .expect("ingest");
        let resolved = store.resolve_reference(&asset.reference).expect("resolve");
        assert_eq!(resolved, normalize_lexically(&asset.abs_path));
        assert!(store.read(&asset.file_name).is_ok());

        for bad in [
            "../.assets/doc-resolve/images/../../../../Windows/win.ini",
            "../../.assets/other/images/x.png",
            "/etc/passwd",
            "file:///C:/x.png",
            "..\\.assets\\doc-resolve\\images\\x.png",
        ] {
            assert!(store.resolve_reference(bad).is_err(), "{bad} resolved");
        }
        assert!(store.resolve_file_name("../index.json").is_err());
        assert!(store.resolve_file_name("x.svg").is_err());
    }

    // -------------------------------------------------- base64 externalize

    #[test]
    fn base64_images_are_externalized_not_persisted() {
        let (_dir, store) = temp_store("external");
        let payload = base64_encode(PNG_1X1);
        let html = format!(
            "<p>看图</p><img alt=\"截图\" src=\"data:image/png;base64,{payload}\"><p>结束</p>"
        );
        let report = store.externalize_pasted_html(&html).expect("report");

        assert_eq!(report.ingested.len(), 1);
        let asset = &report.ingested[0];
        assert!(!report.html.contains("base64"));
        assert!(!report.html.contains(&payload));
        assert!(report.html.contains(&asset.reference));
        assert!(report.html.contains("alt=\"截图\""));
        assert!(report.html.contains("<p>看图</p>"));
        // The rewritten HTML is already commit-clean: re-sanitizing is a no-op.
        let again = sanitizer::sanitize_for_core(&report.html).expect("ok");
        assert_eq!(again.html, report.html);
        assert!(asset.abs_path.exists());
        assert!(store.verify(&asset.file_name).expect("verify"));
    }

    #[test]
    fn non_image_data_uris_are_dropped() {
        let (_dir, store) = temp_store("dropdata");
        let html = "<img src=\"data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==\">";
        let report = store.externalize_pasted_html(html).expect("report");
        assert!(report.ingested.is_empty());
        assert_eq!(report.html, "");
        assert!(store.list_files().is_empty());
    }

    #[test]
    fn commit_path_never_stores_base64() {
        let payload = base64_encode(PNG_1X1);
        let html = format!("<img src=\"data:image/png;base64,{payload}\">");
        let outcome = sanitizer::sanitize_for_core(&html).expect("ok");
        assert_eq!(outcome.html, "");
        assert!(!outcome.html.contains("base64"));
    }

    // ------------------------------------------------------- orphan + GC

    #[test]
    fn orphan_marking_is_non_destructive() {
        let (_dir, store) = temp_store("orphan");
        let asset = store
            .ingest_bytes(PNG_1X1, "o.png", IngestSource::Clipboard)
            .expect("ingest");
        assert!(store
            .mark_orphan(&asset.file_name, "removed from rich text", 1_000)
            .expect("mark"));
        // Marking twice is idempotent.
        assert!(!store
            .mark_orphan(&asset.file_name, "again", 2_000)
            .expect("mark"));
        assert!(asset.abs_path.exists(), "marking must not delete");
        let manifest = store.load_orphans();
        assert!(manifest.contains(&asset.file_name));
        assert_eq!(manifest.entries[0].marked_at_ms, 1_000);
        assert_eq!(manifest.entries[0].blake3.as_deref(), Some(asset.blake3.as_str()));
    }

    #[test]
    fn gc_requires_explicit_user_confirmation() {
        let (_dir, store) = temp_store("confirm");
        let asset = store
            .ingest_bytes(PNG_1X1, "g.png", IngestSource::Clipboard)
            .expect("ingest");
        store.mark_orphan(&asset.file_name, "gone", 0).expect("mark");
        let references = ReferenceSet::new();

        let mut opts = GcOptions::default();
        opts.now_ms = u64::MAX;
        assert_eq!(
            store.run_gc(&references, &opts),
            Err(AssetError::ConfirmationRequired)
        );
        assert!(asset.abs_path.exists());

        opts.user_confirmed = true;
        opts.dry_run = true;
        let dry = store.run_gc(&references, &opts).expect("dry run");
        assert!(dry.dry_run);
        assert_eq!(dry.eligible, vec![asset.file_name.clone()]);
        assert!(dry.deleted.is_empty());
        assert!(asset.abs_path.exists(), "dry run must not delete");

        opts.dry_run = false;
        let wet = store.run_gc(&references, &opts).expect("gc");
        assert_eq!(wet.deleted, vec![asset.file_name.clone()]);
        assert!(!asset.abs_path.exists());
        assert!(store.load_orphans().entries.is_empty());
        assert!(store.load_index().entries.is_empty());
    }

    #[test]
    fn a_referenced_image_is_never_collected() {
        let (_dir, store) = temp_store("live");
        let asset = store
            .ingest_bytes(PNG_1X1, "live.png", IngestSource::Clipboard)
            .expect("ingest");
        store
            .mark_orphan(&asset.file_name, "stale mark", 0)
            .expect("mark");

        // `require_orphan_mark` is off here on purpose: this test proves that a
        // live reference wins even when there is no orphan mark at all.
        let opts = GcOptions {
            user_confirmed: true,
            dry_run: false,
            grace_period_ms: 0,
            require_orphan_mark: false,
            now_ms: u64::MAX,
        };

        // 1. referenced from serialized XML
        let mut references = ReferenceSet::new();
        references.push_xml(format!(
            "<TODOLIST><TASK ID=\"1\"><COMMENTS><![CDATA[<img src=\"{}\">]]></COMMENTS></TASK></TODOLIST>",
            asset.reference
        ));
        let report = store.run_gc(&references, &opts).expect("gc");
        assert!(report.deleted.is_empty());
        assert_eq!(
            report.decisions[0].keep_reason,
            Some(KeepReason::ReferencedInXml)
        );
        assert!(asset.abs_path.exists());

        // 2. referenced only from an open rich-text buffer
        let mut references = ReferenceSet::new();
        references.push_rich_text(format!("<img src=\"{}\">", asset.reference));
        let report = store.run_gc(&references, &opts).expect("gc");
        assert!(report.deleted.is_empty());
        assert_eq!(
            report.decisions[0].keep_reason,
            Some(KeepReason::ReferencedInRichText)
        );

        // 3. referenced only from Undo history
        let mut references = ReferenceSet::new();
        references.push_session_history(&[DescriptionCommit {
            task_id: "1".to_string(),
            previous: Some(TaskComments::html(format!(
                "<img src=\"{}\">",
                asset.reference
            ))),
            next: TaskComments::html("<p>image removed</p>".to_string()),
            editing_started_at_ms: 0,
            committed_at_ms: 0,
            coalesced_keystrokes: 1,
            label: "Update description".to_string(),
        }]);
        let report = store.run_gc(&references, &opts).expect("gc");
        assert!(report.deleted.is_empty());
        assert_eq!(
            report.decisions[0].keep_reason,
            Some(KeepReason::ReferencedInUndoHistory)
        );

        // 4. referenced only from the recovery journal / .bak
        let mut references = ReferenceSet::new();
        references.push_recovery_journal(format!("backup contains {}", asset.file_name));
        let report = store.run_gc(&references, &opts).expect("gc");
        assert!(report.deleted.is_empty());
        assert_eq!(
            report.decisions[0].keep_reason,
            Some(KeepReason::ReferencedInRecoveryJournal)
        );

        assert!(asset.abs_path.exists(), "still-referenced image was collected");
    }

    #[test]
    fn gc_respects_grace_period_and_orphan_mark() {
        let (_dir, store) = temp_store("grace");
        let asset = store
            .ingest_bytes(PNG_1X1, "grace.png", IngestSource::Clipboard)
            .expect("ingest");
        let references = ReferenceSet::new();

        // Unmarked assets are never collected.
        let opts = GcOptions {
            user_confirmed: true,
            dry_run: true,
            grace_period_ms: 0,
            require_orphan_mark: true,
            now_ms: u64::MAX,
        };
        let report = store.gc_candidates(&references, &opts);
        assert_eq!(report[0].keep_reason, Some(KeepReason::NotMarkedAsOrphan));

        store.mark_orphan(&asset.file_name, "removed", 10_000).expect("mark");

        // Inside the grace period.
        let opts = GcOptions {
            user_confirmed: true,
            dry_run: true,
            grace_period_ms: 5_000,
            require_orphan_mark: true,
            now_ms: 12_000,
        };
        let report = store.gc_candidates(&references, &opts);
        assert_eq!(report[0].keep_reason, Some(KeepReason::WithinGracePeriod));

        // After the grace period.
        let opts = GcOptions {
            user_confirmed: true,
            dry_run: true,
            grace_period_ms: 5_000,
            require_orphan_mark: true,
            now_ms: 20_000,
        };
        let report = store.gc_candidates(&references, &opts);
        assert!(report[0].collect);
        assert!(asset.abs_path.exists());
    }

    #[test]
    fn detect_orphans_marks_and_unmarks_from_references() {
        let (_dir, store) = temp_store("detect");
        let kept = store
            .ingest_bytes(PNG_1X1, "a.png", IngestSource::Clipboard)
            .expect("ingest");
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&[7u8; 64]);
        let removed = store
            .ingest_bytes(&jpeg, "b.jpg", IngestSource::DragDrop)
            .expect("ingest");

        let mut references = ReferenceSet::new();
        references.push_xml(format!("<COMMENTS><img src=\"{}\"></COMMENTS>", kept.reference));
        let marked = store.detect_orphans(&references, 1_000).expect("detect");
        assert_eq!(marked, vec![removed.file_name.clone()]);
        assert!(removed.abs_path.exists());
        assert!(kept.abs_path.exists());

        // Undo restores the reference: the mark is cleared automatically.
        references.push_rich_text(format!("<img src=\"{}\">", removed.reference));
        let marked = store.detect_orphans(&references, 2_000).expect("detect");
        assert!(marked.is_empty());
        assert!(store.load_orphans().entries.is_empty());
    }

    #[test]
    fn reference_scan_over_approximates() {
        let text = "prefix ../.assets/d/images/aa11-bb22.png tail and bare cc33.jpg plus \
                    unrelated notes.md and evil.exe";
        let names = collect_image_file_names(text);
        assert!(names.contains("aa11-bb22.png"));
        assert!(names.contains("cc33.jpg"));
        assert!(!names.contains("notes.md"));
        assert!(!names.contains("evil.exe"));
    }

    // ------------------------------------------------- debounce / autosave

    fn session() -> RichTextSession<VirtualClock> {
        RichTextSession::new(
            VirtualClock::new(0),
            RichTextConfig::new(DEFAULT_DEBOUNCE_MS, 30_000),
        )
    }

    #[test]
    fn debounce_window_is_clamped_to_the_specified_range() {
        assert_eq!(RichTextConfig::new(10, 1_000).debounce_ms, MIN_DEBOUNCE_MS);
        assert_eq!(RichTextConfig::new(99_999, 1_000).debounce_ms, MAX_DEBOUNCE_MS);
        assert_eq!(RichTextConfig::new(1_234, 1_000).debounce_ms, 1_234);
        assert_eq!(RichTextConfig::default().debounce_ms, DEFAULT_DEBOUNCE_MS);
    }

    #[test]
    fn keystrokes_never_commit_before_the_debounce_elapses() {
        let mut session = session();
        session.load_core_state("1", TaskComments::html("<p>start</p>"));
        for i in 0..50 {
            assert_eq!(
                session.keystroke("1", &format!("<p>start{i}</p>")),
                KeystrokeOutcome::Buffered
            );
            session.advance_ms(10); // 500ms total, inside the 1500ms window
            assert!(session.tick().is_empty());
        }
        assert_eq!(session.metrics().keystrokes, 50);
        assert_eq!(session.metrics().commits, 0);
        assert_eq!(session.metrics().commands_pushed, 0);
        assert_eq!(session.undo_depth(), 0);
        assert_eq!(session.core_state("1").unwrap().content, "<p>start</p>");
    }

    #[test]
    fn coalescing_produces_exactly_one_command_per_burst() {
        let mut session = session();
        session.load_core_state("1", TaskComments::html("<p>0</p>"));

        // 200 keystrokes over 2 seconds, each resetting the debounce window
        // except the last burst.
        for i in 0..200 {
            session.keystroke("1", &format!("<p>{i}</p>"));
            session.advance_ms(5);
        }
        // Total elapsed 1000ms < 1500ms debounce: nothing committed yet.
        assert!(session.tick().is_empty());
        session.advance_ms(1_500);
        let commits = session.tick();

        assert_eq!(commits.len(), 1, "one burst must yield one commit");
        assert_eq!(commits[0].coalesced_keystrokes, 200);
        assert_eq!(commits[0].next.content, "<p>199</p>");
        assert_eq!(session.metrics().commits, 1);
        assert_eq!(session.metrics().commands_pushed, 1);
        assert_eq!(session.undo_depth(), 1);
        assert_eq!(session.metrics().keystrokes, 200);
        assert_eq!(session.metrics().document_snapshots, 0);
    }

    #[test]
    fn one_description_update_is_exactly_one_undo_step() {
        let mut session = session();
        session.load_core_state("1", TaskComments::html("<p>v0</p>"));

        // Three separate editing bursts.
        for version in 1..=3 {
            for suffix in ["a", "b", "c"] {
                session.keystroke("1", &format!("<p>v{version}{suffix}</p>"));
                session.advance_ms(100);
            }
            session.advance_ms(1_600);
            session.tick();
        }
        assert_eq!(session.metrics().commits, 3);
        assert_eq!(session.metrics().commands_pushed, 3);
        assert_eq!(session.undo_depth(), 3);
        assert_eq!(session.core_state("1").unwrap().content, "<p>v3c</p>");

        // Each undo restores exactly the previous committed value.
        assert_eq!(session.undo().expect("undo").next.content, "<p>v3c</p>");
        assert_eq!(session.core_state("1").unwrap().content, "<p>v2c</p>");
        assert_eq!(session.undo().expect("undo").next.content, "<p>v2c</p>");
        assert_eq!(session.core_state("1").unwrap().content, "<p>v1c</p>");
        assert_eq!(session.undo().expect("undo").next.content, "<p>v1c</p>");
        assert_eq!(session.core_state("1").unwrap().content, "<p>v0</p>");
        assert_eq!(session.undo_depth(), 0);
        assert_eq!(session.metrics().undos, 3);

        // Redo walks forward again, one command at a time.
        session.redo().expect("redo");
        assert_eq!(session.core_state("1").unwrap().content, "<p>v1c</p>");
        session.redo().expect("redo");
        assert_eq!(session.core_state("1").unwrap().content, "<p>v2c</p>");
        assert_eq!(session.metrics().redos, 2);

        // A new edit invalidates the redo branch, like UndoRedoManager.
        session.keystroke("1", "<p>fresh</p>");
        session.advance_ms(1_600);
        session.tick();
        assert_eq!(session.redo_depth(), 0);
    }

    #[test]
    fn unchanged_content_produces_no_command() {
        let mut session = session();
        session.load_core_state("1", TaskComments::html("<p>same</p>"));
        session.keystroke("1", "<p>same</p>");
        session.advance_ms(1_600);
        assert!(session.tick().is_empty());
        assert_eq!(session.metrics().noop_commits, 1);
        assert_eq!(session.metrics().commands_pushed, 0);
        assert_eq!(session.undo_depth(), 0);
    }

    #[test]
    fn flush_commits_immediately_and_cancel_discards() {
        let mut session = session();
        session.load_core_state("1", TaskComments::html("<p>v0</p>"));
        session.keystroke("1", "<p>v1</p>");
        assert!(session.tick().is_empty());
        let commits = session.flush();
        assert_eq!(commits.len(), 1);
        assert_eq!(session.core_state("1").unwrap().content, "<p>v1</p>");

        session.keystroke("1", "<p>v2</p>");
        assert_eq!(session.cancel_pending(), 1);
        assert!(session.flush().is_empty());
        assert_eq!(session.core_state("1").unwrap().content, "<p>v1</p>");
    }

    #[test]
    fn commits_are_sanitized_and_read_only_types_are_refused() {
        let mut session = session();
        session.load_core_state("1", TaskComments::html("<p>ok</p>"));
        session.keystroke("1", "<p>ok<script>alert(1)</script></p>");
        session.advance_ms(1_600);
        session.tick();
        assert_eq!(session.core_state("1").unwrap().content, "<p>ok</p>");
        assert_eq!(session.core_state("1").unwrap().comments_type, CommentsType::Html);

        // A read-only COMMENTSTYPE can never be written through the editor.
        session.load_core_state(
            "2",
            TaskComments {
                comments_type: CommentsType::Rtf,
                content: "{\\rtf1 original}".to_string(),
            },
        );
        session.keystroke("2", "tampered");
        session.advance_ms(1_600);
        assert!(session.tick().is_empty());
        assert_eq!(session.metrics().rejected_commits, 1);
        assert_eq!(session.core_state("2").unwrap().content, "{\\rtf1 original}");
        assert_eq!(session.core_state("2").unwrap().comments_type, CommentsType::Rtf);
    }

    #[test]
    fn commits_never_change_the_comments_type() {
        let mut session = session();
        session.load_core_state("1", TaskComments::plain("plain"));
        session.keystroke("1", "plain edited");
        session.advance_ms(1_600);
        session.tick();
        let state = session.core_state("1").expect("state");
        assert_eq!(state.comments_type, CommentsType::PlainText);
        assert_eq!(state.attr_value(), "PLAIN_TEXT");
        // Plain text is stored verbatim, not HTML-sanitized.
        assert_eq!(state.content, "plain edited");
    }

    #[test]
    fn oversize_drafts_are_refused() {
        let mut session = RichTextSession::new(
            VirtualClock::new(0),
            RichTextConfig {
                max_draft_bytes: 32,
                ..RichTextConfig::default()
            },
        );
        session.load_core_state("1", TaskComments::html("<p>x</p>"));
        assert_eq!(
            session.keystroke("1", &"<p>".repeat(100)),
            KeystrokeOutcome::RejectedDraftTooLarge
        );
        assert_eq!(session.metrics().rejected_commits, 1);
        assert!(session.flush().is_empty());
    }

    #[test]
    fn per_task_drafts_are_independent() {
        let mut session = session();
        session.load_core_state("1", TaskComments::html("<p>a0</p>"));
        session.load_core_state("2", TaskComments::html("<p>b0</p>"));
        session.keystroke("1", "<p>a1</p>");
        session.advance_ms(1_600);
        let commits = session.tick();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].task_id, "1");
        assert_eq!(session.core_state("2").unwrap().content, "<p>b0</p>");

        session.keystroke("2", "<p>b1</p>");
        session.advance_ms(1_600);
        let commits = session.tick();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].task_id, "2");
        assert_eq!(session.undo_depth(), 2);
    }

    #[test]
    fn autosave_interval_is_respected_and_never_per_keystroke() {
        let mut session = RichTextSession::new(
            VirtualClock::new(0),
            RichTextConfig::new(1_000, 30_000).with_session_autosave_interval(10_000),
        );
        assert_eq!(session.config().autosave_interval_ms, 10_000);
        session.load_core_state("1", TaskComments::html("<p>0</p>"));

        // 500 keystrokes over 5 simulated seconds: no serialization at all.
        for i in 0..500 {
            session.keystroke("1", &format!("<p>{i}</p>"));
            session.advance_ms(10);
        }
        assert_eq!(session.metrics().serializations, 0);
        assert_eq!(session.metrics().suppressed_serializations, 500);

        // Debounce elapses -> one commit, but the autosave interval has not.
        session.advance_ms(1_000);
        session.tick();
        assert_eq!(session.metrics().commits, 1);
        assert_eq!(session.metrics().serializations, 0);
        assert_eq!(session.metrics().autosaves_deferred, 1);

        // Crossing the M3 interval serializes exactly once.
        session.keystroke("1", "<p>later</p>");
        session.advance_ms(4_100);
        session.tick();
        assert_eq!(session.metrics().commits, 2);
        assert_eq!(session.metrics().serializations, 1);
        assert_eq!(session.last_serialized_commits(), 2);

        // A clean document is never re-serialized, however much time passes.
        session.advance_ms(60_000);
        session.tick();
        assert_eq!(session.metrics().serializations, 1);

        // Explicit save forces one.
        assert_eq!(session.save_now(), 2);
    }

    #[test]
    fn virtual_clock_never_blocks() {
        let mut session = session();
        let start = std::time::Instant::now();
        session.load_core_state("1", TaskComments::html("<p>0</p>"));
        // Simulate an hour of editing in well over the debounce window.
        for i in 0..3_600 {
            session.keystroke("1", &format!("<p>x{i}</p>"));
            session.advance_ms(1_600);
            session.tick();
        }
        assert_eq!(session.now_ms(), 3_600 * 1_600);
        assert_eq!(session.metrics().keystrokes, 3_600);
        assert_eq!(session.metrics().commits, 3_600);
        assert_eq!(session.metrics().noop_commits, 0);
        assert_eq!(session.metrics().commands_pushed, session.metrics().commits);
        assert_eq!(session.metrics().document_snapshots, 0);
        assert!(
            start.elapsed().as_secs() < 10,
            "the virtual clock must not sleep: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn metrics_invariant_commands_equal_commits() {
        let mut session = session();
        session.load_core_state("1", TaskComments::html("<p>0</p>"));
        for round in 0..6 {
            for keystroke in 0..7 {
                session.keystroke("1", &format!("<p>{round}-{keystroke}</p>"));
                session.advance_ms(50);
            }
            session.advance_ms(1_600);
            session.tick();
            let metrics = session.metrics();
            assert_eq!(metrics.commands_pushed, metrics.commits);
            assert_eq!(metrics.suppressed_serializations, metrics.keystrokes);
            assert_eq!(metrics.document_snapshots, 0);
        }
        assert_eq!(session.metrics().commits, 6);
        assert_eq!(session.metrics().keystrokes, 42);
    }

    #[test]
    fn history_references_protect_images_from_gc() {
        let (_dir, store) = temp_store("history");
        let asset = store
            .ingest_bytes(PNG_1X1, "h.png", IngestSource::Clipboard)
            .expect("ingest");

        let mut session = session();
        session.load_core_state("1", TaskComments::html("<p>before</p>"));
        session.keystroke("1", &format!("<img src=\"{}\">", asset.reference));
        session.advance_ms(1_600);
        session.tick();
        // The user now deletes the image from the text.
        session.keystroke("1", "<p>after</p>");
        session.advance_ms(1_600);
        session.tick();
        assert_eq!(session.undo_depth(), 2);

        store
            .mark_orphan(&asset.file_name, "removed from rich text", 0)
            .expect("mark");

        let mut references = ReferenceSet::new();
        references.push_xml("<TODOLIST><TASK ID=\"1\"><COMMENTS><![CDATA[<p>after</p>]]></COMMENTS></TASK></TODOLIST>");
        references.push_session_history(session.undo_history());
        references.push_session_history(session.redo_history());

        let opts = GcOptions {
            user_confirmed: true,
            dry_run: false,
            grace_period_ms: 0,
            require_orphan_mark: true,
            now_ms: u64::MAX,
        };
        let report = store.run_gc(&references, &opts).expect("gc");
        assert!(report.deleted.is_empty());
        assert_eq!(
            report.decisions[0].keep_reason,
            Some(KeepReason::ReferencedInUndoHistory)
        );
        assert!(asset.abs_path.exists());

        // Undoing the deletion brings the image back and it still resolves.
        session.undo().expect("undo");
        assert_eq!(
            session.core_state("1").unwrap().content,
            format!("<img src=\"{}\">", asset.reference)
        );
        assert!(store.resolve_reference(&asset.reference).is_ok());
    }
}
