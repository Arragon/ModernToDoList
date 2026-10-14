//! M7 Rich Content QA integration tests (INH-1069 … INH-1072).
//!
//! Covers the backend-testable subset of QA-M7-001~016:
//!
//! | QA id | Subject | Test |
//! |-------|---------|------|
//! | QA-M7-001 | PLAIN_TEXT preserved on save | `qa_m7_001_*` |
//! | QA-M7-002 | HTML preserved, content sanitized | `qa_m7_002_*` |
//! | QA-M7-003 | RTF read-only + type indicator | `qa_m7_003_*` |
//! | QA-M7-004 | Unknown COMMENTSTYPE preserved read-only | `qa_m7_004_*` |
//! | QA-M7-005 | Conversion confirmed + undoable | `qa_m7_005_*` |
//! | QA-M7-006 | Viewing never creates/converts | `qa_m7_006_*` |
//! | QA-M7-007 | XSS sanitation matrix | `qa_m7_007_*` |
//! | QA-M7-008 | Dangerous URL matrix | `qa_m7_008_*` |
//! | QA-M7-009 | Four choke points all sanitize | `qa_m7_009_*` |
//! | QA-M7-010 | Managed layout + BLAKE3 | `qa_m7_010_*` |
//! | QA-M7-011 | Relative refs, no base64 in XML | `qa_m7_011_*` |
//! | QA-M7-012 | Clipboard / drag-drop ingestion | `qa_m7_012_*` |
//! | QA-M7-013 | Orphan candidates, no deletion | `qa_m7_013_*` |
//! | QA-M7-014 | GC needs no live reference | `qa_m7_014_*` |
//! | QA-M7-015 | GC explicit, dry-run, grace period | `qa_m7_015_*` |
//! | QA-M7-016 | No per-keystroke serialization | `qa_m7_016_*` |
//!
//! UI-only cases that cannot be covered from Rust are listed at the bottom of
//! this file.
//!
//! ## Why the modules are pulled in with `#[path]`
//!
//! `domain::sanitizer`, `domain::comments` and `domain::rich_text` are not yet
//! declared in `src-tauri/src/domain/mod.rs` (that file is owned by another
//! workstream), so the library crate does not compile them yet. Including them
//! here keeps `cargo test` green regardless of wiring order and exercises the
//! exact same source files. The `pub mod domain { ... }` wrapper makes the
//! `crate::domain::…` and `super::…` paths inside those files resolve
//! identically in both contexts, so once the orchestrator adds the three
//! `pub mod` lines to `domain/mod.rs` nothing here has to change.
//!
//! The XML round-trip checks at the bottom do use the real library, and are
//! separated by the `BEGIN-LIB-ONLY` marker comment.

pub mod domain {
    //! Mirror of `crate::domain` for the M7 modules under test.
    #[path = "../../src/domain/sanitizer.rs"]
    pub mod sanitizer;
    #[path = "../../src/domain/comments.rs"]
    pub mod comments;
    #[path = "../../src/domain/rich_text.rs"]
    pub mod rich_text;
}

use domain::comments::{
    project_for_view, CommentsError, CommentsField, CommentsType, EditorMode, TaskComments,
};
use domain::rich_text::{
    base64_encode, collect_image_file_names, extract_plain_text, extract_search_text,
    opaque_placeholder, sanitize_suggested_file_name, AssetError, DescriptionCommit, GcOptions,
    ImageAssetStore, IngestSource, KeepReason, ReferenceSet, RichTextConfig, RichTextSession,
    VirtualClock,
};
use domain::sanitizer::{
    html_to_plain_text, sanitize_for_core, sanitize_for_editor, sanitize_for_preview,
    sanitize_image_src, sanitize_paste_import, sanitize_url, SrcRejection, UrlRejection,
};

/// A real 1x1 transparent PNG.
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
    0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
    0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
    0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// A minimal JPEG header plus filler.
fn jpeg_bytes() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    bytes.extend_from_slice(&[0x42u8; 64]);
    bytes.extend_from_slice(&[0xFF, 0xD9]);
    bytes
}

fn temp_store(name: &str) -> (tempfile::TempDir, ImageAssetStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ImageAssetStore::new(dir.path(), format!("doc-{name}"), 1).expect("store");
    (dir, store)
}

fn ingest_png(store: &ImageAssetStore, name: &str) -> domain::rich_text::IngestedAsset {
    store
        .ingest_bytes(PNG_1X1, name, IngestSource::Clipboard)
        .expect("ingest png")
}

fn clean(html: &str) -> String {
    sanitize_for_core(html).expect("within size limit").html
}

/// GC options that would delete anything eligible immediately.
fn aggressive_gc() -> GcOptions {
    GcOptions {
        user_confirmed: true,
        dry_run: false,
        grace_period_ms: 0,
        require_orphan_mark: false,
        now_ms: u64::MAX,
    }
}

// ===========================================================================
// QA-M7-001 ~ QA-M7-006 — comments type preservation and conversion matrix
// ===========================================================================

/// QA-M7-001: a PLAIN_TEXT comment stays PLAIN_TEXT through edit + save, and
/// its bytes are stored verbatim (no HTML escaping, no sanitizing).
#[test]
fn qa_m7_001_plain_text_type_is_preserved_on_save() {
    let original = "第一行\n第二行 <not markup> & \"quotes\"";
    let mut field = CommentsField::new(Some(TaskComments::plain(original)));

    // A read-only view reports the plain-text editor.
    let projection = field.view();
    assert_eq!(projection.mode, EditorMode::PlainText);
    assert!(projection.editable);
    assert_eq!(projection.editor_payload, original);
    assert!(projection.type_indicator.contains("PLAIN_TEXT"));

    // Saving through the plain-text path keeps COMMENTSTYPE unchanged.
    let edited = "第一行（已编辑）\n第二行 <still not markup> & \"quotes\"";
    field.save(edited, CommentsType::Html).expect("save"); // wrong default must be ignored
    let value = field.value().expect("present");
    assert_eq!(value.comments_type, CommentsType::PlainText);
    assert_eq!(value.attr_value(), "PLAIN_TEXT");
    assert_eq!(value.content, edited);
    assert_eq!(field.metrics().type_changes, 0);

    // Plain text is never HTML-sanitized: the user's characters are their own.
    field.save("<b>kept as text</b>", CommentsType::PlainText).expect("save");
    assert_eq!(field.value().unwrap().content, "<b>kept as text</b>");
    assert_eq!(field.value().unwrap().comments_type, CommentsType::PlainText);
    // ... but the *preview* rendering is inert.
    let preview = field.view();
    assert!(preview.display_html.contains("&lt;b&gt;kept as text&lt;/b&gt;"));
    assert!(!preview.display_html.contains("<b>"));
}

/// QA-M7-002: an HTML comment stays HTML, and every save produces sanitized
/// HTML.
#[test]
fn qa_m7_002_html_type_is_preserved_and_content_sanitized() {
    let mut field = CommentsField::new(Some(TaskComments::html(
        "<p>original</p>",
    )));
    assert_eq!(field.view().mode, EditorMode::RichText);

    field
        .save(
            "<p>edited <strong>bold</strong></p><script>alert(1)</script>",
            CommentsType::PlainText, // wrong default must be ignored
        )
        .expect("save");

    let value = field.value().expect("present");
    assert_eq!(value.comments_type, CommentsType::Html);
    assert_eq!(value.attr_value(), "HTML");
    assert_eq!(value.content, "<p>edited <strong>bold</strong></p>");
    assert_eq!(field.metrics().type_changes, 0);

    // Legacy authoring spellings survive as their whitelisted equivalents.
    field.save("<b>bold</b><i>it</i>", CommentsType::PlainText).expect("save");
    assert_eq!(field.value().unwrap().content, "<strong>bold</strong><em>it</em>");
    assert_eq!(field.value().unwrap().comments_type, CommentsType::Html);
}

/// QA-M7-003: RTF is read-only — no edit, no convert, but a type indicator.
#[test]
fn qa_m7_003_rtf_is_read_only_with_type_indicator() {
    let rtf = TaskComments {
        comments_type: CommentsType::Rtf,
        content: "{\\rtf1\\ansi 内容 must not be touched.\\par}".to_string(),
    };
    let mut field = CommentsField::new(Some(rtf.clone()));

    let projection = field.view();
    assert_eq!(projection.mode, EditorMode::ReadOnly);
    assert!(!projection.editable);
    assert!(!projection.convertible_to_rich_text);
    assert!(projection.type_indicator.contains("RTF"));
    assert!(projection.type_indicator.contains("只读"));
    // The editor receives nothing to edit, and the RTF is never interpreted as
    // markup: the preview shows an escaped excerpt inside a single <p>.
    assert_eq!(projection.editor_payload, "");
    assert!(projection.display_html.starts_with("<p>"));
    assert!(projection.display_html.ends_with("</p>"));
    // Only the single wrapping paragraph: no markup from the RTF itself.
    assert_eq!(projection.display_html.matches('<').count(), 2);
    assert!(projection.display_html.contains("rtf1"));
    assert!(projection.display_text.contains("内容"));
    assert!(projection.display_text.contains("rtf1"));

    // Editing is refused and the payload is untouched.
    let err = field.save("tampered", CommentsType::PlainText).unwrap_err();
    assert!(matches!(err, CommentsError::ReadOnly(_)));
    assert_eq!(field.value(), Some(&rtf));
    assert_eq!(field.metrics().saves, 0);
    assert_eq!(field.metrics().type_changes, 0);

    // Conversion is refused too.
    assert!(matches!(
        field.convert_to_rich_text(true),
        Err(CommentsError::ReadOnly(_))
    ));
    assert_eq!(field.value(), Some(&rtf));

    // Search extraction yields nothing indexable, but the UI gets a reason.
    assert_eq!(extract_plain_text(&rtf), "");
    assert!(opaque_placeholder(&rtf).unwrap().contains("RTF"));
}

