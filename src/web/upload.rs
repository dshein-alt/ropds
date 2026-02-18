use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};

use crate::db::queries::users;
use crate::state::AppState;
use crate::web::auth::verify_session;
use crate::web::context::{build_context, validate_csrf};

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

fn json_error(status: StatusCode, error: &str) -> Response {
    (
        status,
        axum::Json(serde_json::json!({ "success": false, "error": error })),
    )
        .into_response()
}

fn json_success(data: serde_json::Value) -> Response {
    (StatusCode::OK, axum::Json(data)).into_response()
}

// ---------------------------------------------------------------------------
// Permission check
// ---------------------------------------------------------------------------

/// Core upload permission check (format-agnostic).
/// Returns `Ok(user_id)` on success, `Err(())` on any auth/permission failure.
async fn verify_upload_permission(state: &AppState, jar: &CookieJar) -> Result<i64, ()> {
    if !state.config.upload.allow_upload {
        return Err(());
    }
    let secret = state.config.server.session_secret.as_bytes();
    let user_id = jar
        .get("session")
        .and_then(|c| verify_session(c.value(), secret))
        .ok_or(())?;
    let user = users::get_by_id(&state.db, user_id)
        .await
        .map_err(|_| ())?
        .ok_or(())?;
    if user.is_superuser != 1 && user.allow_upload != 1 {
        return Err(());
    }
    Ok(user_id)
}

/// Check upload permission for JSON API endpoints.
async fn check_upload_permission(state: &AppState, jar: &CookieJar) -> Result<i64, Response> {
    verify_upload_permission(state, jar)
        .await
        .map_err(|()| json_error(StatusCode::FORBIDDEN, "forbidden"))
}

// ---------------------------------------------------------------------------
// Token generation (HMAC-SHA256 over timestamp + counter + pid)
// ---------------------------------------------------------------------------

fn generate_token(secret: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();

    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(b"upload-token:");
    mac.update(&ts.to_le_bytes());
    mac.update(&count.to_le_bytes());
    mac.update(&pid.to_le_bytes());
    hex::encode(mac.finalize().into_bytes())
}

// ---------------------------------------------------------------------------
// Extension validation
// ---------------------------------------------------------------------------

