//! Lossless XML tree model for ModernToDoList 2.0.
//!
//! This module defines the XML tree types that preserve all content from the original
//! document, including unknown attributes, unknown child elements, whitespace, comments,
//! and processing instructions. The tree can be serialized back to produce output that
//! is semantically equivalent to the original input.
//!
//! # Design
//!
//! - `XmlDocument` is the root container, holding encoding metadata and the root element.
//! - `XmlNode` represents any node in the XML tree (element, text, comment, etc.).
//! - `XmlAttribute` stores a name-value pair, preserving the original attribute order.
//! - Unknown content is preserved by design: the parser captures everything, and the
//!   serializer writes everything back without modification.

use serde::{Deserialize, Serialize};

use super::encoding::XmlEncodingMeta;

/// A complete XML document with encoding metadata and a root element.
///
/// This is the top-level container for the lossless XML tree. It carries
/// the encoding metadata (encoding, XML declaration, line endings) that
/// was detected when the document was read, and uses it when serializing
/// back to bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XmlDocument {
    /// Encoding metadata (encoding, XML declaration, line endings).
    pub meta: XmlEncodingMeta,
    /// The root element (typically `<TODOLIST>`).
    pub root: XmlElement,
}

/// An XML element with tag name, attributes, and child nodes.
///
/// Attributes are stored in their original order. Child nodes include
/// both element children and text/whitespace nodes, preserving the
/// original document structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XmlElement {
    /// The element tag name (e.g., "TODOLIST", "TASK", "FILEREFPATH").
    pub tag: String,
    /// Attributes in their original order.
    pub attrs: Vec<XmlAttribute>,
    /// Child nodes (elements, text, comments, CDATA, processing instructions).
    pub children: Vec<XmlNode>,
}

/// An XML attribute name-value pair.
///
/// Both name and value are stored as strings, preserving the original
/// attribute names and values exactly as they appeared in the input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlAttribute {
    /// The attribute name (e.g., "ID", "TITLE", "PRIORITY").
    pub name: String,
    /// The attribute value (after XML entity decoding).
    pub value: String,
}

/// A node in the XML tree.
///
/// This enum covers all XML node types that can appear in a document.
/// Unknown node types are preserved as `Unparsed` to ensure lossless
/// round-tripping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum XmlNode {
    /// An XML element with tag name, attributes, and children.
    Element(XmlElement),
    /// Text content (including whitespace between elements).
    Text(String),
    /// An XML comment (`<!-- ... -->`).
    Comment(String),
    /// A CDATA section (`<![CDATA[ ... ]]>`).
    CData(String),
    /// A processing instruction (`<? ... ?>`, excluding the XML declaration).
    ProcessingInstruction(String),
}