/// QA-M7-004: unknown COMMENTSTYPE values are preserved byte-for-byte and are
/// read-only.
#[test]
fn qa_m7_004_unknown_commentstype_is_preserved_and_read_only() {
    for raw in ["MARKDOWN", "html", "Html", "PLAIN TEXT", "", "SOMETHING_ELSE"] {
        // Byte-exact preservation of the attribute value.
        let parsed = CommentsType::from_attr_value(raw);
        assert_eq!(parsed.as_attr_value(), raw, "{raw:?} not preserved");
        assert_eq!(parsed, CommentsType::Unknown(raw.to_string()));
        assert!(parsed.is_read_only());
        assert!(!parsed.can_convert_to_rich_text());
        assert_eq!(parsed.editor_mode(), EditorMode::ReadOnly);

        let payload = TaskComments {
            comments_type: parsed.clone(),
            content: "# original content".to_string(),
        };
        let mut field = CommentsField::new(Some(payload.clone()));
        let projection = field.view();
        assert_eq!(projection.mode, EditorMode::ReadOnly);
        assert!(!projection.editable);
        assert!(projection.type_indicator.contains("只读"));

        assert!(matches!(
            field.save("# tampered", CommentsType::Html),
            Err(CommentsError::ReadOnly(_))
        ));
        assert!(matches!(
            field.convert_to_rich_text(true),
            Err(CommentsError::ReadOnly(_))
        ));
        assert_eq!(field.value(), Some(&payload), "{raw:?} payload changed");
        assert_eq!(field.value().unwrap().attr_value(), raw);
        assert_eq!(field.metrics().type_changes, 0);
        assert_eq!(extract_plain_text(&payload), "");
    }
}

/// QA-M7-005: "Convert to Rich Text" is confirmation-gated, changes
/// COMMENTSTYPE exactly once, and is fully reversible via Undo.
#[test]
fn qa_m7_005_convert_to_rich_text_is_confirmed_and_undoable() {
    let mut field = CommentsField::new(Some(TaskComments::plain("第一行\n第二行\n\n第二段")));
    let before = field.value().cloned().expect("present");
    assert_eq!(before.attr_value(), "PLAIN_TEXT");
    assert!(field.view().convertible_to_rich_text);

    // Without confirmation nothing happens at all.
    assert_eq!(
        field.convert_to_rich_text(false),
        Err(CommentsError::ConfirmationRequired)
    );
    assert_eq!(field.value(), Some(&before));
    assert_eq!(field.metrics().type_changes, 0);

    // With confirmation the type changes exactly once.
    let conversion = field.convert_to_rich_text(true).expect("converted");
    assert!(conversion.confirmed);
    assert_eq!(conversion.before, before);
    assert_eq!(conversion.after.comments_type, CommentsType::Html);
    assert_eq!(conversion.after.attr_value(), "HTML");
    assert_eq!(
        conversion.after.content,
        "<p>第一行<br>第二行</p><p>第二段</p>"
    );
    assert_eq!(field.comments_type(), CommentsType::Html);
    assert_eq!(field.metrics().type_changes, 1);
    assert_eq!(field.view().mode, EditorMode::RichText);
    // A converted comment is no longer offered the same conversion.
    assert!(!field.view().convertible_to_rich_text);
    assert_eq!(
        field.convert_to_rich_text(true),
        Err(CommentsError::AlreadyRichText)
    );

    // Undo restores the exact previous payload and type, byte for byte.
    field.undo_conversion(&conversion);
    assert_eq!(field.value(), Some(&before));
    assert_eq!(field.comments_type(), CommentsType::PlainText);
    assert_eq!(field.value().unwrap().content, "第一行\n第二行\n\n第二段");
    assert_eq!(field.metrics().type_changes, 0);
    assert_eq!(field.metrics().conversion_undos, 1);
    assert!(field.view().convertible_to_rich_text);

    // Redoing the conversion yields identical bytes.
    let again = field.convert_to_rich_text(true).expect("converted");
    assert_eq!(again.after, conversion.after);
}

/// QA-M7-006: viewing content never auto-creates and never auto-converts the
/// COMMENTS payload or its type.
#[test]
fn qa_m7_006_viewing_never_creates_or_converts_comments() {
    // A task with no COMMENTS at all.
    let mut empty = CommentsField::new(None);
    for _ in 0..100 {
        let projection = empty.view();
        assert!(!projection.has_comments);
        assert!(projection.must_not_materialize);
        assert_eq!(projection.editor_payload, "");
        assert_eq!(projection.display_html, "");
    }
    assert_eq!(empty.value(), None);
    assert_eq!(empty.metrics().views, 100);
    assert_eq!(empty.metrics().materializations, 0);
    assert_eq!(empty.metrics().type_changes, 0);
    assert_eq!(empty.metrics().saves, 0);
    // Looking at the task and saving an empty editor must not create anything.
    empty.save("", CommentsType::PlainText).expect("no-op");
    assert_eq!(empty.value(), None);
    assert_eq!(empty.metrics().materializations, 0);
    // Converting a task with no comments is refused rather than materializing
    // a COMMENTSTYPE on an empty task.
    assert_eq!(
        empty.convert_to_rich_text(true),
        Err(CommentsError::NothingToConvert)
    );
    assert_eq!(empty.value(), None);

    // Every stored type survives repeated viewing unchanged.
    for original in [
        TaskComments::plain("纯文本内容"),
        TaskComments::html("<p>富文本内容</p>"),
        TaskComments {
            comments_type: CommentsType::Rtf,
            content: "{\\rtf1 x}".to_string(),
        },
        TaskComments {
            comments_type: CommentsType::Unknown("MARKDOWN".to_string()),
            content: "# md".to_string(),
        },
    ] {
        let mut field = CommentsField::new(Some(original.clone()));
        for _ in 0..25 {
            // Viewing includes every read-only projection the UI performs.
            let projection = field.view();
            assert_eq!(projection.comments_type, original.comments_type);
            assert_eq!(projection.byte_length, original.content.len());
            let _ = extract_plain_text(&original);
            let _ = extract_search_text(field.value());
            let _ = project_for_view(field.value());
            let _ = opaque_placeholder(&original);
        }
        assert_eq!(
            field.value(),
            Some(&original),
            "viewing mutated {:?}",
            original.comments_type
        );
        assert_eq!(field.metrics().type_changes, 0);
        assert_eq!(field.metrics().materializations, 0);
        assert_eq!(field.metrics().saves, 0);
    }
}

// ===========================================================================
// QA-M7-007 ~ QA-M7-009 — XSS and dangerous URL sanitation matrix
// ===========================================================================

