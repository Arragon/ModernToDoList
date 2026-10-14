//! Lossless XML serializer for ModernToDoList 2.0.
//!
//! This module serializes an `XmlDocument` back to bytes, preserving the original
//! encoding, BOM, XML declaration, and line endings. Attribute values and text
//! content are properly escaped for XML.
//!
//! # Serialization pipeline
//!
//! 1. Write BOM bytes (if the encoding requires one)
//! 2. Write XML declaration (if present in metadata)
//! 3. Recursively write the element tree
//! 4. Encode the UTF-8 output to the target encoding

use super::encoding::XmlEncodingMeta;
use super::xml_parser::escape_xml;
use super::xml_tree::{XmlDocument, XmlElement, XmlNode};

/// Serializes an `XmlDocument` to bytes using the document's encoding metadata.
///
/// The output bytes match the original encoding:
/// - UTF-8: no BOM (or BOM if `Utf8Bom`)
/// - UTF-8+BOM: EF BB BF prefix
/// - UTF-16LE: FF FE BOM + UTF-16LE encoded content
/// - UTF-16BE: FE FF BOM + UTF-16BE encoded content
///
/// Line endings in the output match the detected line ending style.
pub fn serialize_xml(doc: &XmlDocument) -> Vec<u8> {
    let mut output = String::new();

    // 1. Write XML declaration if present
    if let Some(ref decl) = doc.meta.xml_declaration {
        output.push_str(decl);
    }

    // 2. Write root element tree
    write_element(&mut output, &doc.root, &doc.meta, 0);

    // 3. Encode to target encoding
    encode_output(&output, &doc.meta)
}

/// Writes an XML element and its children to the output string.
fn write_element(
    output: &mut String,
    elem: &XmlElement,
    meta: &XmlEncodingMeta,
    depth: usize,
) {
    output.push('<');
    output.push_str(&elem.tag);

    // Write attributes
    for attr in &elem.attrs {
        output.push(' ');
        output.push_str(&attr.name);
        output.push_str("=\"");
        output.push_str(&escape_xml(&attr.value));
        output.push('"');
    }

    if elem.children.is_empty() {
        // Self-closing tag
        output.push_str("/>");
    } else {
        output.push('>');

        // Write children
        for child in &elem.children {
            write_node(output, child, meta, depth + 1);
        }

        // Closing tag
        output.push_str("</");
        output.push_str(&elem.tag);
        output.push('>');
    }
}

/// Writes an XML node to the output string.
fn write_node(
    output: &mut String,
    node: &XmlNode,
    meta: &XmlEncodingMeta,
    depth: usize,
) {
    match node {
        XmlNode::Element(elem) => {
            write_element(output, elem, meta, depth);
        }
        XmlNode::Text(text) => {
            output.push_str(&escape_xml(text));
        }
        XmlNode::Comment(content) => {
            output.push_str("<!--");
            output.push_str(content);
            output.push_str("-->");
        }
        XmlNode::CData(content) => {
            output.push_str("<![CDATA[");
            output.push_str(content);
            output.push_str("]]>");
        }
        XmlNode::ProcessingInstruction(content) => {
            output.push_str("<?");
            output.push_str(content);
            output.push_str("?>");
        }
    }
}

