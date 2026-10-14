//! XML encoding metadata for lossless round-trip preservation.
//!
//! This module defines the encoding-related metadata that must be detected when
//! reading an XML/TDL file and preserved when writing it back. The goal is to
//! ensure that the output bytes are encoding-compatible with the input bytes,
//! satisfying the Semantic Lossless requirement from the compatibility policy.
//!
//! # Supported encodings
//!
//! The ToDoList XML format uses the following encodings in practice:
//!
//! - UTF-8 without BOM (most common)
//! - UTF-8 with BOM (EF BB BF)
//! - UTF-16 LE (with or without BOM FF FE)
//! - UTF-16 BE (with or without BOM FE FF)
//!
//! # Design notes
//!
//! - `XmlEncodingMeta` is detected at parse time and carried through the entire
//!   document lifecycle. It is used by the writer to produce output bytes that
//!   match the original encoding.
//! - The original `<?xml ... ?>` declaration is preserved verbatim when present,
//!   because rewriting it may change attribute ordering, quoting style, or
//!   standalone declarations that downstream tools depend on.
//! - Line endings are preserved to avoid spurious diffs when files are edited
//!   by both ModernToDoList and other tools (including AbstractSpoon TDL).

use serde::{Deserialize, Serialize};

/// The character encoding of an XML document.
///
/// This enum covers the encodings observed in real-world ToDoList/TDL files.
/// The internal processing encoding is always UTF-8; this enum tracks what
/// the original file used so the writer can re-encode correctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum XmlEncoding {
    /// UTF-8 without Byte Order Mark.
    /// This is the most common encoding for ToDoList XML files.
    Utf8,

    /// UTF-8 with Byte Order Mark (EF BB BF).
    /// Some tools emit BOM-prefixed UTF-8 files.
    Utf8Bom,

    /// UTF-16 Little Endian.
    /// AbstractSpoon TDL sometimes produces UTF-16 LE files.
    Utf16Le,

    /// UTF-16 Big Endian.
    /// Less common but must be supported for full compatibility.
    Utf16Be,
}

impl XmlEncoding {
    /// Returns the IANA encoding label for this variant.
    ///
    /// These labels match the encoding names used in XML declarations
    /// (`<?xml version="1.0" encoding="..."?>`).
    pub fn iana_label(&self) -> &'static str {
        match self {
            XmlEncoding::Utf8 | XmlEncoding::Utf8Bom => "UTF-8",
            XmlEncoding::Utf16Le => "UTF-16LE",
            XmlEncoding::Utf16Be => "UTF-16BE",
        }
    }

    /// Returns the BOM (Byte Order Mark) bytes for this encoding, if any.
    ///
    /// - `Utf8` returns `None` (no BOM).
    /// - `Utf8Bom` returns `Some(&[0xEF, 0xBB, 0xBF])`.
    /// - `Utf16Le` returns `Some(&[0xFF, 0xFE])`.
    /// - `Utf16Be` returns `Some(&[0xFE, 0xFF])`.
    pub fn bom_bytes(&self) -> Option<&'static [u8]> {
        match self {
            XmlEncoding::Utf8 => None,
            XmlEncoding::Utf8Bom => Some(&[0xEF, 0xBB, 0xBF]),
            XmlEncoding::Utf16Le => Some(&[0xFF, 0xFE]),
            XmlEncoding::Utf16Be => Some(&[0xFE, 0xFF]),
        }
    }

    /// Returns whether this encoding uses UTF-16.
    pub fn is_utf16(&self) -> bool {
        matches!(self, XmlEncoding::Utf16Le | XmlEncoding::Utf16Be)
    }

    /// Returns whether this encoding includes a BOM.
    pub fn has_bom(&self) -> bool {
        matches!(self, XmlEncoding::Utf8Bom | XmlEncoding::Utf16Le | XmlEncoding::Utf16Be)
    }
}

