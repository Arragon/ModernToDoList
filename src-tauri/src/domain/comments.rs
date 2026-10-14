//! COMMENTSTYPE preservation and editor-mode selection (RD-M7-014~018, INH-1064).
//!
//! The legacy TDL format stores the rich-content flavour of a task's
//! `<COMMENTS>` payload in the `COMMENTSTYPE` attribute. That attribute is a
//! **protected** field (see `docs/compatibility/COMMENT_FORMAT_MATRIX.md`):
//!
//! * it must survive a round trip byte-for-byte, including values this
//!   application does not understand;
//! * it must never change as a side effect of *viewing* content;
//! * it changes only through an explicit, confirmation-gated, undoable
//!   "Convert to Rich Text" action.
//!
//! This module is deliberately self-contained: it does not import
//! `domain::task`, so it can be wired into `Task` without a circular
//! dependency and without disturbing the pre-existing `task::CommentType`.
//!
//! The full behavioural matrix lives in
//! `docs/richcontent/COMMENTS_TYPE_MATRIX.md`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::sanitizer::{self, SanitizeError};

/// Canonical `COMMENTSTYPE` attribute values written by AbstractSpoon TDL.
pub const ATTR_PLAIN_TEXT: &str = "PLAIN_TEXT";
/// Canonical `COMMENTSTYPE` value for HTML comments.
pub const ATTR_HTML: &str = "HTML";
/// Canonical `COMMENTSTYPE` value for RTF comments.
pub const ATTR_RTF: &str = "RTF";

/// The value of a task's `COMMENTSTYPE` attribute.
///
/// Parsing is intentionally *exact*: only the canonical upper-case spellings
/// produced by TDL map onto a known variant. Any other spelling — including
/// `html`, `Html`, `MARKDOWN`, or the empty string — is preserved verbatim in
/// [`CommentsType::Unknown`] and treated as read-only, so the attribute can
/// always be written back unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommentsType {
    /// `PLAIN_TEXT` — unformatted text, editable in the plain-text editor.
    PlainText,
    /// `HTML` — sanitized-HTML rich text, editable in the rich-text editor.
    Html,
    /// `RTF` — Rich Text Format. Read-only in 2.0: no editor, no conversion.
    Rtf,
    /// Any other value, preserved byte-for-byte. Read-only.
    Unknown(String),
}

impl Default for CommentsType {
    fn default() -> Self {
        CommentsType::PlainText
    }
}

impl CommentsType {
    /// Parses the attribute value. Exact canonical spellings only.
    pub fn from_attr_value(value: &str) -> Self {
        match value {
            ATTR_PLAIN_TEXT => CommentsType::PlainText,
            ATTR_HTML => CommentsType::Html,
            ATTR_RTF => CommentsType::Rtf,
            other => CommentsType::Unknown(other.to_string()),
        }
    }

    /// Parses an optional attribute value; `None` (attribute absent) keeps the
    /// TDL default of `PLAIN_TEXT`.
    pub fn from_attr_opt(value: Option<&str>) -> Self {
        match value {
            Some(v) => Self::from_attr_value(v),
            None => CommentsType::PlainText,
        }
    }

    /// The exact string to write back into `COMMENTSTYPE`.
    pub fn as_attr_value(&self) -> &str {
        match self {
            CommentsType::PlainText => ATTR_PLAIN_TEXT,
            CommentsType::Html => ATTR_HTML,
            CommentsType::Rtf => ATTR_RTF,
            CommentsType::Unknown(raw) => raw.as_str(),
        }
    }

    /// True when the payload is rich text this application can edit.
    pub fn is_rich(&self) -> bool {
        matches!(self, CommentsType::Html)
    }

    /// True when the payload may be modified through an editor.
    pub fn is_editable(&self) -> bool {
        matches!(self, CommentsType::PlainText | CommentsType::Html)
    }

    /// True when the payload must be shown read-only with a type indicator.
    pub fn is_read_only(&self) -> bool {
        !self.is_editable()
    }

    /// True when an explicit "Convert to Rich Text" action is offered.
    pub fn can_convert_to_rich_text(&self) -> bool {
        matches!(self, CommentsType::PlainText)
    }

