use std::io::{BufReader, Cursor};

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use image::imageops::FilterType;

use crate::db::models;
use crate::db::queries::books;
use crate::state::AppState;

const THUMB_SIZE: u32 = 200;
const THUMB_JPEG_QUALITY: u8 = 85;
const NOCOVER_SVG: &[u8] = include_bytes!("../../static/images/nocover.svg");

/// GET /opds/cover/:book_id/ — Full-size cover image.
pub async fn cover(State(state): State<AppState>, Path((book_id,)): Path<(i64,)>) -> Response {
    serve_cover(&state, book_id, false).await
}

/// GET /opds/thumb/:book_id/ — Thumbnail cover image.
pub async fn thumbnail(State(state): State<AppState>, Path((book_id,)): Path<(i64,)>) -> Response {
    serve_cover(&state, book_id, true).await
}

async fn serve_cover(state: &AppState, book_id: i64, as_thumbnail: bool) -> Response {
    let book = match books::get_by_id(&state.db, book_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "Book not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB error").into_response(),
    };

    if book.cover == 0 && book.format != "pdf" && book.format != "djvu" {
        return image_response(NOCOVER_SVG, "image/svg+xml");
    }

    let covers_dir = state.config.library.covers_path.clone();
    let root = state.config.library.root_path.clone();
    let path = book.path.clone();
    let filename = book.filename.clone();
    let format = book.format.clone();
    let cat_type = book.cat_type;

    // Try disk cache first, then fallback to re-extraction from book file
    let cover_result = tokio::task::spawn_blocking(move || {
        // 1. Try to load from disk cache
        if let Some(result) = find_cover_file(&covers_dir, book_id) {
            return Some(result);
        }

        // 2. Fallback: re-extract from the book file
        let extracted = extract_book_cover(&root, &path, &filename, &format, cat_type)?;

        // Save extracted cover to disk for next time
        let ext = mime_to_ext(&extracted.1);
        let save_path = covers_dir.join(format!("{book_id}.{ext}"));
        let _ = std::fs::write(&save_path, &extracted.0);

        Some(extracted)
    })
    .await;

    let (cover_data, cover_mime) = match cover_result {
        Ok(Some((data, mime))) => (data, mime),
        _ => return image_response(NOCOVER_SVG, "image/svg+xml"),
    };

    if as_thumbnail {
        match make_thumbnail(&cover_data, THUMB_SIZE) {
            Ok(thumb) => image_response(&thumb, "image/jpeg"),
            Err(_) => image_response(&cover_data, &cover_mime),
        }
    } else {
        image_response(&cover_data, &cover_mime)
    }
}

/// Try to find a cached cover file on disk for the given book id.
fn find_cover_file(covers_dir: &std::path::Path, book_id: i64) -> Option<(Vec<u8>, String)> {
    for ext in ["jpg", "png", "gif"] {
        let path = covers_dir.join(format!("{book_id}.{ext}"));
        if path.exists() {
            let data = std::fs::read(&path).ok()?;
            let mime = ext_to_mime(ext);
            return Some((data, mime));
        }
    }
    None
}

fn ext_to_mime(ext: &str) -> String {
    match ext {
        "png" => "image/png".to_string(),
        "gif" => "image/gif".to_string(),
        _ => "image/jpeg".to_string(),
    }
}

