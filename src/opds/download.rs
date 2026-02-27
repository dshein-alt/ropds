use std::io::{Cursor, Read, Write};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::db::models;
use crate::db::queries::{books, bookshelf};
use crate::state::AppState;

use super::v1::xml;

/// GET /opds/download/:book_id/:zip_flag/
///
/// zip_flag: 0 = original file, 1 = wrapped in ZIP
pub async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((book_id, zip_flag)): Path<(i64, i32)>,
) -> Response {
    let book = match books::get_by_id(&state.db, book_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "Book not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB error").into_response(),
    };

    let root = &state.config.library.root_path;

    // Read the book file bytes
    let data = match read_book_file(root, &book.path, &book.filename, book.cat_type) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to read book {}: {e}", book_id);
            return (StatusCode::NOT_FOUND, "File not found").into_response();
        }
    };

    // Fire-and-forget bookshelf tracking
    if let Some(user_id) = super::auth::get_user_id_from_headers(&state.db, &headers).await {
        let _ = bookshelf::upsert(&state.db, user_id, book_id).await;
    }

    let download_name = title_to_filename(&book.title, &book.format, &book.filename);
    let mime = xml::mime_for_format(&book.format);

    if zip_flag == 1 && !xml::is_nozip_format(&book.format) {
        // Wrap in ZIP — use original filename inside the archive
        match wrap_in_zip(&book.filename, &data) {
            Ok(zipped) => {
                let zip_name = format!("{download_name}.zip");
                let zip_mime = xml::mime_for_zip(&book.format);
                file_response(&zipped, &zip_name, &zip_mime)
            }
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "ZIP error").into_response(),
        }
    } else {
        file_response(&data, &download_name, mime)
    }
}

/// Read a book file from disk. Handles both plain files and files inside ZIP archives.
pub fn read_book_file(
    root: &std::path::Path,
    book_path: &str,
    filename: &str,
    cat_type: i32,
) -> Result<Vec<u8>, std::io::Error> {
    match models::CatType::try_from(cat_type) {
        Ok(models::CatType::Normal) => {
            // Plain file on disk
            let full_path = root.join(book_path).join(filename);
            std::fs::read(&full_path)
        }
        Ok(models::CatType::Zip) | Ok(models::CatType::Inpx) | Ok(models::CatType::Inp) => {
            // File inside a ZIP archive
            // book_path is the relative path to the ZIP file
            let zip_path = root.join(book_path);
            let file = std::fs::File::open(&zip_path)?;
            let reader = std::io::BufReader::new(file);
            let mut archive = zip::ZipArchive::new(reader).map_err(std::io::Error::other)?;

            let mut entry = archive
                .by_name(filename)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;

            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            Ok(data)
        }
        Err(_) => Err(std::io::Error::other(format!(
            "Unknown cat_type: {cat_type}"
        ))),
    }
}

/// Wrap file bytes into a new ZIP archive in memory.
pub fn wrap_in_zip(filename: &str, data: &[u8]) -> Result<Vec<u8>, zip::result::ZipError> {
    let buf = Cursor::new(Vec::new());
    let mut zip_writer = zip::ZipWriter::new(buf);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip_writer.start_file(filename, options)?;
    zip_writer.write_all(data)?;
    let cursor = zip_writer.finish()?;
    Ok(cursor.into_inner())
}

/// Build a safe download filename from the book title and format extension.
///
/// - Collapses consecutive whitespace into a single `_`
/// - Replaces non-alphanumeric, non-Unicode-letter characters with `_`
/// - Collapses consecutive `_` into one
/// - Trims leading/trailing `_`
/// - Falls back to the original filename if the result is empty
pub fn title_to_filename(title: &str, format: &str, original_filename: &str) -> String {
    let safe: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '\'' {
                c
            } else {
                '_'
            }
        })
        .collect();

    // Collapse consecutive underscores and trim
    let mut result = String::new();
    let mut prev_underscore = true; // trim leading
    for c in safe.chars() {
        if c == '_' {
            if !prev_underscore {
                result.push('_');
            }
            prev_underscore = true;
        } else {
            result.push(c);
            prev_underscore = false;
        }
    }
    // Trim trailing underscore
    while result.ends_with('_') {
        result.pop();
    }

    if result.is_empty() {
        original_filename.to_string()
    } else {
        format!("{result}.{format}")
    }
}

/// Build an HTTP response for a file download.
pub fn file_response(data: &[u8], filename: &str, mime: &str) -> Response {
    let content_disposition = format!("attachment; filename=\"{filename}\"");
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, format!("{mime}; name=\"{filename}\"")),
            (header::CONTENT_DISPOSITION, content_disposition),
            (header::CONTENT_LENGTH, data.len().to_string()),
        ],
        data.to_vec(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::CatType;
    use std::io::Write;
    use tempfile::tempdir;

    fn make_zip_with_file(path: &std::path::Path, name: &str, data: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(name, opts).unwrap();
        zip.write_all(data).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn test_wrap_in_zip_roundtrip() {
        let bytes = b"hello-book";
        let zipped = wrap_in_zip("book.fb2", bytes).unwrap();
        let reader = std::io::Cursor::new(zipped);
        let mut archive = zip::ZipArchive::new(reader).unwrap();
        assert_eq!(archive.len(), 1);
        let mut file = archive.by_name("book.fb2").unwrap();
        let mut out = Vec::new();
        use std::io::Read;
        file.read_to_end(&mut out).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn test_title_to_filename_sanitization_and_fallback() {
        assert_eq!(
            title_to_filename("  A  Title / Name ", "fb2", "orig.fb2"),
            "A_Title_Name.fb2"
        );
        assert_eq!(title_to_filename("***", "epub", "orig.epub"), "orig.epub");
    }

    #[test]
    fn test_file_response_headers() {
        let resp = file_response(b"abc", "book.fb2", "application/fb2+xml");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_DISPOSITION).unwrap(),
            "attachment; filename=\"book.fb2\""
        );
        assert_eq!(resp.headers().get(header::CONTENT_LENGTH).unwrap(), "3");
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/fb2+xml; name=\"book.fb2\""
        );
    }

    #[test]
    fn test_read_book_file_normal() {
        let dir = tempdir().unwrap();
        let book_dir = dir.path().join("sub");
        std::fs::create_dir_all(&book_dir).unwrap();
        let full = book_dir.join("book.fb2");
        std::fs::write(&full, b"plain-data").unwrap();

        let data =
            read_book_file(dir.path(), "sub", "book.fb2", i32::from(CatType::Normal)).unwrap();
        assert_eq!(data, b"plain-data");
    }

    #[test]
    fn test_read_book_file_from_zip_archive() {
        let dir = tempdir().unwrap();
        let zip_path = dir.path().join("books.zip");
        make_zip_with_file(&zip_path, "inside.fb2", b"zip-data");

        let data = read_book_file(
            dir.path(),
            "books.zip",
            "inside.fb2",
            i32::from(CatType::Zip),
        )
        .unwrap();
        assert_eq!(data, b"zip-data");
    }

    #[test]
    fn test_read_book_file_unknown_cat_type() {
        let dir = tempdir().unwrap();
        let err = read_book_file(dir.path(), "", "book.fb2", 999).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
    }
}