impl Default for XmlEncoding {
    /// Default encoding is UTF-8 without BOM, the most common format.
    fn default() -> Self {
        XmlEncoding::Utf8
    }
}

/// The line ending style used in an XML document.
///
/// Preserving line endings prevents spurious diffs when files are round-tripped
/// through ModernToDoList and other editors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LineEnding {
    /// Unix-style line feed (`\n`).
    Lf,

    /// Windows-style carriage return + line feed (`\r\n`).
    /// This is the most common line ending for ToDoList files on Windows.
    Crlf,

    /// Classic Mac-style carriage return (`\r`).
    /// Rare but must be preserved for full compatibility.
    Cr,
}

impl LineEnding {
    /// Returns the byte sequence for this line ending.
    pub fn as_bytes(&self) -> &'static [u8] {
        match self {
            LineEnding::Lf => b"\n",
            LineEnding::Crlf => b"\r\n",
            LineEnding::Cr => b"\r",
        }
    }

    /// Returns the string representation of this line ending.
    pub fn as_str(&self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }
}

impl Default for LineEnding {
    /// Default line ending is CRLF, matching the Windows target platform.
    fn default() -> Self {
        LineEnding::Crlf
    }
}

/// Complete encoding metadata for an XML document.
///
/// This structure captures all encoding-related information needed to perform
/// a lossless round-trip: parse the document, modify it, and write it back
/// with the same encoding, BOM, XML declaration, and line endings.
///
/// # Fields
///
/// - `encoding`: The character encoding (UTF-8, UTF-8+BOM, UTF-16LE, UTF-16BE).
/// - `xml_declaration`: The original `<?xml ... ?>` declaration, if present.
///   Preserved verbatim to avoid changing attribute order, quoting, etc.
/// - `line_ending`: The dominant line ending style detected in the file.
///
/// # Lifecycle
///
/// 1. **Detection** (RD-M2-002 ~ RD-M2-005): When reading a file, the encoding
///    layer detects BOM, parses the XML declaration, and determines line endings.
/// 2. **Carrying**: The `XmlEncodingMeta` travels with the document through
///    the entire parse → edit → serialize pipeline.
/// 3. **Writing** (RD-M2-005): The serializer uses this metadata to produce
///    output bytes that match the original encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlEncodingMeta {
    /// The character encoding of the document.
    pub encoding: XmlEncoding,

    /// The original XML declaration, if present.
    ///
    /// Stored as the raw string including `<?xml` and `?>` delimiters.
    /// This preserves the exact attribute ordering, quoting style, and any
    /// extra attributes (e.g., `standalone="yes"`) that the original file used.
    ///
    /// If `None`, the document did not have an XML declaration, and the writer
    /// should not add one.
    pub xml_declaration: Option<String>,

    /// The dominant line ending style in the document.
    pub line_ending: LineEnding,
}

impl XmlEncodingMeta {
    /// Creates a new `XmlEncodingMeta` with the given encoding and default line ending.
    ///
    /// The XML declaration is set to `None` (no declaration).
    pub fn new(encoding: XmlEncoding) -> Self {
        Self {
            encoding,
            xml_declaration: None,
            line_ending: LineEnding::default(),
        }
    }

    /// Creates a default `XmlEncodingMeta` for UTF-8 without BOM and CRLF line endings.
    ///
    /// This is suitable for new documents created by ModernToDoList.
    pub fn default_utf8() -> Self {
        Self {
            encoding: XmlEncoding::Utf8,
            xml_declaration: Some(r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_string()),
            line_ending: LineEnding::Crlf,
        }
    }

    /// Sets the XML declaration.
    pub fn with_xml_declaration(mut self, declaration: Option<String>) -> Self {
        self.xml_declaration = declaration;
        self
    }

    /// Sets the line ending.
    pub fn with_line_ending(mut self, line_ending: LineEnding) -> Self {
        self.line_ending = line_ending;
        self
    }

    /// Returns the encoding label suitable for use in an XML declaration.
    pub fn encoding_label(&self) -> &'static str {
        self.encoding.iana_label()
    }
}