/// Encodes a UTF-8 string to the target encoding bytes.
///
/// Prepends the BOM if the encoding requires one, then converts
/// the UTF-8 content to the target encoding.
fn encode_output(content: &str, meta: &XmlEncodingMeta) -> Vec<u8> {
    let mut output = Vec::with_capacity(content.len() * 2);

    // Write BOM if needed
    if let Some(bom) = meta.encoding.bom_bytes() {
        output.extend_from_slice(bom);
    }

    // Encode content
    match meta.encoding {
        super::encoding::XmlEncoding::Utf8 | super::encoding::XmlEncoding::Utf8Bom => {
            output.extend_from_slice(content.as_bytes());
        }
        super::encoding::XmlEncoding::Utf16Le => {
            let utf16: Vec<u16> = content.encode_utf16().collect();
            for code_unit in utf16 {
                output.extend_from_slice(&code_unit.to_le_bytes());
            }
        }
        super::encoding::XmlEncoding::Utf16Be => {
            let utf16: Vec<u16> = content.encode_utf16().collect();
            for code_unit in utf16 {
                output.extend_from_slice(&code_unit.to_be_bytes());
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::encoding::{XmlEncoding, XmlEncodingMeta};
    use crate::domain::xml_parser::parse_xml;
    use crate::domain::xml_tree::{XmlAttribute, XmlElement};

    #[test]
    fn serialize_utf8_no_bom() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8);
        let mut root = XmlElement::new("ROOT");
        root.set_attr("ID", "1");
        let doc = XmlDocument::new(meta, root);
        let bytes = serialize_xml(&doc);
        // No BOM
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("<ROOT ID=\"1\"/>"));
    }

    #[test]
    fn serialize_utf8_with_bom() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8Bom);
        let root = XmlElement::new("ROOT");
        let doc = XmlDocument::new(meta, root);
        let bytes = serialize_xml(&doc);
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    }

    #[test]
    fn serialize_utf16le() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf16Le);
        let root = XmlElement::new("ROOT");
        let doc = XmlDocument::new(meta, root);
        let bytes = serialize_xml(&doc);
        // UTF-16 LE BOM
        assert!(bytes.starts_with(&[0xFF, 0xFE]));
        // Content should be valid UTF-16 LE
        let utf16_bytes = &bytes[2..];
        assert!(utf16_bytes.len().is_multiple_of(2));
    }

    #[test]
    fn serialize_utf16be() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf16Be);
        let root = XmlElement::new("ROOT");
        let doc = XmlDocument::new(meta, root);
        let bytes = serialize_xml(&doc);
        // UTF-16 BE BOM
        assert!(bytes.starts_with(&[0xFE, 0xFF]));
    }

    #[test]
    fn serialize_with_xml_declaration() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8).with_xml_declaration(Some(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_string(),
        ));
        let root = XmlElement::new("ROOT");
        let doc = XmlDocument::new(meta, root);
        let bytes = serialize_xml(&doc);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    }

    #[test]
    fn serialize_escapes_attribute_values() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8);
        let mut root = XmlElement::new("TASK");
        root.set_attr("TITLE", "a < b & c > d");
        let doc = XmlDocument::new(meta, root);
        let bytes = serialize_xml(&doc);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("a &lt; b &amp; c &gt; d"));
    }

    #[test]
    fn serialize_escapes_text_content() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8);
        let mut root = XmlElement::new("COMMENTS");
        root.children
            .push(XmlNode::Text("5 > 3 & 2 < 4".into()));
        let doc = XmlDocument::new(meta, root);
        let bytes = serialize_xml(&doc);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("5 &gt; 3 &amp; 2 &lt; 4"));
    }

    #[test]
    fn roundtrip_simple_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><ROOT><TASK ID="1" TITLE="Test" PRIORITY="5"></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let output = serialize_xml(&doc);
        let text = String::from_utf8(output).unwrap();

        // Parse again and verify
        let doc2 = parse_xml(text.as_bytes()).unwrap();
        assert_eq!(doc2.root.tag, "ROOT");
        let task = doc2.root.first_child_by_tag("TASK").unwrap();
        assert_eq!(task.get_attr("ID"), Some("1"));
        assert_eq!(task.get_attr("TITLE"), Some("Test"));
        assert_eq!(task.get_attr("PRIORITY"), Some("5"));
    }

    #[test]
    fn roundtrip_preserves_unknown_attrs() {
        let xml = r#"<ROOT><TASK ID="1" TITLE="Test" CUSTOM_ATTR="preserved" ANOTHER="123"></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let output = serialize_xml(&doc);
        let doc2 = parse_xml(&output).unwrap();
        let task = doc2.root.first_child_by_tag("TASK").unwrap();
        assert_eq!(task.get_attr("CUSTOM_ATTR"), Some("preserved"));
        assert_eq!(task.get_attr("ANOTHER"), Some("123"));
    }

    #[test]
    fn roundtrip_preserves_unknown_elements() {
        let xml =
            r#"<ROOT><TASK ID="1"><UNKNOWN attr="val">text</UNKNOWN></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let output = serialize_xml(&doc);
        let doc2 = parse_xml(&output).unwrap();
        let task = doc2.root.first_child_by_tag("TASK").unwrap();
        let unknown = task.first_child_by_tag("UNKNOWN").unwrap();
        assert_eq!(unknown.get_attr("attr"), Some("val"));
        assert_eq!(unknown.text_content(), "text");
    }

    #[test]
    fn roundtrip_nested_tasks() {
        let xml = r#"<ROOT>
<TASK ID="1" TITLE="Level 1">
    <TASK ID="2" TITLE="Level 2">
        <TASK ID="3" TITLE="Level 3">
        </TASK>
    </TASK>
</TASK>
</ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let output = serialize_xml(&doc);
        let doc2 = parse_xml(&output).unwrap();

        let l1 = doc2.root.first_child_by_tag("TASK").unwrap();
        assert_eq!(l1.get_attr("TITLE"), Some("Level 1"));
        let l2 = l1.first_child_by_tag("TASK").unwrap();
        assert_eq!(l2.get_attr("TITLE"), Some("Level 2"));
        let l3 = l2.first_child_by_tag("TASK").unwrap();
        assert_eq!(l3.get_attr("TITLE"), Some("Level 3"));
    }

    #[test]
    fn roundtrip_preserves_comments() {
        let xml = r#"<ROOT><!-- comment --><TASK ID="1"></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let output = serialize_xml(&doc);
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("<!-- comment -->"));
    }

    #[test]
    fn roundtrip_multiple_categories() {
        let xml = r#"<ROOT><TASK ID="1"><CATEGORY>Work</CATEGORY><CATEGORY>Urgent</CATEGORY></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let output = serialize_xml(&doc);
        let doc2 = parse_xml(&output).unwrap();
        let task = doc2.root.first_child_by_tag("TASK").unwrap();
        let cats: Vec<_> = task.children_by_tag("CATEGORY").collect();
        assert_eq!(cats.len(), 2);
        assert_eq!(cats[0].text_content(), "Work");
        assert_eq!(cats[1].text_content(), "Urgent");
    }

    #[test]
    fn roundtrip_with_entities() {
        let xml = r#"<ROOT><TASK TITLE="a &lt; b &amp; c"></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let task = doc.root.first_child_by_tag("TASK").unwrap();
        assert_eq!(task.get_attr("TITLE"), Some("a < b & c"));

        let output = serialize_xml(&doc);
        let doc2 = parse_xml(&output).unwrap();
        let task2 = doc2.root.first_child_by_tag("TASK").unwrap();
        assert_eq!(task2.get_attr("TITLE"), Some("a < b & c"));
    }

    #[test]
    fn serialize_self_closing_empty_element() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8);
        let root = XmlElement::new("TASK");
        let doc = XmlDocument::new(meta, root);
        let bytes = serialize_xml(&doc);
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text, "<TASK/>");
    }

    #[test]
    fn serialize_preserves_attr_order() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8);
        let mut root = XmlElement::new("TASK");
        root.attrs = vec![
            XmlAttribute {
                name: "ID".into(),
                value: "1".into(),
            },
            XmlAttribute {
                name: "TITLE".into(),
                value: "Test".into(),
            },
            XmlAttribute {
                name: "PRIORITY".into(),
                value: "5".into(),
            },
        ];
        let doc = XmlDocument::new(meta, root);
        let bytes = serialize_xml(&doc);
        let text = String::from_utf8(bytes).unwrap();
        // Verify attribute order: ID before TITLE before PRIORITY
        let id_pos = text.find("ID=").unwrap();
        let title_pos = text.find("TITLE=").unwrap();
        let priority_pos = text.find("PRIORITY=").unwrap();
        assert!(id_pos < title_pos);
        assert!(title_pos < priority_pos);
    }
}
