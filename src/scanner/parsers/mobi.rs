use std::io;

use super::{BookMeta, strip_meta};

/// Parse MOBI metadata from a reader.
pub fn parse<R: io::Read>(reader: R) -> Result<BookMeta, mobi::MobiError> {
    let mobi = mobi::Mobi::from_read(reader)?;
    Ok(extract_meta(&mobi))
}

/// Parse MOBI metadata from in-memory bytes (for ZIP archives).
pub fn parse_bytes(data: &[u8]) -> Result<BookMeta, mobi::MobiError> {
    let mobi = mobi::Mobi::new(data.to_vec())?;
    Ok(extract_meta(&mobi))
}

fn extract_meta(mobi: &mobi::Mobi) -> BookMeta {
    let title = strip_meta(&mobi.title());

    let authors: Vec<String> = mobi
        .author()
        .map(|a| {
            a.split(&['&', ';'][..])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let annotation = mobi
        .description()
        .map(|d| strip_html_tags(&d))
        .unwrap_or_default();

    let lang = language_to_code(mobi.language());

    let docdate = mobi.publish_date().unwrap_or_default();

    let (cover_data, cover_type) = extract_cover(mobi);

    BookMeta {
        title,
        authors,
        annotation,
        lang,
        docdate,
        cover_data,
        cover_type,
        ..Default::default()
    }
}

/// Extract the cover image from MOBI records using EXTH CoverOffset metadata.
/// Falls back to the first image record if no CoverOffset is present.
fn extract_cover(mobi: &mobi::Mobi) -> (Option<Vec<u8>>, String) {
    use mobi::headers::ExthRecord;

    let raw_records = mobi.raw_records();
    let first_image = mobi.metadata.mobi.first_image_index as usize;

    // Try EXTH CoverOffset: value is added to first_image_index to get the PDB record index.
    let cover_offset = mobi
        .metadata
        .exth_record(ExthRecord::CoverOffset)
        .and_then(|records| records.first())
        .and_then(|data| {
            if data.len() >= 4 {
                Some(u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize)
            } else {
                None
            }
        });

    if let Some(offset) = cover_offset {
        let record_idx = first_image + offset;
        let all = raw_records.range(0..);
        if let Some(record) = all.get(record_idx)
            && let Some(result) = try_image_data(record.content)
        {
            return result;
        }
    }

    // Fallback: first image record from the filtered list.
    let images = mobi.image_records();
    if let Some(record) = images.first()
        && let Some(result) = try_image_data(record.content)
    {
        return result;
    }

    (None, String::new())
}

/// Try to interpret raw bytes as a cover image; returns Some if valid image data.
fn try_image_data(data: &[u8]) -> Option<(Option<Vec<u8>>, String)> {
    if data.len() > 4 {
        let mime = detect_image_mime(data);
        if mime != "application/octet-stream" {
            return Some((Some(data.to_vec()), mime.to_string()));
        }
    }
    None
}

/// Detect image MIME type from magic bytes.
fn detect_image_mime(data: &[u8]) -> &'static str {
    if data.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if data.starts_with(b"GIF") {
        "image/gif"
    } else {
        "application/octet-stream"
    }
}

/// Strip HTML tags from a string, keeping only text content.
fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut inside_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => result.push(ch),
            _ => {}
        }
    }
    result.trim().to_string()
}

/// Map the `mobi::headers::Language` enum to an ISO 639-1 language code.
fn language_to_code(lang: mobi::headers::Language) -> String {
    use mobi::headers::Language::*;
    match lang {
        English => "en",
        Russian => "ru",
        German => "de",
        French => "fr",
        Spanish => "es",
        Italian => "it",
        Portuguese => "pt",
        Dutch => "nl",
        Swedish => "sv",
        Norwegian => "no",
        Danish => "da",
        Finnish => "fi",
        Polish => "pl",
        Czech => "cs",
        Hungarian => "hu",
        Romanian => "ro",
        Bulgarian => "bg",
        Serbian => "sr",
        Ukrainian => "uk",
        Belarusian => "be",
        Turkish => "tr",
        Greek => "el",
        Arabic => "ar",
        Hebrew => "he",
        Chinese => "zh",
        Japanese => "ja",
        Korean => "ko",
        Hindi => "hi",
        Thai => "th",
        Vietnamese => "vi",
        Indonesian => "id",
        Albanian => "sq",
        Catalan => "ca",
        Estonian => "et",
        Icelandic => "is",
        Latvian => "lv",
        Lithuanian => "lt",
        Macedonian => "mk",
        Malay => "ms",
        Slovak => "sk",
        Slovenian => "sl",
        _ => "",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MOBI test fixture (Lord of the Rings, from the mobi crate's own test data).
    const TEST_MOBI: &[u8] = include_bytes!("../../../tests/data/test_book.mobi");

    #[test]
    fn test_parse_bytes_metadata() {
        let meta = parse_bytes(TEST_MOBI).expect("should parse test MOBI");
        assert_eq!(meta.title, "Test MOBI Book");
        assert_eq!(meta.authors, vec!["J. R. R. Tolkien".to_string()]);
        assert!(
            meta.annotation.contains("extraordinary book"),
            "annotation should contain review text, got: {}",
            &meta.annotation[..80.min(meta.annotation.len())]
        );
        assert_eq!(meta.docdate, "2024-01-15");
        // No genres or series in MOBI format
        assert!(meta.genres.is_empty());
        assert!(meta.series_title.is_none());
    }

    #[test]
    fn test_parse_bytes_cover() {
        let meta = parse_bytes(TEST_MOBI).expect("should parse test MOBI");
        assert!(meta.cover_data.is_some(), "should have cover data");
        assert_eq!(meta.cover_type, "image/jpeg");
    }

    #[test]
    fn test_parse_reader() {
        let meta = parse(std::io::Cursor::new(TEST_MOBI)).expect("should parse via reader");
        assert_eq!(meta.title, "Test MOBI Book");
        assert_eq!(meta.authors, vec!["J. R. R. Tolkien".to_string()]);
    }

    #[test]
    fn test_parse_bytes_invalid_data() {
        let result = parse_bytes(b"not a mobi file");
        assert!(result.is_err(), "should fail on invalid data");
    }

    #[test]
    fn test_author_splitting() {
        // Verify the splitting logic by testing extract_meta through parse_bytes.
        // The test fixture has a single author, so test the split path directly:
        let input = "Author One & Author Two ; Author Three";
        let authors: Vec<String> = input
            .split(&['&', ';'][..])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        assert_eq!(
            authors,
            vec![
                "Author One".to_string(),
                "Author Two".to_string(),
                "Author Three".to_string()
            ]
        );
    }

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<p>Hello</p>"), "Hello");
        assert_eq!(
            strip_html_tags("<b>Bold</b> and <i>italic</i>"),
            "Bold and italic"
        );
        assert_eq!(strip_html_tags("No tags here"), "No tags here");
        assert_eq!(strip_html_tags(""), "");
    }

    #[test]
    fn test_detect_image_mime() {
        assert_eq!(detect_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), "image/jpeg");
        assert_eq!(
            detect_image_mime(&[0x89, 0x50, 0x4E, 0x47, 0x0D]),
            "image/png"
        );
        assert_eq!(detect_image_mime(b"GIF89a"), "image/gif");
        assert_eq!(
            detect_image_mime(&[0x00, 0x00, 0x00, 0x00]),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_language_to_code() {
        use mobi::headers::Language;
        assert_eq!(language_to_code(Language::English), "en");
        assert_eq!(language_to_code(Language::Russian), "ru");
        assert_eq!(language_to_code(Language::Unknown), "");
    }
}
