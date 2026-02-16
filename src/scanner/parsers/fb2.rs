use std::io::BufRead;

use base64::Engine;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

use super::{strip_meta, BookMeta};

/// Parse FB2 XML from any `BufRead` source and return extracted metadata.
pub fn parse(reader: impl BufRead) -> Result<BookMeta, quick_xml::Error> {
    let mut meta = BookMeta::default();
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut path: Vec<String> = Vec::new();

    // Temp state for author parsing
    let mut author_first = String::new();
    let mut author_last = String::new();

    // Cover reference id (from <coverpage><image href="#id"/>)
    let mut cover_ref: Option<String> = None;
    let mut in_annotation = false;
    let mut annotation_parts: Vec<String> = Vec::new();
    let mut description_done = false;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Err(e) => return Err(e),

            Ok(Event::Start(ref e)) => {
                let local = local_name(e.name().as_ref());
                handle_open_tag(&local, e, &path, &mut cover_ref, &mut meta);
                path.push(local);

                if matches_path(&path, &["description", "title-info", "annotation"]) {
                    in_annotation = true;
                }
            }

            Ok(Event::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());
                // Handle attributes but don't push to path (self-closing)
                handle_open_tag(&local, e, &path, &mut cover_ref, &mut meta);
            }

            Ok(Event::End(ref e)) => {
                let local = local_name(e.name().as_ref());

                // Commit author when </author> closes
                if local == "author" && path_contains(&path, "title-info") {
                    let first = strip_meta(&author_first);
                    let last = strip_meta(&author_last);
                    let full = match (first.is_empty(), last.is_empty()) {
                        (false, false) => format!("{first} {last}"),
                        (true, false) => last.clone(),
                        (false, true) => first.clone(),
                        _ => String::new(),
                    };
                    if !full.is_empty() {
                        meta.authors.push(full);
                    }
                    author_first.clear();
                    author_last.clear();
                }

                if local == "annotation" {
                    in_annotation = false;
                    meta.annotation = annotation_parts.join("\n");
                }

                if local == "description" {
                    description_done = true;
                }

                if !path.is_empty() {
                    path.pop();
                }
            }

            Ok(Event::Text(ref e)) => {
                let text = e.decode().unwrap_or_default();

                if !description_done {
                    let tag = path.last().map(|s| s.as_str()).unwrap_or("");

                    // <book-title>
                    if tag == "book-title"
                        && matches_path(&path, &["description", "title-info", "book-title"])
                    {
                        meta.title = strip_meta(&text);
                    }
                    // <genre>
                    else if tag == "genre"
                        && matches_path(&path, &["description", "title-info", "genre"])
                    {
                        let g = text.trim().to_lowercase();
                        if !g.is_empty() {
                            meta.genres.push(g);
                        }
                    }
                    // <lang>
                    else if tag == "lang"
                        && matches_path(&path, &["description", "title-info", "lang"])
                    {
                        meta.lang = strip_meta(&text);
                    }
                    // <first-name> inside <author>
                    else if tag == "first-name"
                        && path_contains(&path, "author")
                        && path_contains(&path, "title-info")
                    {
                        author_first.push_str(&text);
                    }
                    // <last-name> inside <author>
                    else if tag == "last-name"
                        && path_contains(&path, "author")
                        && path_contains(&path, "title-info")
                    {
                        author_last.push_str(&text);
                    }
                    // <date> inside <document-info>
                    else if tag == "date"
                        && matches_path(&path, &["description", "document-info", "date"])
                    {
                        if meta.docdate.is_empty() {
                            meta.docdate = strip_meta(&text);
                        }
                    }
                    // Text inside <annotation>
                    else if in_annotation {
                        let t = text.trim().to_string();
                        if !t.is_empty() {
                            annotation_parts.push(t);
                        }
                    }
                }
            }

            _ => {}
        }
        buf.clear();
    }

    Ok(meta)
}

/// Handle attributes on an opening/empty tag.
fn handle_open_tag(
    local: &str,
    e: &quick_xml::events::BytesStart<'_>,
    path: &[String],
    cover_ref: &mut Option<String>,
    meta: &mut BookMeta,
) {
    // <sequence name="..." number="..."/>
    if local == "sequence" && matches_path_with(path, local, &["description", "title-info", "sequence"]) {
        for attr in e.attributes().flatten() {
            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
            let val = attr.unescape_value().unwrap_or_default();
            match key {
                "name" => meta.series_title = Some(strip_meta(&val)),
                "number" => {
                    let s = strip_meta(&val);
                    meta.series_index = s.parse::<i32>().unwrap_or(0);
                }
                _ => {}
            }
        }
    }

    // <image l:href="#cover.jpg"/> inside <coverpage>
    if local == "image" && (path_contains(path, "coverpage") || path.last().map(|s| s.as_str()) == Some("coverpage")) {
        for attr in e.attributes().flatten() {
            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
            if key.ends_with("href") {
                let val = attr.unescape_value().unwrap_or_default();
                let id = val.trim_start_matches('#').to_lowercase();
                if !id.is_empty() {
                    *cover_ref = Some(id);
                }
            }
        }
    }
}

