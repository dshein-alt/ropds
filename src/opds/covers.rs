use std::io::{BufReader, Cursor};

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use image::imageops::FilterType;

use crate::db::models;
use crate::db::queries::books;
use crate::state::AppState;

const THUMB_SIZE: u32 = 100;

/// GET /opds/cover/:book_id/ — Full-size cover image.
pub async fn cover(
    State(state): State<AppState>,
    Path((book_id,)): Path<(i64,)>,
) -> Response {
    serve_cover(&state, book_id, false).await
}

/// GET /opds/thumb/:book_id/ — Thumbnail cover image (100x100).
pub async fn thumbnail(
    State(state): State<AppState>,
    Path((book_id,)): Path<(i64,)>,
) -> Response {
    serve_cover(&state, book_id, true).await
}

async fn serve_cover(state: &AppState, book_id: i64, as_thumbnail: bool) -> Response {
    let book = match books::get_by_id(&state.db, book_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "Book not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB error").into_response(),
    };

    if book.cover == 0 {
        return (StatusCode::NOT_FOUND, "No cover").into_response();
    }

    let root = &state.config.library.root_path;

    // Extract cover in a blocking task (involves I/O and parsing)
    let root_clone = root.clone();
    let path = book.path.clone();
    let filename = book.filename.clone();
    let format = book.format.clone();
    let cat_type = book.cat_type;

    let cover_result = tokio::task::spawn_blocking(move || {
        extract_book_cover(&root_clone, &path, &filename, &format, cat_type)
    })
    .await;

    let (cover_data, cover_mime) = match cover_result {
        Ok(Some((data, mime))) => (data, mime),
        _ => return (StatusCode::NOT_FOUND, "Cover not available").into_response(),
    };

    if as_thumbnail {
        // Resize to thumbnail
        match make_thumbnail(&cover_data, THUMB_SIZE) {
            Ok(thumb) => image_response(&thumb, "image/jpeg"),
            Err(_) => image_response(&cover_data, &cover_mime),
        }
    } else {
        image_response(&cover_data, &cover_mime)
    }
}

/// Extract cover image from a book file.
fn extract_book_cover(
    root: &std::path::Path,
    book_path: &str,
    filename: &str,
    format: &str,
    cat_type: i32,
) -> Option<(Vec<u8>, String)> {
    let data = read_book_file(root, book_path, filename, cat_type).ok()?;

    match format {
        "fb2" => {
            let reader = BufReader::new(Cursor::new(&data));
            crate::scanner::parsers::fb2::extract_cover(reader)
        }
        "epub" => {
            let cursor = Cursor::new(&data);
            let mut archive = zip::ZipArchive::new(cursor).ok()?;
            // Re-parse OPF for cover
            let opf_path = find_epub_opf(&mut archive)?;
            let opf_data = read_zip_vec(&mut archive, &opf_path).ok()?;
            extract_epub_cover(&opf_data, &opf_path, &mut archive)
        }
        _ => None,
    }
}

/// Read a book file (same logic as download handler).
fn read_book_file(
    root: &std::path::Path,
    book_path: &str,
    filename: &str,
    cat_type: i32,
) -> Result<Vec<u8>, std::io::Error> {
    use std::io::Read;
    match cat_type {
        models::CAT_NORMAL => {
            let full_path = root.join(book_path).join(filename);
            std::fs::read(&full_path)
        }
        models::CAT_ZIP | models::CAT_INPX | models::CAT_INP => {
            let zip_path = root.join(book_path);
            let file = std::fs::File::open(&zip_path)?;
            let reader = BufReader::new(file);
            let mut archive = zip::ZipArchive::new(reader)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            let mut entry = archive
                .by_name(filename)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            Ok(data)
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Unknown cat_type",
        )),
    }
}

