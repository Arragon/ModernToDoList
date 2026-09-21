//! HTML whitelist sanitizer for M7 Rich Content (RD-M7-011~013, INH-1063).
//!
//! This is a **security control**. It is deliberately built as an allow-list
//! re-serializer rather than a deny-list string scrubber:
//!
//! 1. The untrusted fragment is parsed with `html5ever` (via the `scraper`
//!    crate), i.e. with the same spec-compliant tokenizer a WebView2/Chromium
//!    renderer uses. That removes the whole class of "the sanitizer and the
//!    browser disagree about where a tag starts" bugs.
//! 2. The resulting tree is walked and a *brand new* string is emitted. Only
//!    whitelisted elements and whitelisted attributes are ever written out;
//!    everything else simply never reaches the output. Text is re-escaped on
//!    the way out, so a text node can never re-materialise as markup
//!    (mutation-XSS / double-escaping defence).
//! 3. Raw-text and escapable-raw-text elements (`script`, `style`, `noscript`,
//!    `template`, `xmp`, `iframe`, `title`, `textarea`, ...) and all foreign
//!    namespaces (`svg`, `math`) are dropped **together with their subtree**.
//!    Their children are never unwrapped, because their child text is not
//!    ordinary text and unwrapping it is the classic mXSS pivot.
//!
//! Four named entry points expose the control, one per application choke point
//! required by the delivery plan section 2.2:
//!
//! * [`sanitize_paste_import`] — clipboard / drag-drop HTML entering the app.
//! * [`sanitize_for_editor`]   — Core → Editor feed (defence in depth against
//!   XML edited by a third party outside the app).
//! * [`sanitize_for_core`]     — Editor → Core commit (what gets persisted).
//! * [`sanitize_for_preview`]  — read-only preview render.
//!
//! See `docs/richcontent/SANITIZER_POLICY.md` for the full policy statement.

use scraper::{Html, Node};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// HTML namespace. Anything outside it is foreign content and is dropped.
const HTML_NS: &str = "http://www.w3.org/1999/xhtml";

/// Hard input ceiling (DoS guard). Rich-text comments are never this large.
pub const DEFAULT_MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

/// Hard ceiling for a single URL attribute value.
pub const MAX_URL_LEN: usize = 2048;

/// Hard ceiling for a single `alt`/text attribute value.
pub const MAX_TEXT_ATTR_LEN: usize = 512;

/// Maximum number of `../` prefixes accepted in a managed image reference.
pub const MAX_MANAGED_SRC_DEPTH: usize = 4;

/// Elements that are allowed to survive, exactly as required by RD-M7-011.
pub const ALLOWED_TAGS: &[&str] = &[
    "p", "h1", "h2", "h3", "h4", "h5", "h6", "strong", "em", "u", "s", "span", "ul", "ol", "li",
    "blockquote", "a", "code", "pre", "img", "br",
];

/// Elements serialized without a closing tag.
pub const VOID_TAGS: &[&str] = &["br", "img"];

/// Elements that introduce a line break when extracting plain text.
pub const BLOCK_TAGS: &[&str] = &[
    "p", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "blockquote", "pre",
];

/// Legacy/authoring spellings folded onto the whitelist so benign formatting
/// in existing TDL HTML comments survives instead of being flattened.
pub const TAG_ALIASES: &[(&str, &str)] = &[
    ("b", "strong"),
    ("i", "em"),
    ("strike", "s"),
    ("del", "s"),
    ("ins", "u"),
    ("tt", "code"),
    ("big", "span"),
    ("small", "span"),
    ("center", "p"),
    ("font", "span"),
];

/// Elements removed **with their whole subtree**.
///
/// Two families live here:
/// * Actively dangerous hosts: `script`, `iframe`, `object`, `embed`, ...
/// * Raw-text / escapable-raw-text / deferred-content hosts whose children are
///   *not* normal nodes. Unwrapping them is a mutation-XSS pivot, so the
///   subtree goes.
pub const DROPPED_SUBTREE_TAGS: &[&str] = &[
    "script",
    "style",
    "iframe",
    "object",
    "embed",
    "applet",
    "frame",
    "frameset",
    "noframes",
    "noembed",
    "noscript",
    "template",
    "xmp",
    "plaintext",
    "listing",
    "svg",
    "math",
    "base",
    "meta",
    "link",
    "title",
    "form",
    "input",
    "button",
    "select",
    "textarea",
    "option",
    "optgroup",
    "fieldset",
    "output",
    "audio",
    "video",
    "source",
    "track",
    "canvas",
    "dialog",
    "slot",
    "portal",
    "marquee",
    "blink",
    "keygen",
    "param",
    "isindex",
];

// NOTE: `html`, `head` and `body` are deliberately **not** in the drop list.
// `scraper::Html::parse_fragment` builds the structural skeleton
// `Fragment -> html -> {head, body}` and parses the payload into `body`, so
// dropping those names would discard the whole document. They are harmless:
// they are not whitelisted, so they are unwrapped (no tag emitted, attributes
// such as `onload` never serialized) exactly like a literal `<body>` wrapper in
// a legacy TDL HTML comment.

/// CSS properties accepted inside a `style` attribute (RD-M7-012 "limited").
pub const ALLOWED_STYLE_PROPERTIES: &[&str] = &[
    "background-color",
    "color",
    "font-size",
    "font-style",
    "font-weight",
    "height",
    "letter-spacing",
    "line-height",
    "text-align",
    "text-decoration",
    "text-decoration-line",
    "vertical-align",
    "width",
];

/// Image file extensions accepted for managed assets. `svg` is intentionally
/// absent: SVG is an XML scripting host, not an image.
pub const ALLOWED_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Substrings that must never appear in a CSS value.
const STYLE_VALUE_DENYLIST: &[&str] = &[
    "url(",
    "url (",
    "expression",
    "javascript",
    "vbscript",
    "behavior",
    "-moz-binding",
    "@import",
    "/*",
    "*/",
];

/// Returns true when `tag` may survive sanitization as itself.
pub fn is_allowed_tag(tag: &str) -> bool {
    ALLOWED_TAGS.contains(&tag)
}

/// Returns true when `tag` must be removed together with its subtree.
pub fn is_dropped_subtree_tag(tag: &str) -> bool {
    DROPPED_SUBTREE_TAGS.contains(&tag)
}

/// Maps a source tag name onto the emitted whitelist name.
///
/// `None` means "not whitelisted": the caller unwraps the element (children are
/// preserved) unless the name is structurally invalid, in which case the whole
/// subtree is dropped.
pub fn canonical_tag(tag: &str) -> Option<&'static str> {
    if let Some(pos) = ALLOWED_TAGS.iter().position(|t| *t == tag) {
        return Some(ALLOWED_TAGS[pos]);
    }
    for (from, to) in TAG_ALIASES {
        if *from == tag {
            return Some(to);
        }
    }
    None
}

/// Returns true when `tag` is structurally a plausible HTML tag name.
///
/// html5ever happily produces element names such as `scr<script` for the
/// `<scr<script>ipt>` tokenizer probe. Such a name is never unwrapped, because
/// we cannot reason about what the renderer will do with the leftover text.
pub fn is_valid_tag_name(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 64
        && tag
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_void(tag: &str) -> bool {
    VOID_TAGS.contains(&tag)
}

/// True when an element is not in the HTML namespace (SVG, MathML, ...).
///
/// Foreign content is dropped with its whole subtree: its children are parsed
/// under different rules than HTML, so unwrapping them is an mXSS pivot.
fn is_foreign_namespace(element: &scraper::node::Element) -> bool {
    let namespace: &str = &element.name.ns;
    namespace != HTML_NS
}

fn is_block(tag: &str) -> bool {
    BLOCK_TAGS.contains(&tag)
}

/// Attributes allowed per element. Everything else is dropped and reported.
fn allowed_attributes(tag: &str) -> &'static [&'static str] {
    match tag {
        "a" => &["class", "href", "style"],
        "img" => &["alt", "class", "height", "src", "style", "width"],
        "ol" => &["class", "start", "style"],
        _ => &["class", "style"],
    }
}