/// QA-M7-007: the XSS payload matrix is neutralized while benign formatting
/// survives.
#[test]
fn qa_m7_007_xss_payload_matrix_is_neutralized() {
    let payloads = [
        // Classic script injection.
        "<script>alert(1)</script>",
        "<SCRIPT>alert(1)</SCRIPT>",
        "<ScRiPt>alert(1)</sCrIpT>",
        "<script src=\"https://evil.example/x.js\"></script>",
        "<script type=\"text/javascript\">alert(1)</script>",
        // Nested / obfuscated tokenizer probes.
        "<scr<script>ipt>alert(1)</script>",
        "<scri<script>pt>alert(1)</scri</script>pt>",
        "<<script>script>alert(1)<</script>script>",
        // Event handlers, quoted and unquoted.
        "<img src=x onerror=alert(1)>",
        "<img src=\"x\" onerror=\"alert(1)\">",
        "<IMG SRC=x OnErRoR=alert(1)>",
        "<body onload=alert(1)>text</body>",
        "<p onclick=\"alert(1)\" onmouseover=\"alert(2)\">t</p>",
        "<a href=\"#\" onfocus=alert(1) tabindex=0>x</a>",
        "<div onpointerenter=alert(1)>x</div>",
        // Foreign hosts.
        "<iframe src=\"https://evil.example\"></iframe>",
        "<object data=\"x.swf\"></object>",
        "<embed src=\"x.swf\">",
        "<svg><script>alert(1)</script></svg>",
        "<svg/onload=alert(1)>",
        "<math><mtext><table><mglyph><style><img src=x onerror=alert(1)>",
        // Raw-text / deferred-content hosts (mXSS pivots).
        "<noscript><p title=\"</noscript><img src=x onerror=alert(1)>\">t</p></noscript>",
        "<template><script>alert(1)</script></template>",
        "<style>p{background:url('javascript:alert(1)')}</style>",
        "<textarea></textarea><script>alert(1)</script>",
        "<title><script>alert(1)</script></title>",
        "<xmp><script>alert(1)</script></xmp>",
        // Attribute injection through style / class.
        "<p style=\"color:red\" onmouseover=\"alert(1)\">t</p>",
        "<p style=\"width: expression(alert(1))\">t</p>",
        "<p style=\"behavior: url(#default#time2)\">t</p>",
        "<p style=\"-moz-binding: url(https://evil.example/x.xml#x)\">t</p>",
        "<p class=\"a\" style=\"color:red;/*}*/\" id=\"x\" data-evil=\"1\">t</p>",
        // Meta refresh / base hijack.
        "<base href=\"https://evil.example/\">",
        "<meta http-equiv=\"refresh\" content=\"0;url=javascript:alert(1)\">",
        "<link rel=\"stylesheet\" href=\"https://evil.example/x.css\">",
        "<form action=\"https://evil.example\"><input name=\"x\"></form>",
        // Comments and conditional comments.
        "<!--[if IE]><script>alert(1)</script><![endif]-->",
        "<!-- <img src=x onerror=alert(1)> -->",
        // Mutation-XSS through double escaping.
        "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>",
        "&lt;img src=x onerror=alert(1)&gt;",
    ];

    for payload in payloads {
        let out = sanitize_for_core(payload).expect("ok");

        // Rigorous, payload-independent property: a second pass over the output
        // finds nothing left to remove, i.e. no live dangerous construct
        // survived, and the result is stable.
        let recheck = sanitize_for_core(&out.html).expect("ok");
        assert_eq!(recheck.html, out.html, "not stable: {payload}");
        assert_eq!(
            recheck.threat_count(),
            0,
            "live danger survived: {payload} -> {}",
            out.html
        );

        // Direct substring checks, for payloads that actually carried live
        // markup. Already-escaped payloads (`&lt;img src=x onerror=…&gt;`) are
        // inert *text* that must be preserved verbatim, so the literal words
        // may legitimately appear in the output — inside entities, never inside
        // a tag.
        if payload_has_live_threat(payload) {
            let lowered = out.html.to_ascii_lowercase();
            for forbidden in [
                "<script",
                "<iframe",
                "<object",
                "<embed",
                "<svg",
                "<style",
                "<form",
                "<base",
                "<meta",
                "<link",
                "<template",
                "<noscript",
                "onerror",
                "onload",
                "onclick",
                "onmouseover",
                "onpointer",
                "javascript:",
                "expression(",
                "behavior:",
                "-moz-binding",
                "<!--",
                "data:image",
            ] {
                assert!(
                    !lowered.contains(forbidden),
                    "{forbidden:?} survived: {payload} -> {}",
                    out.html
                );
            }
        }

        // Audit-trail completeness: when the payload carried live markup-level
        // danger and the output still contains elements, a threat must have been
        // reported. (`<body>` is the exception that produces neither: in
        // *fragment* parsing html5ever ignores a nested `<body>` start tag
        // outright, attributes included, so there is no node to report on.)
        let needs_report = out.html.contains('<') && payload_has_live_threat(payload);
        assert!(
            !needs_report || out.threat_count() > 0,
            "unreported danger: {payload} -> {}",
            out.html
        );
    }

    // The `<body>` nuance, pinned explicitly.
    let body = sanitize_for_core("<body onload=alert(1)>text</body>").expect("ok");
    assert_eq!(body.html, "text");
    assert!(!body.html.contains('<'));
    assert!(!body.html.to_ascii_lowercase().contains("onload"));

    // Already-escaped markup is inert content and must survive unchanged.
    let escaped = "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>";
    assert_eq!(clean(escaped), escaped);
    assert_eq!(clean(&clean(escaped)), escaped);
}

/// True when a payload contains markup-level danger (as opposed to inert,
/// already-escaped text).
fn payload_has_live_threat(payload: &str) -> bool {
    const MARKERS: &[&str] = &[
        "<script", "<iframe", "<object", "<embed", "<svg", "<style", "<math", "<form", "<base",
        "<meta", "<link", "<template", "<noscript", "<textarea", "<title", "<xmp", "<!--",
        " onerror", " onload", " onclick", " onmouseover", " onfocus", " onpointer",
    ];
    // Without a raw `<` there is no markup at all: the payload is inert text.
    if !payload.contains('<') {
        return false;
    }
    let lowered = payload.to_ascii_lowercase();
    MARKERS.iter().any(|marker| lowered.contains(marker))
}

/// QA-M7-007 (part 2): benign rich-text formatting survives the same pass.
#[test]
fn qa_m7_007_benign_formatting_survives_sanitation() {
    let benign = concat!(
        "<h1>标题</h1><h2>Sub</h2><h3>Sub</h3><h4>a</h4><h5>b</h5><h6>c</h6>",
        "<p>段落 with <strong>bold</strong>, <em>italic</em>, <u>underline</u>, ",
        "<s>strike</s> and <span style=\"color: #ff0000; font-size: 14px\">colored</span>.</p>",
        "<ul><li>项目一</li><li>项目二</li></ul>",
        "<ol start=\"3\"><li>three</li></ol>",
        "<blockquote>引用</blockquote>",
        "<pre><code class=\"language-rust\">let x = 1; // &lt;not a tag&gt;</code></pre>",
        "<p>Inline <code>code</code> and a <a href=\"https://example.com/a?b=1&amp;c=2#frag\">link</a>.</p>",
        "<p>Line<br>break and a non-breaking&nbsp;space.</p>",
    );
    let out = sanitize_for_core(benign).expect("ok");
    assert_eq!(out.threat_count(), 0, "benign content flagged: {:?}", out.removals);
    for expected in [
        "<h1>标题</h1>",
        "<h6>c</h6>",
        "<strong>bold</strong>",
        "<em>italic</em>",
        "<u>underline</u>",
        "<s>strike</s>",
        "<span style=\"color: #ff0000; font-size: 14px\">colored</span>",
        "<ul><li>项目一</li><li>项目二</li></ul>",
        "<ol start=\"3\"><li>three</li></ol>",
        "<blockquote>引用</blockquote>",
        "<pre><code class=\"language-rust\">",
        "&lt;not a tag&gt;",
        "<code>code</code>",
        "href=\"https://example.com/a?b=1&amp;c=2#frag\"",
        "<br>",
        "&nbsp;",
    ] {
        assert!(out.html.contains(expected), "lost {expected:?}\n got {}", out.html);
    }
    // Idempotent: re-sanitizing benign content is a no-op.
    assert_eq!(clean(&out.html), out.html);
}

/// QA-M7-008: the dangerous-URL matrix is rejected; http/https survive.
#[test]
fn qa_m7_008_dangerous_url_matrix_is_rejected() {
    let dangerous = [
        "javascript:alert(1)",
        "JAVASCRIPT:alert(1)",
        "JaVaScRiPt:alert(1)",
        "javascript&#58;alert(1)",
        "java\tscript:alert(1)",
        "java\nscript:alert(1)",
        "java\rscript:alert(1)",
        "java\u{0b}script:alert(1)",
        " javascript:alert(1)",
        "\tjavascript:alert(1)",
        "\njavascript:alert(1)",
        "\u{00}javascript:alert(1)",
        "\u{1f}javascript:alert(1)",
        "&#106;avascript:alert(1)",
        "&#0000106;avascript:alert(1)",
        "&#x6a;avascript:alert(1)",
        "&#X6A;avascript:alert(1)",
        "vbscript:msgbox(1)",
        "VBScript:msgbox(1)",
        "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
        "data:image/png;base64,iVBORw0KGgo=",
        "DATA:text/html,<script>alert(1)</script>",
        "file:///C:/Windows/win.ini",
        "blob:https://evil.example/uuid",
        "about:blank",
        "chrome://settings",
        "ms-msdt:/a/a",
        "search-ms:query=evil",
        "\\\\evil.example\\share\\x",
        "https:/\\evil.example",
        "http://ok.example\\@evil.example",
        "//evil.example/x",
        "/local/absolute",
        "relative/path.html",
        "mailto:someone@example.com",
        "tel:+1234567890",
    ];
    for url in dangerous {
        let result = sanitize_url(url);
        assert!(result.is_err(), "dangerous URL accepted: {url:?} -> {result:?}");
    }

    let safe = [
        "http://example.com",
        "https://example.com",
        "https://example.com/a/b?c=d&e=f#g",
        "HTTPS://EXAMPLE.COM/Upper",
        "https://例子.测试/中文",
        "  https://example.com/trimmed  ",
        "https://example.com/path%20with%20percent",
    ];
    for url in safe {
        let cleaned = sanitize_url(url).expect("safe url refused");
        let lowered = cleaned.to_ascii_lowercase();
        assert!(
            lowered.starts_with("http://") || lowered.starts_with("https://"),
            "unexpected scheme: {cleaned}"
        );
        assert!(!cleaned.contains('\\'));
        assert!(!cleaned.chars().any(|c| c.is_control()));
        assert!(!cleaned.starts_with(' ') && !cleaned.ends_with(' '));
        assert_eq!(
            sanitize_url(&cleaned).expect("idempotent"),
            cleaned,
            "url sanitation not idempotent: {url:?}"
        );
    }

    // The same matrix applied end-to-end through an anchor: the text survives,
    // the href never does.
    for url in dangerous {
        let out = sanitize_for_core(&format!("<a href=\"{url}\">链接</a>")).expect("ok");
        assert!(!out.html.contains("href"), "href survived for {url:?}: {}", out.html);
        assert!(out.html.contains("链接"), "text lost for {url:?}");
    }
}

