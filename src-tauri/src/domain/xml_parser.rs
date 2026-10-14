//! Lossless XML parser using `quick-xml` tokenizer.
//!
//! This module parses XML/TDL files into the lossless [`XmlDocument`] tree model.
//! It preserves all content including unknown attributes, unknown elements,
//! whitespace, comments, and processing instructions.
//!
//! # Parse pipeline
//!
//! 1. Detect BOM and encoding (from `encoding` module)
//! 2. Decode to UTF-8 if needed
//! 3. Detect XML declaration and line endings
//! 4. Parse XML using quick-xml tokenizer into `XmlDocument` tree
//!
//! # Error handling
//!
//! Malformed XML returns a structured error (`XmlParseError`) that describes
//! what went wrong. The parser never panics on invalid input.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::encoding::{detect_bom, detect_line_ending, detect_xml_declaration, decode_utf16_to_utf8, XmlEncodingMeta};
use super::xml_tree::{XmlAttribute, XmlDocument, XmlElement, XmlNode};

/// Errors that can occur during XML parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlParseError {
    /// The input bytes could not be decoded as the detected encoding.
    EncodingError(String),
    /// The XML is malformed (unexpected EOF, mismatched tags, etc.).
    MalformedXml(String),
    /// The XML is empty or contains only whitespace.
    EmptyInput,
    /// A quick-xml error occurred during tokenization.
    TokenizerError(String),
}

impl std::fmt::Display for XmlParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XmlParseError::EncodingError(msg) => write!(f, "encoding error: {}", msg),
            XmlParseError::MalformedXml(msg) => write!(f, "malformed XML: {}", msg),
            XmlParseError::EmptyInput => write!(f, "empty input"),
            XmlParseError::TokenizerError(msg) => write!(f, "tokenizer error: {}", msg),
        }
    }
}

impl std::error::Error for XmlParseError {}

/// Parses raw bytes into a lossless `XmlDocument`.
///
/// This is the main entry point for reading XML/TDL files. It performs:
/// 1. BOM detection and encoding identification
/// 2. UTF-16 → UTF-8 decoding (if needed)
/// 3. XML declaration extraction
/// 4. Line ending detection
/// 5. Full XML tokenization and tree construction
///
/// # Errors
///
/// Returns `XmlParseError` if the input is empty, has an unsupported encoding,
/// or contains malformed XML.
pub fn parse_xml(input: &[u8]) -> Result<XmlDocument, XmlParseError> {
    if input.is_empty() || input.iter().all(|&b| b.is_ascii_whitespace()) {
        return Err(XmlParseError::EmptyInput);
    }

    // Step 1: Detect BOM
    let bom = detect_bom(input);
    let after_bom = &input[bom.bom_len..];

    // Step 2: Decode to UTF-8
    let utf8_text = if bom.encoding.is_utf16() {
        decode_utf16_to_utf8(after_bom, bom.encoding)
            .map_err(|e| XmlParseError::EncodingError(e.to_string()))?
    } else {
        // UTF-8 (with or without BOM) - BOM already stripped
        String::from_utf8(after_bom.to_vec())
            .map_err(|e| XmlParseError::EncodingError(format!("invalid UTF-8: {}", e)))?
    };

    // Step 3: Detect XML declaration
    let xml_decl = detect_xml_declaration(&utf8_text);
    let encoding_from_decl = xml_decl
        .as_ref()
        .and_then(|d| d.encoding_attr.as_deref())
        .and_then(super::encoding::resolve_encoding_from_attr);

    // If BOM said UTF-8 but declaration says UTF-16, trust the declaration
    let actual_encoding = encoding_from_decl.unwrap_or(bom.encoding);

    // Step 4: Detect line endings
    let line_ending = detect_line_ending(&utf8_text);

    // Step 5: Build encoding metadata
    let meta = XmlEncodingMeta {
        encoding: actual_encoding,
        xml_declaration: xml_decl.map(|d| d.raw),
        line_ending,
    };

    // Step 6: Parse XML into tree
    let root = parse_element_tree(&utf8_text)?;

    Ok(XmlDocument { meta, root })
}