impl Default for XmlEncodingMeta {
    /// Default: UTF-8 without BOM, no XML declaration, CRLF line endings.
    fn default() -> Self {
        Self {
            encoding: XmlEncoding::default(),
            xml_declaration: None,
            line_ending: LineEnding::default(),
        }
    }
}

/// Result of BOM (Byte Order Mark) detection on a byte slice.
///
/// Contains the detected encoding and the number of BOM bytes consumed,
/// so the caller can advance past the BOM to the actual XML content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BomDetection {
    /// The detected encoding (including BOM variant).
    pub encoding: XmlEncoding,
    /// The number of bytes consumed by the BOM.
    /// - 0 for UTF-8 without BOM (no BOM present).
    /// - 3 for UTF-8 with BOM (EF BB BF).
    /// - 2 for UTF-16 LE (FF FE) or UTF-16 BE (FE FF).
    pub bom_len: usize,
}

/// Detects the BOM (Byte Order Mark) at the start of a byte slice.
///
/// This function examines the first few bytes of the input to determine
/// if a BOM is present and what encoding it indicates.
///
/// # Detection rules (in priority order)
///
/// 1. `EF BB BF` → UTF-8 with BOM (3 bytes consumed)
/// 2. `FF FE` → UTF-16 LE (2 bytes consumed)
/// 3. `FE FF` → UTF-16 BE (2 bytes consumed)
/// 4. No BOM detected → UTF-8 without BOM (0 bytes consumed)
///
/// Note: UTF-8 BOM is checked before UTF-16 LE because the UTF-8 BOM
/// starts with `EF BB`, which does not conflict with any UTF-16 BOM.
/// However, `FF FE` (UTF-16 LE BOM) is distinct from any UTF-8 BOM.
///
/// # Examples
///
/// ```
/// use moderntodolist_lib::domain::encoding::{detect_bom, XmlEncoding};
///
/// // UTF-8 BOM
/// let bom = detect_bom(&[0xEF, 0xBB, 0xBF, b'<', b'?']);
/// assert_eq!(bom.encoding, XmlEncoding::Utf8Bom);
/// assert_eq!(bom.bom_len, 3);
///
/// // UTF-16 LE BOM
/// let bom = detect_bom(&[0xFF, 0xFE, 0x3C, 0x00]);
/// assert_eq!(bom.encoding, XmlEncoding::Utf16Le);
/// assert_eq!(bom.bom_len, 2);
///
/// // No BOM
/// let bom = detect_bom(&[b'<', b'?', b'x', b'm']);
/// assert_eq!(bom.encoding, XmlEncoding::Utf8);
/// assert_eq!(bom.bom_len, 0);
/// ```
pub fn detect_bom(bytes: &[u8]) -> BomDetection {
    // Check UTF-8 BOM first (3 bytes: EF BB BF)
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        return BomDetection {
            encoding: XmlEncoding::Utf8Bom,
            bom_len: 3,
        };
    }

    // Check UTF-16 LE BOM (2 bytes: FF FE)
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        return BomDetection {
            encoding: XmlEncoding::Utf16Le,
            bom_len: 2,
        };
    }

    // Check UTF-16 BE BOM (2 bytes: FE FF)
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        return BomDetection {
            encoding: XmlEncoding::Utf16Be,
            bom_len: 2,
        };
    }

    // No BOM detected — default to UTF-8 without BOM
    BomDetection {
        encoding: XmlEncoding::Utf8,
        bom_len: 0,
    }
}

/// Result of XML declaration detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlDeclarationInfo {
    /// The full raw XML declaration string, e.g. `<?xml version="1.0" encoding="UTF-8"?>`.
    pub raw: String,
    /// The encoding value extracted from the `encoding` attribute, if present.
    /// This is the raw string value, e.g. `"UTF-8"`, `"utf-16"`, etc.
    pub encoding_attr: Option<String>,
    /// The byte length of the declaration in the UTF-8 input.
    pub byte_len: usize,
}