/// QA-M7-008 (part 2): `src` must be a managed relative path.
#[test]
fn qa_m7_008_image_src_matrix_is_restricted_to_managed_paths() {
    let good = [
        "../.assets/doc-1/images/3f2b9c1e-7a44-4d2b-9f1a-6c8e0d2b4a71.png",
        ".assets/doc-1/images/a1b2c3d4.jpg",
        "../../.assets/doc-1/images/x_y-1.jpeg",
        "../.assets/doc-1/images/a.gif",
        "../.assets/doc-1/images/a.webp",
        "../.assets/doc-1/images/a.bmp",
    ];
    for src in good {
        assert!(sanitize_image_src(src, None).is_ok(), "refused {src}");
        let out = clean(&format!("<img src=\"{src}\" alt=\"x\">"));
        assert!(out.contains(src), "lost managed src {src}: {out}");
    }

    let bad = [
        "https://evil.example/tracker.png",
        "http://evil.example/tracker.png",
        "//evil.example/tracker.png",
        "/etc/passwd",
        "C:\\Windows\\win.ini",
        "file:///C:/Windows/win.ini",
        "data:image/png;base64,iVBORw0KGgo=",
        "javascript:alert(1)",
        "../.assets/doc-1/images/../../../../Windows/x.png",
        "../../../../../.assets/doc-1/images/x.png",
        "../.assets/doc-1/images/x.svg",
        "../.assets/doc-1/images/x.html",
        "../.assets/doc-1/images/x.exe",
        "../.assets/doc-1/images/.hidden",
        "../.assets/../doc-1/images/x.png",
        "../.assets/doc-1/attachments/x.png",
        "../.assets/doc-1/images/%2e%2e%2f%2e%2e%2fx.png",
        "..\\.assets\\doc-1\\images\\x.png",
        "../.assets/doc 1/images/x.png",
        "../.assets//images/x.png",
        "",
    ];
    for src in bad {
        assert!(
            sanitize_image_src(src, None).is_err(),
            "dangerous src accepted: {src:?}"
        );
        let out = clean(&format!("<img src=\"{src}\">"));
        assert!(!out.contains("<img"), "img survived for {src:?}: {out}");
    }

    // Cross-document references are refused when the document id is pinned.
    let pinned = "../.assets/doc-1/images/a1b2c3d4.png";
    assert!(sanitize_image_src(pinned, Some("doc-1")).is_ok());
    assert_eq!(
        sanitize_image_src(pinned, Some("doc-2")),
        Err(SrcRejection::WrongDocument {
            found: "doc-1".to_string()
        })
    );
    assert!(matches!(
        sanitize_url("javascript:alert(1)"),
        Err(UrlRejection::DangerousScheme(_))
    ));
}

/// QA-M7-009: all four choke points sanitize independently, and the whole
/// pipeline is stable when the passes are chained.
#[test]
fn qa_m7_009_all_four_choke_points_sanitize() {
    let hostile = concat!(
        "<p onclick=\"steal()\">Benign</p>",
        "<script>alert(1)</script>",
        "<img src=\"x\" onerror=\"alert(2)\">",
        "<a href=\"javascript:alert(3)\">link</a>",
        "<iframe src=\"https://evil.example\"></iframe>",
        "<svg><script>alert(4)</script></svg>",
        "<p style=\"behavior: url(#default#time2)\">styled</p>"
    );

    let paste = sanitize_paste_import(hostile).expect("ok");
    let feed = sanitize_for_editor(hostile).expect("ok");
    let commit = sanitize_for_core(hostile).expect("ok");
    let preview = sanitize_for_preview(hostile).expect("ok");

    // Each entry point is distinct and correctly labelled.
    assert_eq!(paste.context, domain::sanitizer::SanitizeContext::PasteImport);
    assert_eq!(feed.context, domain::sanitizer::SanitizeContext::CoreToEditor);
    assert_eq!(commit.context, domain::sanitizer::SanitizeContext::EditorToCore);
    assert_eq!(preview.context, domain::sanitizer::SanitizeContext::PreviewRender);
    assert_eq!(paste.context.as_str(), "paste_import");
    assert_eq!(feed.context.as_str(), "core_to_editor");
    assert_eq!(commit.context.as_str(), "editor_to_core");
    assert_eq!(preview.context.as_str(), "preview_render");

    for (label, out) in [
        ("paste", &paste),
        ("feed", &feed),
        ("commit", &commit),
        ("preview", &preview),
    ] {
        assert_eq!(out.html, "<p>Benign</p><a>link</a><p>styled</p>", "{label} output");
        assert!(out.threat_count() >= 4, "{label} under-reported: {:?}", out.removals);
        assert!(out.changed());
        assert!(!out.is_clean());
    }

    // Chaining every pass in pipeline order converges and stays converged.
    let mut current = hostile.to_string();
    for pass in 0..4 {
        let next = match pass {
            0 => sanitize_paste_import(&current).expect("ok").html,
            1 => sanitize_for_editor(&current).expect("ok").html,
            2 => sanitize_for_core(&current).expect("ok").html,
            _ => sanitize_for_preview(&current).expect("ok").html,
        };
        assert_eq!(next, "<p>Benign</p><a>link</a><p>styled</p>", "pass {pass}");
        current = next;
    }
    // A stored value that a third party tampered with is cleaned on the way
    // back into the editor (defence in depth).
    let tampered = "<p>ok</p><img src=x onerror=alert(1)>";
    assert_eq!(sanitize_for_editor(tampered).expect("ok").html, "<p>ok</p>");
}

// ===========================================================================
// QA-M7-010 ~ QA-M7-015 — managed image asset lifecycle matrix
// ===========================================================================

/// QA-M7-010: managed storage layout, UUID file names and BLAKE3 verification.
#[test]
fn qa_m7_010_managed_image_layout_and_blake3_verification() {
    let (dir, store) = temp_store("layout");
    let asset = ingest_png(&store, "screenshot.png");

    // Layout: <workspace>/.assets/<doc>/images/<uuid>.<ext>
    let expected_dir = dir.path().join(".assets").join("doc-layout").join("images");
    assert_eq!(store.images_dir(), expected_dir);
    assert_eq!(asset.abs_path.parent().unwrap(), expected_dir);
    assert!(asset.abs_path.exists());

    // File name is a UUID plus a sniffed extension, never the suggested name.
    let stem = asset.file_name.strip_suffix(".png").expect("png");
    assert_eq!(stem.len(), 36, "not a uuid: {stem}");
    assert_eq!(stem.matches('-').count(), 4);
    assert_ne!(asset.file_name, "screenshot.png");
    assert_eq!(asset.source_name, "screenshot.png");
    assert_eq!(asset.id, stem);

    // BLAKE3 metadata + verification.
    assert_eq!(asset.blake3.len(), 64);
    assert_eq!(asset.blake3, domain::rich_text::blake3_hex(PNG_1X1));
    assert!(store.verify(&asset.file_name).expect("verify"));
    assert_eq!(store.read(&asset.file_name).expect("read"), PNG_1X1);
    assert_eq!(asset.size_bytes, PNG_1X1.len() as u64);
    assert_eq!(asset.mime, "image/png");
    assert_eq!(asset.source, IngestSource::Clipboard);

    // Tampering with the stored bytes is detected.
    let mut tampered = PNG_1X1.to_vec();
    tampered[20] ^= 0xFF;
    std::fs::write(&asset.abs_path, &tampered).expect("write");
    assert!(!store.verify(&asset.file_name).expect("verify"));

    // Index metadata is persisted next to the images.
    let index = store.load_index();
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.entries[0].file_name, asset.file_name);
    assert!(store.index_path().exists());

    // The format is sniffed, not trusted from the file name.
    let store2 = ImageAssetStore::new(dir.path(), "doc-sniff", 1).expect("store");
    let mislabelled = store2
        .ingest_bytes(&jpeg_bytes(), "definitely-a.png", IngestSource::DragDrop)
        .expect("ingest");
    assert!(mislabelled.file_name.ends_with(".jpg"));
    assert_eq!(mislabelled.mime, "image/jpeg");
    // Non-images are refused, whatever they are called.
    assert_eq!(
        store2.ingest_bytes(b"MZ\x90\x00executable", "x.png", IngestSource::DragDrop),
        Err(AssetError::UnsupportedImageFormat)
    );
    assert_eq!(
        store2.ingest_bytes(b"<svg onload=alert(1)/>", "x.png", IngestSource::DragDrop),
        Err(AssetError::UnsupportedImageFormat)
    );
    assert_eq!(
        store2.ingest_bytes(b"", "x.png", IngestSource::Clipboard),
        Err(AssetError::EmptyPayload)
    );
    assert_eq!(
        store2.ingest_bytes(&vec![0u8; domain::rich_text::MAX_IMAGE_BYTES + 1], "big.png", IngestSource::Clipboard),
        Err(AssetError::PayloadTooLarge {
            size: domain::rich_text::MAX_IMAGE_BYTES + 1,
            limit: domain::rich_text::MAX_IMAGE_BYTES
        })
    );
}