    /// The editor the UI must open for this type (RD-M7-014/015/017).
    pub fn editor_mode(&self) -> EditorMode {
        match self {
            CommentsType::PlainText => EditorMode::PlainText,
            CommentsType::Html => EditorMode::RichText,
            CommentsType::Rtf | CommentsType::Unknown(_) => EditorMode::ReadOnly,
        }
    }

    /// Stable machine-readable kind, for logs and IPC clients.
    pub fn kind(&self) -> &'static str {
        match self {
            CommentsType::PlainText => "plain_text",
            CommentsType::Html => "html",
            CommentsType::Rtf => "rtf",
            CommentsType::Unknown(_) => "unknown",
        }
    }

    /// Human-readable type indicator shown next to a read-only payload.
    pub fn type_indicator(&self) -> String {
        match self {
            CommentsType::PlainText => "纯文本 (PLAIN_TEXT)".to_string(),
            CommentsType::Html => "富文本 (HTML)".to_string(),
            CommentsType::Rtf => "RTF · 只读".to_string(),
            CommentsType::Unknown(raw) => {
                if raw.is_empty() {
                    "未知类型 · 只读".to_string()
                } else {
                    format!("未知类型 ({}) · 只读", sanitize_indicator(raw))
                }
            }
        }
    }
}

/// Clips a raw type string before it is echoed into UI text.
fn sanitize_indicator(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>()
        .replace(['<', '>', '"', '&'], "")
}

/// Which editor the UI must present for a task's comments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorMode {
    /// Plain textarea / contenteditable path.
    PlainText,
    /// Tiptap rich-text editor fed with sanitized HTML.
    RichText,
    /// No editor at all; type indicator plus read-only display.
    ReadOnly,
}

impl EditorMode {
    /// Stable machine-readable name.
    pub fn as_str(&self) -> &'static str {
        match self {
            EditorMode::PlainText => "plain_text",
            EditorMode::RichText => "rich_text",
            EditorMode::ReadOnly => "read_only",
        }
    }
}

impl std::fmt::Display for EditorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The COMMENTS payload of one task: the protected type plus the raw content.
///
/// This is the value object the orchestrator should hang off `Task` (see the
/// wiring note in the module documentation and the M7 report).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskComments {
    /// The `COMMENTSTYPE` value. Protected.
    pub comments_type: CommentsType,
    /// The raw `<COMMENTS>` payload (CDATA-unwrapped, not XML-unescaped).
    pub content: String,
}

impl TaskComments {
    /// Builds a plain-text comment.
    pub fn plain(content: impl Into<String>) -> Self {
        Self {
            comments_type: CommentsType::PlainText,
            content: content.into(),
        }
    }

    /// Builds an HTML comment. The caller is responsible for sanitizing.
    pub fn html(content: impl Into<String>) -> Self {
        Self {
            comments_type: CommentsType::Html,
            content: content.into(),
        }
    }

    /// Builds a comment from an XML attribute value plus payload.
    pub fn from_attr(attr: Option<&str>, content: impl Into<String>) -> Self {
        Self {
            comments_type: CommentsType::from_attr_opt(attr),
            content: content.into(),
        }
    }

    /// The exact `COMMENTSTYPE` value to serialize.
    pub fn attr_value(&self) -> &str {
        self.comments_type.as_attr_value()
    }

    /// Editor mode driven by the type.
    pub fn editor_mode(&self) -> EditorMode {
        self.comments_type.editor_mode()
    }

    /// True when the payload may be edited.
    pub fn is_editable(&self) -> bool {
        self.comments_type.is_editable()
    }

    /// Payload size in bytes.
    pub fn len(&self) -> usize {
        self.content.len()
    }

    /// True when there is no payload at all.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

/// Errors produced by the comments write paths.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum CommentsError {
    /// `COMMENTSTYPE` is `RTF` or an unknown value: the payload is read-only.
    #[error("comments of type '{0}' are read-only and cannot be modified")]
    ReadOnly(String),
    /// The conversion action was not confirmed by the user.
    #[error("conversion to rich text requires explicit user confirmation")]
    ConfirmationRequired,
    /// The payload is already `HTML`.
    #[error("comments are already rich text")]
    AlreadyRichText,
    /// There is no COMMENTS payload to convert.
    #[error("task has no comments to convert")]
    NothingToConvert,
    /// The requested target type is not a supported conversion target.
    #[error("'{0}' is not a valid conversion target")]
    UnsupportedTarget(String),
    /// The sanitizer refused the input (too large).
    #[error("rich-text content rejected: {0}")]
    Rejected(String),
}