impl XmlElement {
    /// Creates a new element with the given tag name and no attributes or children.
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            attrs: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Gets the value of an attribute by name.
    pub fn get_attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }

    /// Sets an attribute value, or adds it if it doesn't exist.
    ///
    /// If the attribute exists, its value is updated in place.
    /// If not, a new attribute is appended to the end.
    pub fn set_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        if let Some(attr) = self.attrs.iter_mut().find(|a| a.name == name) {
            attr.value = value;
        } else {
            self.attrs.push(XmlAttribute { name, value });
        }
    }

    /// Removes an attribute by name. Returns true if it was found and removed.
    pub fn remove_attr(&mut self, name: &str) -> bool {
        let len_before = self.attrs.len();
        self.attrs.retain(|a| a.name != name);
        self.attrs.len() < len_before
    }

    /// Returns only the element children (skipping text, comment, etc. nodes).
    pub fn child_elements(&self) -> impl Iterator<Item = &XmlElement> {
        self.children.iter().filter_map(|n| match n {
            XmlNode::Element(e) => Some(e),
            _ => None,
        })
    }

    /// Returns only the element children with a specific tag name.
    pub fn children_by_tag<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a XmlElement> + 'a {
        self.child_elements().filter(move |e| e.tag == tag)
    }

    /// Returns the first child element with the given tag name.
    pub fn first_child_by_tag(&self, tag: &str) -> Option<&XmlElement> {
        self.child_elements().find(|e| e.tag == tag)
    }

    /// Returns the concatenated text content of this element's text children.
    ///
    /// This collects all `XmlNode::Text` children and concatenates them.
    /// For elements like `<COMMENTS>text</COMMENTS>`, this returns "text".
    pub fn text_content(&self) -> String {
        let mut result = String::new();
        for child in &self.children {
            if let XmlNode::Text(text) = child {
                result.push_str(text);
            }
        }
        result
    }

    /// Sets the text content of this element, replacing any existing text children.
    ///
    /// Non-text children (sub-elements, comments) are preserved.
    /// A single text node is appended at the end.
    pub fn set_text_content(&mut self, text: impl Into<String>) {
        // Remove existing text nodes
        self.children.retain(|n| !matches!(n, XmlNode::Text(_)));
        let text = text.into();
        if !text.is_empty() {
            self.children.push(XmlNode::Text(text));
        }
    }

    /// Returns true if this element has no children.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl XmlDocument {
    /// Creates a new document with the given encoding metadata and root element.
    pub fn new(meta: XmlEncodingMeta, root: XmlElement) -> Self {
        Self { meta, root }
    }

    /// Returns the TODOLIST root attributes (project-level metadata).
    pub fn root_attrs(&self) -> &[XmlAttribute] {
        &self.root.attrs
    }

    /// Gets a root-level attribute value by name.
    pub fn get_root_attr(&self, name: &str) -> Option<&str> {
        self.root.get_attr(name)
    }

    /// Sets a root-level attribute value.
    pub fn set_root_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.root.set_attr(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_get_set_attr() {
        let mut elem = XmlElement::new("TASK");
        assert_eq!(elem.get_attr("ID"), None);

        elem.set_attr("ID", "42");
        assert_eq!(elem.get_attr("ID"), Some("42"));

        elem.set_attr("ID", "99");
        assert_eq!(elem.get_attr("ID"), Some("99"));
        assert_eq!(elem.attrs.len(), 1); // Updated in place
    }

    #[test]
    fn element_remove_attr() {
        let mut elem = XmlElement::new("TASK");
        elem.set_attr("ID", "1");
        elem.set_attr("TITLE", "Test");
        assert!(elem.remove_attr("ID"));
        assert_eq!(elem.get_attr("ID"), None);
        assert_eq!(elem.get_attr("TITLE"), Some("Test"));
        assert!(!elem.remove_attr("NONEXISTENT"));
    }

    #[test]
    fn element_children_by_tag() {
        let mut root = XmlElement::new("TODOLIST");
        root.children.push(XmlNode::Text("\n    ".into()));
        let mut task1 = XmlElement::new("TASK");
        task1.set_attr("ID", "1");
        root.children.push(XmlNode::Element(task1));
        root.children.push(XmlNode::Text("\n    ".into()));
        let mut task2 = XmlElement::new("TASK");
        task2.set_attr("ID", "2");
        root.children.push(XmlNode::Element(task2));

        let tasks: Vec<_> = root.children_by_tag("TASK").collect();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].get_attr("ID"), Some("1"));
        assert_eq!(tasks[1].get_attr("ID"), Some("2"));
    }

    #[test]
    fn element_text_content() {
        let mut elem = XmlElement::new("COMMENTS");
        elem.children
            .push(XmlNode::Text("Hello world".into()));
        assert_eq!(elem.text_content(), "Hello world");

        elem.set_text_content("New comment");
        assert_eq!(elem.text_content(), "New comment");
        assert_eq!(elem.children.len(), 1); // Only one text node
    }

    #[test]
    fn element_set_text_preserves_non_text_children() {
        let mut elem = XmlElement::new("TASK");
        elem.children
            .push(XmlNode::Text("trailing".into()));
        let sub = XmlElement::new("FILEREFPATH");
        elem.children.push(XmlNode::Element(sub));

        elem.set_text_content("new text");
        // Text node replaced, element child preserved
        assert_eq!(elem.children.len(), 2);
        assert!(elem.children.iter().any(|n| matches!(n, XmlNode::Element(e) if e.tag == "FILEREFPATH")));
    }

    #[test]
    fn xml_document_creation() {
        use crate::domain::encoding::{LineEnding, XmlEncoding};
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8);
        let root = XmlElement::new("TODOLIST");
        let doc = XmlDocument::new(meta, root);
        assert_eq!(doc.root.tag, "TODOLIST");
        assert_eq!(doc.meta.encoding, XmlEncoding::Utf8);
        assert_eq!(doc.meta.line_ending, LineEnding::Crlf);
    }
}