/// The application choke point at which sanitization runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SanitizeContext {
    /// Clipboard / drag-drop HTML entering the application (RD-M7-019).
    PasteImport,
    /// Stored content being fed Core → Editor.
    CoreToEditor,
    /// Editor content being committed Editor → Core (persisted to XML).
    EditorToCore,
    /// Content being rendered in the read-only preview.
    PreviewRender,
}

impl SanitizeContext {
    /// Stable machine-readable name.
    pub fn as_str(&self) -> &'static str {
        match self {
            SanitizeContext::PasteImport => "paste_import",
            SanitizeContext::CoreToEditor => "core_to_editor",
            SanitizeContext::EditorToCore => "editor_to_core",
            SanitizeContext::PreviewRender => "preview_render",
        }
    }
}

impl std::fmt::Display for SanitizeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a node or attribute was removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    /// Element is not on the whitelist; children were preserved.
    DisallowedTag,
    /// Element is on the drop-with-subtree list (`script`, `iframe`, ...).
    DangerousTag,
    /// Element name is not a plausible tag name (`scr<script`).
    MalformedTagName,
    /// Element belongs to a foreign namespace (SVG / MathML).
    ForeignNamespace,
    /// `on*` event handler attribute.
    EventHandler,
    /// Attribute not whitelisted for this element.
    DisallowedAttribute,
    /// Attribute carried a namespaced (`xlink:href`) name.
    NamespacedAttribute,
    /// URL attribute rejected by the scheme allow-list.
    DangerousUrl,
    /// `src` was not a managed relative asset path.
    NonManagedSrc,
    /// `style` declaration rejected by the CSS mini-sanitizer.
    DisallowedStyle,
    /// Attribute value failed its shape validation.
    InvalidValue,
    /// HTML comment node.
    HtmlComment,
    /// Doctype node.
    Doctype,
    /// Processing instruction node.
    ProcessingInstruction,
    /// `data:` URI image captured for externalization.
    DataUriImage,
}

/// One recorded removal. Kept for the security audit trail and QA assertions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removal {
    /// Element name the removal happened on (or `#comment`, `#doctype`).
    pub tag: String,
    /// Attribute name, when the removal concerns an attribute.
    pub attribute: Option<String>,
    /// Offending value, truncated for safe logging (never emitted verbatim).
    pub value: Option<String>,
    /// Why it was removed.
    pub reason: RemovalReason,
}

impl Removal {
    /// True when this removal represents an actual attack surface rather than
    /// benign authoring noise (e.g. an unwrapped `<div>`).
    pub fn is_threat(&self) -> bool {
        matches!(
            self.reason,
            RemovalReason::DangerousTag
                | RemovalReason::MalformedTagName
                | RemovalReason::ForeignNamespace
                | RemovalReason::EventHandler
                | RemovalReason::DangerousUrl
                | RemovalReason::NonManagedSrc
                | RemovalReason::DisallowedStyle
                | RemovalReason::NamespacedAttribute
                | RemovalReason::DataUriImage
        )
    }
}

/// A `data:` URI image payload lifted out of pasted HTML so the asset service
/// can externalize it (no base64 is ever persisted into the XML).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineDataImage {
    /// Declared MIME type, e.g. `image/png`.
    pub mime: String,
    /// Raw base64 payload (still encoded).
    pub base64: String,
    /// `alt` text of the originating `<img>`, if any.
    pub alt: String,
    /// Position of the `<img>` in the output stream, used to re-insert the
    /// managed reference after externalization.
    pub slot: usize,
}

/// Policy knobs for a single sanitization run.
#[derive(Debug, Clone)]
pub struct SanitizeOptions {
    /// Which choke point is calling.
    pub context: SanitizeContext,
    /// When true, `data:` URI images are captured into
    /// [`SanitizeOutcome::inline_images`] instead of only being dropped.
    /// Only meaningful for [`SanitizeContext::PasteImport`].
    pub capture_data_images: bool,
    /// When set, a managed `src` must point at exactly this document id.
    pub managed_document_id: Option<String>,
    /// Optional rewrite applied to accepted managed `src` values, used by the
    /// preview path to turn a document-relative reference into something the
    /// WebView can load. Returning `None` drops the image.
    pub src_rewrite: Option<fn(&str) -> Option<String>>,
    /// Input size ceiling.
    pub max_input_bytes: usize,
}

impl Default for SanitizeOptions {
    fn default() -> Self {
        Self {
            context: SanitizeContext::EditorToCore,
            capture_data_images: false,
            managed_document_id: None,
            src_rewrite: None,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
        }
    }
}

impl SanitizeOptions {
    /// Options for a given choke point with all other knobs at their default.
    pub fn for_context(context: SanitizeContext) -> Self {
        Self {
            context,
            capture_data_images: context == SanitizeContext::PasteImport,
            ..Default::default()
        }
    }
}

/// Sanitizer-level refusal (as opposed to per-node removal, which is reported
/// inside the outcome).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SanitizeError {
    /// Input exceeded [`SanitizeOptions::max_input_bytes`].
    #[error("rich-text input too large: {size} bytes (limit {limit})")]
    InputTooLarge { size: usize, limit: usize },
}

/// Result of one sanitization run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizeOutcome {
    /// The sanitized, re-serialized fragment. Safe to persist and to render.
    pub html: String,
    /// Choke point this outcome was produced for.
    pub context: SanitizeContext,
    /// Everything that was removed, in document order.
    pub removals: Vec<Removal>,
    /// `data:` URI images captured for externalization.
    pub inline_images: Vec<InlineDataImage>,
    /// Input size in bytes.
    pub input_bytes: usize,
    /// Output size in bytes.
    pub output_bytes: usize,
}

impl SanitizeOutcome {
    /// True when anything at all was removed or captured.
    pub fn changed(&self) -> bool {
        !self.removals.is_empty() || !self.inline_images.is_empty()
    }

    /// Number of security-relevant removals.
    pub fn threat_count(&self) -> usize {
        self.removals.iter().filter(|r| r.is_threat()).count()
    }

    /// True when the input contained no attack surface at all.
    pub fn is_clean(&self) -> bool {
        self.threat_count() == 0 && self.inline_images.is_empty()
    }

    /// Convenience accessor for the sanitized HTML.
    pub fn html(&self) -> &str {
        &self.html
    }
}

// ---------------------------------------------------------------------------
// Public entry points (RD-M7-013: four distinct choke points)
// ---------------------------------------------------------------------------

/// Sanitizes HTML arriving from the clipboard or a drag-drop
/// (`SanitizeContext::PasteImport`).
///
/// `data:` URI images are captured into [`SanitizeOutcome::inline_images`] so
/// the asset service can externalize them; the `<img>` element itself is
/// removed from the output and must be re-inserted with a managed reference.
pub fn sanitize_paste_import(html: &str) -> Result<SanitizeOutcome, SanitizeError> {
    sanitize_with(html, &SanitizeOptions::for_context(SanitizeContext::PasteImport))
}

/// Sanitizes stored content before it is handed to the editor
/// (`SanitizeContext::CoreToEditor`).
///
/// Defence in depth: the XML file may have been edited by AbstractSpoon TDL or
/// by hand, so stored HTML is never trusted just because we wrote it.
pub fn sanitize_for_editor(html: &str) -> Result<SanitizeOutcome, SanitizeError> {
    sanitize_with(html, &SanitizeOptions::for_context(SanitizeContext::CoreToEditor))
}

/// Sanitizes editor output before it is committed to Core and persisted
/// (`SanitizeContext::EditorToCore`).
pub fn sanitize_for_core(html: &str) -> Result<SanitizeOutcome, SanitizeError> {
    sanitize_with(html, &SanitizeOptions::for_context(SanitizeContext::EditorToCore))
}

/// Sanitizes content for the read-only preview (`SanitizeContext::PreviewRender`).
pub fn sanitize_for_preview(html: &str) -> Result<SanitizeOutcome, SanitizeError> {
    sanitize_with(html, &SanitizeOptions::for_context(SanitizeContext::PreviewRender))
}

