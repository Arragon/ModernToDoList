//! M10 RC Regression Matrix A — XML Compatibility (INH-1124, spec section 5.5).
//!
//! 13 cases (A01..A13) covering every encoding fixture (utf8 with/without BOM,
//! utf16le, utf16be), unknown element/attribute/metadata preservation,
//! comment + CDATA preservation, DEPENDENCY elements, FileLink elements
//! (single + multi), malformed/truncated input rejection with a clear error,
//! and the real-world london-underground fixture.
//!
//! All cases exercise the real public API of `moderntodolist_lib`
//! (parse_xml / serialize_xml / detect_bom / mappers / validator).
//! New fixtures used here live under `tests/fixtures/xml/rc/` and were built
//! so that BOM and XML declaration are mutually consistent (unlike some
//! legacy encoding fixtures whose declaration says "utf-8" while the bytes
//! are UTF-16).

use moderntodolist_lib::domain::encoding::XmlEncodingMeta;
use moderntodolist_lib::domain::mappers::{read_document_metadata, read_task};
use moderntodolist_lib::domain::task::{CommentType, TaskTree};
use moderntodolist_lib::domain::validator::{validate_document, validate_task_tree};
use moderntodolist_lib::domain::xml_parser::XmlParseError;
use moderntodolist_lib::domain::{
    detect_bom, parse_xml, serialize_xml, XmlEncoding, XmlDocument, XmlNode,
};

use std::fs;
use std::path::{Path, PathBuf};

/// Root of the fixture directory: <repo>/tests/fixtures/xml
fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate root should have parent")
        .join("tests")
        .join("fixtures")
        .join("xml")
}