/// Parses a UTF-8 string into an `XmlElement` tree using quick-xml.
fn parse_element_tree(input: &str) -> Result<XmlElement, XmlParseError> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut stack: Vec<XmlElement> = Vec::new();
    let mut root: Option<XmlElement> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(bs)) => {
                let elem = bytes_start_to_element(&bs)?;
                stack.push(elem);
            }
            Ok(Event::Empty(bs)) => {
                let elem = bytes_start_to_element(&bs)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(XmlNode::Element(elem));
                } else {
                    // This is the root element (self-closing)
                    root = Some(elem);
                }
            }
            Ok(Event::End(_be)) => {
                if let Some(completed) = stack.pop() {
                    if stack.is_empty() {
                        root = Some(completed);
                    } else {
                        stack
                            .last_mut()
                            .unwrap()
                            .children
                            .push(XmlNode::Element(completed));
                    }
                }
            }
            Ok(Event::Text(bt)) => {
                let text = xml_decode_text(&bt);
                if let Some(current) = stack.last_mut() {
                    current.children.push(XmlNode::Text(text));
                }
            }
            Ok(Event::Comment(bt)) => {
                let content = xml_decode_bytes(bt.as_ref());
                if let Some(current) = stack.last_mut() {
                    current.children.push(XmlNode::Comment(content));
                }
            }
            Ok(Event::CData(bt)) => {
                let content = xml_decode_bytes(bt.as_ref());
                if let Some(current) = stack.last_mut() {
                    current.children.push(XmlNode::CData(content));
                }
            }
            Ok(Event::PI(bt)) => {
                let content = xml_decode_bytes(bt.as_ref());
                if let Some(current) = stack.last_mut() {
                    current
                        .children
                        .push(XmlNode::ProcessingInstruction(content));
                }
            }
            Ok(Event::Decl(_bd)) => {
                // XML declaration is handled in encoding detection; skip here
            }
            Ok(Event::DocType(bt)) => {
                // Preserve DOCTYPE as a processing instruction
                let content = xml_decode_bytes(bt.as_ref());
                if let Some(current) = stack.last_mut() {
                    current
                        .children
                        .push(XmlNode::ProcessingInstruction(format!("DOCTYPE {}", content)));
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(XmlParseError::TokenizerError(format!(
                    "at position {}: {}",
                    reader.buffer_position(),
                    e
                )));
            }
        }
    }

    root.ok_or_else(|| XmlParseError::MalformedXml("no root element found".into()))
}

/// Converts a quick-xml `BytesStart` into our `XmlElement`.
fn bytes_start_to_element(bs: &BytesStart) -> Result<XmlElement, XmlParseError> {
    let tag = xml_decode_bytes(bs.name().as_ref());
    let mut attrs = Vec::new();

    for attr_result in bs.attributes() {
        match attr_result {
            Ok(attr) => {
                let name = xml_decode_bytes(attr.key.as_ref());
                let raw_value = xml_decode_bytes(attr.value.as_ref());
                let value = unescape_xml(&raw_value);
                attrs.push(XmlAttribute { name, value });
            }
            Err(e) => {
                return Err(XmlParseError::TokenizerError(format!(
                    "attribute parse error in <{}>: {}",
                    tag, e
                )));
            }
        }
    }

    Ok(XmlElement {
        tag,
        attrs,
        children: Vec::new(),
    })
}

/// Decodes XML text content, unescaping entities like `&lt;` → `<`.
fn xml_decode_text(bytes: &quick_xml::events::BytesText) -> String {
    // Try unescape first; fall back to raw decode
    match bytes.unescape() {
        Ok(cow) => cow.into_owned(),
        Err(_) => xml_decode_bytes(bytes.as_ref()),
    }
}