/// Detects and extracts the XML declaration (`<?xml ... ?>`) from UTF-8 input.
///
/// This function scans the beginning of the input for an XML declaration
/// without requiring a full XML parser. It extracts the raw declaration
/// string and the `encoding` attribute value if present.
///
/// # Behavior
///
/// - The declaration must start at the very beginning of the input (after BOM).
/// - The declaration ends at the first `?>` sequence.
/// - The `encoding` attribute is extracted by simple pattern matching.
/// - If no declaration is found, returns `None`.
///
/// # Limitations
///
/// - This function operates on UTF-8 input. For UTF-16 files, decode to UTF-8 first.
/// - Does not validate the full XML declaration syntax; only extracts encoding.
/// - Does not handle declarations split across multiple lines differently from single-line.
pub fn detect_xml_declaration(utf8_input: &str) -> Option<XmlDeclarationInfo> {
    let trimmed = utf8_input.trim_start();
    let leading_ws = utf8_input.len() - trimmed.len();

    // Must start with `<?xml`
    if !trimmed.starts_with("<?xml") {
        return None;
    }

    // Find the closing `?>`
    let end = trimmed.find("?>")?;
    let decl_str = &trimmed[..end + 2]; // include `?>`

    // Extract encoding attribute value
    let encoding_attr = extract_encoding_attr(decl_str);

    Some(XmlDeclarationInfo {
        raw: decl_str.to_string(),
        encoding_attr,
        byte_len: leading_ws + decl_str.len(),
    })
}

/// Extracts the `encoding` attribute value from an XML declaration string.
fn extract_encoding_attr(declaration: &str) -> Option<String> {
    // Look for `encoding` followed by `=` and a quoted value
    let lower = declaration.to_ascii_lowercase();
    let enc_pos = lower.find("encoding")?;
    let after_enc = &declaration[enc_pos + 8..]; // skip "encoding"
    let after_enc = after_enc.trim_start();

    // Must have `=`
    if !after_enc.starts_with('=') {
        return None;
    }
    let after_eq = after_enc[1..].trim_start();

    // Must have a quote character
    let quote = after_eq.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }

    let value_start = 1; // skip opening quote
    let value_end = after_eq[value_start..].find(quote)?;
    Some(after_eq[value_start..value_start + value_end].to_string())
}

/// Decodes UTF-16 bytes (with known endianness) to a UTF-8 string.
///
/// This function handles both UTF-16 LE and UTF-16 BE input.
/// The `encoding` parameter must be either `Utf16Le` or `Utf16Be`.
///
/// # Errors
///
/// Returns an error if:
/// - The encoding is not UTF-16 LE or BE.
/// - The input has an odd number of bytes (incomplete code unit).
/// - The input contains unpaired surrogates (uses lossy replacement).
///
/// # Design notes
///
/// - Unpaired surrogates are replaced with U+FFFD (replacement character)
///   rather than causing a hard failure, to maximize data recovery.
/// - The BOM bytes should already be stripped before calling this function.
pub fn decode_utf16_to_utf8(bytes: &[u8], encoding: XmlEncoding) -> Result<String, DecodeError> {
    match encoding {
        XmlEncoding::Utf16Le => decode_utf16le(bytes),
        XmlEncoding::Utf16Be => decode_utf16be(bytes),
        _ => Err(DecodeError::UnsupportedEncoding(format!(
            "{:?} is not a UTF-16 encoding",
            encoding
        ))),
    }
}

/// Errors that can occur during encoding conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The input has an odd number of bytes, making complete UTF-16 code units impossible.
    OddByteCount,
    /// The specified encoding is not a supported UTF-16 variant.
    UnsupportedEncoding(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::OddByteCount => write!(f, "UTF-16 input has odd byte count"),
            DecodeError::UnsupportedEncoding(msg) => write!(f, "unsupported encoding: {}", msg),
        }
    }
}

impl std::error::Error for DecodeError {}