/// QA-M7-011: rich text references managed images by relative path and never
/// carries base64 into the XML.
#[test]
fn qa_m7_011_relative_references_and_no_base64_in_xml() {
    let (_dir, store) = temp_store("refs");
    let asset = ingest_png(&store, "pic.png");

    // Reference form required by RD-M7-022.
    assert_eq!(
        asset.reference,
        format!("../.assets/doc-refs/images/{}", asset.file_name)
    );
    assert!(asset.reference.starts_with("../"));
    assert!(!asset.reference.contains('\\'));
    assert!(std::path::Path::new(&asset.reference).is_relative());

    // The reference resolves back to the stored file and nowhere else.
    let resolved = store.resolve_reference(&asset.reference).expect("resolve");
    assert_eq!(resolved, domain::rich_text::normalize_lexically(&asset.abs_path));

    // A depth-0 document (XML directly in the workspace root) omits the "../".
    let flat = ImageAssetStore::new(store.workspace_root(), "doc-refs", 0).expect("store");
    assert_eq!(
        flat.reference_for("a-b.png"),
        ".assets/doc-refs/images/a-b.png"
    );

    // Pasted base64 is externalized, not embedded.
    let payload = base64_encode(PNG_1X1);
    let pasted = format!(
        "<p>看图</p><img alt=\"截图\" src=\"data:image/png;base64,{payload}\"><p>结束</p>"
    );
    let report = store.externalize_pasted_html(&pasted).expect("report");
    assert_eq!(report.ingested.len(), 1);
    // Content-addressed dedupe: the pasted PNG is the one already stored.
    assert!(report.ingested[0].deduplicated);
    assert_eq!(report.ingested[0].file_name, asset.file_name);
    assert!(!report.html.contains("base64"), "base64 leaked: {}", report.html);
    assert!(!report.html.contains(&payload));
    assert!(report.html.contains(&asset.reference));
    assert!(report.html.contains("alt=\"截图\""));
    assert!(report.html.contains("<p>看图</p>"));
    assert!(report.html.contains("<p>结束</p>"));
    // The rewritten HTML is already commit-clean.
    assert_eq!(clean(&report.html), report.html);
    // What would actually be written into <COMMENTS> in the XML.
    let serialized = format!("<COMMENTS><![CDATA[{}]]></COMMENTS>", report.html);
    assert!(!serialized.contains("base64"));
    assert!(serialized.len() < pasted.len());

    // The commit path drops base64 outright when there is no asset service.
    let commit = sanitize_for_core(&pasted).expect("ok");
    assert!(!commit.html.contains("base64"));
    assert!(!commit.html.contains("<img"));
    // A non-image data URI is never stored.
    let hostile = "<img src=\"data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==\">";
    let report = store.externalize_pasted_html(hostile).expect("report");
    assert!(report.ingested.is_empty());
    assert_eq!(report.html, "");
}

/// QA-M7-012: clipboard and drag-drop ingestion accept raw bytes plus a
/// suggested name, and the name is sanitized against path traversal.
#[test]
fn qa_m7_012_clipboard_and_dragdrop_ingestion_sanitize_filenames() {
    let (_dir, store) = temp_store("ingest");

    let from_clipboard = store
        .ingest_bytes(PNG_1X1, "ClipboardImage.png", IngestSource::Clipboard)
        .expect("clipboard");
    assert_eq!(from_clipboard.source, IngestSource::Clipboard);
    assert_eq!(from_clipboard.source_name, "ClipboardImage.png");
    assert!(from_clipboard.abs_path.exists());

    let from_drop = store
        .ingest_bytes(&jpeg_bytes(), "photo.jpg", IngestSource::DragDrop)
        .expect("dragdrop");
    assert_eq!(from_drop.source, IngestSource::DragDrop);
    assert_ne!(from_drop.file_name, from_clipboard.file_name);
    assert_eq!(store.list_files().len(), 2);

    // Hostile suggested names are neutralized (display name only; the on-disk
    // name is always <uuid>.<ext>).
    let hostile_names = [
        ("../../etc/passwd", "passwd"),
        ("..\\..\\Windows\\System32\\evil.png", "evil.png"),
        ("C:\\Users\\x\\evil.png", "evil.png"),
        ("//server/share/evil.png", "evil.png"),
        ("con.png", "_con.png"),
        ("NUL", "_NUL"),
        ("LPT1.jpg", "_LPT1.jpg"),
        ("a<b>c|d?e*f\"g:h.png", "abcdefgh.png"),
        ("....", "image"),
        ("", "image"),
        ("\u{0}hidden.png", "hidden.png"),
    ];
    for (raw, expected) in hostile_names {
        assert_eq!(
            sanitize_suggested_file_name(raw),
            expected,
            "filename not sanitized: {raw:?}"
        );
    }
    // Unicode display names are preserved.
    assert_eq!(
        sanitize_suggested_file_name("截图 2026-09-22 上午.png"),
        "截图 2026-09-22 上午.png"
    );
    // Over-long names are capped with the extension intact.
    let long = format!("{}.png", "x".repeat(400));
    let capped = sanitize_suggested_file_name(&long);
    assert!(capped.chars().count() <= 120);
    assert!(capped.ends_with(".png"));

    // A hostile suggested name can never influence the stored path.
    let asset = store
        .ingest_bytes(PNG_1X1, "../../../../evil.png", IngestSource::DragDrop)
        .expect("ingest");
    assert_eq!(asset.abs_path.parent().unwrap(), store.images_dir());
    assert!(asset.abs_path.starts_with(store.images_dir()));
    assert_eq!(asset.file_name.matches('/').count(), 0);
    assert_eq!(asset.file_name.matches('\\').count(), 0);
    // Only one new file appeared (the PNG dedupes onto the clipboard asset).
    assert!(asset.deduplicated);
    assert_eq!(store.list_files().len(), 2);
    // Nothing was written outside the asset root.
    assert!(!store.workspace_root().join("evil.png").exists());
    assert!(!std::path::Path::new("/etc/passwd").exists());
}

/// QA-M7-013: removing an image from rich text marks an orphan candidate and
/// deletes nothing.
#[test]
fn qa_m7_013_removed_image_becomes_orphan_candidate_without_deletion() {
    let (_dir, store) = temp_store("orphan");
    let kept = ingest_png(&store, "kept.png");
    let mut removed_bytes = jpeg_bytes();
    removed_bytes[10] = 0x99; // distinct payload
    let removed = store
        .ingest_bytes(&removed_bytes, "removed.jpg", IngestSource::Clipboard)
        .expect("ingest");

    // Rich text still references `kept`; `removed` was deleted by the user.
    let mut references = ReferenceSet::new();
    references.push_xml(format!(
        "<COMMENTS><![CDATA[<img src=\"{}\">]]></COMMENTS>",
        kept.reference
    ));

    let marked = store.detect_orphans(&references, 1_000).expect("detect");
    assert_eq!(marked, vec![removed.file_name.clone()]);
    assert!(removed.abs_path.exists(), "detect_orphans must not delete");
    assert!(kept.abs_path.exists());

    let manifest = store.load_orphans();
    assert!(manifest.contains(&removed.file_name));
    assert!(!manifest.contains(&kept.file_name));
    assert_eq!(manifest.entries[0].marked_at_ms, 1_000);
    assert_eq!(manifest.entries[0].blake3.as_deref(), Some(removed.blake3.as_str()));
    assert!(store.orphan_manifest_path().exists());

    // Marking is idempotent.
    assert!(!store
        .mark_orphan(&removed.file_name, "again", 2_000)
        .expect("mark"));
    assert_eq!(store.load_orphans().entries.len(), 1);

    // Undo restoring the reference clears the stale mark automatically.
    references.push_rich_text(format!("<img src=\"{}\">", removed.reference));
    let marked = store.detect_orphans(&references, 3_000).expect("detect");
    assert!(marked.is_empty());
    assert!(store.load_orphans().entries.is_empty());
    assert!(removed.abs_path.exists());

    // An explicit orphan mark never overrides a live reference: both assets are
    // marked by hand, both are still referenced, so neither may be collected.
    references = ReferenceSet::new();
    references.push_xml(format!("<img src=\"{}\">", kept.reference));
    references.push_rich_text(format!("<img src=\"{}\">", removed.reference));
    store.mark_orphan(&kept.file_name, "manual", 4_000).expect("mark");
    store
        .mark_orphan(&removed.file_name, "manual", 4_000)
        .expect("mark");
    let report = store
        .run_gc(&references, &aggressive_gc())
        .expect("gc");
    assert!(report.deleted.is_empty());
    assert!(report.eligible.is_empty());
    assert_eq!(
        report.decisions.iter().find(|d| d.file_name == kept.file_name).unwrap().keep_reason,
        Some(KeepReason::ReferencedInXml)
    );
    assert_eq!(
        report.decisions.iter().find(|d| d.file_name == removed.file_name).unwrap().keep_reason,
        Some(KeepReason::ReferencedInRichText)
    );
    assert!(kept.abs_path.exists());
    assert!(removed.abs_path.exists());
}

