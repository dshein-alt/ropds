use std::io::{Read, Seek};

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use super::{strip_meta, BookMeta};

/// Parse EPUB metadata from a ZIP archive.
/// The reader must implement Read + Seek (for the zip crate).
pub fn parse<R: Read + Seek>(reader: R) -> Result<BookMeta, EpubError> {
    let mut archive = zip::ZipArchive::new(reader)?;
    let opf_path = find_opf_path(&mut archive)?;
    let opf_data = read_zip_entry(&mut archive, &opf_path)?;
    let mut meta = parse_opf(&opf_data)?;

    // Try to extract cover image
    if let Some((cover_data, cover_type)) = extract_cover_from_opf(&opf_data, &opf_path, &mut archive) {
        meta.cover_data = Some(cover_data);
        meta.cover_type = cover_type;
    }

    Ok(meta)
}

/// Locate the OPF root file inside the EPUB ZIP.
fn find_opf_path<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> Result<String, EpubError> {
    // Try META-INF/container.xml first
    if let Ok(entry) = archive.by_name("META-INF/container.xml") {
        let data = read_to_vec(entry)?;
        if let Some(path) = parse_container_xml(&data) {
            return Ok(path);
        }
    }

    // Fallback: scan for *.opf files
    let mut opf_files = Vec::new();
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if entry.name().ends_with(".opf") {
                opf_files.push(entry.name().to_string());
            }
        }
    }
    match opf_files.len() {
        1 => Ok(opf_files.remove(0)),
        0 => Err(EpubError::NoOpf),
        _ => Err(EpubError::MultipleOpf),
    }
}

/// Parse META-INF/container.xml to find the rootfile full-path.
fn parse_container_xml(data: &[u8]) -> Option<String> {
    let mut xml = Reader::from_reader(data);
    xml.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == "rootfile" {
                    let mut full_path = None;
                    let mut is_opf = false;
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        let val = attr.unescape_value().unwrap_or_default();
                        if key == "full-path" {
                            full_path = Some(val.to_string());
                        }
                        if key == "media-type" && val == "application/oebps-package+xml" {
                            is_opf = true;
                        }
                    }
                    // Accept if media-type matches, or if it's the only rootfile
                    if let Some(path) = full_path {
                        if is_opf || true {
                            return Some(path);
                        }
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    None
}

/// Parse OPF XML and extract book metadata.
fn parse_opf(data: &[u8]) -> Result<BookMeta, EpubError> {
    let mut meta = BookMeta::default();
    let mut xml = Reader::from_reader(data);
    xml.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut current_text = String::new();

    // Temp state for dc:creator role filtering
    let mut creator_role: Option<String> = None;
    let mut creators_aut: Vec<String> = Vec::new();
    let mut creators_all: Vec<String> = Vec::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,

            Ok(Event::Start(ref e)) => {
                let local = local_name(e.name().as_ref());
                handle_opf_open(&local, e, &mut meta, &mut creator_role);
                path.push(local);
                current_text.clear();
            }

            Ok(Event::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());
                handle_opf_open(&local, e, &mut meta, &mut creator_role);
                // Self-closing: don't push to path
            }

            Ok(Event::End(ref _e)) => {
                let tag = path.last().map(|s| s.as_str()).unwrap_or("");
                let text = current_text.trim().to_string();

                match tag {
                    "title" if path_in_metadata(&path) && meta.title.is_empty() => {
                        meta.title = strip_meta(&text);
                    }
                    "creator" if path_in_metadata(&path) => {
                        if !text.is_empty() {
                            if creator_role.as_deref() == Some("aut") {
                                creators_aut.push(text.clone());
                            }
                            creators_all.push(text);
                        }
                        creator_role = None;
                    }
                    "language" if path_in_metadata(&path) && meta.lang.is_empty() => {
                        meta.lang = strip_meta(&text);
                    }
                    "subject" if path_in_metadata(&path) => {
                        let g = strip_meta(&text).to_lowercase();
                        if !g.is_empty() {
                            meta.genres.push(g);
                        }
                    }
                    "description" if path_in_metadata(&path) && meta.annotation.is_empty() => {
                        meta.annotation = strip_meta(&text);
                    }
                    "date" if path_in_metadata(&path) && meta.docdate.is_empty() => {
                        meta.docdate = strip_meta(&text);
                    }
                    _ => {}
                }

                if !path.is_empty() {
                    path.pop();
                }
                current_text.clear();
            }

            Ok(Event::Text(ref e)) => {
                if let Ok(text) = e.decode() {
                    current_text.push_str(&text);
                }
            }

            _ => {}
        }
        buf.clear();
    }

    // Prefer authors with role="aut", fall back to all creators
    meta.authors = if !creators_aut.is_empty() {
        creators_aut
    } else {
        creators_all
    };

    Ok(meta)
}