fn decode_utf16le(bytes: &[u8]) -> Result<String, DecodeError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(DecodeError::OddByteCount);
    }
    let u16_vec: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    Ok(String::from_utf16_lossy(&u16_vec))
}

fn decode_utf16be(bytes: &[u8]) -> Result<String, DecodeError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(DecodeError::OddByteCount);
    }
    let u16_vec: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect();
    Ok(String::from_utf16_lossy(&u16_vec))
}

/// Detects the dominant line ending style in a UTF-8 string.
///
/// Scans the input and returns the most frequently occurring line ending.
/// If no line endings are found, returns the default (CRLF).
pub fn detect_line_ending(text: &str) -> LineEnding {
    let mut crlf_count = 0usize;
    let mut lf_count = 0usize;
    let mut cr_count = 0usize;

    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                crlf_count += 1;
                i += 2;
                continue;
            } else {
                cr_count += 1;
            }
        } else if bytes[i] == b'\n' {
            lf_count += 1;
        }
        i += 1;
    }

    if crlf_count >= lf_count && crlf_count >= cr_count {
        if crlf_count == 0 {
            LineEnding::default()
        } else {
            LineEnding::Crlf
        }
    } else if lf_count >= cr_count {
        LineEnding::Lf
    } else {
        LineEnding::Cr
    }
}

