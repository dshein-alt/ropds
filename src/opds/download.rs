use std::io::{Cursor, Read, Write};

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::db::models;
use crate::db::queries::books;
use crate::state::AppState;

use super::xml;

/// GET /opds/download/:book_id/:zip_flag/
///
/// zip_flag: 0 = original file, 1 = wrapped in ZIP
pub async fn download(
    State(state): State<AppState>,
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

    let filename = &book.filename;
    let mime = xml::mime_for_format(&book.format);

    if zip_flag == 1 && !xml::is_nozip_format(&book.format) {
        // Wrap in ZIP
        match wrap_in_zip(filename, &data) {
            Ok(zipped) => {
                let zip_name = format!("{filename}.zip");
                let zip_mime = xml::mime_for_zip(&book.format);
                file_response(&zipped, &zip_name, &zip_mime)
            }
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "ZIP error").into_response(),
        }
    } else {
        file_response(&data, filename, mime)
    }
}

/// Read a book file from disk. Handles both plain files and files inside ZIP archives.
fn read_book_file(
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

            let mut entry = archive.by_name(filename)
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
fn wrap_in_zip(filename: &str, data: &[u8]) -> Result<Vec<u8>, zip::result::ZipError> {
    let buf = Cursor::new(Vec::new());
    let mut zip_writer = zip::ZipWriter::new(buf);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip_writer.start_file(filename, options)?;
    zip_writer.write_all(data)?;
    let cursor = zip_writer.finish()?;
    Ok(cursor.into_inner())
}

/// Build an HTTP response for a file download.
fn file_response(data: &[u8], filename: &str, mime: &str) -> Response {
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