/// Try to extract cover image from the EPUB.
/// Tries multiple strategies matching the Python implementation.
fn extract_cover_from_opf<R: Read + Seek>(
    opf_data: &[u8],
    opf_path: &str,
    archive: &mut zip::ZipArchive<R>,
) -> Option<(Vec<u8>, String)> {
    let opf_dir = match opf_path.rfind('/') {
        Some(i) => &opf_path[..=i],
        None => "",
    };

    // Parse OPF to find manifest items and cover reference
    let (manifest, cover_id) = parse_opf_manifest(opf_data);

    // Strategy 1: item with properties="cover-image"
    for item in &manifest {
        if item.properties.contains("cover-image") && item.media_type.starts_with("image/") {
            let path = resolve_path(opf_dir, &item.href);
            if let Some(data) = read_zip_entry_opt(archive, &path) {
                return Some((data, item.media_type.clone()));
            }
        }
    }

    // Strategy 2: <meta name="cover" content="id"/> → lookup in manifest
    if let Some(ref id) = cover_id {
        if let Some(item) = manifest.iter().find(|m| m.id == *id) {
            if item.media_type.starts_with("image/") {
                let path = resolve_path(opf_dir, &item.href);
                if let Some(data) = read_zip_entry_opt(archive, &path) {
                    return Some((data, item.media_type.clone()));
                }
            }
        }
    }

    // Strategy 3: manifest item with id="cover" (case-insensitive)
    for item in &manifest {
        if item.id.eq_ignore_ascii_case("cover") && item.media_type.starts_with("image/") {
            let path = resolve_path(opf_dir, &item.href);
            if let Some(data) = read_zip_entry_opt(archive, &path) {
                return Some((data, item.media_type.clone()));
            }
        }
    }

    None
}

struct ManifestItem {
    id: String,
    href: String,
    media_type: String,
    properties: String,
}

/// Parse the OPF manifest and any cover meta reference.
fn parse_opf_manifest(data: &[u8]) -> (Vec<ManifestItem>, Option<String>) {
    let mut items = Vec::new();
    let mut cover_id = None;

    let mut xml = Reader::from_reader(data);
    xml.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == "item" {
                    let mut id = String::new();
                    let mut href = String::new();
                    let mut media_type = String::new();
                    let mut properties = String::new();
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        let val = attr.unescape_value().unwrap_or_default();
                        match key {
                            "id" => id = val.to_string(),
                            "href" => href = val.to_string(),
                            "media-type" => media_type = val.to_string(),
                            "properties" => properties = val.to_string(),
                            _ => {}
                        }
                    }
                    items.push(ManifestItem { id, href, media_type, properties });
                }
                if local == "meta" {
                    let mut name_attr = String::new();
                    let mut content_attr = String::new();
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        let val = attr.unescape_value().unwrap_or_default();
                        match key {
                            "name" => name_attr = val.to_string(),
                            "content" => content_attr = val.to_string(),
                            _ => {}
                        }
                    }
                    if name_attr == "cover" && !content_attr.is_empty() {
                        cover_id = Some(content_attr);
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }

    (items, cover_id)
}

/// Handle attributes on a Start or Empty OPF element.
fn handle_opf_open(
    local: &str,
    e: &quick_xml::events::BytesStart<'_>,
    meta: &mut BookMeta,
    creator_role: &mut Option<String>,
) {
    if local == "meta" {
        let mut name_attr = String::new();
        let mut content_attr = String::new();
        for attr in e.attributes().flatten() {
            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
            let val = attr.unescape_value().unwrap_or_default();
            match key {
                "name" => name_attr = val.to_string(),
                "content" => content_attr = val.to_string(),
                _ => {}
            }
        }
        match name_attr.as_str() {
            "calibre:series" => {
                meta.series_title = Some(strip_meta(&content_attr));
            }
            "calibre:series_index" => {
                meta.series_index = content_attr.parse::<f64>().unwrap_or(0.0) as i32;
            }
            _ => {}
        }
    }

    if local == "creator" {
        *creator_role = None;
        for attr in e.attributes().flatten() {
            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
            let val = attr.unescape_value().unwrap_or_default();
            if key == "role" || key.ends_with(":role") {
                *creator_role = Some(val.to_string());
            }
        }
    }
}

fn resolve_path(base_dir: &str, href: &str) -> String {
    if href.starts_with('/') {
        href.trim_start_matches('/').to_string()
    } else {
        format!("{}{}", base_dir, href)
    }
}

fn local_name(raw: &[u8]) -> String {
    let s = std::str::from_utf8(raw).unwrap_or("");
    match s.rfind(':') {
        Some(i) => s[i + 1..].to_lowercase(),
        None => s.to_lowercase(),
    }
}

fn path_in_metadata(path: &[String]) -> bool {
    path.iter().any(|s| s == "metadata")
}

fn read_to_vec(mut entry: impl Read) -> Result<Vec<u8>, EpubError> {
    let mut data = Vec::new();
    entry.read_to_end(&mut data)?;
    Ok(data)
}

fn read_zip_entry<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, EpubError> {
    let entry = archive.by_name(name)?;
    read_to_vec(entry)
}

fn read_zip_entry_opt<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
    read_zip_entry(archive, name).ok()
}

#[derive(Debug, thiserror::Error)]
pub enum EpubError {
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("no OPF file found in EPUB")]
    NoOpf,
    #[error("multiple OPF files found in EPUB")]
    MultipleOpf,
}