/// Resolves the encoding from an XML declaration's `encoding` attribute value.
///
/// Maps common encoding names to our `XmlEncoding` enum.
/// Returns `None` if the encoding is not recognized or not supported.
pub fn resolve_encoding_from_attr(attr_value: &str) -> Option<XmlEncoding> {
    match attr_value.to_ascii_uppercase().as_str() {
        "UTF-8" | "UTF8" => Some(XmlEncoding::Utf8),
        "UTF-16" | "UTF-16LE" | "UTF16LE" => Some(XmlEncoding::Utf16Le),
        "UTF-16BE" | "UTF16BE" => Some(XmlEncoding::Utf16Be),
        // Note: "UTF-16" without BOM typically defaults to LE on Windows
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_encoding_default_is_utf8() {
        assert_eq!(XmlEncoding::default(), XmlEncoding::Utf8);
    }

    #[test]
    fn xml_encoding_iana_labels() {
        assert_eq!(XmlEncoding::Utf8.iana_label(), "UTF-8");
        assert_eq!(XmlEncoding::Utf8Bom.iana_label(), "UTF-8");
        assert_eq!(XmlEncoding::Utf16Le.iana_label(), "UTF-16LE");
        assert_eq!(XmlEncoding::Utf16Be.iana_label(), "UTF-16BE");
    }

    #[test]
    fn xml_encoding_bom_bytes() {
        assert_eq!(XmlEncoding::Utf8.bom_bytes(), None);
        assert_eq!(XmlEncoding::Utf8Bom.bom_bytes(), Some(&[0xEF, 0xBB, 0xBF][..]));
        assert_eq!(XmlEncoding::Utf16Le.bom_bytes(), Some(&[0xFF, 0xFE][..]));
        assert_eq!(XmlEncoding::Utf16Be.bom_bytes(), Some(&[0xFE, 0xFF][..]));
    }

    #[test]
    fn xml_encoding_is_utf16() {
        assert!(!XmlEncoding::Utf8.is_utf16());
        assert!(!XmlEncoding::Utf8Bom.is_utf16());
        assert!(XmlEncoding::Utf16Le.is_utf16());
        assert!(XmlEncoding::Utf16Be.is_utf16());
    }

    #[test]
    fn xml_encoding_has_bom() {
        assert!(!XmlEncoding::Utf8.has_bom());
        assert!(XmlEncoding::Utf8Bom.has_bom());
        assert!(XmlEncoding::Utf16Le.has_bom());
        assert!(XmlEncoding::Utf16Be.has_bom());
    }

    #[test]
    fn line_ending_default_is_crlf() {
        assert_eq!(LineEnding::default(), LineEnding::Crlf);
    }

    #[test]
    fn line_ending_as_bytes() {
        assert_eq!(LineEnding::Lf.as_bytes(), b"\n");
        assert_eq!(LineEnding::Crlf.as_bytes(), b"\r\n");
        assert_eq!(LineEnding::Cr.as_bytes(), b"\r");
    }

    #[test]
    fn line_ending_as_str() {
        assert_eq!(LineEnding::Lf.as_str(), "\n");
        assert_eq!(LineEnding::Crlf.as_str(), "\r\n");
        assert_eq!(LineEnding::Cr.as_str(), "\r");
    }

    #[test]
    fn encoding_meta_default() {
        let meta = XmlEncodingMeta::default();
        assert_eq!(meta.encoding, XmlEncoding::Utf8);
        assert_eq!(meta.xml_declaration, None);
        assert_eq!(meta.line_ending, LineEnding::Crlf);
    }

    #[test]
    fn encoding_meta_default_utf8() {
        let meta = XmlEncodingMeta::default_utf8();
        assert_eq!(meta.encoding, XmlEncoding::Utf8);
        assert!(meta.xml_declaration.is_some());
        assert!(meta.xml_declaration.as_ref().unwrap().contains("UTF-8"));
        assert_eq!(meta.line_ending, LineEnding::Crlf);
    }

    #[test]
    fn encoding_meta_builder_methods() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf16Le)
            .with_xml_declaration(Some(r#"<?xml version="1.0" encoding="UTF-16"?>"#.to_string()))
            .with_line_ending(LineEnding::Lf);

        assert_eq!(meta.encoding, XmlEncoding::Utf16Le);
        assert!(meta.xml_declaration.is_some());
        assert_eq!(meta.line_ending, LineEnding::Lf);
    }

    #[test]
    fn encoding_meta_encoding_label() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf16Be);
        assert_eq!(meta.encoding_label(), "UTF-16BE");
    }

    #[test]
    fn encoding_meta_clone_and_eq() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8Bom)
            .with_xml_declaration(Some(r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_string()))
            .with_line_ending(LineEnding::Crlf);

        let cloned = meta.clone();
        assert_eq!(meta, cloned);
    }

    #[test]
    fn xml_encoding_serde_roundtrip() {
        let encoding = XmlEncoding::Utf16Le;
        let json = serde_json::to_string(&encoding).unwrap();
        let decoded: XmlEncoding = serde_json::from_str(&json).unwrap();
        assert_eq!(encoding, decoded);
    }

    #[test]
    fn line_ending_serde_roundtrip() {
        let le = LineEnding::Lf;
        let json = serde_json::to_string(&le).unwrap();
        let decoded: LineEnding = serde_json::from_str(&json).unwrap();
        assert_eq!(le, decoded);
    }

    #[test]
    fn encoding_meta_serde_roundtrip() {
        let meta = XmlEncodingMeta::new(XmlEncoding::Utf8Bom)
            .with_xml_declaration(Some(r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_string()))
            .with_line_ending(LineEnding::Lf);

        let json = serde_json::to_string(&meta).unwrap();
        let decoded: XmlEncodingMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, decoded);
    }

    // === BOM detection tests (RD-M2-002) ===

    #[test]
    fn detect_bom_utf8() {
        let bytes = [0xEF, 0xBB, 0xBF, b'<', b'?', b'x', b'm'];
        let result = detect_bom(&bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf8Bom);
        assert_eq!(result.bom_len, 3);
    }

    #[test]
    fn detect_bom_utf16_le() {
        let bytes = [0xFF, 0xFE, 0x3C, 0x00, 0x3F, 0x00];
        let result = detect_bom(&bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf16Le);
        assert_eq!(result.bom_len, 2);
    }

    #[test]
    fn detect_bom_utf16_be() {
        let bytes = [0xFE, 0xFF, 0x00, 0x3C, 0x00, 0x3F];
        let result = detect_bom(&bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf16Be);
        assert_eq!(result.bom_len, 2);
    }

    #[test]
    fn detect_bom_none_utf8_no_bom() {
        let bytes = *b"<?xml";
        let result = detect_bom(&bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf8);
        assert_eq!(result.bom_len, 0);
    }

    #[test]
    fn detect_bom_empty_slice() {
        let bytes: &[u8] = &[];
        let result = detect_bom(bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf8);
        assert_eq!(result.bom_len, 0);
    }

    #[test]
    fn detect_bom_single_byte() {
        let bytes = [0xEF];
        let result = detect_bom(&bytes);
        // Single byte is not enough for any BOM
        assert_eq!(result.encoding, XmlEncoding::Utf8);
        assert_eq!(result.bom_len, 0);
    }

    #[test]
    fn detect_bom_two_bytes_partial_utf8_bom() {
        // EF BB is not a complete UTF-8 BOM (needs EF BB BF)
        let bytes = [0xEF, 0xBB];
        let result = detect_bom(&bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf8);
        assert_eq!(result.bom_len, 0);
    }

    #[test]
    fn detect_bom_exact_utf8_bom() {
        let bytes = [0xEF, 0xBB, 0xBF];
        let result = detect_bom(&bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf8Bom);
        assert_eq!(result.bom_len, 3);
    }

    #[test]
    fn detect_bom_exact_utf16_le_bom() {
        let bytes = [0xFF, 0xFE];
        let result = detect_bom(&bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf16Le);
        assert_eq!(result.bom_len, 2);
    }

    #[test]
    fn detect_bom_exact_utf16_be_bom() {
        let bytes = [0xFE, 0xFF];
        let result = detect_bom(&bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf16Be);
        assert_eq!(result.bom_len, 2);
    }

    #[test]
    fn detect_bom_priority_utf8_before_utf16le() {
        // EF BB BF starts with EF, which is not FF, so no conflict.
        // But verify the detection is correct.
        let bytes = [0xEF, 0xBB, 0xBF, 0xFF, 0xFE];
        let result = detect_bom(&bytes);
        assert_eq!(result.encoding, XmlEncoding::Utf8Bom);
        assert_eq!(result.bom_len, 3);
    }

    // === XML declaration detection tests (RD-M2-003) ===

    #[test]
    fn detect_xml_decl_basic() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?><TODOLIST/>"#;
        let result = detect_xml_declaration(input).unwrap();
        assert_eq!(result.raw, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        assert_eq!(result.encoding_attr.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn detect_xml_decl_utf16_encoding() {
        let input = r#"<?xml version="1.0" encoding="UTF-16"?><TODOLIST/>"#;
        let result = detect_xml_declaration(input).unwrap();
        assert_eq!(result.encoding_attr.as_deref(), Some("UTF-16"));
    }

    #[test]
    fn detect_xml_decl_no_encoding_attr() {
        let input = r#"<?xml version="1.0"?><TODOLIST/>"#;
        let result = detect_xml_declaration(input).unwrap();
        assert!(result.encoding_attr.is_none());
    }

    #[test]
    fn detect_xml_decl_single_quotes() {
        let input = "<?xml version='1.0' encoding='UTF-8'?><TODOLIST/>";
        let result = detect_xml_declaration(input).unwrap();
        assert_eq!(result.encoding_attr.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn detect_xml_decl_no_declaration() {
        let input = "<TODOLIST><TASK ID=\"1\"/></TODOLIST>";
        assert!(detect_xml_declaration(input).is_none());
    }

    #[test]
    fn detect_xml_decl_empty_input() {
        assert!(detect_xml_declaration("").is_none());
    }

    #[test]
    fn detect_xml_decl_with_leading_whitespace() {
        let input = r#"  <?xml version="1.0" encoding="UTF-8"?><TODOLIST/>"#;
        let result = detect_xml_declaration(input).unwrap();
        assert_eq!(result.encoding_attr.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn detect_xml_decl_case_insensitive_encoding() {
        let input = r#"<?xml version="1.0" encoding="utf-8"?><TODOLIST/>"#;
        let result = detect_xml_declaration(input).unwrap();
        assert_eq!(result.encoding_attr.as_deref(), Some("utf-8"));
    }

    // === UTF-16 decode tests (RD-M2-004) ===

    #[test]
    fn decode_utf16le_basic_ascii() {
        let bytes = [0x48, 0x00, 0x65, 0x00, 0x6C, 0x00, 0x6C, 0x00, 0x6F, 0x00];
        let result = decode_utf16_to_utf8(&bytes, XmlEncoding::Utf16Le).unwrap();
        assert_eq!(result, "Hello");
    }

    #[test]
    fn decode_utf16be_basic_ascii() {
        let bytes = [0x00, 0x48, 0x00, 0x65, 0x00, 0x6C, 0x00, 0x6C, 0x00, 0x6F];
        let result = decode_utf16_to_utf8(&bytes, XmlEncoding::Utf16Be).unwrap();
        assert_eq!(result, "Hello");
    }

    #[test]
    fn decode_utf16le_xml_declaration() {
        let bytes = [0x3C, 0x00, 0x3F, 0x00, 0x78, 0x00, 0x6D, 0x00, 0x6C, 0x00];
        let result = decode_utf16_to_utf8(&bytes, XmlEncoding::Utf16Le).unwrap();
        assert_eq!(result, "<?xml");
    }

    #[test]
    fn decode_utf16_odd_byte_count() {
        let bytes = [0x48, 0x00, 0x65];
        let result = decode_utf16_to_utf8(&bytes, XmlEncoding::Utf16Le);
        assert_eq!(result, Err(DecodeError::OddByteCount));
    }

    #[test]
    fn decode_utf16_unsupported_encoding() {
        let bytes = [0x48, 0x00];
        let result = decode_utf16_to_utf8(&bytes, XmlEncoding::Utf8);
        assert!(matches!(result, Err(DecodeError::UnsupportedEncoding(_))));
    }

    #[test]
    fn decode_utf16le_empty() {
        let bytes: &[u8] = &[];
        let result = decode_utf16_to_utf8(bytes, XmlEncoding::Utf16Le).unwrap();
        assert_eq!(result, "");
    }

    // === Line ending detection tests ===

    #[test]
    fn detect_line_ending_crlf() {
        assert_eq!(detect_line_ending("hello\r\nworld\r\n"), LineEnding::Crlf);
    }

    #[test]
    fn detect_line_ending_lf() {
        assert_eq!(detect_line_ending("hello\nworld\n"), LineEnding::Lf);
    }

    #[test]
    fn detect_line_ending_cr() {
        assert_eq!(detect_line_ending("hello\rworld\r"), LineEnding::Cr);
    }

    #[test]
    fn detect_line_ending_no_newlines() {
        assert_eq!(detect_line_ending("hello world"), LineEnding::default());
    }

    // === Encoding attribute resolution tests ===

    #[test]
    fn resolve_encoding_utf8() {
        assert_eq!(resolve_encoding_from_attr("UTF-8"), Some(XmlEncoding::Utf8));
        assert_eq!(resolve_encoding_from_attr("utf-8"), Some(XmlEncoding::Utf8));
    }

    #[test]
    fn resolve_encoding_utf16() {
        assert_eq!(resolve_encoding_from_attr("UTF-16"), Some(XmlEncoding::Utf16Le));
        assert_eq!(resolve_encoding_from_attr("UTF-16LE"), Some(XmlEncoding::Utf16Le));
        assert_eq!(resolve_encoding_from_attr("UTF-16BE"), Some(XmlEncoding::Utf16Be));
    }

    #[test]
    fn resolve_encoding_unknown() {
        assert_eq!(resolve_encoding_from_attr("ISO-8859-1"), None);
        assert_eq!(resolve_encoding_from_attr("Shift_JIS"), None);
    }
}