impl From<SanitizeError> for CommentsError {
    fn from(err: SanitizeError) -> Self {
        CommentsError::Rejected(err.to_string())
    }
}

/// Everything the UI needs to render a task's comments, computed from a shared
/// reference so that *viewing cannot mutate anything*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentsProjection {
    /// Whether a COMMENTS payload exists at all.
    pub has_comments: bool,
    /// The protected type (echoed for the UI; `PlainText` when absent).
    pub comments_type: CommentsType,
    /// Editor the UI must open.
    pub mode: EditorMode,
    /// Whether the editor may accept input.
    pub editable: bool,
    /// Whether "Convert to Rich Text" is offered.
    pub convertible_to_rich_text: bool,
    /// Localized type indicator, always shown for read-only types.
    pub type_indicator: String,
    /// Payload for the editor: raw text for plain mode, sanitized HTML for rich
    /// mode, empty for read-only mode.
    pub editor_payload: String,
    /// Safe HTML for the read-only preview.
    pub display_html: String,
    /// Plain text for search indexing and text previews.
    pub display_text: String,
    /// Payload size in bytes.
    pub byte_length: usize,
    /// True when the UI must not create a COMMENTS element just because the
    /// user looked at the task (RD-M7-018).
    pub must_not_materialize: bool,
}

/// Projects a task's comments for display.
///
/// Takes `Option<&TaskComments>` — a shared reference. There is no code path
/// from here to a mutation, which is exactly the invariant RD-M7-018 demands:
/// opening, previewing or indexing a task never creates and never converts a
/// COMMENTS payload.
pub fn project_for_view(comments: Option<&TaskComments>) -> CommentsProjection {
    let Some(comments) = comments else {
        return CommentsProjection {
            has_comments: false,
            comments_type: CommentsType::PlainText,
            mode: EditorMode::PlainText,
            editable: true,
            convertible_to_rich_text: false,
            type_indicator: CommentsType::PlainText.type_indicator(),
            editor_payload: String::new(),
            display_html: String::new(),
            display_text: String::new(),
            byte_length: 0,
            must_not_materialize: true,
        };
    };

    let byte_length = comments.content.len();
    let mode = comments.editor_mode();
    let convertible = comments.comments_type.can_convert_to_rich_text();

    let (editor_payload, display_html, display_text) = match &comments.comments_type {
        CommentsType::PlainText => {
            let text = comments.content.clone();
            (
                text.clone(),
                plain_text_to_preview_html(&text),
                text,
            )
        }
        CommentsType::Html => {
            // Core → Editor feed: sanitized (RD-M7-013).
            let for_editor = sanitize(&comments.content, |html| {
                sanitizer::sanitize_for_editor(html)
            });
            // Preview render: sanitized again on the display path.
            let for_preview = sanitize(&comments.content, |html| {
                sanitizer::sanitize_for_preview(html)
            });
            let text = sanitizer::html_to_plain_text(&for_editor);
            (for_editor, for_preview, text)
        }
        CommentsType::Rtf | CommentsType::Unknown(_) => {
            // Never interpret an RTF/unknown payload as markup. Show an escaped
            // excerpt so the user can see that content exists.
            let excerpt = read_only_excerpt(&comments.content);
            let html = format!(
                "<p>{}</p>",
                escape_html(&excerpt)
            );
            (String::new(), html, excerpt)
        }
    };

    CommentsProjection {
        has_comments: true,
        comments_type: comments.comments_type.clone(),
        mode,
        editable: comments.is_editable(),
        convertible_to_rich_text: convertible,
        type_indicator: comments.comments_type.type_indicator(),
        editor_payload,
        display_html,
        display_text,
        byte_length,
        must_not_materialize: false,
    }
}

fn sanitize<F>(html: &str, f: F) -> String
where
    F: FnOnce(&str) -> Result<sanitizer::SanitizeOutcome, SanitizeError>,
{
    match f(html) {
        Ok(outcome) => outcome.html,
        // Refusing to render is the safe failure mode.
        Err(_) => String::new(),
    }
}

