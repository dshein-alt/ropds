pub mod epub;
pub mod fb2;
pub mod inpx;
pub mod mobi;

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
/// Always strips: & ` - . ; # \ and whitespace.
/// Strips enclosing quote pairs: '' "" «» (only when they wrap the entire string).
pub fn strip_meta(s: &str) -> String {
    let trimmed = s.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '&' | '`' | '-' | '.' | ';' | '#' | '\\')
    });

    // Strip matching quote pairs that enclose the whole string
    let trimmed = strip_matching_pair(trimmed, '\'', '\'');
    let trimmed = strip_matching_pair(trimmed, '"', '"');
    let trimmed = strip_matching_pair(trimmed, '\u{00AB}', '\u{00BB}'); // « »

    trimmed.to_string()
}

/// Strip a matching open/close pair if they enclose the entire string.
fn strip_matching_pair(s: &str, open: char, close: char) -> &str {
    if s.starts_with(open) && s.ends_with(close) && s.len() > open.len_utf8() {
        &s[open.len_utf8()..s.len() - close.len_utf8()]
    } else {
        s
    }
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

/// Reorder only two-part names: "First Last" → "Last First".
/// Keep all other forms as-is (besides whitespace and outer punctuation cleanup).
/// For comma-separated two-part names like "Asimov, Isaac", normalize to "Asimov Isaac".
pub fn normalise_author_name(name: &str) -> String {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = strip_meta(&name);
    if name.is_empty() {
        return String::new();
    }

    if name.contains(',') {
        let normalized = name
            .replace(',', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        return if normalized.split_whitespace().count() == 2 {
            normalized
        } else {
            name
        };
    }

    let parts: Vec<&str> = name.split_whitespace().collect();
    if parts.len() != 2 {
        return name;
    }

    format!("{} {}", parts[1], parts[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_meta_and_quotes() {
        assert_eq!(strip_meta("  --Title.;  "), "Title");
        assert_eq!(strip_meta("'Quoted'"), "Quoted");
        assert_eq!(strip_meta("\"Quoted\""), "Quoted");
        assert_eq!(strip_meta("«Quoted»"), "Quoted");
    }

    #[test]
    fn test_detect_lang_code() {
        assert_eq!(detect_lang_code("Alpha"), 2);
        assert_eq!(detect_lang_code("9lives"), 3);
        assert_eq!(detect_lang_code("Журнал"), 1);
        assert_eq!(detect_lang_code(""), 9);
        assert_eq!(detect_lang_code("🙂"), 9);
    }

    #[test]
    fn test_normalise_author_name() {
        assert_eq!(normalise_author_name("John Tolkien"), "Tolkien John");
        assert_eq!(
            normalise_author_name("John Ronald Tolkien"),
            "John Ronald Tolkien"
        );
        assert_eq!(normalise_author_name("Asimov, Isaac"), "Asimov Isaac");
        assert_eq!(
            normalise_author_name("Asimov, Isaac Jr."),
            "Asimov, Isaac Jr"
        );
        assert_eq!(normalise_author_name("  Single  "), "Single");
        assert_eq!(normalise_author_name(""), "");
    }
}