fn find_epub_opf<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<String> {
    // Try container.xml
    if let Ok(entry) = archive.by_name("META-INF/container.xml") {
        let data = read_to_vec(entry).ok()?;
        if let Some(path) = parse_container_rootfile(&data) {
            return Some(path);
        }
    }
    // Fallback: find *.opf
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if entry.name().ends_with(".opf") {
                return Some(entry.name().to_string());
            }
        }
    }
    None
}

fn parse_container_rootfile(data: &[u8]) -> Option<String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut xml = Reader::from_reader(data);
    xml.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let qname = e.name();
                let name = std::str::from_utf8(qname.as_ref()).unwrap_or("");
                if name.ends_with("rootfile") || name == "rootfile" {
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        if key == "full-path" {
                            return Some(attr.unescape_value().unwrap_or_default().to_string());
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

fn extract_epub_cover<R: std::io::Read + std::io::Seek>(
    opf_data: &[u8],
    opf_path: &str,
    archive: &mut zip::ZipArchive<R>,
) -> Option<(Vec<u8>, String)> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let opf_dir = match opf_path.rfind('/') {
        Some(i) => &opf_path[..=i],
        None => "",
    };

    let mut cover_id: Option<String> = None;
    let mut manifest: Vec<(String, String, String, String)> = Vec::new(); // (id, href, media_type, properties)

    let mut xml = Reader::from_reader(opf_data);
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
                    manifest.push((id, href, media_type, properties));
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

    // Strategy 1: properties="cover-image"
    for (_, href, media_type, properties) in &manifest {
        if properties.contains("cover-image") && media_type.starts_with("image/") {
            let path = resolve_path(opf_dir, href);
            if let Some(data) = read_zip_opt(archive, &path) {
                return Some((data, media_type.clone()));
            }
        }
    }

    // Strategy 2: meta name="cover" → manifest id lookup
    if let Some(ref cid) = cover_id {
        for (id, href, media_type, _) in &manifest {
            if id == cid && media_type.starts_with("image/") {
                let path = resolve_path(opf_dir, href);
                if let Some(data) = read_zip_opt(archive, &path) {
                    return Some((data, media_type.clone()));
                }
            }
        }
    }

    // Strategy 3: id="cover"
    for (id, href, media_type, _) in &manifest {
        if id.eq_ignore_ascii_case("cover") && media_type.starts_with("image/") {
            let path = resolve_path(opf_dir, href);
            if let Some(data) = read_zip_opt(archive, &path) {
                return Some((data, media_type.clone()));
            }
        }
    }

    None
}

fn local_name(raw: &[u8]) -> String {
    let s = std::str::from_utf8(raw).unwrap_or("");
    match s.rfind(':') {
        Some(i) => s[i + 1..].to_lowercase(),
        None => s.to_lowercase(),
    }
}

fn resolve_path(base_dir: &str, href: &str) -> String {
    if href.starts_with('/') {
        href.trim_start_matches('/').to_string()
    } else {
        format!("{base_dir}{href}")
    }
}

fn read_to_vec(mut entry: impl std::io::Read) -> Result<Vec<u8>, std::io::Error> {
    let mut data = Vec::new();
    entry.read_to_end(&mut data)?;
    Ok(data)
}

fn read_zip_vec<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, std::io::Error> {
    let entry = archive
        .by_name(name)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;
    read_to_vec(entry)
}

fn read_zip_opt<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
    read_zip_vec(archive, name).ok()
}

/// Resize an image to a thumbnail (square).
fn make_thumbnail(data: &[u8], size: u32) -> Result<Vec<u8>, image::ImageError> {
    let img = image::load_from_memory(data)?;
    let thumb = img.resize(size, size, FilterType::Lanczos3);
    let mut buf = Cursor::new(Vec::new());
    thumb.write_to(&mut buf, image::ImageFormat::Jpeg)?;
    Ok(buf.into_inner())
}

fn image_response(data: &[u8], mime: &str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (header::CONTENT_LENGTH, data.len().to_string()),
        ],
        data.to_vec(),
    )
        .into_response()
}