/// Runs the whitelist sanitizer with an explicit policy.
pub fn sanitize_with(html: &str, opts: &SanitizeOptions) -> Result<SanitizeOutcome, SanitizeError> {
    if html.len() > opts.max_input_bytes {
        return Err(SanitizeError::InputTooLarge {
            size: html.len(),
            limit: opts.max_input_bytes,
        });
    }

    let mut outcome = SanitizeOutcome {
        html: String::new(),
        context: opts.context,
        removals: Vec::new(),
        inline_images: Vec::new(),
        input_bytes: html.len(),
        output_bytes: 0,
    };

    let parsed = Html::parse_fragment(html);
    let mut buf = String::with_capacity(html.len());
    let mut slot = 0usize;

    // Explicit DFS stack: `(node, next child index, tag to close on pop)`.
    // The node reference type is inferred, which keeps this module free of any
    // direct dependency on `ego-tree` (not a direct dependency of this crate).
    let mut stack: Vec<(_, usize, Option<String>)> = Vec::with_capacity(16);
    stack.push((parsed.tree.root(), 0, None));

    while !stack.is_empty() {
        let (node, idx) = {
            let frame = stack.last().expect("non-empty stack");
            (frame.0, frame.1)
        };
        stack.last_mut().expect("non-empty stack").1 += 1;

        let child = node.children().nth(idx);
        let child = match child {
            Some(c) => c,
            None => {
                let closing = stack.pop().and_then(|frame| frame.2);
                if let Some(tag) = closing {
                    buf.push_str("</");
                    buf.push_str(&tag);
                    buf.push('>');
                }
                continue;
            }
        };

        match child.value() {
            Node::Text(text) => {
                escape_text_into(&mut buf, &**text);
            }
            Node::Comment(comment) => {
                outcome.removals.push(Removal {
                    tag: "#comment".to_string(),
                    attribute: None,
                    value: Some(truncate(&**comment, 64)),
                    reason: RemovalReason::HtmlComment,
                });
            }
            Node::Doctype(doctype) => {
                outcome.removals.push(Removal {
                    tag: "#doctype".to_string(),
                    attribute: None,
                    value: Some(truncate(doctype.name(), 64)),
                    reason: RemovalReason::Doctype,
                });
            }
            Node::ProcessingInstruction(pi) => {
                outcome.removals.push(Removal {
                    tag: "#pi".to_string(),
                    attribute: None,
                    value: Some(truncate(&**pi, 64)),
                    reason: RemovalReason::ProcessingInstruction,
                });
            }
            Node::Element(element) => {
                let raw_name = element.name();
                let foreign = is_foreign_namespace(element);

                if foreign {
                    outcome.removals.push(Removal {
                        tag: raw_name.to_string(),
                        attribute: None,
                        value: None,
                        reason: RemovalReason::ForeignNamespace,
                    });
                    record_event_handlers(element, raw_name, &mut outcome);
                    continue; // subtree dropped
                }
                if is_dropped_subtree_tag(raw_name) {
                    outcome.removals.push(Removal {
                        tag: raw_name.to_string(),
                        attribute: None,
                        value: None,
                        reason: RemovalReason::DangerousTag,
                    });
                    record_event_handlers(element, raw_name, &mut outcome);
                    continue; // subtree dropped
                }
                if !is_valid_tag_name(raw_name) {
                    outcome.removals.push(Removal {
                        tag: truncate(raw_name, 64),
                        attribute: None,
                        value: None,
                        reason: RemovalReason::MalformedTagName,
                    });
                    record_event_handlers(element, raw_name, &mut outcome);
                    // Unwrap rather than drop the subtree. A probe such as
                    // `<scr<script>ipt>` tokenizes to an element literally named
                    // `scr<script` that is never closed, so everything after it
                    // in the fragment becomes its descendant. Dropping the
                    // subtree would let a 12-byte attacker prefix silently
                    // delete the rest of the user's comment. Unwrapping is safe
                    // because no tag is emitted and every text node is escaped
                    // on the way out, so nothing can re-form into markup.
                    stack.push((child, 0, None));
                    continue;
                }

                let canonical = match canonical_tag(raw_name) {
                    Some(name) => name,
                    None => {
                        outcome.removals.push(Removal {
                            tag: raw_name.to_string(),
                            attribute: None,
                            value: None,
                            reason: RemovalReason::DisallowedTag,
                        });
                        // No tag and no attributes are emitted for an unwrapped
                        // element, so its handlers must be reported separately.
                        record_event_handlers(element, raw_name, &mut outcome);
                        // Unwrap: keep children, emit no tags.
                        stack.push((child, 0, None));
                        continue;
                    }
                };

                let attrs = sanitize_attributes(element, canonical, opts, &mut outcome, &mut slot);

                // An <img> without a usable managed src is dropped entirely so
                // no broken/placeholder image is ever persisted.
                if canonical == "img" && !attrs.iter().any(|(name, _)| *name == "src") {
                    continue;
                }

                buf.push('<');
                buf.push_str(canonical);
                for (name, value) in &attrs {
                    buf.push(' ');
                    buf.push_str(name);
                    buf.push_str("=\"");
                    escape_attr_into(&mut buf, value);
                    buf.push('"');
                }
                buf.push('>');

                if is_void(canonical) {
                    continue;
                }
                stack.push((child, 0, Some(canonical.to_string())));
            }
            _ => {}
        }
    }

    outcome.output_bytes = buf.len();
    outcome.html = buf;
    Ok(outcome)
}

/// Records every `on*` handler on an element that is dropped or unwrapped
/// wholesale.
///
/// Those elements never reach [`sanitize_attributes`], so without this the
/// audit trail would silently under-report the most common attack attribute.
fn record_event_handlers(
    element: &scraper::node::Element,
    tag: &str,
    outcome: &mut SanitizeOutcome,
) {
    for (name, value) in element.attrs.iter() {
        let local: &str = &name.local;
        if local.starts_with("on") {
            outcome.removals.push(Removal {
                tag: tag.to_string(),
                attribute: Some(local.to_string()),
                value: Some(truncate(&**value, 64)),
                reason: RemovalReason::EventHandler,
            });
        }
    }
}