fn read_fixture(rel: &str) -> Vec<u8> {
    let path = fixtures_root().join(rel);
    fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// Parse -> serialize -> re-parse and assert structural equivalence of the
/// root's direct element children (tags, attribute names/values, order).
fn roundtrip_assert_structure(bytes: &[u8], label: &str) -> (XmlDocument, XmlDocument, Vec<u8>) {
    let doc1 = parse_xml(bytes).unwrap_or_else(|e| panic!("{label}: parse failed: {e}"));
    let out = serialize_xml(&doc1);
    assert!(!out.is_empty(), "{label}: serialize produced empty output");
    let doc2 = parse_xml(&out).unwrap_or_else(|e| panic!("{label}: re-parse failed: {e}"));

    assert_eq!(doc1.root.tag, doc2.root.tag, "{label}: root tag");
    assert_eq!(doc1.root.attrs, doc2.root.attrs, "{label}: root attrs (name/value/order)");

    fn walk(a: &moderntodolist_lib::domain::XmlElement, b: &moderntodolist_lib::domain::XmlElement, label: &str, path: &str) {
        assert_eq!(a.tag, b.tag, "{label}: tag at {path}");
        assert_eq!(a.attrs, b.attrs, "{label}: attrs at {path} <{}>", a.tag);
        let ea: Vec<_> = a.children.iter().filter_map(|n| match n { XmlNode::Element(e) => Some(e), _ => None }).collect();
        let eb: Vec<_> = b.children.iter().filter_map(|n| match n { XmlNode::Element(e) => Some(e), _ => None }).collect();
        assert_eq!(ea.len(), eb.len(), "{label}: element child count at {path} <{}>", a.tag);
        for (i, (x, y)) in ea.iter().zip(eb.iter()).enumerate() {
            walk(x, y, label, &format!("{path}/{i}:{}", x.tag));
        }
    }
    walk(&doc1.root, &doc2.root, label, "root");
    (doc1, doc2, out)
}

/// Collect every TASK element in document order (recursive).
fn collect_task_elements(doc: &XmlDocument) -> Vec<moderntodolist_lib::domain::XmlElement> {
    fn rec(el: &moderntodolist_lib::domain::XmlElement, out: &mut Vec<moderntodolist_lib::domain::XmlElement>) {
        if el.tag == "TASK" {
            out.push(el.clone());
        }
        for child in el.child_elements() {
            rec(child, out);
        }
    }
    let mut out = Vec::new();
    rec(&doc.root, &mut out);
    out
}

/// Build a TaskTree the same way the session layer does (public mappers only).
fn build_task_tree(doc: &XmlDocument) -> TaskTree {
    let mut tree = TaskTree::new();
    for el in collect_task_elements(doc) {
        let task = read_task(&el);
        let id = task.id.clone();
        tree.add_task(task);
        // Root-level tasks: direct TASK children of TODOLIST.
        if doc.root.tag == "TODOLIST" {
            let is_root = doc.root.child_elements().any(|c| c.tag == "TASK" && c.get_attr("ID") == Some(id.as_str()));
            if is_root {
                tree.add_root_id(id);
            }
        }
    }
    tree
}

// ─── A01: UTF-8 with BOM ─────────────────────────────────────────────────────

/// QA-M10-A01: UTF-8 BOM round-trip.
///
/// Part 1 uses the legacy `encoding/utf8-bom.xml` fixture whose declaration
/// says `encoding="utf-8"`; the parser lets the declaration override BOM
/// detection, so the re-serialized bytes lose the BOM. This matches the
/// documented M2 gate behavior (see round_trip_test.rs notes) and is
/// reported as compatibility finding F1 (BOM dropped on re-save).
/// Part 2 uses the new `rc/utf8-bom-preserved.xml` (BOM, no declaration) and
/// asserts the BOM IS preserved byte-for-byte, proving the serializer's
/// BOM path itself is correct.
#[test]
fn qa_m10_a01_utf8_bom_roundtrip() {
    // Part 1: legacy fixture — BOM detected, structure survives, declaration override documented.
    let bytes = read_fixture("encoding/utf8-bom.xml");
    let bom = detect_bom(&bytes);
    assert_eq!(bom.encoding, XmlEncoding::Utf8Bom);
    assert_eq!(bom.bom_len, 3);

    let (doc, _doc2, out) = roundtrip_assert_structure(&bytes, "utf8-bom.xml");
    assert_eq!(doc.meta.encoding, XmlEncoding::Utf8,
        "declaration encoding=\"utf-8\" overrides BOM detection (documented parser behavior)");
    assert!(!out.starts_with(&[0xEF, 0xBB, 0xBF]),
        "with the declaration override the serialized bytes lose the BOM (finding F1)");

    // Part 2: RC fixture with consistent BOM and no declaration — BOM preserved byte-exactly.
    let bytes2 = read_fixture("rc/utf8-bom-preserved.xml");
    let bom2 = detect_bom(&bytes2);
    assert_eq!(bom2.encoding, XmlEncoding::Utf8Bom);
    let doc = parse_xml(&bytes2).unwrap();
    assert_eq!(doc.meta.encoding, XmlEncoding::Utf8Bom);
    let out2 = serialize_xml(&doc);
    assert!(out2.starts_with(&[0xEF, 0xBB, 0xBF]), "UTF-8 BOM must be re-emitted");
    assert_eq!(out2, bytes2, "byte-level round-trip must be lossless for the RC BOM fixture");
    // Content check: Chinese title survives.
    let task = doc.root.first_child_by_tag("TASK").unwrap();
    assert_eq!(read_task(task).title, "BOM task 中文标题");
    assert!(validate_document(&doc).is_empty());
}

// ─── A02: UTF-8 without BOM ──────────────────────────────────────────────────

/// QA-M10-A02: UTF-8 (no BOM) round-trip with numeric character references.
#[test]
fn qa_m10_a02_utf8_no_bom_roundtrip() {
    let bytes = read_fixture("encoding/utf8-no-bom.xml");
    let bom = detect_bom(&bytes);
    assert_eq!(bom.encoding, XmlEncoding::Utf8);
    assert_eq!(bom.bom_len, 0);

    let (doc, _doc2, out) = roundtrip_assert_structure(&bytes, "utf8-no-bom.xml");
    assert_eq!(doc.meta.encoding, XmlEncoding::Utf8);
    assert!(!out.starts_with(&[0xEF, 0xBB, 0xBF]), "must not invent a BOM");
    assert!(detect_bom(&out).encoding == XmlEncoding::Utf8);

    // Numeric character references (&#x2014; &#x4E2D;...) decode to real characters.
    let tasks = collect_task_elements(&doc);
    assert!(!tasks.is_empty());
    let t = read_task(&tasks[0]);
    assert!(t.title.contains('\u{2014}'), "em dash entity should decode: {:?}", t.title);
    assert!(t.title.contains('中'), "CJK entity should decode: {:?}", t.title);
    assert!(validate_document(&doc).is_empty());
}

// ─── A03: UTF-16 LE ──────────────────────────────────────────────────────────

/// QA-M10-A03: UTF-16 LE round-trip.
///
/// Part 1: legacy `encoding/utf16le.xml` — BOM (FF FE) detected as Utf16Le,
/// bytes decode correctly even though the declaration says utf-8 (the
/// declaration only steers the *output* encoding; documented M2 behavior,
/// reported as finding F2).
/// Part 2: new `rc/utf16le-consistent.xml` (FF FE BOM + declaration
/// encoding="utf-16") round-trips BYTE-IDENTICALLY as UTF-16 LE, proving the
/// full UTF-16 encode/decode pipeline is lossless.
#[test]
fn qa_m10_a03_utf16le_roundtrip() {
    let bytes = read_fixture("encoding/utf16le.xml");
    let bom = detect_bom(&bytes);
    assert_eq!(bom.encoding, XmlEncoding::Utf16Le);
    assert_eq!(bom.bom_len, 2);

    let (doc, _doc2, _out) = roundtrip_assert_structure(&bytes, "utf16le.xml");
    // Declaration says utf-8 -> parser trusts declaration for the output meta.
    assert_eq!(doc.meta.encoding, XmlEncoding::Utf8, "documented declaration-override behavior (finding F2)");

    // RC fixture: consistent UTF-16LE file.
    let rc = read_fixture("rc/utf16le-consistent.xml");
    assert_eq!(detect_bom(&rc).encoding, XmlEncoding::Utf16Le);
    let doc_rc = parse_xml(&rc).unwrap();
    assert_eq!(doc_rc.meta.encoding, XmlEncoding::Utf16Le);

    let out = serialize_xml(&doc_rc);
    assert!(out.starts_with(&[0xFF, 0xFE]), "UTF-16 LE BOM must be re-emitted");
    assert_eq!((out.len() - 2) % 2, 0, "body must be whole UTF-16 code units");
    assert_eq!(out, rc, "byte-level round-trip must be lossless for consistent UTF-16LE");

    // Content survives the UTF-16 decode: CJK titles and the nested task
    // (the fixture holds TASK 1 with nested TASK 3, plus TASK 2 => 3 total).
    let tasks = collect_task_elements(&doc_rc);
    assert_eq!(tasks.len(), 3);
    let t1 = read_task(&tasks[0]);
    assert_eq!(t1.title, "任务一 中文标题");
    let nested = read_task(&tasks[1]);
    assert_eq!(nested.title, "嵌套任务");
    assert_eq!(nested.percent_done, 25);
    assert_eq!(read_task(&tasks[2]).title, "Second task plain ascii");
    let comments = t1.comments.unwrap();
    assert_eq!(comments.content, "多行备注\r\n第二行内容");
    assert!(validate_document(&doc_rc).is_empty());
}

// ─── A04: UTF-16 BE ──────────────────────────────────────────────────────────

/// QA-M10-A04: UTF-16 BE round-trip. Same two-part strategy as A03:
/// legacy fixture (BOM FE FF detected, declaration override documented) and
/// the consistent `rc/utf16be-consistent.xml` which must round-trip
/// byte-identically as UTF-16 BE.
#[test]
fn qa_m10_a04_utf16be_roundtrip() {
    let bytes = read_fixture("encoding/utf16be.xml");
    let bom = detect_bom(&bytes);
    assert_eq!(bom.encoding, XmlEncoding::Utf16Be);
    assert_eq!(bom.bom_len, 2);
    let (doc, _doc2, _out) = roundtrip_assert_structure(&bytes, "utf16be.xml");
    assert_eq!(doc.meta.encoding, XmlEncoding::Utf8, "documented declaration-override behavior (finding F2)");

    let rc = read_fixture("rc/utf16be-consistent.xml");
    assert_eq!(detect_bom(&rc).encoding, XmlEncoding::Utf16Be);
    let doc_rc = parse_xml(&rc).unwrap();
    assert_eq!(doc_rc.meta.encoding, XmlEncoding::Utf16Be);

    let out = serialize_xml(&doc_rc);
    assert!(out.starts_with(&[0xFE, 0xFF]), "UTF-16 BE BOM must be re-emitted");
    assert_eq!(out, rc, "byte-level round-trip must be lossless for consistent UTF-16BE");

    let tasks = collect_task_elements(&doc_rc);
    assert_eq!(tasks.len(), 2);
    assert_eq!(read_task(&tasks[0]).title, "大端任务 中文标题");
    assert!(validate_document(&doc_rc).is_empty());
}

// ─── A05: Unknown element preservation ───────────────────────────────────────

/// QA-M10-A05: Unknown child elements (CUSTOMDATA, PLUGINDATA with nested
/// SETTING) survive parse -> serialize -> re-parse, and are captured by
/// `read_task` into `unknown_children` for the domain layer.
#[test]
fn qa_m10_a05_unknown_element_preservation() {
    let bytes = read_fixture("unknown/unknown-element.xml");
    let (doc, doc2, out) = roundtrip_assert_structure(&bytes, "unknown-element.xml");

    let text = decode_for_assertions(&doc.meta, &out);
    assert!(text.contains("<CUSTOMDATA>"), "CUSTOMDATA element must survive: {text}");
    assert!(text.contains("<FIELD1>Custom value 1</FIELD1>"));
    assert!(text.contains("<FIELD2>Custom value 2</FIELD2>"));
    assert!(text.contains(r#"<PLUGINDATA PluginID="com.example.plugin">"#));
    assert!(text.contains(r#"<SETTING Name="option1" Value="b"#) || text.contains(r#"<SETTING Name="option1" Value="true"/>"#),
        "nested SETTING element must survive");

    // Domain mapping: unknown children captured as raw XML strings.
    let tasks = collect_task_elements(&doc2);
    let t = read_task(&tasks[0]);
    assert!(t.unknown_children.iter().any(|c| c.contains("CUSTOMDATA") && c.contains("Custom value 1")),
        "unknown_children must capture CUSTOMDATA: {:?}", t.unknown_children);
    assert!(t.unknown_children.iter().any(|c| c.contains("PLUGINDATA")),
        "unknown_children must capture PLUGINDATA");
}

/// Decode serialized bytes to a string according to the document encoding
/// (test helper for content assertions).
fn decode_for_assertions(meta: &XmlEncodingMeta, bytes: &[u8]) -> String {
    match meta.encoding {
        XmlEncoding::Utf16Le | XmlEncoding::Utf16Be => {
            let bom = detect_bom(bytes);
            moderntodolist_lib::domain::encoding::decode_utf16_to_utf8(&bytes[bom.bom_len..], meta.encoding)
                .expect("valid utf16")
        }
        _ => {
            let bom = detect_bom(bytes);
            String::from_utf8(bytes[bom.bom_len..].to_vec()).expect("valid utf8")
        }
    }
}

// ─── A06: Unknown attribute preservation ─────────────────────────────────────

/// QA-M10-A06: Unknown attributes (CUSTOM_ATTR, CUSTOM_PLUGIN_DATA) survive
/// the round-trip in their original positions, and `read_task` captures them
/// in `unknown_attrs`.
#[test]
fn qa_m10_a06_unknown_attribute_preservation() {
    let bytes = read_fixture("unknown/unknown-attribute.xml");
    let (doc, doc2, _out) = roundtrip_assert_structure(&bytes, "unknown-attribute.xml");

    let t1 = doc.root.first_child_by_tag("TASK").unwrap();
    let t2 = doc2.root.first_child_by_tag("TASK").unwrap();
    assert_eq!(t1.get_attr("CUSTOM_ATTR"), Some("test"));
    assert_eq!(t2.get_attr("CUSTOM_ATTR"), Some("test"), "CUSTOM_ATTR must survive round-trip");
    assert_eq!(t2.get_attr("CUSTOM_PLUGIN_DATA"), Some("plugin-value-123"));

    // Positional fidelity: attribute order is preserved exactly (byte-level compat).
    let names1: Vec<&str> = t1.attrs.iter().map(|a| a.name.as_str()).collect();
    let names2: Vec<&str> = t2.attrs.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names1, names2, "attribute order must be preserved");
    let pos_custom = names2.iter().position(|n| *n == "CUSTOM_ATTR").unwrap();
    let pos_pd = names2.iter().position(|n| *n == "PERCENTDONE").unwrap();
    assert_eq!(pos_custom, pos_pd + 1, "CUSTOM_ATTR sits right after PERCENTDONE as in the source");

    // Domain mapping captures unknown attrs.
    let task = read_task(t2);
    assert!(task.unknown_attrs.contains(&("CUSTOM_ATTR".to_string(), "test".to_string())));
    assert!(task.unknown_attrs.contains(&("CUSTOM_PLUGIN_DATA".to_string(), "plugin-value-123".to_string())));
    // Known attrs must NOT leak into unknown_attrs.
    assert!(!task.unknown_attrs.iter().any(|(k, _)| k == "TITLE" || k == "PRIORITY"));
}

// ─── A07: Unknown METADATA element (root + task level) ──────────────────────

/// QA-M10-A07: GUID-keyed METADATA elements at root level and task level
/// survive the round-trip; `read_document_metadata` maps root METADATA into
/// `root_metadata`, `read_task` maps task METADATA into `task.metadata`.
#[test]
fn qa_m10_a07_unknown_metadata_element() {
    const GUID: &str = "FA40B83E-E934-D494-8FB3-8EC9748FA4E8";
    let bytes = read_fixture("unknown/metadata-element.xml");
    let (doc, doc2, out) = roundtrip_assert_structure(&bytes, "metadata-element.xml");

    // Root-level METADATA preserved with GUID attr and Windows-path value.
    let root_meta = doc2.root.first_child_by_tag("METADATA").expect("root METADATA must survive");
    let val = root_meta.get_attr(GUID).expect("GUID attr must survive");
    assert!(val.starts_with(r"D:\_code\ToDoList_Dev"), "path value preserved: {val}");
    let text = decode_for_assertions(&doc.meta, &out);
    assert!(text.contains(GUID), "GUID must appear in serialized bytes");

    // Domain mapping: document metadata.
    let dm = read_document_metadata(&doc2.root, doc2.meta.clone());
    assert_eq!(dm.root_metadata.len(), 1);
    assert!(dm.root_metadata[0].attrs.iter().any(|(k, v)| k == GUID && v.starts_with(r"D:\_code")));

    // Task-level METADATA preserved.
    let tasks = collect_task_elements(&doc2);
    let t = read_task(&tasks[0]);
    assert_eq!(t.metadata.len(), 1);
    assert!(t.metadata[0].attrs.iter().any(|(k, _)| k == GUID));
    assert!(validate_document(&doc2).is_empty());
}

// ─── A08: Comment (and CDATA) preservation ───────────────────────────────────

/// QA-M10-A08: XML comments (`<!-- -->`) at root level, task level and after
/// the last task survive byte-identically, CDATA sections survive verbatim,
/// and COMMENTSTYPE (HTML vs PLAIN_TEXT) is never silently converted.
#[test]
fn qa_m10_a08_comment_and_cdata_preservation() {
    // Part 1: RC fixture — byte-identical round-trip of comments + CDATA + unknown plugin element.
    let rc = read_fixture("rc/xml-comments.xml");
    let doc = parse_xml(&rc).unwrap();
    let out = serialize_xml(&doc);
    assert_eq!(out, rc, "comments/CDATA fixture must round-trip byte-identically");

    let count_comments = |d: &XmlDocument| -> usize {
        let mut n = 0;
        fn rec(el: &moderntodolist_lib::domain::XmlElement, n: &mut usize) {
            for c in &el.children {
                match c {
                    XmlNode::Comment(_) => *n += 1,
                    XmlNode::Element(e) => rec(e, n),
                    _ => {}
                }
            }
        }
        rec(&d.root, &mut n);
        n
    };
    assert_eq!(count_comments(&doc), 3, "root, task-level and trailing comments");

    let task_el = doc.root.first_child_by_tag("TASK").unwrap();
    let comments_el = task_el.first_child_by_tag("COMMENTS").unwrap();
    let cdata = comments_el.children.iter().find_map(|n| match n { XmlNode::CData(c) => Some(c.clone()), _ => None });
    assert_eq!(
        cdata.as_deref(),
        Some("<html><body><p>Rich <b>text</b> & symbols</p></body></html>"),
        "CDATA content must be preserved verbatim"
    );
    let t = read_task(task_el);
    assert_eq!(t.comments_type, CommentType::Html);
    assert!(!t.comments_type.is_editable(), "HTML comments are not plain-editable");

    // Part 2: legacy fixtures — COMMENTSTYPE is preserved, never auto-converted.
    let (html_doc, _, _) = roundtrip_assert_structure(&read_fixture("comments/html-comment.xml"), "html-comment.xml");
    let ht = read_task(html_doc.root.first_child_by_tag("TASK").unwrap());
    assert_eq!(ht.comments_type, CommentType::Html);
    // The CDATA payload itself IS preserved at the XML tree level:
    let comments_el = html_doc.root.first_child_by_tag("TASK").unwrap()
        .first_child_by_tag("COMMENTS").unwrap();
    assert!(comments_el.children.iter().any(|n| matches!(n, XmlNode::CData(c) if c.contains("<b>HTML</b>"))),
        "CDATA node must survive in the lossless XML tree");
    // BUG F5 — FIXED. read_task used to map COMMENTS via
    // XmlElement::text_content(), which collected only Text nodes and IGNORED
    // XmlNode::CData, so a CDATA-wrapped HTML comment mapped to an EMPTY
    // domain-level comment and the real content was overwritten on save.
    // text_content() now concatenates Text and CData in document order, so the
    // CDATA payload reaches the domain layer intact.
    assert!(ht.comments.as_ref().unwrap().content.contains("<b>HTML</b>"),
        "F5 fixed: CDATA content must reach the domain mapping");

    let (plain_doc, _, _) = roundtrip_assert_structure(&read_fixture("comments/plain-text-comment.xml"), "plain-text-comment.xml");
    let pt = read_task(plain_doc.root.first_child_by_tag("TASK").unwrap());
    assert_eq!(pt.comments_type, CommentType::Plain);
    let c = pt.comments.as_ref().unwrap();
    assert!(c.content.contains("Special characters: < > & \"quotes\""),
        "entities must decode in plain comment: {:?}", c.content);
    assert_eq!(c.comment_type, CommentType::Plain);
}

// ─── A09: DEPENDENCY element ─────────────────────────────────────────────────

/// QA-M10-A09: DEPENDENCY/TASKID/DEPENDENCYTYPE survive the round-trip, map
/// into `Task.dependencies`, and pass semantic validation (target exists).
#[test]
fn qa_m10_a09_dependency_element() {
    let bytes = read_fixture("dependencies/local-dependency.xml");
    let (doc, doc2, out) = roundtrip_assert_structure(&bytes, "local-dependency.xml");

    let text = decode_for_assertions(&doc.meta, &out);
    assert!(text.contains("<DEPENDENCY>"));
    assert!(text.contains("<TASKID>2</TASKID>"));
    assert!(text.contains("<DEPENDENCYTYPE>0</DEPENDENCYTYPE>"));

    let tasks = collect_task_elements(&doc2);
    assert_eq!(tasks.len(), 2);
    let t1 = read_task(&tasks[0]);
    assert_eq!(t1.id.as_str(), "1");
    assert_eq!(t1.dependencies.len(), 1);
    assert_eq!(t1.dependencies[0].task_id, "2");
    assert_eq!(t1.dependencies[0].dependency_type, 0);

    // Semantic validation: dependency target exists -> no OrphanedDependency.
    let tree = build_task_tree(&doc2);
    assert_eq!(tree.len(), 2);
    assert!(validate_task_tree(&tree).is_empty(), "local dependency must validate");
    assert!(validate_document(&doc2).is_empty());
}

// ─── A10: FileLink single ────────────────────────────────────────────────────

/// QA-M10-A10: a single FILEREFPATH (relative Windows path with backslashes)
/// survives the round-trip byte-for-byte and maps to `Task.file_links`.
#[test]
fn qa_m10_a10_filelink_single() {
    let bytes = read_fixture("attachments/single-filelink.xml");
    let (doc, doc2, out) = roundtrip_assert_structure(&bytes, "single-filelink.xml");

    let text = decode_for_assertions(&doc.meta, &out);
    assert!(text.contains(r"<FILEREFPATH>.\Documents\report.pdf</FILEREFPATH>"),
        "backslash path must survive verbatim");

    let t = read_task(doc2.root.first_child_by_tag("TASK").unwrap());
    assert_eq!(t.file_links.len(), 1);
    assert_eq!(t.file_links[0].path, r".\Documents\report.pdf");
    assert!(validate_document(&doc).is_empty());
}

// ─── A11: FileLink multi ─────────────────────────────────────────────────────

/// QA-M10-A11: multiple FILEREFPATH elements survive in their original order,
/// mixing relative and absolute Windows paths.
#[test]
fn qa_m10_a11_filelink_multi() {
    let bytes = read_fixture("attachments/multi-filelink.xml");
    let (doc, doc2, out) = roundtrip_assert_structure(&bytes, "multi-filelink.xml");

    let t = read_task(doc2.root.first_child_by_tag("TASK").unwrap());
    let paths: Vec<&str> = t.file_links.iter().map(|l| l.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            r".\Documents\report.pdf",
            r".\Images\screenshot.png",
            r"C:\Users\TestUser\Documents\spreadsheet.xlsx",
        ],
        "file link order must be preserved"
    );

    // Order also preserved in serialized bytes.
    let text = decode_for_assertions(&doc.meta, &out);
    let p1 = text.find(r".\Documents\report.pdf").unwrap();
    let p2 = text.find(r".\Images\screenshot.png").unwrap();
    let p3 = text.find(r"C:\Users\TestUser\Documents\spreadsheet.xlsx").unwrap();
    assert!(p1 < p2 && p2 < p3, "FILEREFPATH order must be preserved in output bytes");
}

// ─── A12: Malformed / truncated input rejection ─────────────────────────────

/// QA-M10-A12: malformed inputs are rejected with a clear structured error,
/// never a panic and never silent partial acceptance.
///
/// - `malformed/truncated.xml` (cut mid-attribute) -> Err with non-empty message
/// - empty / whitespace-only input -> `XmlParseError::EmptyInput`
/// - garbage bytes -> Err
/// - `malformed/invalid-encoding.xml` (UTF-16 LE bytes whose declaration lies
///   "utf-8") -> parsed via BOM-driven decoding without data loss; the
///   mismatch is tolerated per the documented parser precedence (finding F3).
#[test]
fn qa_m10_a12_malformed_and_truncated_rejected() {
    // Truncated document must be rejected.
    let truncated = read_fixture("malformed/truncated.xml");
    let err = parse_xml(&truncated).expect_err("truncated XML must be rejected");
    let msg = err.to_string();
    assert!(!msg.is_empty(), "error must carry a clear message");
    assert!(
        matches!(err, XmlParseError::MalformedXml(_) | XmlParseError::TokenizerError(_)),
        "structured error expected, got {err:?}"
    );

    // Empty and whitespace-only input.
    assert_eq!(parse_xml(b""), Err(XmlParseError::EmptyInput));
    assert_eq!(parse_xml(b"  \r\n\t "), Err(XmlParseError::EmptyInput));

    // Garbage bytes.
    assert!(parse_xml(b"<<<<not xml>>>>").is_err());
    assert!(parse_xml(&[0x00, 0x01, 0x02, 0xFF, 0xFE]).is_err());

    // Declaration/BOM mismatch: BOM wins for decoding -> content is recoverable.
    let mismatched = read_fixture("malformed/invalid-encoding.xml");
    assert_eq!(detect_bom(&mismatched).encoding, XmlEncoding::Utf16Le);
    let doc = parse_xml(&mismatched).expect("BOM-driven decoding must recover the content");
    assert_eq!(doc.root.tag, "TODOLIST");
    let tasks = collect_task_elements(&doc);
    assert!(!tasks.is_empty(), "tasks must be readable despite the lying declaration");
    assert!(validate_document(&doc).is_empty());
}

// ─── A13: Real-world document ────────────────────────────────────────────────

/// QA-M10-A13: the real-world london-underground.xml (9 tasks, GUID metadata,
/// FILEREFPATHs with embedded newlines, mixed content) parses, validates,
/// maps to the domain model, and round-trips with full structural fidelity.
#[test]
fn qa_m10_a13_realworld_london_underground() {
    let path = fixtures_root().join("real-world").join("london-underground.xml");
    let bytes = fs::read(&path).unwrap();
    let (doc, doc2, _out) = roundtrip_assert_structure(&bytes, "london-underground.xml");

    // Document-level checks.
    assert_eq!(doc.root.tag, "TODOLIST");
    assert!(validate_document(&doc).is_empty());
    assert_eq!(doc.root.get_attr("FILENAME"), Some("London Underground.xml"));
    assert_eq!(doc.root.get_attr("NEXTUNIQUEID"), Some("10"));

    // 9 tasks, all with unique stable ids and titles.
    let tasks = collect_task_elements(&doc2);
    assert_eq!(tasks.len(), 9, "london-underground has 9 TASK elements");
    let mut tree = TaskTree::new();
    let mut ids = Vec::new();
    for el in &tasks {
        let t = read_task(el);
        assert!(!t.title.is_empty());
        ids.push(t.id.as_str().to_string());
        tree.add_task(t);
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 9, "task ids must be unique");
    assert_eq!(tree.len(), 9);
    assert!(validate_task_tree(&tree).is_empty());

    // Root metadata (plugin GUID) mapped.
    let dm = read_document_metadata(&doc2.root, doc2.meta.clone());
    assert_eq!(dm.next_unique_id, 10);
    assert!(dm.root_metadata.iter().any(|m| m.attrs.iter().any(|(k, _)| k.contains("FA40B83E"))));

    // Every FILEREFPATH in the document survives the round-trip (8 links;
    // one of the 9 tasks carries none — exactly like the source file).
    let links: usize = tree.iter().map(|t| t.file_links.len()).sum();
    assert_eq!(links, 8, "all file links preserved");
    assert!(tree.iter().any(|t| t.file_links.iter().any(|l| l.path.contains('\n') || l.path.contains("London"))),
        "multi-line file link content preserved");

    // Title spot-checks (canonical identity of content).
    let titles: Vec<&str> = tasks.iter().filter_map(|el| el.get_attr("TITLE")).collect();
    assert!(titles.contains(&"South Kensigton"), "original typo'd title must be preserved verbatim");
    assert!(titles.contains(&"Euston"));
}

/// Sanity guard so `Path` import is used across platforms (fixtures are read
/// through absolute paths built from CARGO_MANIFEST_DIR).
#[test]
fn qa_m10_a00_fixtures_root_resolves() {
    let root = fixtures_root();
    assert!(Path::new(&root).join("MANIFEST.sha256").exists());
    assert!(Path::new(&root).join("rc").join("index-source.xml").exists());
}