/// QA-M7-014: GC eligibility requires no live reference in XML, rich text,
/// Undo history **or** the recovery journal.
#[test]
fn qa_m7_014_gc_requires_no_live_reference_in_any_source() {
    let (_dir, store) = temp_store("liveness");
    let asset = ingest_png(&store, "live.png");
    let reference = asset.reference.clone();
    let file_name = asset.file_name.clone();
    store.mark_orphan(&file_name, "stale", 0).expect("mark");
    let opts = aggressive_gc();

    let cases: Vec<(&str, ReferenceSet, KeepReason)> = vec![
        (
            "xml",
            {
                let mut r = ReferenceSet::new();
                r.push_xml(format!("<COMMENTS><![CDATA[<img src=\"{reference}\">]]></COMMENTS>"));
                r
            },
            KeepReason::ReferencedInXml,
        ),
        (
            "rich_text",
            {
                let mut r = ReferenceSet::new();
                r.push_rich_text(format!("<img src=\"{reference}\">"));
                r
            },
            KeepReason::ReferencedInRichText,
        ),
        (
            "undo_history",
            {
                let mut r = ReferenceSet::new();
                r.push_session_history(&[DescriptionCommit {
                    task_id: "1".to_string(),
                    previous: Some(TaskComments::html(format!("<img src=\"{reference}\">"))),
                    next: TaskComments::html("<p>deleted</p>".to_string()),
                    editing_started_at_ms: 0,
                    committed_at_ms: 0,
                    coalesced_keystrokes: 1,
                    label: "Update description".to_string(),
                }]);
                r
            },
            KeepReason::ReferencedInUndoHistory,
        ),
        (
            "recovery_journal",
            {
                let mut r = ReferenceSet::new();
                r.push_recovery_journal(format!("{{\"backup\":\"{file_name}\"}}"));
                r
            },
            KeepReason::ReferencedInRecoveryJournal,
        ),
    ];

    for (label, references, expected) in cases {
        let report = store.run_gc(&references, &opts).expect("gc");
        assert!(report.deleted.is_empty(), "{label}: image was collected");
        assert_eq!(report.eligible.len(), 0, "{label}");
        assert_eq!(report.decisions.len(), 1);
        assert_eq!(report.decisions[0].keep_reason, Some(expected.clone()), "{label}");
        assert!(asset.abs_path.exists(), "{label}: file removed from disk");
    }

    // With every source empty the image becomes eligible.
    let report = store
        .run_gc(&ReferenceSet::new(), &opts)
        .expect("gc");
    assert_eq!(report.deleted, vec![file_name.clone()]);
    assert!(!asset.abs_path.exists());
    assert!(store.load_orphans().entries.is_empty());
    assert!(store.load_index().entries.is_empty());

    // The reference scanner over-approximates, so a bare file name anywhere in
    // any corpus is enough to keep the asset.
    let names = collect_image_file_names(&format!("日志提到 {file_name} 需要复查"));
    assert!(names.contains(&file_name));
}

/// QA-M7-015: GC is explicit, user-confirmed, honours dry-run and the grace
/// period, and there is no automatic deletion path.
#[test]
fn qa_m7_015_gc_is_explicit_dry_run_and_grace_period_gated() {
    let (_dir, store) = temp_store("gc");
    let asset = ingest_png(&store, "gc.png");
    let file_name = asset.file_name.clone();
    let references = ReferenceSet::new();

    // Unconfirmed: refused, nothing touched.
    let mut opts = GcOptions::default();
    opts.now_ms = u64::MAX;
    assert!(!opts.user_confirmed);
    assert!(opts.dry_run);
    assert_eq!(
        store.run_gc(&references, &opts),
        Err(AssetError::ConfirmationRequired)
    );
    assert!(asset.abs_path.exists());

    // An unmarked asset is never collected, even when confirmed.
    opts.user_confirmed = true;
    opts.require_orphan_mark = true;
    let report = store.run_gc(&references, &opts).expect("gc");
    assert!(report.deleted.is_empty());
    assert_eq!(
        report.decisions[0].keep_reason,
        Some(KeepReason::NotMarkedAsOrphan)
    );

    // Mark it, then stay inside the grace period.
    store.mark_orphan(&file_name, "removed", 10_000).expect("mark");
    opts.grace_period_ms = 7 * 24 * 3_600_000;
    opts.now_ms = 10_000 + 3_600_000; // one hour later
    let report = store.run_gc(&references, &opts).expect("gc");
    assert!(report.deleted.is_empty());
    assert_eq!(
        report.decisions[0].keep_reason,
        Some(KeepReason::WithinGracePeriod)
    );
    assert!(asset.abs_path.exists());

    // Dry run outside the grace period reports but does not delete.
    opts.now_ms = 10_000 + 8 * 24 * 3_600_000;
    opts.dry_run = true;
    let report = store.run_gc(&references, &opts).expect("dry run");
    assert!(report.dry_run);
    assert_eq!(report.eligible, vec![file_name.clone()]);
    assert!(report.deleted.is_empty());
    assert!(asset.abs_path.exists());

    // A real, confirmed run deletes and cleans up the metadata.
    opts.dry_run = false;
    let report = store.run_gc(&references, &opts).expect("gc");
    assert_eq!(report.deleted, vec![file_name.clone()]);
    assert!(!asset.abs_path.exists());
    assert!(store.load_orphans().entries.is_empty());
    assert!(store.load_index().entries.is_empty());
    assert!(store.list_files().is_empty());

    // Running GC again is a no-op, not an error.
    let report = store.run_gc(&references, &opts).expect("gc");
    assert!(report.deleted.is_empty());
    assert!(report.decisions.is_empty());
}

// ===========================================================================
// QA-M7-016 — no per-keystroke snapshot or serialization
// ===========================================================================

/// Builds a large but realistic rich-text description (~64 KB).
fn large_description(paragraphs: usize) -> String {
    let mut html = String::with_capacity(paragraphs * 96);
    for i in 0..paragraphs {
        html.push_str(&format!(
            "<p>段落 {i}: 这是一段较长的富文本内容，用来模拟大型任务的描述。<strong>重点</strong>与\
             <a href=\"https://example.com/{i}\">链接</a>。</p>"
        ));
    }
    html
}

/// QA-M7-016: editing a large rich-text description never snapshots or
/// serializes the document per keystroke.
#[test]
fn qa_m7_016_no_per_keystroke_snapshot_or_serialization() {
    const KEYSTROKES: u64 = 1_000;
    const AUTOSAVE_INTERVAL_MS: u64 = 30_000;

    let big = large_description(600);
    assert!(big.len() > 40_000, "fixture should be large: {}", big.len());

    let mut session = RichTextSession::new(
        VirtualClock::new(0),
        RichTextConfig::new(1_500, AUTOSAVE_INTERVAL_MS),
    );
    session.load_core_state("1", TaskComments::html(big.clone()));

    // Instrumentation: the application's serializer is invoked only when the
    // session's autosave gate says so.
    let mut serializer_calls: u64 = 0;
    let mut serialized_bytes: u64 = 0;
    let mut seen_serializations: u64 = 0;

    let started = std::time::Instant::now();
    for i in 0..KEYSTROKES {
        // One keystroke appends a character to a >40 KB document.
        let draft = format!("{big}<p>{i}</p>");
        session.keystroke("1", &draft);
        session.advance_ms(10); // 10s of typing in total, all inside the debounce
        session.tick();

        let gate = session.metrics().serializations;
        if gate > seen_serializations {
            serializer_calls += gate - seen_serializations;
            serialized_bytes += big.len() as u64;
            seen_serializations = gate;
        }

        // Per-keystroke invariants.
        assert_eq!(session.metrics().document_snapshots, 0);
        assert_eq!(session.metrics().serializations, 0);
        assert_eq!(session.metrics().commits, 0);
        assert_eq!(session.metrics().commands_pushed, 0);
    }
    let typing_elapsed = started.elapsed();

    let metrics = session.metrics();
    assert_eq!(metrics.keystrokes, KEYSTROKES);
    assert_eq!(metrics.suppressed_serializations, KEYSTROKES);
    assert_eq!(metrics.serializations, 0);
    assert_eq!(metrics.document_snapshots, 0);
    assert_eq!(serializer_calls, 0);
    assert_eq!(serialized_bytes, 0);
    assert_eq!(session.undo_depth(), 0, "no command per keystroke");

    // The whole burst coalesces into exactly one Core commit / one command.
    session.advance_ms(1_500);
    let commits = session.tick();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].coalesced_keystrokes, KEYSTROKES);
    assert_eq!(session.metrics().commits, 1);
    assert_eq!(session.metrics().commands_pushed, 1);
    assert_eq!(session.undo_depth(), 1);
    // One commit, still no serialization: the M3 interval has not elapsed.
    assert_eq!(session.metrics().serializations, 0);
    assert_eq!(session.metrics().autosaves_deferred, 1);

    // Crossing the M3 autosave interval serializes exactly once.
    session.keystroke("1", &format!("{big}<p>final</p>"));
    session.advance_ms(AUTOSAVE_INTERVAL_MS);
    session.tick();
    assert_eq!(session.metrics().commits, 2);
    assert_eq!(session.metrics().serializations, 1);
    assert_eq!(session.metrics().document_snapshots, 0);
    let gate = session.metrics().serializations;
    serializer_calls += gate - seen_serializations;
    serialized_bytes += big.len() as u64;

    // 1000 keystrokes produced 1 serialization, not 1000.
    assert_eq!(serializer_calls, 1);
    assert_eq!(serialized_bytes, big.len() as u64);
    assert!(
        serializer_calls * 100 < KEYSTROKES,
        "serialization is not bounded: {serializer_calls} for {KEYSTROKES} keystrokes"
    );

    // And the keystroke path itself stays cheap even on a large document.
    assert!(
        typing_elapsed.as_secs() < 20,
        "keystroke path is too slow on a large document: {typing_elapsed:?}"
    );

    // One description update == one UndoableCommand, at any document size.
    session.undo().expect("undo");
    assert_eq!(session.undo_depth(), 1);
    assert_eq!(session.redo_depth(), 1);
    assert_eq!(session.metrics().document_snapshots, 0);
}