fn read_only_excerpt(content: &str) -> String {
    let trimmed = content.trim();
    let short: String = trimmed.chars().take(320).collect();
    if short.chars().count() < trimmed.chars().count() {
        format!("{short}…")
    } else {
        short
    }
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Renders plain text as inert preview HTML (escaped, newlines become `<br>`).
pub fn plain_text_to_preview_html(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len() + 16);
    out.push_str("<p>");
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            out.push_str("<br>");
        }
        first = false;
        out.push_str(&escape_html(line.trim_end_matches('\r')));
    }
    out.push_str("</p>");
    out
}

/// Converts plain text into rich text for the explicit conversion action.
///
/// Blank lines separate paragraphs; single newlines become `<br>`. The result
/// is passed through the commit sanitizer so the persisted bytes are always
/// policy-clean.
pub fn plain_text_to_html(text: &str) -> Result<String, CommentsError> {
    let mut html = String::with_capacity(text.len() + 32);
    let mut paragraph: Vec<&str> = Vec::new();

    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            flush_paragraph(&mut html, &paragraph);
            paragraph.clear();
        } else {
            paragraph.push(line);
        }
    }
    flush_paragraph(&mut html, &paragraph);

    Ok(sanitizer::sanitize_for_core(&html)?.html)
}

fn flush_paragraph(html: &mut String, lines: &[&str]) {
    if lines.is_empty() {
        return;
    }
    html.push_str("<p>");
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            html.push_str("<br>");
        }
        html.push_str(&escape_html(line.trim()));
    }
    html.push_str("</p>");
}

/// Produces the sanitized payload for a save in the given mode.
///
/// Plain text is stored verbatim — it is *text*, and escaping or sanitizing it
/// would corrupt the user's content (the escaping happens at render time in
/// [`project_for_view`]).
pub fn encode_for_save(comments_type: &CommentsType, content: &str) -> Result<String, CommentsError> {
    match comments_type {
        CommentsType::PlainText => Ok(content.to_string()),
        CommentsType::Html => Ok(sanitizer::sanitize_for_core(content)?.html),
        CommentsType::Rtf => Err(CommentsError::ReadOnly(ATTR_RTF.to_string())),
        CommentsType::Unknown(raw) => Err(CommentsError::ReadOnly(raw.clone())),
    }
}

/// An explicit, confirmed, reversible `PLAIN_TEXT` → `HTML` conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentsConversion {
    /// Exact payload before the conversion (restored by Undo).
    pub before: TaskComments,
    /// Payload after the conversion.
    pub after: TaskComments,
    /// Records that the user confirmed the destructive-ish action.
    pub confirmed: bool,
}

impl CommentsConversion {
    /// The value Undo must restore.
    pub fn undo_value(&self) -> TaskComments {
        self.before.clone()
    }
}

/// Plans (but does not apply) a conversion, so the UI can show a confirmation
/// dialog first.
pub fn plan_conversion_to_rich_text(
    current: &TaskComments,
    confirmed: bool,
) -> Result<CommentsConversion, CommentsError> {
    if !confirmed {
        return Err(CommentsError::ConfirmationRequired);
    }
    match &current.comments_type {
        CommentsType::PlainText => {}
        CommentsType::Html => return Err(CommentsError::AlreadyRichText),
        CommentsType::Rtf => return Err(CommentsError::ReadOnly(ATTR_RTF.to_string())),
        CommentsType::Unknown(raw) => return Err(CommentsError::ReadOnly(raw.clone())),
    }
    let html = plain_text_to_html(&current.content)?;
    Ok(CommentsConversion {
        before: current.clone(),
        after: TaskComments::html(html),
        confirmed: true,
    })
}

/// Instrumentation for the no-implicit-mutation invariant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentsMetrics {
    /// Times the payload was projected for viewing.
    pub views: u64,
    /// Times a save was accepted.
    pub saves: u64,
    /// Times the save path was rejected (read-only type).
    pub rejections: u64,
    /// Times `COMMENTSTYPE` actually changed value.
    pub type_changes: u64,
    /// Times a COMMENTS payload was created where none existed.
    pub materializations: u64,
    /// Times a conversion was undone.
    pub conversion_undos: u64,
}

/// A task's COMMENTS field with the M7 invariants enforced in one place.
///
/// This is the model the session/command layer should drive: viewing is
/// read-only, saving preserves the type, and the only way the type changes is
/// an explicit confirmed conversion that Undo can revert.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CommentsField {
    value: Option<TaskComments>,
    metrics: CommentsMetrics,
}