fn mime_to_ext(mime: &str) -> &str {
    match mime {
        "image/png" => "png",
        "image/gif" => "gif",
        _ => "jpg",
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
            // Find cover reference id, then extract binary from raw bytes
            // (raw byte search is more reliable than XML parsing for malformed FB2)
            let cover_id = find_fb2_cover_ref(&data)?;
            crate::scanner::parsers::fb2::extract_cover_from_bytes(&data, &cover_id)
        }
        "epub" => {
            let cursor = Cursor::new(&data);
            let mut archive = zip::ZipArchive::new(cursor).ok()?;
            // Re-parse OPF for cover
            let opf_path = find_epub_opf(&mut archive)?;
            let opf_data = read_zip_vec(&mut archive, &opf_path).ok()?;
            extract_epub_cover(&opf_data, &opf_path, &mut archive)
        }
        "pdf" => match crate::pdf::render_first_page_jpeg_from_bytes(&data) {
            Ok(jpg) => Some((jpg, "image/jpeg".to_string())),
            Err(e) => {
                tracing::warn!(
                    "Failed to render PDF cover for {}/{}: {}",
                    book_path,
                    filename,
                    e
                );
                None
            }
        },
        "djvu" => match crate::djvu::render_first_page_jpeg_from_bytes(&data) {
            Ok(jpg) => Some((jpg, "image/jpeg".to_string())),
            Err(e) => {
                tracing::warn!(
                    "Failed to render DJVU cover for {}/{}: {}",
                    book_path,
                    filename,
                    e
                );
                None
            }
        },
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
    match models::CatType::try_from(cat_type) {
        Ok(models::CatType::Normal) => {
            let full_path = root.join(book_path).join(filename);
            std::fs::read(&full_path)
        }
        Ok(models::CatType::Zip) | Ok(models::CatType::Inpx) | Ok(models::CatType::Inp) => {
            let zip_path = root.join(book_path);
            let file = std::fs::File::open(&zip_path)?;
            let reader = BufReader::new(file);
            let mut archive = zip::ZipArchive::new(reader).map_err(std::io::Error::other)?;
            let mut entry = archive
                .by_name(filename)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            Ok(data)
        }
        Err(_) => Err(std::io::Error::other("Unknown cat_type")),
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
        if let Ok(entry) = archive.by_index(i)
            && entry.name().ends_with(".opf")
        {
            return Some(entry.name().to_string());
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

/// Resize an image to a thumbnail, preserving aspect ratio.
fn make_thumbnail(data: &[u8], size: u32) -> Result<Vec<u8>, image::ImageError> {
    let img = image::load_from_memory(data)?;
    let thumb = img.resize(size, size, FilterType::Lanczos3);
    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, THUMB_JPEG_QUALITY);
    thumb.write_with_encoder(encoder)?;
    Ok(buf.into_inner())
}

/// Find the cover image reference id from raw FB2 bytes.
/// Searches for `<coverpage>...<image ...href="#id"/>...</coverpage>`.
fn find_fb2_cover_ref(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    let cp_start = text.find("<coverpage")?;
    let cp_end = text[cp_start..].find("</coverpage>")? + cp_start;
    let coverpage = &text[cp_start..cp_end];

    // Find <image ...href="#id"...> within coverpage
    let img_start = coverpage.find("<image ")?;
    let img_end = coverpage[img_start..].find('>')? + img_start;
    let img_tag = &coverpage[img_start..=img_end];

    // Extract href attribute (could be l:href or xlink:href)
    let href_pos = img_tag.find("href=\"")?;
    let val_start = href_pos + 6;
    let val_end = img_tag[val_start..].find('"')? + val_start;
    let href = &img_tag[val_start..val_end];

    let id = href.trim_start_matches('#').to_lowercase();
    if id.is_empty() { None } else { Some(id) }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn test_ext_and_mime_mappings() {
        assert_eq!(ext_to_mime("png"), "image/png");
        assert_eq!(ext_to_mime("gif"), "image/gif");
        assert_eq!(ext_to_mime("jpg"), "image/jpeg");

        assert_eq!(mime_to_ext("image/png"), "png");
        assert_eq!(mime_to_ext("image/gif"), "gif");
        assert_eq!(mime_to_ext("image/jpeg"), "jpg");
    }

    #[test]
    fn test_find_cover_file_prefers_known_extensions() {
        let dir = tempdir().unwrap();
        let jpg_path = dir.path().join("42.jpg");
        std::fs::write(&jpg_path, b"jpg-bytes").unwrap();

        let found = find_cover_file(dir.path(), 42).unwrap();
        assert_eq!(found.0, b"jpg-bytes");
        assert_eq!(found.1, "image/jpeg");
    }

    #[test]
    fn test_parse_container_rootfile_and_path_helpers() {
        let xml = br#"
            <container version="1.0">
              <rootfiles>
                <rootfile full-path="OPS/content.opf" media-type="application/oebps-package+xml"/>
              </rootfiles>
            </container>
        "#;
        assert_eq!(
            parse_container_rootfile(xml),
            Some("OPS/content.opf".to_string())
        );
        assert_eq!(parse_container_rootfile(b"<container/>"), None);

        assert_eq!(local_name(b"opf:item"), "item");
        assert_eq!(local_name(b"item"), "item");
        assert_eq!(resolve_path("OPS/", "images/c.jpg"), "OPS/images/c.jpg");
        assert_eq!(resolve_path("OPS/", "/images/c.jpg"), "images/c.jpg");
    }

    #[test]
    fn test_find_fb2_cover_ref() {
        let fb2 = br##"
            <FictionBook>
              <description>
                <title-info>
                  <coverpage><image l:href="#CoverImage"/></coverpage>
                </title-info>
              </description>
            </FictionBook>
        "##;
        assert_eq!(find_fb2_cover_ref(fb2), Some("coverimage".to_string()));
        assert_eq!(find_fb2_cover_ref(b"<FictionBook/>"), None);
    }

    #[test]
    fn test_find_epub_opf_from_container_and_fallback_scan() {
        let zip_data = make_zip(&[
            (
                "META-INF/container.xml",
                br#"<container><rootfiles><rootfile full-path="OPS/content.opf"/></rootfiles></container>"#,
            ),
            ("OPS/content.opf", b"<package/>"),
        ]);
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_data)).unwrap();
        assert_eq!(
            find_epub_opf(&mut archive),
            Some("OPS/content.opf".to_string())
        );

        let zip_data = make_zip(&[("book.opf", b"<package/>")]);
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_data)).unwrap();
        assert_eq!(find_epub_opf(&mut archive), Some("book.opf".to_string()));
    }

    #[test]
    fn test_extract_epub_cover_properties_strategy() {
        let cover = b"cover-bytes";
        let zip_data = make_zip(&[("OPS/images/cover.jpg", cover)]);
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_data)).unwrap();

        let opf = br#"
            <package xmlns="http://www.idpf.org/2007/opf">
              <manifest>
                <item id="img1" href="images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>
              </manifest>
            </package>
        "#;
        let result = extract_epub_cover(opf, "OPS/content.opf", &mut archive).unwrap();
        assert_eq!(result.0, cover);
        assert_eq!(result.1, "image/jpeg");
    }

    #[test]
    fn test_extract_epub_cover_meta_cover_strategy() {
        let cover = b"meta-cover";
        let zip_data = make_zip(&[("OPS/img/c.png", cover)]);
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_data)).unwrap();

        let opf = br#"
            <package xmlns="http://www.idpf.org/2007/opf">
              <metadata>
                <meta name="cover" content="cover-img"/>
              </metadata>
              <manifest>
                <item id="cover-img" href="img/c.png" media-type="image/png"/>
              </manifest>
            </package>
        "#;
        let result = extract_epub_cover(opf, "OPS/content.opf", &mut archive).unwrap();
        assert_eq!(result.0, cover);
        assert_eq!(result.1, "image/png");
    }

    #[test]
    fn test_make_thumbnail_success_and_invalid_input() {
        let image = image::DynamicImage::new_rgb8(2, 2);
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let thumb = make_thumbnail(&png.into_inner(), 32).unwrap();
        assert!(!thumb.is_empty());
        assert_eq!(
            image::guess_format(&thumb).unwrap(),
            image::ImageFormat::Jpeg
        );

        assert!(make_thumbnail(b"not-an-image", 32).is_err());
    }

    #[test]
    fn test_image_response_headers() {
        let response = image_response(b"abc", "image/jpeg");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
        assert_eq!(response.headers().get(header::CONTENT_LENGTH).unwrap(), "3");
    }
}