/// QA-M7-016 (part 2): the debounce window matches RD-M7-026 (1-2s) and the
/// autosave interval is taken from the session, never shortened.
#[test]
fn qa_m7_016_debounce_and_autosave_intervals_match_the_spec() {
    assert_eq!(domain::rich_text::MIN_DEBOUNCE_MS, 1_000);
    assert_eq!(domain::rich_text::MAX_DEBOUNCE_MS, 2_000);
    let config = RichTextConfig::new(1, 1);
    assert!(config.debounce_ms >= 1_000 && config.debounce_ms <= 2_000);
    let config = RichTextConfig::new(60_000, 1);
    assert!(config.debounce_ms <= 2_000);

    // The M3 session interval is adopted verbatim and never reduced.
    let config = RichTextConfig::default().with_session_autosave_interval(120_000);
    assert_eq!(config.autosave_interval_ms, 120_000);
    let config = RichTextConfig::new(1_500, 5).with_session_autosave_interval(0);
    assert_eq!(config.autosave_interval_ms, 5);

    // Nothing commits before the window elapses, even under sustained typing.
    let mut session = RichTextSession::new(
        VirtualClock::new(0),
        RichTextConfig::new(2_000, 60_000),
    );
    session.load_core_state("1", TaskComments::html("<p>0</p>"));
    for i in 0..190 {
        session.keystroke("1", &format!("<p>{i}</p>"));
        session.advance_ms(10);
        assert!(session.tick().is_empty(), "committed too early at {i}");
    }
    assert_eq!(session.metrics().commits, 0);
    session.advance_ms(2_000); // 2000ms of quiet, exactly the debounce window
    assert_eq!(session.tick().len(), 1);
    assert_eq!(session.metrics().serializations, 0);
}

/// RD-M7-028 companion: search-text extraction across all comment types.
#[test]
fn qa_m7_006_search_text_extraction_matrix() {
    let plain = TaskComments::plain("第一行\n第二行 <raw> & more");
    assert_eq!(extract_plain_text(&plain), "第一行\n第二行 <raw> & more");

    let html = TaskComments::html(concat!(
        "<h1>标题</h1><p>正文 <strong>加粗</strong> 与 <em>斜体</em></p>",
        "<ul><li>项目一</li><li>项目二</li></ul>",
        "<p>line1<br>line2</p>",
        "<script>var leaked = 'secret';</script>",
        "<a href=\"javascript:alert(1)\">危险链接</a>"
    ));
    let text = extract_plain_text(&html);
    assert_eq!(
        text,
        "标题\n正文 加粗 与 斜体\n项目一\n项目二\nline1\nline2\n危险链接"
    );
    assert!(!text.contains("secret"));
    assert!(!text.contains("javascript"));
    assert!(!text.contains('<'));
    assert!(text.contains("标题"));
    assert!(text.contains("项目一"));

    // CJK is contiguous: no spaces inserted between Chinese characters.
    let cjk = TaskComments::html("<p>这是一个<strong>中文</strong>测试</p><p>第二段</p>");
    let text = extract_plain_text(&cjk);
    assert_eq!(text, "这是一个中文测试\n第二段");
    assert!(!text.contains("中 文"));
    assert_eq!(html_to_plain_text("<p>任务</p>"), "任务");

    // Opaque types contribute nothing to the index but explain themselves.
    let rtf = TaskComments {
        comments_type: CommentsType::Rtf,
        content: "{\\rtf1 内容}".to_string(),
    };
    assert_eq!(extract_plain_text(&rtf), "");
    assert_eq!(extract_search_text(None), "");
    assert!(opaque_placeholder(&rtf).is_some());
    let markdown = TaskComments {
        comments_type: CommentsType::Unknown("MARKDOWN".to_string()),
        content: "# 标题".to_string(),
    };
    assert_eq!(extract_plain_text(&markdown), "");
    assert!(opaque_placeholder(&markdown).unwrap().contains("MARKDOWN"));
    assert_eq!(opaque_placeholder(&plain), None);
}

// BEGIN-LIB-ONLY-SECTION
//
// Everything below uses the real library crate (the lossless M2 XML core) and
// therefore only compiles once `domain::sanitizer` / `comments` / `rich_text`
// are declared in `src-tauri/src/domain/mod.rs`. It is stripped from the
// isolated scratch build.

use moderntodolist_lib::domain::{parse_xml, serialize_xml, XmlElement, XmlNode};

fn fixture_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate root has a parent")
        .join("tests")
        .join("fixtures")
        .join("xml")
        .join("richtext")
}

fn load_fixture(name: &str) -> moderntodolist_lib::domain::XmlDocument {
    let path = fixture_root().join(name);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    parse_xml(&bytes).unwrap_or_else(|e| panic!("{name}: parse failed: {e}"))
}

fn tasks(doc: &moderntodolist_lib::domain::XmlDocument) -> Vec<&XmlElement> {
    doc.root.children_by_tag("TASK").collect()
}

/// CDATA-aware COMMENTS extraction.
///
/// `XmlElement::text_content()` only concatenates `XmlNode::Text` children, so
/// a TDL HTML comment stored as `<![CDATA[...]]>` reads back empty. This helper
/// is what the M7 read path needs; see the report for the one-line
/// `mappers::read_task` fix.
fn comments_payload(element: &XmlElement) -> String {
    let mut out = String::new();
    for child in &element.children {
        match child {
            XmlNode::Text(text) => out.push_str(text),
            XmlNode::CData(text) => out.push_str(text),
            _ => {}
        }
    }
    out
}

fn comments_of(doc: &moderntodolist_lib::domain::XmlDocument, index: usize) -> (String, String) {
    let task = tasks(doc).get(index).expect("task").clone();
    let comment_type = task.get_attr("COMMENTSTYPE").unwrap_or("").to_string();
    let payload = task
        .first_child_by_tag("COMMENTS")
        .map(comments_payload)
        .unwrap_or_default();
    (comment_type, payload)
}

/// QA-M7-001 (XML level): a PLAIN_TEXT comment survives parse → serialize
/// byte-stably, and the extracted text is index-ready.
#[test]
fn qa_m7_001_xml_plain_text_comment_roundtrip() {
    let doc = load_fixture("plain-text-cjk.xml");
    let (comment_type, payload) = comments_of(&doc, 0);
    assert_eq!(comment_type, "PLAIN_TEXT");
    assert!(payload.contains("第一行中文内容"));
    assert!(payload.contains("<不是标记>"));

    let serialized = serialize_xml(&doc);
    let reparsed = parse_xml(&serialized).expect("re-parse");
    let (comment_type2, payload2) = comments_of(&reparsed, 0);
    assert_eq!(comment_type2, "PLAIN_TEXT");
    assert_eq!(payload2, payload);

    // Domain-level view of the same payload.
    let comments = TaskComments::from_attr(Some(&comment_type2), payload2.clone());
    let mut field = CommentsField::new(Some(comments.clone()));
    field.save(&payload2, CommentsType::Html).expect("save");
    assert_eq!(field.value().unwrap().attr_value(), "PLAIN_TEXT");
    assert_eq!(field.value().unwrap().content, payload2);
    assert_eq!(extract_plain_text(field.value().unwrap()), payload2);

    // The task without comments must not gain a COMMENTSTYPE-driven payload.
    let task2 = tasks(&reparsed).get(1).expect("task 2").clone();
    assert_eq!(task2.get_attr("COMMENTSTYPE"), None);
    assert!(task2.first_child_by_tag("COMMENTS").is_none());
    let mut empty = CommentsField::new(None);
    for _ in 0..10 {
        let projection = empty.view();
        assert!(projection.must_not_materialize);
    }
    assert_eq!(empty.value(), None);
}

/// QA-M7-002 (XML level): a CDATA-wrapped HTML comment is readable and its
/// sanitized form is what the editor and preview receive.
#[test]
fn qa_m7_002_xml_html_cdata_payload_extraction() {
    let doc = load_fixture("html-legacy-cdata.xml");
    let (comment_type, payload) = comments_of(&doc, 0);
    assert_eq!(comment_type, "HTML");
    assert!(payload.starts_with("<html><body>"));
    assert!(payload.contains("<b>legacy</b>"));
    assert!(payload.contains("中文"));

    // The CDATA wrapper survives serialization.
    let serialized = serialize_xml(&doc);
    assert!(serialized.windows(9).any(|w| w == b"<![CDATA["));
    let reparsed = parse_xml(&serialized).expect("re-parse");
    assert_eq!(comments_of(&reparsed, 0), (comment_type.clone(), payload.clone()));

    // Feeding the payload through the M7 read path.
    let comments = TaskComments::from_attr(Some(&comment_type), payload);
    let mut field = CommentsField::new(Some(comments));
    let projection = field.view();
    assert_eq!(projection.mode, EditorMode::RichText);
    assert!(projection.editable);
    // Legacy spellings are folded, the wrapper is dropped, CJK is intact.
    assert!(projection.editor_payload.contains("<strong>legacy</strong>"));
    assert!(projection.editor_payload.contains("<em>formatting</em>"));
    assert!(projection.editor_payload.contains("中文"));
    assert!(!projection.editor_payload.contains("<html>"));
    assert!(!projection.editor_payload.contains("<body>"));
    assert_eq!(
        projection.display_text,
        "This is a legacy comment with formatting and 中文.\nItem 1\nItem 2\nLink: Example"
    );

    // Saving keeps COMMENTSTYPE=HTML.
    field
        .save(&projection.editor_payload, CommentsType::PlainText)
        .expect("save");
    assert_eq!(field.value().unwrap().attr_value(), "HTML");
}