impl CommentsField {
    /// Wraps an existing payload.
    pub fn new(value: Option<TaskComments>) -> Self {
        Self {
            value,
            metrics: CommentsMetrics::default(),
        }
    }

    /// The current payload.
    pub fn value(&self) -> Option<&TaskComments> {
        self.value.as_ref()
    }

    /// The protected type, or the TDL default when absent.
    pub fn comments_type(&self) -> CommentsType {
        self.value
            .as_ref()
            .map(|c| c.comments_type.clone())
            .unwrap_or_default()
    }

    /// Instrumentation counters.
    pub fn metrics(&self) -> CommentsMetrics {
        self.metrics
    }

    /// Read-only view. Increments `views` and nothing else.
    pub fn view(&mut self) -> CommentsProjection {
        let projection = project_for_view(self.value.as_ref());
        self.metrics.views += 1;
        return projection;
    }

    /// Saves new content, preserving `COMMENTSTYPE`.
    ///
    /// `default_new_type` is only consulted when no payload exists yet, i.e.
    /// when the user explicitly creates the first comment. It must be
    /// `PlainText` or `Html`.
    pub fn save(
        &mut self,
        new_content: &str,
        default_new_type: CommentsType,
    ) -> Result<(), CommentsError> {
        match self.value.clone() {
            None => {
                if new_content.is_empty() {
                    // Nothing typed, nothing to create. No COMMENTSTYPE churn.
                    return Ok(());
                }
                if !matches!(
                    default_new_type,
                    CommentsType::PlainText | CommentsType::Html
                ) {
                    self.metrics.rejections += 1;
                    return Err(CommentsError::UnsupportedTarget(
                        default_new_type.as_attr_value().to_string(),
                    ));
                }
                let content = encode_for_save(&default_new_type, new_content)?;
                self.value = Some(TaskComments {
                    comments_type: default_new_type,
                    content,
                });
                self.metrics.saves += 1;
                self.metrics.materializations += 1;
                Ok(())
            }
            Some(current) => {
                let content = match encode_for_save(&current.comments_type, new_content) {
                    Ok(content) => content,
                    Err(err) => {
                        self.metrics.rejections += 1;
                        return Err(err);
                    }
                };
                let next = TaskComments {
                    comments_type: current.comments_type.clone(),
                    content,
                };
                if next.comments_type != current.comments_type {
                    // Unreachable by construction; kept as a hard invariant.
                    self.metrics.type_changes += 1;
                }
                self.value = Some(next);
                self.metrics.saves += 1;
                Ok(())
            }
        }
    }

    /// Explicit "Convert to Rich Text". Confirmation-gated and reversible.
    pub fn convert_to_rich_text(&mut self, confirmed: bool) -> Result<CommentsConversion, CommentsError> {
        let current = match self.value.clone() {
            Some(current) => current,
            None => {
                if !confirmed {
                    return Err(CommentsError::ConfirmationRequired);
                }
                return Err(CommentsError::NothingToConvert);
            }
        };
        let conversion = plan_conversion_to_rich_text(&current, confirmed)?;
        self.value = Some(conversion.after.clone());
        self.metrics.type_changes += 1;
        Ok(conversion)
    }

    /// Undoes a conversion, restoring the exact previous payload and type.
    pub fn undo_conversion(&mut self, conversion: &CommentsConversion) {
        self.value = Some(conversion.undo_value());
        self.metrics.conversion_undos += 1;
        if self.metrics.type_changes > 0 {
            self.metrics.type_changes -= 1;
        }
    }