/// Collects and validates the attributes of one element.
///
/// Output is sorted by attribute name: `scraper` stores attributes in a hash
/// map, so iterating them directly would make the sanitized bytes
/// non-deterministic across runs — unacceptable for a value that is persisted
/// into a byte-stable XML document.
fn sanitize_attributes(
    element: &scraper::node::Element,
    tag: &'static str,
    opts: &SanitizeOptions,
    outcome: &mut SanitizeOutcome,
    slot: &mut usize,
) -> Vec<(String, String)> {
    let allowed = allowed_attributes(tag);

    let mut raw: Vec<(&str, &str, bool)> = element
        .attrs
        .iter()
        .map(|(name, value)| (&*name.local, &**value, name.prefix.is_some()))
        .collect();
    raw.sort_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(b.1)));

    let mut out: Vec<(String, String)> = Vec::with_capacity(raw.len());

    for (name, value, namespaced) in raw {
        if name.starts_with("on") {
            outcome.removals.push(Removal {
                tag: tag.to_string(),
                attribute: Some(name.to_string()),
                value: Some(truncate(value, 64)),
                reason: RemovalReason::EventHandler,
            });
            continue;
        }
        if namespaced {
            outcome.removals.push(Removal {
                tag: tag.to_string(),
                attribute: Some(name.to_string()),
                value: Some(truncate(value, 64)),
                reason: RemovalReason::NamespacedAttribute,
            });
            continue;
        }
        if !allowed.contains(&name) {
            outcome.removals.push(Removal {
                tag: tag.to_string(),
                attribute: Some(name.to_string()),
                value: Some(truncate(value, 64)),
                reason: RemovalReason::DisallowedAttribute,
            });
            continue;
        }

        match name {
            "href" => match sanitize_url(value) {
                Ok(clean) => out.push((name.to_string(), clean)),
                Err(rejection) => {
                    outcome.removals.push(Removal {
                        tag: tag.to_string(),
                        attribute: Some(name.to_string()),
                        value: Some(truncate(value, 64)),
                        reason: if rejection.is_security_relevant() {
                            RemovalReason::DangerousUrl
                        } else {
                            RemovalReason::InvalidValue
                        },
                    });
                }
            },
            "src" => {
                if is_data_uri(value) {
                    let mut placeholder = None;
                    if opts.capture_data_images {
                        if let Some(captured) = parse_data_uri(value) {
                            let alt = element
                                .attrs
                                .iter()
                                .find(|(k, _)| &*k.local == "alt")
                                .map(|(_, v)| truncate(&**v, MAX_TEXT_ATTR_LEN))
                                .unwrap_or_default();
                            outcome.inline_images.push(InlineDataImage {
                                mime: captured.0,
                                base64: captured.1,
                                alt,
                                slot: *slot,
                            });
                            // Keep the element in the stream with a placeholder
                            // src so the caller can splice in the managed
                            // reference after externalizing the bytes. An
                            // unreplaced placeholder is not a valid managed
                            // reference, so a later commit pass drops the img.
                            let index = outcome.inline_images.len() - 1;
                            placeholder = Some(inline_image_placeholder(index));
                        }
                    }
                    outcome.removals.push(Removal {
                        tag: tag.to_string(),
                        attribute: Some(name.to_string()),
                        value: Some(truncate(value, 32)),
                        reason: RemovalReason::DataUriImage,
                    });
                    if let Some(token) = placeholder {
                        out.push((name.to_string(), token));
                    }
                } else {
                    match sanitize_image_src(value, opts.managed_document_id.as_deref()) {
                        Ok(clean) => {
                            let final_src = match opts.src_rewrite {
                                Some(rewrite) => match rewrite(&clean) {
                                    Some(rewritten) => rewritten,
                                    None => {
                                        outcome.removals.push(Removal {
                                            tag: tag.to_string(),
                                            attribute: Some(name.to_string()),
                                            value: Some(truncate(&clean, 64)),
                                            reason: RemovalReason::NonManagedSrc,
                                        });
                                        continue;
                                    }
                                },
                                None => clean,
                            };
                            out.push((name.to_string(), final_src));
                        }
                        Err(_) => {
                            outcome.removals.push(Removal {
                                tag: tag.to_string(),
                                attribute: Some(name.to_string()),
                                value: Some(truncate(value, 64)),
                                reason: RemovalReason::NonManagedSrc,
                            });
                        }
                    }
                }
            }
            "style" => match sanitize_style(value) {
                Some(clean) => out.push((name.to_string(), clean)),
                None => outcome.removals.push(Removal {
                    tag: tag.to_string(),
                    attribute: Some(name.to_string()),
                    value: Some(truncate(value, 64)),
                    reason: RemovalReason::DisallowedStyle,
                }),
            },
            "class" => match sanitize_class(value) {
                Some(clean) => out.push((name.to_string(), clean)),
                None => outcome.removals.push(Removal {
                    tag: tag.to_string(),
                    attribute: Some(name.to_string()),
                    value: Some(truncate(value, 64)),
                    reason: RemovalReason::InvalidValue,
                }),
            },
            "alt" => {
                let clean = sanitize_text_attr(value);
                if !clean.is_empty() {
                    out.push((name.to_string(), clean));
                }
            }
            "width" | "height" | "start" => match sanitize_dimension(value) {
                Some(clean) => out.push((name.to_string(), clean)),
                None => outcome.removals.push(Removal {
                    tag: tag.to_string(),
                    attribute: Some(name.to_string()),
                    value: Some(truncate(value, 32)),
                    reason: RemovalReason::InvalidValue,
                }),
            },
            _ => {
                outcome.removals.push(Removal {
                    tag: tag.to_string(),
                    attribute: Some(name.to_string()),
                    value: Some(truncate(value, 64)),
                    reason: RemovalReason::DisallowedAttribute,
                });
            }
        }
    }

    *slot += 1;
    out
}

// ---------------------------------------------------------------------------
// URL policy (RD-M7-012)
// ---------------------------------------------------------------------------

/// Why a `href` value was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlRejection {
    /// Value was empty after normalization.
    Empty,
    /// Value exceeded [`MAX_URL_LEN`].
    TooLong,
    /// Scheme is not `http`/`https` (`javascript:`, `data:`, `vbscript:`, ...).
    DangerousScheme(String),
    /// No scheme at all: relative or protocol-relative URLs are refused for
    /// hyperlinks.
    SchemeRequired,
    /// Scheme syntax invalid, or authority missing (`https:foo`).
    Malformed,
    /// Contains a backslash or another character browsers normalize away.
    InvalidCharacter(String),
}

impl UrlRejection {
    /// True when the rejection represents an attack rather than a typo.
    pub fn is_security_relevant(&self) -> bool {
        matches!(
            self,
            UrlRejection::DangerousScheme(_) | UrlRejection::InvalidCharacter(_)
        )
    }
}

impl std::fmt::Display for UrlRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UrlRejection::Empty => write!(f, "empty url"),
            UrlRejection::TooLong => write!(f, "url longer than {MAX_URL_LEN} bytes"),
            UrlRejection::DangerousScheme(s) => write!(f, "scheme '{s}' is not allowed"),
            UrlRejection::SchemeRequired => write!(f, "url must carry an explicit http(s) scheme"),
            UrlRejection::Malformed => write!(f, "malformed url"),
            UrlRejection::InvalidCharacter(c) => write!(f, "url contains '{c}'"),
        }
    }
}

/// True for the characters a browser strips from the *ends* of a URL before
/// resolving it (C0 controls, space, DEL).
fn is_url_strippable(c: char) -> bool {
    (c as u32) <= 0x20 || c == '\u{7f}'
}

/// True when `value` starts with a `data:` scheme (after control stripping).
pub fn is_data_uri(value: &str) -> bool {
    normalize_url_chars(value)
        .to_ascii_lowercase()
        .starts_with("data:")
}

/// Applies the URL normalization a browser performs before resolution:
/// strip leading/trailing C0-control-or-space, then remove every ASCII tab,
/// LF and CR from the interior.
///
/// Doing the same here closes the `java\tscript:` / ` javascript:` /
/// `JAVaSCRIPT:` bypass family without silently rewriting anything else.
fn normalize_url_chars(raw: &str) -> String {
    raw.trim_matches(is_url_strippable)
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

/// Normalizes and validates a hyperlink URL. Only `http`/`https` survive.
///
/// The returned string is the *cleaned* value, which is what the caller must
/// serialize — never the raw input.
pub fn sanitize_url(raw: &str) -> Result<String, UrlRejection> {
    if raw.len() > MAX_URL_LEN {
        return Err(UrlRejection::TooLong);
    }

    let cleaned = normalize_url_chars(raw);
    if cleaned.is_empty() {
        return Err(UrlRejection::Empty);
    }
    if cleaned.contains('\\') {
        // Browsers map `\` onto `/`, which turns `https:/\evil.com` into a
        // protocol-relative URL. Refuse rather than guess.
        return Err(UrlRejection::InvalidCharacter("\\\\".to_string()));
    }
    if cleaned.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
        return Err(UrlRejection::InvalidCharacter("control".to_string()));
    }

    // A scheme is the token before the first ':' that appears before any
    // '/', '?' or '#'.
    let boundary = match cleaned.find([':', '/', '?', '#']) {
        Some(i) if cleaned.as_bytes()[i] == b':' => i,
        _ => return Err(UrlRejection::SchemeRequired),
    };

    let scheme = &cleaned[..boundary];
    if !is_valid_scheme_syntax(scheme) {
        return Err(UrlRejection::Malformed);
    }
    let lower = scheme.to_ascii_lowercase();
    if lower != "http" && lower != "https" {
        return Err(UrlRejection::DangerousScheme(lower));
    }
    if !cleaned[boundary + 1..].starts_with("//") {
        return Err(UrlRejection::Malformed);
    }
    Ok(cleaned)
}

fn is_valid_scheme_syntax(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

/// Why an `<img src>` value was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SrcRejection {
    /// Empty after normalization.
    Empty,
    /// Absolute filesystem path.
    AbsolutePath,
    /// Protocol-relative (`//host/...`).
    ProtocolRelative,
    /// Carries any scheme (`data:`, `http:`, `file:`, `javascript:`, ...).
    HasScheme(String),
    /// Percent-encoded — managed references are always literal ASCII.
    PercentEncoding,
    /// Contains whitespace or a control character in the interior.
    InvalidCharacter(String),
    /// `..` traversal beyond the allowed prefix depth, or a `.` segment.
    Traversal,
    /// Not shaped like `.assets/<doc>/images/<file>`.
    NotManagedLayout,
    /// Points at another document's asset directory.
    WrongDocument { found: String },
    /// File name is not `<stem>.<ext>` with a safe stem.
    BadFileName,
    /// Extension not in [`ALLOWED_IMAGE_EXTENSIONS`].
    BadExtension(String),
    /// Value too long.
    TooLong,
}