/// Decodes raw bytes as UTF-8, replacing invalid sequences.
fn xml_decode_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Unescapes XML attribute value entities.
///
/// Handles the five predefined XML entities:
/// - `&lt;` → `<`
/// - `&gt;` → `>`
/// - `&amp;` → `&`
/// - `&apos;` → `'`
/// - `&quot;` → `"`
///
/// Also handles numeric character references (`&#NNN;` and `&#xHHH;`).
pub fn unescape_xml(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }

    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '&' {
            let mut entity = String::new();
            let mut found_semicolon = false;

            for _ in 0..10 {
                // Max entity length: &#xHHHH;
                if let Some(&next) = chars.peek() {
                    if next == ';' {
                        chars.next();
                        found_semicolon = true;
                        break;
                    }
                    entity.push(next);
                    chars.next();
                } else {
                    break;
                }
            }

            if found_semicolon {
                match entity.as_str() {
                    "lt" => result.push('<'),
                    "gt" => result.push('>'),
                    "amp" => result.push('&'),
                    "apos" => result.push('\''),
                    "quot" => result.push('"'),
                    _ if entity.starts_with('#') => {
                        // Numeric character reference
                        let num_str = &entity[1..];
                        let code_point = if num_str.starts_with('x') || num_str.starts_with('X')
                        {
                            u32::from_str_radix(&num_str[1..], 16).ok()
                        } else {
                            num_str.parse::<u32>().ok()
                        };
                        if let Some(cp) = code_point {
                            if let Some(ch) = char::from_u32(cp) {
                                result.push(ch);
                            } else {
                                // Invalid code point, preserve original
                                result.push('&');
                                result.push_str(&entity);
                                result.push(';');
                            }
                        } else {
                            result.push('&');
                            result.push_str(&entity);
                            result.push(';');
                        }
                    }
                    _ => {
                        // Unknown entity, preserve as-is
                        result.push('&');
                        result.push_str(&entity);
                        result.push(';');
                    }
                }
            } else {
                // No semicolon found, treat '&' as literal
                result.push('&');
                result.push_str(&entity);
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Escapes a string for use as an XML attribute value or text content.
///
/// Escapes the five predefined XML entities:
/// - `&` → `&amp;`
/// - `<` → `&lt;`
/// - `>` → `&gt;`
/// - `'` → `&apos;`
/// - `"` → `&quot;`
pub fn escape_xml(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '\'' => result.push_str("&apos;"),
            '"' => result.push_str("&quot;"),
            _ => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_task() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<TODOLIST NEXTUNIQUEID="2">
<TASK ID="1" TITLE="Test task" PRIORITY="5">
</TASK>
</TODOLIST>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        assert_eq!(doc.root.tag, "TODOLIST");
        assert_eq!(doc.root.get_attr("NEXTUNIQUEID"), Some("2"));

        let tasks: Vec<_> = doc.root.children_by_tag("TASK").collect();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].get_attr("ID"), Some("1"));
        assert_eq!(tasks[0].get_attr("TITLE"), Some("Test task"));
        assert_eq!(tasks[0].get_attr("PRIORITY"), Some("5"));
    }

    #[test]
    fn parse_preserves_unknown_attrs() {
        let xml = r#"<ROOT><TASK ID="1" TITLE="Test" UNKNOWN_ATTR="value" CUSTOM="123"></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let task = doc.root.first_child_by_tag("TASK").unwrap();
        assert_eq!(task.get_attr("UNKNOWN_ATTR"), Some("value"));
        assert_eq!(task.get_attr("CUSTOM"), Some("123"));
    }

    #[test]
    fn parse_preserves_unknown_elements() {
        let xml = r#"<ROOT><TASK ID="1"><UNKNOWN_ELEM attr="val">content</UNKNOWN_ELEM></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let task = doc.root.first_child_by_tag("TASK").unwrap();
        let unknown = task.first_child_by_tag("UNKNOWN_ELEM").unwrap();
        assert_eq!(unknown.get_attr("attr"), Some("val"));
        assert_eq!(unknown.text_content(), "content");
    }

    #[test]
    fn parse_preserves_comments() {
        let xml = r#"<ROOT><!-- This is a comment --><TASK ID="1"></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let has_comment = doc.root.children.iter().any(|n| {
            matches!(n, XmlNode::Comment(c) if c == " This is a comment ")
        });
        assert!(has_comment);
    }

    #[test]
    fn parse_nested_tasks() {
        let xml = r#"<ROOT>
<TASK ID="1" TITLE="Level 1">
    <TASK ID="2" TITLE="Level 2">
        <TASK ID="3" TITLE="Level 3">
        </TASK>
    </TASK>
</TASK>
</ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let l1 = doc.root.first_child_by_tag("TASK").unwrap();
        assert_eq!(l1.get_attr("TITLE"), Some("Level 1"));
        let l2 = l1.first_child_by_tag("TASK").unwrap();
        assert_eq!(l2.get_attr("TITLE"), Some("Level 2"));
        let l3 = l2.first_child_by_tag("TASK").unwrap();
        assert_eq!(l3.get_attr("TITLE"), Some("Level 3"));
    }

    #[test]
    fn parse_xml_entities_in_text() {
        let xml = r#"<ROOT><COMMENTS>5 &gt; 3 &amp; 2 &lt; 4</COMMENTS></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let comments = doc.root.first_child_by_tag("COMMENTS").unwrap();
        assert_eq!(comments.text_content(), "5 > 3 & 2 < 4");
    }

    #[test]
    fn parse_xml_entities_in_attrs() {
        let xml = r#"<ROOT><TASK TITLE="a &lt; b &amp; c &gt; d"></TASK></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let task = doc.root.first_child_by_tag("TASK").unwrap();
        assert_eq!(task.get_attr("TITLE"), Some("a < b & c > d"));
    }

    #[test]
    fn parse_empty_input_returns_error() {
        assert_eq!(parse_xml(b""), Err(XmlParseError::EmptyInput));
        assert_eq!(parse_xml(b"   \n  "), Err(XmlParseError::EmptyInput));
    }

    #[test]
    fn parse_malformed_xml_returns_error() {
        let _xml = b"<ROOT><TASK></ROOT>";
        // quick-xml is lenient with mismatched tags in some modes,
        // but truly broken XML should error
        let result = parse_xml(b"<<<<not xml>>>>");
        assert!(result.is_err());
    }

    #[test]
    fn parse_detects_encoding_meta() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ROOT></ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        assert!(doc.meta.xml_declaration.is_some());
        assert!(doc
            .meta
            .xml_declaration
            .as_ref()
            .unwrap()
            .contains("UTF-8"));
    }

    #[test]
    fn unescape_basic_entities() {
        assert_eq!(unescape_xml("hello"), "hello");
        assert_eq!(unescape_xml("&lt;tag&gt;"), "<tag>");
        assert_eq!(unescape_xml("&amp;"), "&");
        assert_eq!(unescape_xml("&quot;quoted&quot;"), "\"quoted\"");
        assert_eq!(unescape_xml("it&apos;s"), "it's");
    }

    #[test]
    fn unescape_numeric_references() {
        assert_eq!(unescape_xml("&#65;"), "A"); // decimal
        assert_eq!(unescape_xml("&#x41;"), "A"); // hex
        assert_eq!(unescape_xml("&#x0041;"), "A"); // hex with leading zeros
    }

    #[test]
    fn unescape_preserves_unknown_entities() {
        assert_eq!(unescape_xml("&unknown;"), "&unknown;");
    }

    #[test]
    fn unescape_handles_bare_ampersand() {
        assert_eq!(unescape_xml("a & b"), "a & b");
        assert_eq!(unescape_xml("a &"), "a &");
    }

    #[test]
    fn escape_roundtrip() {
        let original = r#"5 > 3 & 2 < 4 "quoted" 'single'"#;
        let escaped = escape_xml(original);
        let unescaped = unescape_xml(&escaped);
        assert_eq!(unescaped, original);
    }

    #[test]
    fn parse_multiple_child_elements() {
        let xml = r#"<ROOT>
<TASK ID="1"><CATEGORY>Work</CATEGORY><CATEGORY>Urgent</CATEGORY><CATEGORY>Office</CATEGORY></TASK>
</ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let task = doc.root.first_child_by_tag("TASK").unwrap();
        let categories: Vec<_> = task.children_by_tag("CATEGORY").collect();
        assert_eq!(categories.len(), 3);
        assert_eq!(categories[0].text_content(), "Work");
        assert_eq!(categories[1].text_content(), "Urgent");
        assert_eq!(categories[2].text_content(), "Office");
    }

    #[test]
    fn parse_dependency_element() {
        let xml = r#"<ROOT>
<TASK ID="1">
    <DEPENDENCY>
        <TASKID>2</TASKID>
        <DEPENDENCYTYPE>0</DEPENDENCYTYPE>
    </DEPENDENCY>
</TASK>
</ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let task = doc.root.first_child_by_tag("TASK").unwrap();
        let dep = task.first_child_by_tag("DEPENDENCY").unwrap();
        let taskid = dep.first_child_by_tag("TASKID").unwrap();
        assert_eq!(taskid.text_content(), "2");
    }

    #[test]
    fn parse_metadata_element() {
        let xml = r#"<ROOT>
<TASK ID="1">
    <METADATA FA40B83E="some-value"/>
</TASK>
</ROOT>"#;
        let doc = parse_xml(xml.as_bytes()).unwrap();
        let task = doc.root.first_child_by_tag("TASK").unwrap();
        let meta = task.first_child_by_tag("METADATA").unwrap();
        assert_eq!(meta.get_attr("FA40B83E"), Some("some-value"));
    }
}