/// Validate the extension of `filename` against a list of allowed extensions.
/// Returns `Some(lowercase_ext)` if valid, `None` otherwise.
fn validate_extension(filename: &str, allowed: &[String]) -> Option<String> {
    let ext = std::path::Path::new(filename)
        .extension()?
        .to_string_lossy()
        .to_lowercase();
    // Must be alphanumeric only
    if !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    if allowed.iter().any(|a| a.eq_ignore_ascii_case(&ext)) {
        Some(ext)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Filename sanitisation
// ---------------------------------------------------------------------------

/// Sanitise a filename: strip path components, keep safe characters only.
fn sanitize_filename(name: &str) -> String {
    let stem = std::path::Path::new(name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let sanitized: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim().trim_matches('.');
    if trimmed.is_empty() {
        "uploaded_book".to_string()
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// Upload state persisted as JSON on disk
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct UploadState {
    temp_path: String,
    original_filename: String,
    extension: String,
    size: i64,
    title: String,
    authors: Vec<String>,
    genres: Vec<String>,
    annotation: String,
    docdate: String,
    lang: String,
    series_title: Option<String>,
    series_index: i32,
    has_cover: bool,
    cover_type: String,
    cover_path: Option<String>,
    user_id: i64,
    created_at: String,
}

// ---------------------------------------------------------------------------
// Stale upload cleanup
// ---------------------------------------------------------------------------

/// Remove abandoned upload temp files older than `max_age_secs`.
/// Reads each `upload_*.json` state file; if `created_at` is older than the
/// threshold, deletes the state file plus its associated book and cover files.
fn cleanup_stale_uploads(temp_dir: &std::path::Path, max_age_secs: u64) {
    let cutoff = chrono::Utc::now()
        - chrono::Duration::seconds(max_age_secs as i64);

    let entries = match std::fs::read_dir(temp_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Only look at state JSON files
        if !name.starts_with("upload_") || !name.ends_with(".json") {
            continue;
        }

        let json = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let state: UploadState = match serde_json::from_str(&json) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let created = match chrono::DateTime::parse_from_rfc3339(&state.created_at) {
            Ok(dt) => dt.with_timezone(&chrono::Utc),
            Err(_) => continue,
        };

        if created < cutoff {
            tracing::info!("Cleaning up stale upload: {name}");
            let _ = std::fs::remove_file(&state.temp_path);
            if let Some(ref cover) = state.cover_path {
                let _ = std::fs::remove_file(cover);
            }
            let _ = std::fs::remove_file(&path);
        }
    }
}

// ---------------------------------------------------------------------------
// ZIP extraction helper
// ---------------------------------------------------------------------------

/// Extract a single book file from a ZIP archive.
/// Returns `(data, extension, filename)` or an error-code string.
fn extract_book_from_zip(
    zip_data: &[u8],
    allowed_exts: &[String],
    max_bytes: u64,
) -> Result<(Vec<u8>, String, String), &'static str> {
    use std::io::{Cursor, Read};

    let reader = Cursor::new(zip_data);
    let mut archive = zip::ZipArchive::new(reader).map_err(|_| "error_unsupported")?;

    // Hard limit on number of entries
    if archive.len() > 100 {
        return Err("error_unsupported");
    }

    // Find the first (and only) supported book file
    let mut book_entry: Option<(usize, String, String)> = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|_| "error_unsupported")?;
        if entry.is_dir() {
            continue;
        }

        let name = entry.name().to_string();
        let ext = std::path::Path::new(&name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        // Skip nested zips
        if ext == "zip" {
            continue;
        }

        // Is it a supported book format?
        if allowed_exts
            .iter()
            .any(|a| a.eq_ignore_ascii_case(&ext) && ext != "zip")
        {
            if book_entry.is_some() {
                return Err("error_unsupported"); // multiple books in zip
            }
            // Check uncompressed size
            if entry.size() > max_bytes {
                return Err("error_too_large");
            }
            book_entry = Some((i, ext, name));
        }
    }

    let (index, ext, name) = book_entry.ok_or("error_unsupported")?;

    let entry = archive.by_index(index).map_err(|_| "error_unsupported")?;
    let mut data = Vec::new();
    // Use take() to enforce a hard streaming read limit — declared zip sizes can be forged
    entry
        .take(max_bytes + 1)
        .read_to_end(&mut data)
        .map_err(|_| "error_unsupported")?;
    if data.len() as u64 > max_bytes {
        return Err("error_too_large");
    }

    let filename = std::path::Path::new(&name)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    Ok((data, ext, filename))
}

// ---------------------------------------------------------------------------
// GET /web/upload — render the upload page
// ---------------------------------------------------------------------------

pub async fn upload_page(State(state): State<AppState>, jar: CookieJar) -> Response {
    if verify_upload_permission(&state, &jar).await.is_err() {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mut ctx = build_context(&state, &jar, "upload").await;

    // Build supported-formats string (excluding "zip")
    let formats: Vec<&str> = state
        .config
        .library
        .book_extensions
        .iter()
        .filter(|e| e.as_str() != "zip")
        .map(|e| e.as_str())
        .collect();
    ctx.insert("supported_formats", &formats.join(", "));

    // Build accepted-extensions string for the HTML file input
    let accepted: Vec<String> = formats.iter().map(|e| format!(".{e}")).collect();
    let mut accepted_str = accepted.join(",");
    if state.config.library.scan_zip {
        accepted_str.push_str(",.zip");
    }
    ctx.insert("accepted_extensions", &accepted_str);
    ctx.insert("max_upload_size_mb", &state.config.upload.max_upload_size_mb);

    match state.tera.render("web/upload.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /web/upload/file — receive, validate, parse metadata
// ---------------------------------------------------------------------------

pub async fn upload_file(
    State(state): State<AppState>,
    jar: CookieJar,
    mut multipart: axum::extract::Multipart,
) -> Response {
    // 0. Clean up stale uploads (older than 1 hour) in a blocking task
    let upload_path = state.config.upload.upload_path.clone();
    tokio::task::spawn(async move {
        let _ = tokio::task::spawn_blocking(move || {
            cleanup_stale_uploads(&upload_path, 3600);
        })
        .await;
    });

    // 1. Permission check
    let user_id = match check_upload_permission(&state, &jar).await {
        Ok(id) => id,
        Err(r) => return r,
    };

    let max_bytes = state.config.upload.max_upload_size_mb * 1024 * 1024;
    let mut csrf_token_value = String::new();
    let mut file_data: Option<(String, Vec<u8>)> = None; // (filename, bytes)

    // 2. Read multipart fields
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "csrf_token" => {
                csrf_token_value = field.text().await.unwrap_or_default();
            }
            "file" => {
                let filename = field.file_name().unwrap_or("").to_string();
                let bytes = field.bytes().await.unwrap_or_default();
                if bytes.len() as u64 > max_bytes {
                    return json_error(StatusCode::BAD_REQUEST, "error_too_large");
                }
                file_data = Some((filename, bytes.to_vec()));
            }
            _ => {}
        }
    }

    // 3. CSRF validation
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &csrf_token_value) {
        return json_error(StatusCode::FORBIDDEN, "forbidden");
    }

    // 4. Validate that a file was provided
    let (original_filename, data) = match file_data {
        Some(d) if !d.1.is_empty() => d,
        _ => return json_error(StatusCode::BAD_REQUEST, "error_no_file"),
    };

    // 5. Validate and extract extension
    let allowed_exts = &state.config.library.book_extensions;
    let extension = match validate_extension(&original_filename, allowed_exts) {
        Some(ext) => ext,
        None => return json_error(StatusCode::BAD_REQUEST, "error_unsupported"),
    };

    // 6. Handle ZIP: extract the single book file inside
    let (book_data, book_ext, book_filename) = if extension == "zip" {
        if !state.config.library.scan_zip {
            return json_error(StatusCode::BAD_REQUEST, "error_unsupported");
        }
        match extract_book_from_zip(&data, allowed_exts, max_bytes) {
            Ok(result) => result,
            Err(error_code) => return json_error(StatusCode::BAD_REQUEST, error_code),
        }
    } else {
        (data, extension.clone(), original_filename.clone())
    };

    // 7. Generate token and save to temp dir
    let token = generate_token(secret);
    let temp_dir = &state.config.upload.upload_path;
    let temp_file = temp_dir.join(format!("upload_{token}.{book_ext}"));

    if let Err(e) = std::fs::write(&temp_file, &book_data) {
        tracing::error!("Failed to write temp file: {e}");
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "error_upload");
    }

    // 8. Parse metadata (in blocking task to avoid blocking the async runtime)
    let book_ext_clone = book_ext.clone();
    let temp_file_clone = temp_file.clone();
    let meta_result = tokio::task::spawn_blocking(move || {
        crate::scanner::parse_book_file(&temp_file_clone, &book_ext_clone)
    })
    .await;

    let mut meta = match meta_result {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => {
            tracing::warn!("Failed to parse uploaded book: {e}");
            let _ = std::fs::remove_file(&temp_file);
            return json_error(StatusCode::BAD_REQUEST, "error_parse");
        }
        Err(e) => {
            tracing::error!("spawn_blocking error: {e}");
            let _ = std::fs::remove_file(&temp_file);
            return json_error(StatusCode::BAD_REQUEST, "error_parse");
        }
    };

    // If the parser used the temp filename as fallback title, replace with original name
    if meta.title.starts_with("upload_") {
        meta.title = std::path::Path::new(&book_filename)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
    }

    // 9. Save cover to temp if present
    let cover_path = if let Some(ref cover_data) = meta.cover_data {
        let cover_ext = match meta.cover_type.as_str() {
            "image/png" => "png",
            "image/gif" => "gif",
            _ => "jpg",
        };
        let cover_file = temp_dir.join(format!("upload_{token}_cover.{cover_ext}"));
        if std::fs::write(&cover_file, cover_data).is_ok() {
            Some(cover_file.to_string_lossy().to_string())
        } else {
            None
        }
    } else {
        None
    };

    // 10. Save upload state JSON
    let upload_state = UploadState {
        temp_path: temp_file.to_string_lossy().to_string(),
        original_filename: book_filename,
        extension: book_ext.clone(),
        size: book_data.len() as i64,
        title: meta.title.clone(),
        authors: meta.authors.clone(),
        genres: meta.genres.clone(),
        annotation: meta.annotation.clone(),
        docdate: meta.docdate.clone(),
        lang: meta.lang.clone(),
        series_title: meta.series_title.clone(),
        series_index: meta.series_index,
        has_cover: meta.cover_data.is_some(),
        cover_type: meta.cover_type.clone(),
        cover_path,
        user_id,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let state_file = temp_dir.join(format!("upload_{token}.json"));
    let state_json = serde_json::to_string(&upload_state).unwrap_or_default();
    if let Err(e) = std::fs::write(&state_file, &state_json) {
        tracing::error!("Failed to write upload state: {e}");
        let _ = std::fs::remove_file(&temp_file);
        if let Some(ref cp) = upload_state.cover_path {
            let _ = std::fs::remove_file(cp);
        }
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "error_upload");
    }

    // 11. Return success with parsed metadata
    json_success(serde_json::json!({
        "success": true,
        "token": token,
        "meta": {
            "title": meta.title,
            "authors": meta.authors,
            "genres": meta.genres,
            "format": book_ext,
            "size": book_data.len(),
            "lang": meta.lang,
            "has_cover": meta.cover_data.is_some(),
        }
    }))
}

// ---------------------------------------------------------------------------
// GET /web/upload/cover/{token} — serve temp cover image
// ---------------------------------------------------------------------------

pub async fn upload_cover(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::extract::Path(token): axum::extract::Path<String>,
) -> Response {
    let user_id = match verify_upload_permission(&state, &jar).await {
        Ok(id) => id,
        Err(()) => return StatusCode::FORBIDDEN.into_response(),
    };

    // Validate token format (hex chars only, reasonable length)
    if !token.chars().all(|c| c.is_ascii_hexdigit()) || token.len() > 64 {
        return StatusCode::NOT_FOUND.into_response();
    }

    // Read upload state to get cover path and verify ownership
    let temp_dir = &state.config.upload.upload_path;
    let state_file = temp_dir.join(format!("upload_{token}.json"));
    let state_json = match std::fs::read_to_string(&state_file) {
        Ok(s) => s,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let upload_state: UploadState = match serde_json::from_str(&state_json) {
        Ok(s) => s,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    // Verify the current user owns this upload
    if upload_state.user_id != user_id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let cover_path = match upload_state.cover_path {
        Some(p) => p,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let data = match std::fs::read(&cover_path) {
        Ok(d) => d,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let content_type = match upload_state.cover_type.as_str() {
        "image/png" => "image/png",
        "image/gif" => "image/gif",
        _ => "image/jpeg",
    };

    ([(axum::http::header::CONTENT_TYPE, content_type)], data).into_response()
}

// ---------------------------------------------------------------------------
// POST /web/upload/publish — move to library + insert into DB
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PublishForm {
    pub token: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub csrf_token: String,
}

pub async fn publish(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(form): axum::Json<PublishForm>,
) -> Response {
    // 1. Permission check
    let user_id = match check_upload_permission(&state, &jar).await {
        Ok(id) => id,
        Err(r) => return r,
    };

    // 2. CSRF check
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return json_error(StatusCode::FORBIDDEN, "forbidden");
    }

    // 3. Validate token format
    if !form.token.chars().all(|c| c.is_ascii_hexdigit()) || form.token.len() > 64 {
        return json_error(StatusCode::BAD_REQUEST, "error_publish");
    }

    // 4. Read upload state
    let temp_dir = &state.config.upload.upload_path;
    let state_file = temp_dir.join(format!("upload_{}.json", form.token));
    let state_json = match std::fs::read_to_string(&state_file) {
        Ok(s) => s,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "error_publish"),
    };
    let upload_state: UploadState = match serde_json::from_str(&state_json) {
        Ok(s) => s,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "error_publish"),
    };

    // 5. Verify user owns this upload
    if upload_state.user_id != user_id {
        return json_error(StatusCode::FORBIDDEN, "forbidden");
    }

    // 6. Build a safe destination filename
    let safe_filename = format!(
        "{}.{}",
        sanitize_filename(&upload_state.original_filename),
        upload_state.extension
    );
    let root_path = &state.config.library.root_path;
    let dest_path = root_path.join(&safe_filename);

    // 7. Check for DB duplicate
    if let Ok(Some(_)) =
        crate::db::queries::books::find_by_path_and_filename(&state.db, "", &safe_filename).await
    {
        return json_error(StatusCode::CONFLICT, "error_duplicate");
    }

    // 8. Atomically create destination file (prevents TOCTOU race on disk)
    let source_data = match std::fs::read(&upload_state.temp_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to read temp file: {e}");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "error_publish");
        }
    };
    {
        use std::io::Write;
        let mut dest_file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest_path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return json_error(StatusCode::CONFLICT, "error_duplicate");
            }
            Err(e) => {
                tracing::error!("Failed to create destination file: {e}");
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "error_publish");
            }
        };
        if let Err(e) = dest_file.write_all(&source_data) {
            tracing::error!("Failed to write to destination: {e}");
            let _ = std::fs::remove_file(&dest_path);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "error_publish");
        }
    }

    // 9. Build BookMeta and insert into DB
    let cover_data = upload_state
        .cover_path
        .as_ref()
        .and_then(|p| std::fs::read(p).ok());

    // Use user-submitted title if provided, otherwise fall back to parsed title
    let publish_title = {
        let t = form.title.trim().to_string();
        if t.is_empty() || t.chars().count() > 256 || t.chars().any(|c| c.is_control()) {
            upload_state.title.clone()
        } else {
            t
        }
    };

    let meta = crate::scanner::parsers::BookMeta {
        title: publish_title,
        authors: if form.authors.is_empty() {
            upload_state.authors.clone()
        } else {
            form.authors
        },
        genres: if form.genres.is_empty() {
            upload_state.genres.clone()
        } else {
            form.genres
        },
        annotation: upload_state.annotation.clone(),
        docdate: upload_state.docdate.clone(),
        lang: upload_state.lang.clone(),
        series_title: upload_state.series_title.clone(),
        series_index: upload_state.series_index,
        cover_data,
        cover_type: upload_state.cover_type.clone(),
    };

    // Ensure root catalog exists (empty path = root)
    let catalog_id =
        match crate::scanner::ensure_catalog(&state.db, "", crate::db::models::CAT_NORMAL).await {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to ensure catalog: {e}");
                let _ = std::fs::remove_file(&dest_path);
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "error_publish");
            }
        };

    let book_id = match crate::scanner::insert_book_with_meta(
        &state.db,
        catalog_id,
        &safe_filename,
        "", // path relative to root
        &upload_state.extension,
        upload_state.size,
        crate::db::models::CAT_NORMAL,
        &meta,
        &state.config.opds.covers_dir,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Failed to insert book into DB: {e}");
            // Rollback: delete the copied file
            let _ = std::fs::remove_file(&dest_path);
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "error_publish");
        }
    };

    // 10. Update counters (non-critical, log on failure)
    if let Err(e) = crate::db::queries::counters::update_all(&state.db).await {
        tracing::warn!("Failed to update counters after publish: {e}");
    }

    // 11. Clean up temp files
    let _ = std::fs::remove_file(&upload_state.temp_path);
    if let Some(ref cover) = upload_state.cover_path {
        let _ = std::fs::remove_file(cover);
    }
    let _ = std::fs::remove_file(&state_file);

    // 12. Return success
    json_success(serde_json::json!({
        "success": true,
        "book_id": book_id,
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_extension_valid() {
        let allowed = vec!["fb2".into(), "epub".into(), "pdf".into(), "zip".into()];
        assert_eq!(validate_extension("book.fb2", &allowed), Some("fb2".into()));
        assert_eq!(
            validate_extension("Book.EPUB", &allowed),
            Some("epub".into())
        );
        assert_eq!(validate_extension("a.pdf", &allowed), Some("pdf".into()));
    }

    #[test]
    fn test_validate_extension_invalid() {
        let allowed = vec!["fb2".into(), "epub".into()];
        assert_eq!(validate_extension("virus.exe", &allowed), None);
        assert_eq!(validate_extension("noext", &allowed), None);
        assert_eq!(validate_extension("bad.fb2.exe", &allowed), None);
    }

    #[test]
    fn test_validate_extension_non_alphanumeric() {
        let allowed = vec!["fb2".into()];
        // A crafted extension with non-alphanumeric chars
        assert_eq!(validate_extension("file.fb2;rm", &allowed), None);
    }

    #[test]
    fn test_sanitize_filename_normal() {
        assert_eq!(sanitize_filename("My Book.epub"), "My Book");
        assert_eq!(sanitize_filename("hello-world_v2.fb2"), "hello-world_v2");
    }

    #[test]
    fn test_sanitize_filename_path_traversal() {
        // file_stem() extracts only the final component's stem,
        // stripping directory traversal automatically
        assert_eq!(sanitize_filename("../../etc/passwd.fb2"), "passwd");
    }

    #[test]
    fn test_sanitize_filename_empty() {
        // ".fb2" is treated as a hidden file with no extension, stem is ".fb2"
        // After trimming leading dots -> "fb2"
        assert_eq!(sanitize_filename(".fb2"), "fb2");
        assert_eq!(sanitize_filename(""), "uploaded_book");
    }

    #[test]
    fn test_sanitize_filename_special_chars() {
        assert_eq!(sanitize_filename("<script>.fb2"), "_script_");
    }

    #[test]
    fn test_generate_token_hex() {
        let token = generate_token(b"test-secret");
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!token.is_empty());
        // HMAC-SHA256 always produces 64 hex chars
        assert_eq!(token.len(), 64);
    }

    #[test]
    fn test_generate_token_unique() {
        let secret = b"test-secret";
        // Tokens differ because of atomic counter even with same timestamp
        let t1 = generate_token(secret);
        let t2 = generate_token(secret);
        assert_ne!(t1, t2);
    }
}
