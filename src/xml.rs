//! XML helpers for documents of unknown encoding (FB2, OPF, container.xml).
//!
//! quick-xml's [`DecodingReader`] detects the encoding from a 64-byte prefix and
//! panics in [`set_encoding`] when the XML declaration does not fit in that
//! prefix — UTF-16 documents (2 bytes per character) and UTF-8-BOM documents
//! with a long declaration always overflow it. The scanner used to call
//! `set_encoding()` from its `Event::Decl` handler, so a single legacy-encoded
//! book aborted the whole scan (issue #25).
//!
//! [`decode`] replaces that: the encoding is determined up front from the BOM
//! and the encoding declaration, the document is transcoded to UTF-8 once, and
//! parsing then runs over plain UTF-8 with no encoding state to mutate.
//!
//! [`set_encoding`]: quick_xml::encoding::DecodingReader::set_encoding

use std::borrow::Cow;

use encoding_rs::Encoding;
use quick_xml::encoding::DetectedEncoding;
use quick_xml::reader::Reader;

/// Bytes examined when sniffing the `encoding="..."` declaration.
const DECL_SNIFF_WINDOW: usize = 1024;

/// Transcode an XML document of unknown encoding into UTF-8.
///
/// Malformed bytes are replaced with U+FFFD rather than aborting the parse, so
/// a legacy file with a few broken characters still yields its metadata.
pub fn decode(bytes: &[u8]) -> Cow<'_, str> {
    let (encoding, bom_len) = detect_encoding(bytes);
    encoding.decode_without_bom_handling(&bytes[bom_len..]).0
}

/// Create a quick-xml reader over a document already transcoded by [`decode`].
pub fn reader(text: &str) -> Reader<&[u8]> {
    Reader::from_str(text)
}

/// Pick the encoding of a document and the length of its BOM.
///
/// Precedence:
/// 1. A UTF-16 BOM — unambiguous, and the declaration can only say "UTF-16"
///    without naming the byte order.
/// 2. The `encoding="..."` declaration — outranks a UTF-8 BOM, because legacy
///    FB2 files routinely carry a UTF-8 BOM in front of a windows-1251
///    declaration.
/// 3. The BOM / NUL-interleaved `<?x` pattern, per the W3C guessing algorithm.
/// 4. UTF-8.
fn detect_encoding(bytes: &[u8]) -> (&'static Encoding, usize) {
    let detected = quick_xml::encoding::detect_encoding(bytes);

    // A UTF-16 BOM is unambiguous and outranks the declaration, which can only
    // say "UTF-16" and cannot name the byte order.
    match detected {
        Some(DetectedEncoding::Utf16BeBom) => return (encoding_rs::UTF_16BE, 2),
        Some(DetectedEncoding::Utf16LeBom) => return (encoding_rs::UTF_16LE, 2),
        _ => {}
    }

    // The declaration outranks a UTF-8 BOM: legacy FB2 files routinely carry a
    // UTF-8 BOM in front of a windows-1251 declaration.
    if let Some(declared) = declared_encoding(bytes) {
        // That BOM is file noise, not text: drop it so it does not decode into
        // "ï»¿" ahead of the root element.
        return (declared, utf8_bom_len(bytes));
    }

    // Fall back to the BOM / NUL-interleaved "<?x" pattern, then to UTF-8.
    match detected {
        Some(DetectedEncoding::Utf8Bom) => (encoding_rs::UTF_8, 3),
        Some(DetectedEncoding::Utf16BeLike) => (encoding_rs::UTF_16BE, 0),
        Some(DetectedEncoding::Utf16LeLike) => (encoding_rs::UTF_16LE, 0),
        _ => (encoding_rs::UTF_8, 0),
    }
}

/// Read the `encoding="..."` attribute of the XML declaration, if any.
///
/// Returns `None` for UTF-8 (the default) and for documents whose declaration
/// is not byte-searchable (UTF-16 without a BOM), leaving BOM/pattern
/// detection to decide.
fn declared_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    let window = &bytes[..bytes.len().min(DECL_SNIFF_WINDOW)];
    // Look inside the declaration only: an `encoding="..."` attribute deeper in
    // the document (e.g. an OPF meta tag) is not an encoding declaration.
    let decl_end = find(window, b"?>").map_or(window.len(), |i| i + 2);
    let decl = &window[..decl_end];

    let key = find(decl, b"encoding")?;
    let mut pos = key + b"encoding".len();

    pos = skip_spaces(decl, pos);
    if decl.get(pos) != Some(&b'=') {
        return None;
    }
    pos = skip_spaces(decl, pos + 1);

    let quote = *decl.get(pos)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let label_start = pos + 1;
    let label_end = label_start + find(&decl[label_start..], &[quote])?;

    let encoding = encoding_rs::Encoding::for_label(&decl[label_start..label_end])?;
    // UTF-8 is also the fallback encoding, so a UTF-8 label needs no override.
    (encoding != encoding_rs::UTF_8).then_some(encoding)
}

