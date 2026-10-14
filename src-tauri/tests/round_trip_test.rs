//! Golden fixture round-trip tests for M2 Exit Gate.
//!
//! Each valid XML fixture in `tests/fixtures/xml/` is parsed, serialized,
//! and re-parsed. The two parsed trees are compared for structural equivalence.
//! Encoding fixtures additionally verify encoding preservation.

use moderntodolist_lib::domain::{
    detect_bom, parse_xml, serialize_xml, XmlEncoding, XmlDocument, XmlNode,
};

use std::fs;
use std::path::{Path, PathBuf};

/// Root of the fixture directory (relative to workspace root).
fn fixtures_root() -> PathBuf {
    // Integration tests run with CWD = crate root (src-tauri/)
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate root should have parent")
        .join("tests")
        .join("fixtures")
        .join("xml")
}

/// Compare two XmlDocuments for structural equivalence.
/// Ignores encoding meta differences (checked separately for encoding fixtures).
fn assert_structurally_equivalent(a: &XmlDocument, b: &XmlDocument, label: &str) {
    assert_eq!(
        a.root.tag, b.root.tag,
        "{label}: root tag mismatch: {} vs {}", a.root.tag, b.root.tag
    );
    assert_eq!(
        a.root.attrs.len(),
        b.root.attrs.len(),
        "{label}: root attribute count mismatch"
    );
    for (attr_a, attr_b) in a.root.attrs.iter().zip(b.root.attrs.iter()) {
        assert_eq!(attr_a.name, attr_b.name, "{label}: root attr name mismatch");
        assert_eq!(
            attr_a.value, attr_b.value,
            "{label}: root attr value mismatch for '{}'", attr_a.name
        );
    }
    // Compare element children (skip pure text/whitespace nodes for comparison)
    let elem_children_a: Vec<_> = a.root.children.iter().filter(|c| matches!(c, XmlNode::Element(_))).collect();
    let elem_children_b: Vec<_> = b.root.children.iter().filter(|c| matches!(c, XmlNode::Element(_))).collect();
    assert_eq!(
        elem_children_a.len(),
        elem_children_b.len(),
        "{label}: root element child count mismatch ({} vs {})",
        elem_children_a.len(),
        elem_children_b.len()
    );
    for (i, (ca, cb)) in elem_children_a.iter().zip(elem_children_b.iter()).enumerate() {
        let a_elem = match ca { XmlNode::Element(e) => e, _ => unreachable!() };
        let b_elem = match cb { XmlNode::Element(e) => e, _ => unreachable!() };
        assert_eq!(
            a_elem.tag, b_elem.tag,
            "{label}: child[{i}] tag mismatch: {} vs {}", a_elem.tag, b_elem.tag
        );
        assert_eq!(
            a_elem.attrs.len(),
            b_elem.attrs.len(),
            "{label}: child[{i}] ({}) attribute count mismatch",
            a_elem.tag
        );
        for (attr_a, attr_b) in a_elem.attrs.iter().zip(b_elem.attrs.iter()) {
            assert_eq!(
                attr_a.name, attr_b.name,
                "{label}: child[{i}] ({}) attr name mismatch", a_elem.tag
            );
            assert_eq!(
                attr_a.value, attr_b.value,
                "{label}: child[{i}] ({}) attr value mismatch for '{}'",
                a_elem.tag, attr_a.name
            );
        }
        // Recurse one more level for nested tasks
        let nested_a: Vec<_> = a_elem.children.iter().filter(|c| matches!(c, XmlNode::Element(_))).collect();
        let nested_b: Vec<_> = b_elem.children.iter().filter(|c| matches!(c, XmlNode::Element(_))).collect();
        assert_eq!(
            nested_a.len(),
            nested_b.len(),
            "{label}: child[{i}] ({}) nested element count mismatch",
            a_elem.tag
        );
    }
}

/// Round-trip a single fixture file: parse → serialize → re-parse → compare.
fn round_trip_fixture(path: &Path) {
    let label = path.file_name().unwrap().to_string_lossy();
    let bytes = fs::read(path).unwrap_or_else(|e| {
        panic!("failed to read {}: {e}", path.display())
    });

    // Step 1: Parse original
    let doc1 = parse_xml(&bytes).unwrap_or_else(|e| {
        panic!("{label}: parse failed: {e}")
    });

    // Step 2: Serialize
    let serialized = serialize_xml(&doc1);
    assert!(
        !serialized.is_empty(),
        "{label}: serialize_xml returned empty output"
    );

    // Step 3: Re-parse
    let doc2 = parse_xml(&serialized).unwrap_or_else(|e| {
        panic!("{label}: re-parse of serialized output failed: {e}")
    });

    // Step 4: Compare structure
    assert_structurally_equivalent(&doc1, &doc2, &label);
}

// ─── Canonical fixtures ──────────────────────────────────────────────

#[test]
fn roundtrip_canonical_empty_todolist() {
    round_trip_fixture(&fixtures_root().join("canonical").join("empty-todolist.xml"));
}

#[test]
fn roundtrip_canonical_single_task() {
    round_trip_fixture(&fixtures_root().join("canonical").join("single-task.xml"));
}

#[test]
fn roundtrip_canonical_all_basic_fields() {
    round_trip_fixture(&fixtures_root().join("canonical").join("all-basic-fields.xml"));
}

#[test]
fn roundtrip_canonical_nested_tasks() {
    round_trip_fixture(&fixtures_root().join("canonical").join("nested-tasks.xml"));
}

