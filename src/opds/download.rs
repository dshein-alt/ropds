use std::io::{Cursor, Read, Write};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::db::models;
use crate::db::queries::{books, bookshelf};
use crate::state::AppState;

use super::xml;

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
    match cat_type {
        models::CAT_NORMAL => {
            // Plain file on disk
            let full_path = root.join(book_path).join(filename);
            std::fs::read(&full_path)
        }
        models::CAT_ZIP | models::CAT_INPX | models::CAT_INP => {
            // File inside a ZIP archive
            // book_path is the relative path to the ZIP file
            let zip_path = root.join(book_path);
            let file = std::fs::File::open(&zip_path)?;
            let reader = std::io::BufReader::new(file);
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
            format!("Unknown cat_type: {cat_type}"),
        )),
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