impl std::fmt::Display for SrcRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SrcRejection::Empty => write!(f, "empty src"),
            SrcRejection::AbsolutePath => write!(f, "absolute path is not a managed reference"),
            SrcRejection::ProtocolRelative => write!(f, "protocol-relative src is not allowed"),
            SrcRejection::HasScheme(s) => write!(f, "src scheme '{s}' is not allowed"),
            SrcRejection::PercentEncoding => write!(f, "percent-encoding is not allowed in src"),
            SrcRejection::InvalidCharacter(c) => write!(f, "src contains '{c}'"),
            SrcRejection::Traversal => write!(f, "src escapes the managed asset directory"),
            SrcRejection::NotManagedLayout => write!(f, "src is not .assets/<doc>/images/<file>"),
            SrcRejection::WrongDocument { found } => write!(f, "src belongs to document '{found}'"),
            SrcRejection::BadFileName => write!(f, "src file name is not a managed asset name"),
            SrcRejection::BadExtension(e) => write!(f, "extension '{e}' is not an allowed image"),
            SrcRejection::TooLong => write!(f, "src too long"),
        }
    }
}

/// Validates a document-relative managed image reference.
///
/// Accepted shape: `(../){0,4}.assets/<doc-id>/images/<stem>.<ext>` where
/// `<ext>` is one of [`ALLOWED_IMAGE_EXTENSIONS`] and `<stem>` is
/// `[A-Za-z0-9_-]{1,100}`. Absolute paths, protocol-relative paths, any
/// scheme, percent-encoding and traversal are all refused.
pub fn sanitize_image_src(raw: &str, expected_document_id: Option<&str>) -> Result<String, SrcRejection> {
    if raw.len() > MAX_URL_LEN {
        return Err(SrcRejection::TooLong);
    }
    // Only the ends are trimmed: an interior space or control character means
    // the value is not one of our managed references, and silently rewriting it
    // could redirect the reference at a different document.
    let cleaned = raw.trim_matches(is_url_strippable);
    if cleaned.is_empty() {
        return Err(SrcRejection::Empty);
    }
    if cleaned.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(SrcRejection::InvalidCharacter("whitespace".to_string()));
    }
    if cleaned.contains('%') {
        return Err(SrcRejection::PercentEncoding);
    }
    if cleaned.contains('\\') {
        return Err(SrcRejection::Traversal);
    }
    if cleaned.starts_with("//") {
        return Err(SrcRejection::ProtocolRelative);
    }
    if cleaned.starts_with('/') {
        return Err(SrcRejection::AbsolutePath);
    }
    if let Some(colon) = cleaned.find(':') {
        let scheme = &cleaned[..colon];
        if is_valid_scheme_syntax(scheme) {
            return Err(SrcRejection::HasScheme(scheme.to_ascii_lowercase()));
        }
    }

    let mut rest = cleaned;
    let mut depth = 0usize;
    loop {
        if let Some(tail) = rest.strip_prefix("../") {
            depth += 1;
            if depth > MAX_MANAGED_SRC_DEPTH {
                return Err(SrcRejection::Traversal);
            }
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("./") {
            rest = tail;
        } else {
            break;
        }
    }

    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() != 4 || parts[0] != ".assets" || parts[2] != "images" {
        return Err(SrcRejection::NotManagedLayout);
    }
    if !is_safe_path_segment(parts[1]) {
        return Err(SrcRejection::NotManagedLayout);
    }
    if let Some(expected) = expected_document_id {
        if parts[1] != expected {
            return Err(SrcRejection::WrongDocument {
                found: parts[1].to_string(),
            });
        }
    }
    validate_asset_file_name(parts[3])?;
    Ok(cleaned.to_string())
}

/// True when a path segment is a plain, non-traversing identifier.
pub fn is_safe_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 128
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !segment.contains("..")
}

/// Validates the `<uuid>.<ext>` file name of a managed image asset.
///
/// Extensions must already be lower case: the asset service always writes
/// lower-case extensions, so an upper-case one means the reference did not come
/// from us.
pub fn validate_asset_file_name(name: &str) -> Result<(), SrcRejection> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some(pair) => pair,
        None => return Err(SrcRejection::BadFileName),
    };
    if stem.is_empty()
        || stem.len() > 100
        || !stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(SrcRejection::BadFileName);
    }
    if !ALLOWED_IMAGE_EXTENSIONS.contains(&ext) {
        return Err(SrcRejection::BadExtension(ext.to_string()));
    }
    Ok(())
}

/// Builds the document-relative reference used inside rich text.
///
/// `depth` is the number of directory levels between the XML document and the
/// workspace root, i.e. how many `../` prefixes are needed.
pub fn managed_image_reference(document_id: &str, file_name: &str, depth: usize) -> String {
    let mut out = String::new();
    for _ in 0..depth.min(MAX_MANAGED_SRC_DEPTH) {
        out.push_str("../");
    }
    out.push_str(".assets/");
    out.push_str(document_id);
    out.push_str("/images/");
    out.push_str(file_name);
    out
}

/// Placeholder `src` emitted for a captured inline `data:` image.
///
/// The paste-import pass removes the base64 payload from the HTML and leaves
/// this token behind so the asset service can splice in the managed reference
/// once the bytes are on disk. The token is deliberately **not** a valid
/// managed reference: if it is ever left unreplaced, the next sanitization pass
/// drops the `<img>` instead of persisting a broken reference.
pub fn inline_image_placeholder(index: usize) -> String {
    format!("{{{{m7-inline-image:{index}}}}}")
}

/// Splits `data:<mime>;base64,<payload>` into `(mime, payload)`.
///
/// Returns `None` for anything that is not a base64 payload of an allowed
/// raster image type, so the caller can refuse it. `svg+xml` is deliberately
/// refused: SVG is a scripting host.
pub fn parse_data_uri(raw: &str) -> Option<(String, String)> {
    let cleaned = normalize_url_chars(raw);
    let lower = cleaned.to_ascii_lowercase();
    if !lower.starts_with("data:") {
        return None;
    }
    // Slice the original (not the lower-cased copy) so the base64 payload keeps
    // its case; the prefix length is identical.
    let rest = &cleaned["data:".len()..];
    let (meta, payload) = rest.split_once(',')?;
    let meta_lower = meta.to_ascii_lowercase();
    if !meta_lower.ends_with(";base64") {
        return None;
    }
    let mime = meta_lower[..meta_lower.len() - ";base64".len()].to_string();
    if !ALLOWED_DATA_URI_MIMES.contains(&mime.as_str()) {
        return None;
    }
    if payload.is_empty() || payload.len() > DEFAULT_MAX_INPUT_BYTES {
        return None;
    }
    if !payload
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
    {
        return None;
    }
    Some((mime, payload.to_string()))
}

/// MIME types accepted for externalization of pasted inline images.
pub const ALLOWED_DATA_URI_MIMES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
];

// ---------------------------------------------------------------------------
// Attribute value policy
// ---------------------------------------------------------------------------

/// Sanitizes a `class` attribute: safe tokens only, order preserved, deduped.
pub fn sanitize_class(raw: &str) -> Option<String> {
    let mut tokens: Vec<&str> = Vec::new();
    for token in raw.split_whitespace() {
        if token.len() > 64 {
            continue;
        }
        if !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            continue;
        }
        if !tokens.contains(&token) {
            tokens.push(token);
        }
        if tokens.len() == 8 {
            break;
        }
    }
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

/// Sanitizes a free-text attribute (`alt`): strips controls, caps the length.
pub fn sanitize_text_attr(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT_ATTR_LEN)
        .collect();
    cleaned
}

/// Validates a numeric dimension (`width`, `height`, `start`).
///
/// Accepts `123` or `123px`; everything else (including `100%`, `auto` and
/// `expression(...)`) is refused.
pub fn sanitize_dimension(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 8 {
        return None;
    }
    let digits = trimmed.strip_suffix("px").unwrap_or(trimmed);
    if digits.is_empty() || digits.len() > 6 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(digits.to_string())
}