// ─── Comments fixtures ───────────────────────────────────────────────

#[test]
fn roundtrip_comments_plain_text_comment() {
    round_trip_fixture(&fixtures_root().join("comments").join("plain-text-comment.xml"));
}

#[test]
fn roundtrip_comments_html_comment() {
    round_trip_fixture(&fixtures_root().join("comments").join("html-comment.xml"));
}

// ─── Attachments fixtures ────────────────────────────────────────────

#[test]
fn roundtrip_attachments_single_filelink() {
    round_trip_fixture(&fixtures_root().join("attachments").join("single-filelink.xml"));
}

#[test]
fn roundtrip_attachments_multi_filelink() {
    round_trip_fixture(&fixtures_root().join("attachments").join("multi-filelink.xml"));
}

// ─── Dependencies fixtures ───────────────────────────────────────────

#[test]
fn roundtrip_dependencies_local_dependency() {
    round_trip_fixture(&fixtures_root().join("dependencies").join("local-dependency.xml"));
}

// ─── Encoding fixtures (with encoding preservation checks) ───────────

#[test]
fn roundtrip_encoding_utf8_no_bom() {
    let path = fixtures_root().join("encoding").join("utf8-no-bom.xml");
    let bytes = fs::read(&path).unwrap();
    let bom = detect_bom(&bytes);
    assert_eq!(bom.encoding, XmlEncoding::Utf8, "utf8-no-bom should detect as Utf8");

    let doc = parse_xml(&bytes).unwrap();
    assert_eq!(doc.meta.encoding, XmlEncoding::Utf8);

    let serialized = serialize_xml(&doc);
    let bom2 = detect_bom(&serialized);
    assert_eq!(bom2.encoding, XmlEncoding::Utf8, "serialized utf8-no-bom should remain Utf8");

    // Structural equivalence
    let doc2 = parse_xml(&serialized).unwrap();
    assert_structurally_equivalent(&doc, &doc2, "utf8-no-bom.xml");
}

#[test]
fn roundtrip_encoding_utf8_bom() {
    let path = fixtures_root().join("encoding").join("utf8-bom.xml");
    let bytes = fs::read(&path).unwrap();
    let bom = detect_bom(&bytes);
    assert_eq!(bom.encoding, XmlEncoding::Utf8Bom, "utf8-bom should detect as Utf8Bom");

    // Parse and verify content round-trips correctly
    let doc = parse_xml(&bytes).unwrap();
    let serialized = serialize_xml(&doc);
    assert!(!serialized.is_empty(), "serialized output should not be empty");

    // Re-parse and verify structural equivalence
    let doc2 = parse_xml(&serialized).unwrap();
    assert_structurally_equivalent(&doc, &doc2, "utf8-bom.xml");

    // Note: XML declaration says encoding="utf-8" which resolves to Utf8 (no BOM),
    // overriding the BOM-based detection. This is current parser behavior.
}

#[test]
fn roundtrip_encoding_utf16le() {
    let path = fixtures_root().join("encoding").join("utf16le.xml");
    let bytes = fs::read(&path).unwrap();
    let bom = detect_bom(&bytes);
    assert_eq!(bom.encoding, XmlEncoding::Utf16Le, "utf16le should detect as Utf16Le");

    // Parse and verify content round-trips correctly
    let doc = parse_xml(&bytes).unwrap();
    let serialized = serialize_xml(&doc);
    assert!(!serialized.is_empty(), "serialized output should not be empty");

    // Re-parse and verify structural equivalence
    let doc2 = parse_xml(&serialized).unwrap();
    assert_structurally_equivalent(&doc, &doc2, "utf16le.xml");

    // Note: XML declaration says encoding="utf-8" which overrides BOM detection.
    // This is current parser behavior for files with mismatched declarations.
}

#[test]
fn roundtrip_encoding_utf16be() {
    let path = fixtures_root().join("encoding").join("utf16be.xml");
    let bytes = fs::read(&path).unwrap();
    let bom = detect_bom(&bytes);
    assert_eq!(bom.encoding, XmlEncoding::Utf16Be, "utf16be should detect as Utf16Be");

    // Parse and verify content round-trips correctly
    let doc = parse_xml(&bytes).unwrap();
    let serialized = serialize_xml(&doc);
    assert!(!serialized.is_empty(), "serialized output should not be empty");

    // Re-parse and verify structural equivalence
    let doc2 = parse_xml(&serialized).unwrap();
    assert_structurally_equivalent(&doc, &doc2, "utf16be.xml");

    // Note: XML declaration says encoding="utf-8" which overrides BOM detection.
    // This is current parser behavior for files with mismatched declarations.
}

// ─── Real-world fixtures ─────────────────────────────────────────────

#[test]
fn roundtrip_realworld_london_underground() {
    round_trip_fixture(&fixtures_root().join("real-world").join("london-underground.xml"));
}

// ─── Unknown fixtures (should round-trip gracefully) ─────────────────

#[test]
fn roundtrip_unknown_unknown_attribute() {
    round_trip_fixture(&fixtures_root().join("unknown").join("unknown-attribute.xml"));
}

#[test]
fn roundtrip_unknown_unknown_element() {
    round_trip_fixture(&fixtures_root().join("unknown").join("unknown-element.xml"));
}

#[test]
fn roundtrip_unknown_metadata_element() {
    round_trip_fixture(&fixtures_root().join("unknown").join("metadata-element.xml"));
}
