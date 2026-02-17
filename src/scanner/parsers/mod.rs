pub mod epub;
pub mod fb2;
pub mod inpx;

/// Metadata extracted from a single book file.
#[derive(Debug, Clone, Default)]
pub struct BookMeta {
    pub title: String,
    pub authors: Vec<String>,
    pub genres: Vec<String>,
    pub annotation: String,
    pub lang: String,
    pub series_title: Option<String>,
    pub series_index: i32,
    pub docdate: String,
    /// Raw cover image bytes (JPEG/PNG), if found.
    pub cover_data: Option<Vec<u8>>,
    /// MIME type of the cover image (e.g. "image/jpeg").
    pub cover_type: String,
}

/// Strip leading/trailing whitespace and common punctuation from metadata strings.
/// Mirrors the Python `strip_symbols` constant.
pub fn strip_meta(s: &str) -> String {
    s.trim_matches(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '»' | '«' | '\'' | '"' | '&' | '-' | '.' | '#' | '\\' | '`' | ';'
            )
    })
    .to_string()
}

/// Determine the `lang_code` for a string by inspecting its first character.
///   1 = Cyrillic, 2 = Latin, 3 = Digit, 9 = Other
pub fn detect_lang_code(s: &str) -> i32 {
    match s.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => 2,
        Some(c) if c.is_ascii_digit() => 3,
        Some(c) if is_cyrillic(c) => 1,
        _ => 9,
    }
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}' | '\u{0500}'..='\u{052F}')
}

/// Reorder "First Last" → "Last First" (matching Python scanner behaviour).
/// If the name already contains a comma, just replace commas with spaces.
pub fn normalise_author_name(name: &str) -> String {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = strip_meta(&name);
    if name.is_empty() {
        return String::new();
    }
    if name.contains(',') {
        return name
            .replace(',', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
    }
    let parts: Vec<&str> = name.split_whitespace().collect();
    if parts.len() <= 1 {
        return name;
    }
    // Move last word to front: "First Middle Last" → "Last First Middle"
    let last = parts[parts.len() - 1];
    let rest = &parts[..parts.len() - 1];
    format!("{} {}", last, rest.join(" "))
}