/// Extract cover image from an FB2 file by doing a full parse for <binary> elements.
pub fn extract_cover(reader: impl BufRead) -> Option<(Vec<u8>, String)> {
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut path: Vec<String> = Vec::new();

    let mut cover_ref: Option<String> = None;
    let mut in_cover_binary = false;
    let mut cover_b64 = String::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,

            Ok(Event::Start(ref e)) => {
                let local = local_name(e.name().as_ref());

                if local == "image"
                    && (path_contains(&path, "coverpage")
                        || path.last().map(|s| s.as_str()) == Some("coverpage"))
                {
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        if key.ends_with("href") {
                            let val = attr.unescape_value().unwrap_or_default();
                            let id = val.trim_start_matches('#').to_lowercase();
                            if !id.is_empty() {
                                cover_ref = Some(id);
                            }
                        }
                    }
                }

                if local == "binary" {
                    if let Some(ref wanted) = cover_ref {
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            if key == "id" {
                                let val = attr.unescape_value().unwrap_or_default();
                                if val.to_lowercase() == *wanted {
                                    in_cover_binary = true;
                                }
                            }
                        }
                    }
                }

                path.push(local);
            }

            Ok(Event::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == "image"
                    && (path_contains(&path, "coverpage")
                        || path.last().map(|s| s.as_str()) == Some("coverpage"))
                {
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        if key.ends_with("href") {
                            let val = attr.unescape_value().unwrap_or_default();
                            let id = val.trim_start_matches('#').to_lowercase();
                            if !id.is_empty() {
                                cover_ref = Some(id);
                            }
                        }
                    }
                }
            }

            Ok(Event::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == "binary" && in_cover_binary {
                    let clean: String = cover_b64.chars().filter(|c| !c.is_whitespace()).collect();
                    if let Ok(data) = base64::engine::general_purpose::STANDARD.decode(&clean) {
                        let mime = guess_image_mime(&data);
                        return Some((data, mime));
                    }
                    return None;
                }
                if !path.is_empty() {
                    path.pop();
                }
            }

            Ok(Event::Text(ref e)) => {
                if in_cover_binary {
                    if let Ok(text) = e.decode() {
                        cover_b64.push_str(&text);
                    }
                }
            }

            _ => {}
        }
        buf.clear();
    }
    None
}

/// Get the local name of an XML tag, stripping any namespace prefix.
fn local_name(raw: &[u8]) -> String {
    let s = std::str::from_utf8(raw).unwrap_or("");
    match s.rfind(':') {
        Some(i) => s[i + 1..].to_lowercase(),
        None => s.to_lowercase(),
    }
}

/// Check whether the tag path ends with the given suffix sequence.
/// `path` is the current stack (not yet including the current tag).
fn matches_path(path: &[String], suffix: &[&str]) -> bool {
    if path.len() < suffix.len() {
        return false;
    }
    let start = path.len() - suffix.len();
    path[start..]
        .iter()
        .zip(suffix.iter())
        .all(|(a, b)| a == b)
}

/// Check path match including a tag that hasn't been pushed yet.
fn matches_path_with(path: &[String], current_tag: &str, suffix: &[&str]) -> bool {
    if suffix.is_empty() {
        return false;
    }
    if suffix.last() != Some(&current_tag) {
        return false;
    }
    let parent_suffix = &suffix[..suffix.len() - 1];
    if parent_suffix.is_empty() {
        return true;
    }
    matches_path(path, parent_suffix)
}

/// Check if any element in the path matches the given tag name.
fn path_contains(path: &[String], tag: &str) -> bool {
    path.iter().any(|s| s == tag)
}

/// Guess MIME type from image magic bytes.
fn guess_image_mime(data: &[u8]) -> String {
    if data.starts_with(b"\x89PNG") {
        "image/png".to_string()
    } else if data.starts_with(b"\xFF\xD8\xFF") {
        "image/jpeg".to_string()
    } else if data.starts_with(b"GIF8") {
        "image/gif".to_string()
    } else {
        "image/jpeg".to_string()
    }
}