/// QA-M7-004 (XML level): RTF, MARKDOWN and lower-case `html` COMMENTSTYPE
/// values survive the round trip byte-for-byte, together with unknown
/// attributes, unknown elements and XML comments.
#[test]
fn qa_m7_004_xml_opaque_commentstypes_are_preserved() {
    let doc = load_fixture("opaque-commentstypes.xml");
    let before: Vec<String> = tasks(&doc)
        .iter()
        .map(|t| t.get_attr("COMMENTSTYPE").unwrap_or("").to_string())
        .collect();
    assert_eq!(before, vec!["RTF", "MARKDOWN", "html"]);

    let serialized = serialize_xml(&doc);
    let serialized_text = String::from_utf8_lossy(&serialized).to_string();
    assert!(serialized_text.contains("COMMENTSTYPE=\"RTF\""));
    assert!(serialized_text.contains("COMMENTSTYPE=\"MARKDOWN\""));
    assert!(serialized_text.contains("COMMENTSTYPE=\"html\""));
    // Unknown attribute, unknown element and XML comment all survive.
    assert!(serialized_text.contains("M7UNKNOWNATTR=\"keep-me\""));
    assert!(serialized_text.contains("<UNKNOWN_ELEMENT"));
    assert!(serialized_text.contains("must be preserved byte-for-byte"));

    let reparsed = parse_xml(&serialized).expect("re-parse");
    let after: Vec<String> = tasks(&reparsed)
        .iter()
        .map(|t| t.get_attr("COMMENTSTYPE").unwrap_or("").to_string())
        .collect();
    assert_eq!(after, before);

    // Each opaque payload is read-only at the domain level.
    for (index, expected) in [(0, "RTF"), (1, "MARKDOWN"), (2, "html")] {
        let (raw_type, payload) = comments_of(&reparsed, index);
        assert_eq!(raw_type, expected);
        let comments = TaskComments::from_attr(Some(&raw_type), payload.clone());
        assert_eq!(comments.attr_value(), expected);
        let mut field = CommentsField::new(Some(comments.clone()));
        assert_eq!(field.view().mode, EditorMode::ReadOnly);
        assert!(matches!(
            field.save("tampered", CommentsType::Html),
            Err(CommentsError::ReadOnly(_))
        ));
        assert!(matches!(
            field.convert_to_rich_text(true),
            Err(CommentsError::ReadOnly(_))
        ));
        assert_eq!(field.value(), Some(&comments));
        assert_eq!(field.value().unwrap().content, payload);
        assert_eq!(extract_plain_text(&comments), "");
    }

    // The RTF payload bytes are untouched, backslashes and all.
    let (_, rtf_payload) = comments_of(&reparsed, 0);
    assert!(rtf_payload.starts_with("{\\rtf1\\ansi\\deff0"));
    assert!(rtf_payload.contains("\\par}"));
}

/// QA-M7-007 (XML level): a hostile HTML comment stored in a real document is
/// neutralized on every read path, and the surviving bytes are stable.
#[test]
fn qa_m7_007_xml_hostile_comment_is_neutralized_before_render() {
    let doc = load_fixture("html-hostile.xml");
    let (comment_type, payload) = comments_of(&doc, 0);
    assert_eq!(comment_type, "HTML");
    assert!(payload.contains("<script>"));

    // Core → Editor feed.
    let for_editor = sanitize_for_editor(&payload).expect("ok").html;
    // Preview render.
    let for_preview = sanitize_for_preview(&payload).expect("ok").html;
    for cleaned in [&for_editor, &for_preview] {
        let lowered = cleaned.to_ascii_lowercase();
        assert!(!lowered.contains("<script"));
        assert!(!lowered.contains("onerror"));
        assert!(!lowered.contains("onclick"));
        assert!(!lowered.contains("<iframe"));
        assert!(!lowered.contains("<svg"));
        assert!(!lowered.contains("<object"));
        assert!(!lowered.contains("javascript:"));
        assert!(!lowered.contains("behavior"));
        // No live element other than the whitelist survived: re-sanitizing the
        // output finds nothing left to remove.
        let recheck = sanitize_for_core(cleaned).expect("ok");
        assert_eq!(&recheck.html, cleaned);
        assert_eq!(recheck.threat_count(), 0);
        // Benign text survives, including everything after the tokenizer probe.
        assert!(cleaned.contains("Benign paragraph"));
        assert!(cleaned.contains("click"));
        assert!(cleaned.contains("entity"));
        assert!(cleaned.contains("styled"));
    }
    assert_eq!(for_editor, for_preview);
    // Stable under repeated passes.
    assert_eq!(clean(&for_editor), for_editor);
    // Search text carries no live markup.
    let comments = TaskComments::from_attr(Some(&comment_type), payload);
    let text = extract_plain_text(&comments);
    assert!(!text.contains("<script"));
    assert!(!text.contains("<iframe"));
    assert!(text.contains("Benign paragraph"));
    assert!(text.contains("click"));
    assert!(text.contains("styled"));
}

/// QA-M7-011 (XML level): managed image references round-trip through the XML
/// core unchanged, and non-managed references are dropped on read.
#[test]
fn qa_m7_011_xml_managed_image_reference_survives_roundtrip() {
    let doc = load_fixture("html-managed-image.xml");
    let (comment_type, payload) = comments_of(&doc, 0);
    assert_eq!(comment_type, "HTML");
    let reference = "../.assets/doc-m7-images/images/3f2b9c1e-7a44-4d2b-9f1a-6c8e0d2b4a71.png";
    assert!(payload.contains(reference));

    let serialized = serialize_xml(&doc);
    let serialized_text = String::from_utf8_lossy(&serialized).to_string();
    assert!(serialized_text.contains(reference));
    // The XML round trip is byte-preserving (M2 invariant): reading a document
    // never rewrites stored content, so the legacy base64 blob in this fixture
    // is still there on disk. What matters is that it can never reach the
    // editor, the preview or a new commit — asserted below.
    assert!(serialized_text.contains("iVBORw0KGgo"));
    let reparsed = parse_xml(&serialized).expect("re-parse");
    assert_eq!(comments_of(&reparsed, 0), (comment_type.clone(), payload.clone()));

    // The managed reference survives sanitization; remote and base64 do not.
    let mut options = domain::sanitizer::SanitizeOptions::for_context(
        domain::sanitizer::SanitizeContext::CoreToEditor,
    );
    options.managed_document_id = Some("doc-m7-images".to_string());
    let cleaned = domain::sanitizer::sanitize_with(&payload, &options)
        .expect("ok")
        .html;
    assert!(cleaned.contains(reference));
    assert!(cleaned.contains("alt=\"截图\""));
    assert!(!cleaned.contains("evil.example"));
    assert!(!cleaned.contains("data:image"));
    assert!(!cleaned.contains("iVBORw0KGgo"), "base64 reached the editor");
    assert_eq!(cleaned.matches("<img").count(), 1);

    // The reference set used by GC picks the managed file name out of the XML.
    let mut references = ReferenceSet::new();
    references.push_xml(serialized_text.clone());
    let names = references.referenced_file_names();
    assert!(names.contains("3f2b9c1e-7a44-4d2b-9f1a-6c8e0d2b4a71.png"));

    // A traversing / absolute / protocol-relative reference is refused.
    let (_, hostile) = comments_of(&reparsed, 1);
    let cleaned = clean(&hostile);
    assert!(!cleaned.contains("<img"));
    assert!(!cleaned.contains("win.ini"));
    assert!(!cleaned.contains("boot.png"));
    assert!(!cleaned.contains("evil.example"));
    assert!(sanitize_image_src(reference, Some("doc-m7-images")).is_ok());
    assert!(sanitize_image_src(reference, Some("other-doc")).is_err());
}

// ===========================================================================
// Cases that CANNOT be covered from Rust (UI-only)
// ===========================================================================
//
// * QA-M7-003 (partial): the RTF/unknown **type indicator widget** rendering,
//   its tooltip text and the disabled state of the toolbar buttons are Vue
//   concerns. The backend invariants (read-only, no convert, indicator string)
//   are covered above.
// * QA-M7-005 (partial): the **confirmation dialog** itself (focus order,
//   default button, Esc handling) is UI. The backend gate
//   (`ConfirmationRequired`) and Undo reversibility are covered above.
// * QA-M7-006 (partial): that the Tiptap editor does not *itself* rewrite the
//   document on mount (ProseMirror normalization) can only be asserted in the
//   WebView. The Rust side guarantees viewing never mutates Core.
// * QA-M7-009 (partial): the actual clipboard read (`ClipboardEvent`) and the
//   file drag-drop payload extraction happen in the WebView; only the
//   post-extraction sanitize/ingest path is covered here.
// * QA-M7-012 (partial): the OS clipboard image capture and the drag-drop
//   cursor/insert-position behaviour are platform UI concerns.
// * QA-M7-015 (partial): the maintenance dialog that surfaces the GC action
//   and its progress reporting is UI.
// * QA-M7-016 (partial): WebView-side editor latency, ProseMirror transaction
//   cost and Tiptap's local history depth need the Performance API in the
//   renderer. The Core-side counters (no per-keystroke commit, no per-keystroke
//   serialization, no document snapshot) are covered above.
// * Toolbar state, keyboard shortcuts, font-size picker and colour picker
//   (RD-M7-003~010, INH-1062) are entirely frontend and are not covered here.