/// Length of a leading UTF-8 BOM, which is stripped before legacy decoding.
fn utf8_bom_len(bytes: &[u8]) -> usize {
    usize::from(bytes.starts_with(&[0xEF, 0xBB, 0xBF])) * 3
}

fn skip_spaces(bytes: &[u8], mut pos: usize) -> usize {
    while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    pos
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(s: &str, bom: bool) -> Vec<u8> {
        let mut out = if bom { vec![0xFF, 0xFE] } else { vec![] };
        for u in s.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    fn utf16be(s: &str, bom: bool) -> Vec<u8> {
        let mut out = if bom { vec![0xFE, 0xFF] } else { vec![] };
        for u in s.encode_utf16() {
            out.extend_from_slice(&u.to_be_bytes());
        }
        out
    }

    /// The declaration of a UTF-16 document is twice as long as its UTF-8
    /// equivalent, so it never fits quick-xml's 64-byte detection prefix.
    #[test]
    fn test_decode_utf16_with_bom() {
        let doc = r#"<?xml version="1.0" encoding="UTF-16"?>
<root><title>Основание</title></root>"#;
        assert_eq!(decode(&utf16le(doc, true)), doc);
        // A big-endian document declaring the label "UTF-16" (which encoding_rs
        // reads as little-endian) must follow the BOM, not the label.
        assert_eq!(decode(&utf16be(doc, true)), doc);
    }

    #[test]
    fn test_decode_utf16_without_bom() {
        let doc = r#"<?xml version="1.0" encoding="UTF-16"?>
<root><title>Основание</title></root>"#;
        assert_eq!(decode(&utf16le(doc, false)), doc);
        assert_eq!(decode(&utf16be(doc, false)), doc);
    }

    /// Legacy FB2 files commonly pair a UTF-8 BOM with a windows-1251 body;
    /// the declaration has to win over the BOM.
    #[test]
    fn test_decode_utf8_bom_with_windows1251_declaration() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(
            b"<?xml version=\"1.0\" encoding=\"windows-1251\" standalone=\"yes\"?>\n<root>",
        );
        bytes.extend_from_slice(&[0xD2, 0xE5, 0xF1, 0xF2]); // "Тест"
        bytes.extend_from_slice(b"</root>");

        let text = decode(&bytes);
        assert_eq!(
            text,
            "<?xml version=\"1.0\" encoding=\"windows-1251\" standalone=\"yes\"?>\n<root>Тест</root>"
        );
    }

    #[test]
    fn test_decode_windows1251_without_bom() {
        let mut bytes = b"<?xml version=\"1.0\" encoding=\"windows-1251\"?>\n<root>".to_vec();
        bytes.extend_from_slice(&[0xD2, 0xE5, 0xF1, 0xF2]); // "Тест"
        bytes.extend_from_slice(b"</root>");
        assert!(decode(&bytes).ends_with("<root>Тест</root>"));
    }

    /// A few undecodable bytes must not cost the whole document.
    #[test]
    fn test_decode_replaces_malformed_bytes() {
        let mut bytes = b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<root>".to_vec();
        bytes.extend_from_slice(&[0xD0, 0x94]); // "Д"
        bytes.extend_from_slice(&[0xFF]); // not valid UTF-8 in any position
        bytes.extend_from_slice(b"</root>");

        let text = decode(&bytes);
        assert!(text.contains('Д'));
        assert!(text.contains('\u{FFFD}'));
        assert!(text.ends_with("</root>"));
    }

    #[test]
    fn test_decode_plain_utf8_is_borrowed() {
        let doc = r#"<?xml version="1.0" encoding="utf-8"?><root>Тест</root>"#;
        assert!(matches!(decode(doc.as_bytes()), Cow::Borrowed(_)));
    }

    #[test]
    fn test_decode_utf8_bom_is_stripped() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"<root>ok</root>");
        assert_eq!(decode(&bytes), "<root>ok</root>");
    }

    /// An `encoding="..."` attribute outside the declaration must not be taken
    /// for an encoding declaration.
    #[test]
    fn test_ignores_encoding_attribute_outside_declaration() {
        let doc = r#"<root encoding="windows-1251"><a href="x"/></root>"#;
        assert!(matches!(decode(doc.as_bytes()), Cow::Borrowed(_)));
    }

    #[test]
    fn test_unknown_declaration_falls_back_to_utf8() {
        let doc = r#"<?xml version="1.0" encoding="x-made-up-encoding"?><root>Тест</root>"#;
        assert_eq!(decode(doc.as_bytes()), doc);
    }

    #[test]
    fn test_reader_parses_decoded_document() {
        use quick_xml::events::Event;

        let mut bytes = b"<?xml version=\"1.0\" encoding=\"windows-1251\"?>\n<root>".to_vec();
        bytes.extend_from_slice(&[0xD2, 0xE5, 0xF1, 0xF2]);
        bytes.extend_from_slice(b"</root>");

        let text = decode(&bytes);
        let mut xml = reader(&text);
        let mut found = String::new();
        loop {
            match xml.read_event() {
                Ok(Event::Text(e)) => found = e.xml10_content().into_owned(),
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        assert_eq!(found, "Тест");
    }
}