/// Mini CSS sanitizer for the `style` attribute (RD-M7-012 "style (limited)").
///
/// Returns a canonicalized `prop: value; prop: value` string with properties
/// sorted alphabetically (deterministic output), or `None` when nothing
/// survived.
pub fn sanitize_style(raw: &str) -> Option<String> {
    if raw.len() > 1024 {
        return None;
    }
    let mut kept: Vec<(String, String)> = Vec::new();

    for declaration in raw.split(';') {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }
        let (property, value) = match declaration.split_once(':') {
            Some(pair) => pair,
            None => return None, // malformed style attribute: refuse the whole thing
        };
        let property = property.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() || value.len() > 64 {
            return None;
        }
        if !ALLOWED_STYLE_PROPERTIES.contains(&property.as_str()) {
            continue;
        }
        if !is_safe_style_value(value) {
            return None;
        }
        if kept.len() >= 16 {
            return None;
        }
        kept.push((property, value.to_string()));
    }

    if kept.is_empty() {
        return None;
    }
    kept.sort_by(|a, b| a.0.cmp(&b.0));
    kept.dedup_by(|a, b| a.0 == b.0);

    let joined = kept
        .iter()
        .map(|(p, v)| format!("{p}: {v}"))
        .collect::<Vec<_>>()
        .join("; ");
    if joined.len() > MAX_TEXT_ATTR_LEN {
        return None;
    }
    Some(joined)
}

fn is_safe_style_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if STYLE_VALUE_DENYLIST
        .iter()
        .any(|bad| lower.contains(bad))
    {
        return false;
    }
    value.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                ' ' | '#' | '%' | '.' | ',' | '-' | '_' | '(' | ')' | '/' | '+'
            )
    })
}

// ---------------------------------------------------------------------------
// Plain-text extraction (RD-M7-028 helper)
// ---------------------------------------------------------------------------