    /// The `COMMENTSTYPE` value to serialize, mirroring the pre-existing
    /// `task::CommentType::default()` behaviour for tasks without comments.
    pub fn xml_attr_value(&self) -> &str {
        match &self.value {
            Some(comments) => comments.attr_value(),
            None => ATTR_PLAIN_TEXT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attr_value_roundtrip_is_exact() {
        for raw in [
            "PLAIN_TEXT",
            "HTML",
            "RTF",
            "MARKDOWN",
            "html",
            "Html",
            "",
            "WEIRD TYPE",
        ] {
            let parsed = CommentsType::from_attr_value(raw);
            assert_eq!(parsed.as_attr_value(), raw, "byte-exact preservation of {raw:?}");
        }
    }

    #[test]
    fn only_canonical_spellings_are_recognized() {
        assert_eq!(CommentsType::from_attr_value("PLAIN_TEXT"), CommentsType::PlainText);
        assert_eq!(CommentsType::from_attr_value("HTML"), CommentsType::Html);
        assert_eq!(CommentsType::from_attr_value("RTF"), CommentsType::Rtf);
        assert_eq!(
            CommentsType::from_attr_value("MARKDOWN"),
            CommentsType::Unknown("MARKDOWN".to_string())
        );
        assert_eq!(
            CommentsType::from_attr_value("html"),
            CommentsType::Unknown("html".to_string())
        );
        assert_eq!(CommentsType::from_attr_opt(None), CommentsType::PlainText);
    }

    #[test]
    fn editor_mode_matrix() {
        assert_eq!(CommentsType::PlainText.editor_mode(), EditorMode::PlainText);
        assert_eq!(CommentsType::Html.editor_mode(), EditorMode::RichText);
        assert_eq!(CommentsType::Rtf.editor_mode(), EditorMode::ReadOnly);
        assert_eq!(
            CommentsType::Unknown("MARKDOWN".into()).editor_mode(),
            EditorMode::ReadOnly
        );
    }

    #[test]
    fn editability_matrix() {
        assert!(CommentsType::PlainText.is_editable());
        assert!(CommentsType::Html.is_editable());
        assert!(!CommentsType::Rtf.is_editable());
        assert!(CommentsType::Rtf.is_read_only());
        assert!(!CommentsType::Unknown("X".into()).is_editable());

        assert!(CommentsType::PlainText.can_convert_to_rich_text());
        assert!(!CommentsType::Html.can_convert_to_rich_text());
        assert!(!CommentsType::Rtf.can_convert_to_rich_text());
        assert!(!CommentsType::Unknown("X".into()).can_convert_to_rich_text());
    }

    #[test]
    fn type_indicators_are_localized_and_safe() {
        assert!(CommentsType::Rtf.type_indicator().contains("只读"));
        let weird = CommentsType::Unknown("<script>alert(1)</script>".to_string());
        let indicator = weird.type_indicator();
        assert!(indicator.contains("只读"));
        // The raw value is echoed as inert text: no markup, no quotes, no ampersands.
        assert!(!indicator.contains('<'));
        assert!(!indicator.contains('>'));
        assert!(!indicator.contains('"'));
        assert!(!indicator.contains('&'));
        assert!(!indicator.contains("<script>"));
    }

    #[test]
    fn plain_text_save_preserves_type_and_bytes() {
        let mut field = CommentsField::new(Some(TaskComments::plain("line1\nline2 <raw> & more")));
        field.save("edited\nstill <raw>", CommentsType::PlainText).expect("ok");
        let value = field.value().expect("present");
        assert_eq!(value.comments_type, CommentsType::PlainText);
        assert_eq!(value.attr_value(), "PLAIN_TEXT");
        // Plain text is stored verbatim: no escaping, no sanitizing.
        assert_eq!(value.content, "edited\nstill <raw>");
        assert_eq!(field.metrics().type_changes, 0);
    }

    #[test]
    fn html_save_preserves_type_and_sanitizes() {
        let mut field = CommentsField::new(Some(TaskComments::html("<p>old</p>")));
        field
            .save("<p>new<script>alert(1)</script></p>", CommentsType::PlainText)
            .expect("ok");
        let value = field.value().expect("present");
        assert_eq!(value.comments_type, CommentsType::Html);
        assert_eq!(value.content, "<p>new</p>");
        assert_eq!(field.metrics().type_changes, 0);
    }

    #[test]
    fn the_default_new_type_cannot_override_an_existing_type() {
        // A UI bug that always passes `Html` must not upgrade plain text.
        let mut field = CommentsField::new(Some(TaskComments::plain("hi")));
        field.save("hi there", CommentsType::Html).expect("ok");
        assert_eq!(field.value().unwrap().comments_type, CommentsType::PlainText);
        assert_eq!(field.metrics().type_changes, 0);
    }

    #[test]
    fn rtf_and_unknown_are_read_only() {
        for (raw, initial) in [
            ("RTF", TaskComments { comments_type: CommentsType::Rtf, content: "{\\rtf1 hello}".into() }),
            ("MARKDOWN", TaskComments { comments_type: CommentsType::Unknown("MARKDOWN".into()), content: "# hi".into() }),
        ] {
            let mut field = CommentsField::new(Some(initial.clone()));
            let err = field.save("tampered", CommentsType::PlainText).unwrap_err();
            assert!(matches!(err, CommentsError::ReadOnly(_)), "{raw}: {err}");
            assert_eq!(field.value(), Some(&initial));
            assert_eq!(field.metrics().saves, 0);
            assert_eq!(field.metrics().rejections, 1);
            assert_eq!(field.metrics().type_changes, 0);
            assert!(!field.view().editable);
            assert!(!field.view().convertible_to_rich_text);
            assert_eq!(field.view().mode, EditorMode::ReadOnly);
            assert!(field.view().type_indicator.contains("只读"));
            // Conversion is refused too.
            assert!(matches!(
                field.convert_to_rich_text(true),
                Err(CommentsError::ReadOnly(_))
            ));
        }
    }

    #[test]
    fn viewing_never_creates_or_converts_comments() {
        // RD-M7-018: no auto-create, no auto-convert on view.
        let mut field = CommentsField::new(None);
        for _ in 0..50 {
            let projection = field.view();
            assert!(!projection.has_comments);
            assert!(projection.must_not_materialize);
            assert_eq!(projection.editor_payload, "");
        }
        assert_eq!(field.value(), None);
        assert_eq!(field.metrics().views, 50);
        assert_eq!(field.metrics().materializations, 0);
        assert_eq!(field.metrics().type_changes, 0);
        assert_eq!(field.metrics().saves, 0);

        // Same for every stored type.
        for initial in [
            TaskComments::plain("plain"),
            TaskComments::html("<p>html</p>"),
            TaskComments { comments_type: CommentsType::Rtf, content: "{\\rtf1}".into() },
            TaskComments { comments_type: CommentsType::Unknown("MARKDOWN".into()), content: "#x".into() },
        ] {
            let mut field = CommentsField::new(Some(initial.clone()));
            for _ in 0..20 {
                let projection = field.view();
                assert_eq!(projection.comments_type, initial.comments_type);
                assert_eq!(projection.byte_length, initial.content.len());
            }
            assert_eq!(field.value(), Some(&initial), "viewing mutated {:?}", initial.comments_type);
            assert_eq!(field.metrics().type_changes, 0);
            assert_eq!(field.metrics().materializations, 0);
        }
    }

    #[test]
    fn viewing_html_does_not_downgrade_or_upgrade_the_type() {
        let mut field = CommentsField::new(Some(TaskComments::html("<p>x</p>")));
        let projection = field.view();
        assert_eq!(projection.mode, EditorMode::RichText);
        assert!(!projection.convertible_to_rich_text);
        assert_eq!(field.comments_type(), CommentsType::Html);
        assert_eq!(field.metrics().type_changes, 0);
    }

    #[test]
    fn save_of_empty_content_does_not_materialize_a_comment() {
        let mut field = CommentsField::new(None);
        field.save("", CommentsType::PlainText).expect("ok");
        assert_eq!(field.value(), None);
        assert_eq!(field.metrics().materializations, 0);
    }

    #[test]
    fn first_explicit_edit_creates_a_plain_text_comment() {
        let mut field = CommentsField::new(None);
        field.save("typed by user", CommentsType::PlainText).expect("ok");
        let value = field.value().expect("created");
        assert_eq!(value.comments_type, CommentsType::PlainText);
        assert_eq!(value.attr_value(), "PLAIN_TEXT");
        assert_eq!(field.metrics().materializations, 1);
    }

    #[test]
    fn conversion_requires_confirmation() {
        let mut field = CommentsField::new(Some(TaskComments::plain("hello")));
        let err = field.convert_to_rich_text(false).unwrap_err();
        assert_eq!(err, CommentsError::ConfirmationRequired);
        assert_eq!(field.value().unwrap().comments_type, CommentsType::PlainText);
        assert_eq!(field.metrics().type_changes, 0);
    }

    #[test]
    fn conversion_is_reversible_via_undo() {
        let mut field = CommentsField::new(Some(TaskComments::plain("第一行\n第二行\n\n新段落")));
        let before = field.value().cloned().expect("present");

        let conversion = field.convert_to_rich_text(true).expect("confirmed");
        assert!(conversion.confirmed);
        assert_eq!(conversion.before, before);
        assert_eq!(conversion.after.comments_type, CommentsType::Html);
        assert_eq!(
            conversion.after.content,
            "<p>第一行<br>第二行</p><p>新段落</p>"
        );
        assert_eq!(field.comments_type(), CommentsType::Html);
        assert_eq!(field.metrics().type_changes, 1);

        field.undo_conversion(&conversion);
        assert_eq!(field.value(), Some(&before));
        assert_eq!(field.comments_type(), CommentsType::PlainText);
        assert_eq!(field.metrics().type_changes, 0);
        assert_eq!(field.metrics().conversion_undos, 1);
    }

    #[test]
    fn converting_html_is_refused() {
        let mut field = CommentsField::new(Some(TaskComments::html("<p>x</p>")));
        assert_eq!(
            field.convert_to_rich_text(true),
            Err(CommentsError::AlreadyRichText)
        );
        assert_eq!(field.comments_type(), CommentsType::Html);
    }

    #[test]
    fn converting_a_task_without_comments_is_refused() {
        let mut field = CommentsField::new(None);
        assert_eq!(
            field.convert_to_rich_text(true),
            Err(CommentsError::NothingToConvert)
        );
        assert_eq!(field.value(), None);
    }

    #[test]
    fn plain_text_to_html_escapes_and_blocks() {
        assert_eq!(plain_text_to_html("").expect("ok"), "");
        assert_eq!(
            plain_text_to_html("a & b < c").expect("ok"),
            "<p>a &amp; b &lt; c</p>"
        );
        assert_eq!(
            plain_text_to_html("one\ntwo\n\nthree").expect("ok"),
            "<p>one<br>two</p><p>three</p>"
        );
        // Markup in plain text must never become live markup.
        let out = plain_text_to_html("<script>alert(1)</script>").expect("ok");
        assert!(!out.contains("<script>"));
        assert!(out.contains("&lt;script&gt;"));
    }

    #[test]
    fn plain_text_preview_html_is_inert() {
        let html = plain_text_to_preview_html("<img src=x onerror=alert(1)>\nsecond");
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
        assert!(html.contains("<br>second"));
        assert_eq!(plain_text_to_preview_html(""), "");
    }

    #[test]
    fn projection_payloads_are_safe_per_mode() {
        let plain = project_for_view(Some(&TaskComments::plain("a < b")));
        assert_eq!(plain.editor_payload, "a < b");
        assert!(plain.display_html.contains("a &lt; b"));
        assert_eq!(plain.display_text, "a < b");

        let rich = project_for_view(Some(&TaskComments::html(
            "<p onclick=\"x()\">hi</p><script>alert(1)</script>",
        )));
        assert_eq!(rich.editor_payload, "<p>hi</p>");
        assert_eq!(rich.display_html, "<p>hi</p>");
        assert_eq!(rich.display_text, "hi");

        let rtf = project_for_view(Some(&TaskComments {
            comments_type: CommentsType::Rtf,
            content: "{\\rtf1 <b>x</b>}".to_string(),
        }));
        assert_eq!(rtf.editor_payload, "");
        assert_eq!(rtf.mode, EditorMode::ReadOnly);
        assert!(!rtf.display_html.contains("<b>"));
        assert!(rtf.display_html.contains("&lt;b&gt;"));

        let none = project_for_view(None);
        assert!(!none.has_comments);
        assert!(none.must_not_materialize);
    }

    #[test]
    fn xml_attr_value_mirrors_the_legacy_default() {
        assert_eq!(CommentsField::new(None).xml_attr_value(), "PLAIN_TEXT");
        let rtf = CommentsField::new(Some(TaskComments {
            comments_type: CommentsType::Unknown("MARKDOWN".into()),
            content: String::new(),
        }));
        assert_eq!(rtf.xml_attr_value(), "MARKDOWN");
    }

    #[test]
    fn task_comments_constructors_are_consistent() {
        let c = TaskComments::from_attr(Some("RTF"), "{\\rtf1}");
        assert_eq!(c.comments_type, CommentsType::Rtf);
        assert_eq!(c.attr_value(), "RTF");
        assert!(!c.is_editable());
        assert_eq!(c.editor_mode(), EditorMode::ReadOnly);
        assert_eq!(c.len(), 7);
        assert!(!c.is_empty());
    }
}