/// Converts an HTML fragment to search-indexable plain text.
///
/// Dangerous subtrees (`script`, `style`, ...) contribute nothing — their text
/// never reaches the search index. Block-level elements are separated by a
/// single `\n`; CJK text is passed through untouched (no spurious spaces are
/// inserted, so Chinese phrases remain contiguous and searchable).
pub fn html_to_plain_text(html: &str) -> String {
    let parsed = Html::parse_fragment(html);
    let mut buf = String::with_capacity(html.len());

    let mut stack: Vec<(_, usize, Option<&'static str>)> = Vec::with_capacity(16);
    stack.push((parsed.tree.root(), 0, None));

    while !stack.is_empty() {
        let (node, idx) = {
            let frame = stack.last().expect("non-empty stack");
            (frame.0, frame.1)
        };
        stack.last_mut().expect("non-empty stack").1 += 1;

        let child = node.children().nth(idx);
        let child = match child {
            Some(c) => c,
            None => {
                let closing = stack.pop().and_then(|frame| frame.2);
                if closing.is_some() {
                    buf.push('\n');
                }
                continue;
            }
        };

        match child.value() {
            Node::Text(text) => buf.push_str(&**text),
            Node::Element(element) => {
                let raw_name = element.name();
                let foreign = is_foreign_namespace(element);
                if foreign || is_dropped_subtree_tag(raw_name) || !is_valid_tag_name(raw_name) {
                    continue;
                }
                let tag = canonical_tag(raw_name);
                if tag == Some("br") {
                    buf.push('\n');
                    continue;
                }
                // Unknown-but-valid elements (`div`, `section`, `figure`, ...)
                // are treated as block separators so extraction stays readable.
                let breaks = tag.map(is_block).unwrap_or(true);
                if breaks && !buf.is_empty() && !buf.ends_with('\n') {
                    buf.push('\n');
                }
                let marker: Option<&'static str> = if breaks { Some(tag.unwrap_or("div")) } else { None };
                stack.push((child, 0, marker));
            }
            _ => {}
        }
    }

    normalize_extracted_text(&mut buf);
    buf
}

/// Collapses the raw extraction into index-friendly text: NBSP → space,
/// runs of newlines → one newline, per-line trailing whitespace removed,
/// whole string trimmed. CJK characters are never altered.
fn normalize_extracted_text(buf: &mut String) {
    let mut out = String::with_capacity(buf.len());
    for line in buf.replace('\u{a0}', " ").split('\n') {
        let trimmed = line.trim_end_matches(|c: char| c.is_whitespace());
        if trimmed.is_empty() {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    let trimmed = out.trim_matches('\n').to_string();
    *buf = trimmed;
}

// ---------------------------------------------------------------------------
// Escaping helpers
// ---------------------------------------------------------------------------

/// Escapes a text node so it can never re-enter the stream as markup.
fn escape_text_into(buf: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => buf.push_str("&amp;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '\u{a0}' => buf.push_str("&nbsp;"),
            _ => buf.push(c),
        }
    }
}

/// Escapes an attribute value (always emitted inside double quotes).
fn escape_attr_into(buf: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '&' => buf.push_str("&amp;"),
            '"' => buf.push_str("&quot;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '\u{a0}' => buf.push_str("&nbsp;"),
            _ => buf.push(c),
        }
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let short: String = value.chars().take(max_chars).collect();
    // Never log a value that could itself break out of a quoting context.
    short.replace(['<', '>', '"', '\''], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> String {
        sanitize_for_core(html).expect("within size limit").html
    }

    fn outcome(html: &str) -> SanitizeOutcome {
        sanitize_for_core(html).expect("within size limit")
    }

    #[test]
    fn benign_formatting_survives() {
        let input = "<p>Hello <strong>world</strong> and <em>friends</em>.</p>\
                     <ul><li>one</li><li>two</li></ul>\
                     <h2>Title</h2><blockquote>q</blockquote>\
                     <pre><code class=\"language-rust\">let x = 1;</code></pre>\
                     <a href=\"https://example.com/a?b=1&amp;c=2\">link</a><br>line";
        let out = clean(input);
        for fragment in [
            "<p>Hello <strong>world</strong> and <em>friends</em>.</p>",
            "<ul><li>one</li><li>two</li></ul>",
            "<h2>Title</h2>",
            "<blockquote>q</blockquote>",
            "<pre><code class=\"language-rust\">let x = 1;</code></pre>",
            "href=\"https://example.com/a?b=1&amp;c=2\"",
            "<br>",
        ] {
            assert!(out.contains(fragment), "missing {fragment:?} in {out}");
        }
        assert_eq!(outcome(input).threat_count(), 0);
    }

    #[test]
    fn legacy_aliases_are_folded_not_flattened() {
        assert_eq!(clean("<b>bold</b>"), "<strong>bold</strong>");
        assert_eq!(clean("<i>it</i>"), "<em>it</em>");
        assert_eq!(clean("<strike>x</strike>"), "<s>x</s>");
        assert_eq!(clean("<div>wrap</div>"), "wrap");
    }

    #[test]
    fn script_and_event_handlers_are_removed() {
        let out = outcome("<p onclick=\"steal()\">hi<script>alert(1)</script></p>");
        assert_eq!(out.html, "<p>hi</p>");
        assert!(out
            .removals
            .iter()
            .any(|r| r.reason == RemovalReason::EventHandler));
        assert!(out
            .removals
            .iter()
            .any(|r| r.reason == RemovalReason::DangerousTag && r.tag == "script"));
        assert!(!out.html.to_ascii_lowercase().contains("alert"));
    }

    #[test]
    fn nested_script_probe_cannot_produce_an_element() {
        // `<scr<script>ipt>` tokenizes to an element literally named
        // `scr<script`, which is never closed — so the rest of the fragment
        // becomes its descendant. It must be reported, must never be emitted as
        // an element, and must not swallow the surrounding content.
        let out = outcome("<scr<script>ipt>alert(1)</script><p>keep me</p>");
        assert!(!out.html.contains("<scr"));
        assert!(!out.html.to_ascii_lowercase().contains("<script"));
        assert!(out
            .removals
            .iter()
            .any(|r| r.reason == RemovalReason::MalformedTagName));
        // The probe residue survives only as escaped text, and the user's real
        // content is preserved instead of being deleted by the drop.
        assert_eq!(out.html, "ipt&gt;alert(1)<p>keep me</p>");

        // Re-parsing the output finds no live threat and is stable.
        let recheck = sanitize_for_core(&out.html).expect("ok");
        assert_eq!(recheck.html, out.html);
        assert_eq!(recheck.threat_count(), 0);
    }

    #[test]
    fn mixed_case_and_whitespace_obfuscation_is_defeated() {
        for payload in [
            "<SCRIPT>alert(1)</SCRIPT>",
            "<ScRiPt>alert(1)</ScRiPt>",
            "<img src=x onerror=alert(1)>",
            "<IMG SRC=x OnErRoR=alert(1)>",
            "<img src=\"x\" ONERROR=\"alert(1)\">",
        ] {
            let out = clean(payload);
            assert!(
                !out.to_ascii_lowercase().contains("script") && !out.contains("onerror"),
                "payload survived: {payload} -> {out}"
            );
            assert!(!out.contains("alert"), "payload survived: {payload} -> {out}");
        }
    }

    #[test]
    fn javascript_scheme_variants_are_rejected() {
        let payloads = [
            "javascript:alert(1)",
            "JAVASCRIPT:alert(1)",
            "JaVaScRiPt:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            "java\rscript:alert(1)",
            " javascript:alert(1)",
            "\tjavascript:alert(1)",
            "\u{01}javascript:alert(1)",
            "&#106;avascript:alert(1)",
            "&#0000106;avascript:alert(1)",
            "&#x6a;avascript:alert(1)",
            "javascript&#58;alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
        ];
        for payload in payloads {
            let html = format!("<a href=\"{payload}\">click</a>");
            let out = outcome(&html);
            assert!(
                !out.html.to_ascii_lowercase().contains("javascript"),
                "scheme survived: {payload} -> {}",
                out.html
            );
            assert!(!out.html.contains("href"), "href survived: {payload}");
            assert!(out.html.contains("click"), "text lost for {payload}");
        }
    }

    #[test]
    fn entity_encoded_scheme_is_decoded_then_rejected() {
        // html5ever decodes character references in attribute values, so the
        // sanitizer sees the real scheme. The anchor survives as inert text.
        let out = outcome("<a href=\"&#106;avascript:alert(1)\">x</a>");
        assert_eq!(out.html, "<a>x</a>");
        assert!(!out.html.contains("href"));
        assert!(!out.html.to_ascii_lowercase().contains("javascript"));
        assert!(out
            .removals
            .iter()
            .any(|r| r.reason == RemovalReason::DangerousUrl));
    }

    #[test]
    fn unquoted_attribute_injection_is_neutralized() {
        let out = clean("<a href=javascript:alert(1) class=ok>x</a>");
        assert!(!out.contains("javascript"));
        assert!(out.contains("class=\"ok\""));

        let out2 = clean("<img src=../.assets/doc/images/a.png onload=alert(1)>");
        assert!(!out2.contains("onload"));
        assert!(out2.contains("src=\"../.assets/doc/images/a.png\""));
    }

    #[test]
    fn backslash_and_protocol_relative_urls_are_rejected() {
        assert!(sanitize_url("https:/\\evil.com").is_err());
        assert!(sanitize_url("\\\\evil.com").is_err());
        assert!(sanitize_url("//evil.com/x").is_err());
        assert!(sanitize_url("/local/path").is_err());
        assert!(sanitize_url("https://good.example/x").is_ok());
        assert!(sanitize_url("HTTP://GOOD.EXAMPLE/X").is_ok());
    }

    #[test]
    fn svg_and_mathml_subtrees_are_dropped() {
        let out = clean("<p>a</p><svg><script>alert(1)</script><circle r=\"9\"/></svg><math><mi>x</mi></math>");
        assert_eq!(out, "<p>a</p>");
        let out2 = clean("<svg/onload=alert(1)>");
        assert!(!out2.contains("alert"));
    }

    #[test]
    fn iframe_object_embed_are_dropped_with_children() {
        let out = clean("before<iframe src=\"https://evil.example\">fallback</iframe>mid\
                         <object data=\"x.swf\">inner</object><embed src=\"y\">after");
        assert_eq!(out, "beforemidafter");
    }

    #[test]
    fn raw_text_hosts_are_dropped_not_unwrapped() {
        // Unwrapping <noscript>/<style>/<template> children is the classic
        // mutation-XSS pivot; the subtree must disappear entirely.
        for payload in [
            "<noscript><p title=\"</noscript><img src=x onerror=alert(1)>\">t</p></noscript>",
            "<style>p{}</style><img src=x onerror=alert(1)>",
            "<template><script>alert(1)</script></template>",
            "<textarea></textarea><script>alert(1)</script>",
            "<title><script>alert(1)</script></title>",
        ] {
            let out = clean(payload);
            assert!(!out.to_ascii_lowercase().contains("onerror"), "{payload} -> {out}");
            assert!(!out.contains("<script"), "{payload} -> {out}");
            assert!(!out.contains("alert(1)"), "{payload} -> {out}");
        }
    }

    #[test]
    fn data_uri_images_are_dropped_by_default() {
        let out = outcome("<img src=\"data:image/png;base64,iVBORw0KGgo=\">");
        assert_eq!(out.html, "");
        assert!(out.inline_images.is_empty(), "commit path must not capture");
        assert!(out
            .removals
            .iter()
            .any(|r| r.reason == RemovalReason::DataUriImage));
    }

    #[test]
    fn paste_import_captures_data_uri_for_externalization() {
        let out = sanitize_paste_import("<p>x</p><img alt=\"pic\" src=\"data:image/png;base64,iVBORw0KGgo=\">")
            .expect("ok");
        assert_eq!(out.html, "<p>x</p><img alt=\"pic\" src=\"{{m7-inline-image:0}}\">");
        assert_eq!(out.inline_images.len(), 1);
        assert_eq!(out.inline_images[0].mime, "image/png");
        assert_eq!(out.inline_images[0].base64, "iVBORw0KGgo=");
        assert_eq!(out.inline_images[0].alt, "pic");
        assert_eq!(out.inline_images[0].slot, 1);
        assert!(!out.html.contains("base64"));
        assert_eq!(inline_image_placeholder(0), "{{m7-inline-image:0}}");
        // An unreplaced placeholder never survives a commit pass.
        assert_eq!(clean(&out.html), "<p>x</p>");
    }

    #[test]
    fn sanitization_is_idempotent() {
        let inputs = [
            "<p onclick=x>hi</p><script>alert(1)</script>",
            "<a href=\"javascript:alert(1)\">l</a>",
            "<p>a &lt;script&gt; b &amp; c</p>",
            "<span style=\"color: red; position: absolute\">s</span>",
            "<img src=\"../.assets/d/images/u.png\" alt=\"x\">",
            "<p>&nbsp;中文&nbsp;</p>",
        ];
        for input in inputs {
            let once = clean(input);
            let twice = clean(&once);
            let thrice = clean(&twice);
            assert_eq!(once, twice, "not idempotent for {input}");
            assert_eq!(twice, thrice, "not stable for {input}");
        }
    }

    #[test]
    fn double_escaped_text_never_becomes_markup() {
        // Text that *looks* like markup must stay inert forever.
        let once = clean("<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>");
        assert_eq!(once, "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>");
        assert_eq!(clean(&once), once);
        assert!(!clean(&clean(&once)).contains("<script>"));
    }

    #[test]
    fn style_attribute_is_limited_and_canonicalized() {
        assert_eq!(
            sanitize_style("color: red; text-align: center").as_deref(),
            Some("color: red; text-align: center")
        );
        // Sorted, deterministic output regardless of source order.
        assert_eq!(
            sanitize_style("text-align:center;color:red").as_deref(),
            Some("color: red; text-align: center")
        );
        assert_eq!(
            sanitize_style("color: red; position: absolute; z-index: 9").as_deref(),
            Some("color: red")
        );
        assert!(sanitize_style("background: url(javascript:alert(1))").is_none());
        assert!(sanitize_style("width: expression(alert(1))").is_none());
        assert!(sanitize_style("behavior: url(#default#time2)").is_none());
        assert!(sanitize_style("-moz-binding: url(x)").is_none());
        assert!(sanitize_style("color: red /* } */ ; evil: yes").is_none());
        assert!(sanitize_style("color:red\"onmouseover=alert(1)").is_none());
    }

    #[test]
    fn style_survives_on_allowed_elements() {
        let out = clean("<span style=\"color: #ff0000; font-size: 14px\">t</span>");
        assert!(out.contains("color: #ff0000"));
        assert!(out.contains("font-size: 14px"));
    }

    #[test]
    fn src_must_be_a_managed_relative_path() {
        let good = "../.assets/doc-1/images/0d6f4a1c-1b6a-4c3d-9d1e-2f3a4b5c6d7e.png";
        assert!(sanitize_image_src(good, Some("doc-1")).is_ok());
        assert!(sanitize_image_src(".assets/doc-1/images/a_b-1.jpg", None).is_ok());
        assert!(sanitize_image_src("../../.assets/doc-1/images/x.gif", None).is_ok());

        assert_eq!(
            sanitize_image_src("/etc/passwd", None),
            Err(SrcRejection::AbsolutePath)
        );
        assert!(matches!(
            sanitize_image_src("file:///C:/Windows/x.png", None),
            Err(SrcRejection::HasScheme(_))
        ));
        assert!(matches!(
            sanitize_image_src("data:image/png;base64,AA==", None),
            Err(SrcRejection::HasScheme(_))
        ));
        assert_eq!(
            sanitize_image_src("../.assets/doc-1/images/../../../../Windows/x.png", None),
            Err(SrcRejection::NotManagedLayout)
        );
        assert_eq!(
            sanitize_image_src("../../../../../.assets/d/images/x.png", None),
            Err(SrcRejection::Traversal)
        );
        assert_eq!(
            sanitize_image_src("../.assets/doc-1/images/x.png", Some("doc-2")),
            Err(SrcRejection::WrongDocument {
                found: "doc-1".to_string()
            })
        );
        assert!(matches!(
            sanitize_image_src("../.assets/doc-1/images/x.svg", None),
            Err(SrcRejection::BadExtension(_))
        ));
        assert_eq!(
            sanitize_image_src("../.%2E/assets/d/images/x.png", None),
            Err(SrcRejection::PercentEncoding)
        );
        assert_eq!(
            sanitize_image_src("..\\.assets\\d\\images\\x.png", None),
            Err(SrcRejection::Traversal)
        );
    }

    #[test]
    fn managed_document_id_is_enforced_end_to_end() {
        let opts = SanitizeOptions {
            managed_document_id: Some("doc-9".to_string()),
            ..SanitizeOptions::for_context(SanitizeContext::EditorToCore)
        };
        let ok = sanitize_with(
            "<img src=\"../.assets/doc-9/images/a1b2c3d4.png\">",
            &opts,
        )
        .expect("ok");
        assert!(ok.html.contains("doc-9"));
        let bad = sanitize_with(
            "<img src=\"../.assets/other/images/a1b2c3d4.png\">",
            &opts,
        )
        .expect("ok");
        assert_eq!(bad.html, "");
    }

    #[test]
    fn img_without_valid_src_is_dropped() {
        assert_eq!(clean("<img>"), "");
        assert_eq!(clean("<img src=\"\">"), "");
        assert_eq!(clean("<img src=\"https://evil.example/x.png\">"), "");
        assert_eq!(clean("<img src=x onerror=alert(1)>"), "");
    }

    #[test]
    fn html_comments_and_doctype_are_stripped() {
        let out = outcome("<!DOCTYPE html><!--[if IE]><script>alert(1)</script><![endif]--><p>x</p>");
        assert_eq!(out.html, "<p>x</p>");
        assert!(!out.html.to_ascii_lowercase().contains("doctype"));
        assert!(!out.html.contains("alert"));
        assert!(!out.html.contains("<!--"));
        assert!(out
            .removals
            .iter()
            .any(|r| r.reason == RemovalReason::HtmlComment));

        // A doctype only becomes a node in full-document parsing; either way it
        // never reaches the output.
        let doc = Html::parse_document("<!DOCTYPE html><html><body><p>y</p></body></html>");
        assert!(doc.tree.root().descendants().any(|n| matches!(
            n.value(),
            Node::Doctype(_)
        )));
        assert_eq!(clean("<!DOCTYPE html><p>y</p>"), "<p>y</p>");
    }

    #[test]
    fn attribute_order_is_deterministic() {
        let input = "<a style=\"color: red\" class=\"c1\" href=\"https://e.example\">x</a>";
        let a = clean(input);
        let b = clean(input);
        assert_eq!(a, b);
        assert!(a.starts_with("<a class=\"c1\" href=\"https://e.example\" style=\"color: red\">"));
    }

    #[test]
    fn legacy_html_body_wrapper_is_unpacked() {
        let out = clean("<html><body><p>This is an <b>HTML</b> comment.</p></body></html>");
        assert_eq!(out, "<p>This is an <strong>HTML</strong> comment.</p>");
    }

    #[test]
    fn oversize_input_is_refused() {
        let opts = SanitizeOptions {
            max_input_bytes: 16,
            ..Default::default()
        };
        let err = sanitize_with("<p>0123456789abcdef0123456789</p>", &opts).unwrap_err();
        assert!(matches!(err, SanitizeError::InputTooLarge { .. }));
    }

    #[test]
    fn four_entry_points_are_distinct_and_all_safe() {
        let payload = "<p onclick=\"x()\">hi</p><script>alert(1)</script>\
                       <a href=\"javascript:alert(1)\">l</a>";
        let paste = sanitize_paste_import(payload).expect("ok");
        let feed = sanitize_for_editor(payload).expect("ok");
        let commit = sanitize_for_core(payload).expect("ok");
        let preview = sanitize_for_preview(payload).expect("ok");

        assert_eq!(paste.context, SanitizeContext::PasteImport);
        assert_eq!(feed.context, SanitizeContext::CoreToEditor);
        assert_eq!(commit.context, SanitizeContext::EditorToCore);
        assert_eq!(preview.context, SanitizeContext::PreviewRender);
        for out in [&paste, &feed, &commit, &preview] {
            assert_eq!(out.html, "<p>hi</p><a>l</a>");
            assert!(out.threat_count() >= 2);
        }
        assert_ne!(
            SanitizeContext::PasteImport.as_str(),
            SanitizeContext::PreviewRender.as_str()
        );
    }

    #[test]
    fn preview_src_rewrite_is_applied() {
        let opts = SanitizeOptions {
            src_rewrite: Some(|src| Some(format!("asset://localhost/{src}"))),
            ..SanitizeOptions::for_context(SanitizeContext::PreviewRender)
        };
        let out = sanitize_with("<img src=\"../.assets/d/images/x.png\">", &opts).expect("ok");
        assert!(out.html.contains("asset://localhost/../.assets/d/images/x.png"));
    }

    #[test]
    fn plain_text_extraction_uses_block_separators() {
        let text = html_to_plain_text(
            "<h1>Title</h1><p>First <strong>para</strong></p><ul><li>alpha</li><li>beta</li></ul>\
             <p>line1<br>line2</p><script>secret()</script>",
        );
        assert_eq!(text, "Title\nFirst para\nalpha\nbeta\nline1\nline2");
        assert!(!text.contains("secret"));
    }

    #[test]
    fn plain_text_extraction_preserves_cjk() {
        let text = html_to_plain_text("<p>任务描述</p><ul><li>第一项</li><li>第二项</li></ul>");
        assert_eq!(text, "任务描述\n第一项\n第二项");
        assert!(text.contains("任务描述"));
        // No spurious spaces inserted between CJK characters.
        assert!(!text.contains("任 务"));
    }

    #[test]
    fn plain_text_extraction_handles_entities_and_nbsp() {
        let text = html_to_plain_text("<p>a&nbsp;b &amp; c &lt;d&gt;</p>");
        assert_eq!(text, "a b & c <d>");
    }

    #[test]
    fn data_uri_parser_is_strict() {
        assert_eq!(
            parse_data_uri("data:image/png;base64,AAAA").map(|(m, _)| m).as_deref(),
            Some("image/png")
        );
        assert!(parse_data_uri("data:text/html;base64,AAAA").is_none());
        assert!(parse_data_uri("data:image/png,AAAA").is_none());
        assert!(parse_data_uri("data:image/svg+xml;base64,AAAA").is_none());
        assert!(parse_data_uri("https://x/y.png").is_none());
        assert!(is_data_uri("  DATA:image/png;base64,AA=="));
    }
}
