use super::*;

/// GET /web/download/:book_id/:zip_flag — download a book via the web UI.
///
/// Reuses the OPDS download helpers for file reading and response building.
/// Tracks the download on the user's bookshelf via session cookie.
pub async fn web_download(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((book_id, zip_flag)): Path<(i64, i32)>,
) -> Response {
    let book = match books::get_by_id(&state.db, book_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "Book not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB error").into_response(),
    };

    let root = &state.config.library.root_path;

    let data = match crate::opds::download::read_book_file(
        root,
        &book.path,
        &book.filename,
        book.cat_type,
    ) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to read book {}: {e}", book_id);
            return (StatusCode::NOT_FOUND, "File not found").into_response();
        }
    };

    // Fire-and-forget bookshelf tracking via session cookie
    let secret = state.config.server.session_secret.as_bytes();
    if let Some(user_id) = jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        let _ = bookshelf::upsert(&state.db, user_id, book_id).await;
    }

    let download_name =
        crate::opds::download::title_to_filename(&book.title, &book.format, &book.filename);
    let mime = crate::opds::xml::mime_for_format(&book.format);

    if zip_flag == 1 && !crate::opds::xml::is_nozip_format(&book.format) {
        match crate::opds::download::wrap_in_zip(&book.filename, &data) {
            Ok(zipped) => {
                let zip_name = format!("{download_name}.zip");
                let zip_mime = crate::opds::xml::mime_for_zip(&book.format);
                crate::opds::download::file_response(&zipped, &zip_name, &zip_mime)
            }
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "ZIP error").into_response(),
        }
    } else {
        crate::opds::download::file_response(&data, &download_name, mime)
    }
}

// ── Reader ─────────────────────────────────────────────────────────

/// Supported formats for the embedded reader.
fn is_reader_format(format: &str) -> bool {
    matches!(format, "epub" | "fb2" | "mobi" | "djvu" | "pdf")
}

/// GET /web/reader/:book_id — reader page
pub async fn web_reader(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(book_id): Path<i64>,
    Query(params): Query<ReaderOpenParams>,
) -> Response {
    if !state.config.reader.enable {
        return (StatusCode::NOT_FOUND, "Reader is disabled").into_response();
    }

    let book = match books::get_by_id(&state.db, book_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "Book not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB error").into_response(),
    };

    if !is_reader_format(&book.format) {
        return (StatusCode::BAD_REQUEST, "Unsupported format for reader").into_response();
    }

    let secret = state.config.server.session_secret.as_bytes();
    let mut saved_position = String::new();
    let mut saved_progress: f64 = 0.0;
    let mut recent_books = Vec::new();

    if let Some(user_id) = jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        if let Ok(Some(pos)) = reading_positions::get_position(&state.db, user_id, book_id).await {
            saved_position = pos.position.clone();
            saved_progress = pos.progress;
        }
        // Touch/create the reading position so the book appears in "last read"
        // immediately, even before the JS client sends its first position update.
        let _ = reading_positions::save_position(
            &state.db,
            user_id,
            book_id,
            &saved_position,
            saved_progress,
            state.config.reader.read_history_max,
        )
        .await;
        recent_books = reading_positions::get_recent(&state.db, user_id, 10)
            .await
            .unwrap_or_default();
    }

    let book_authors = authors::get_for_book(&state.db, book.id)
        .await
        .unwrap_or_default();
    let authors_str: String = book_authors
        .iter()
        .map(|a| a.full_name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());
    let t = crate::web::i18n::get_locale(&state.translations, &locale);
    let theme = &state.config.web.theme;

    let mut ctx = tera::Context::new();
    ctx.insert("t", t);
    ctx.insert("locale", &locale);
    ctx.insert("default_theme", theme);
    ctx.insert("app_title", &state.config.opds.title);
    ctx.insert("version", env!("CARGO_PKG_VERSION"));
    ctx.insert("book_id", &book.id);
    ctx.insert("book_title", &book.title);
    ctx.insert("book_format", &book.format);
    ctx.insert("book_authors", &authors_str);
    ctx.insert("saved_position", &saved_position);
    ctx.insert("saved_progress", &saved_progress);
    ctx.insert("recent_books", &recent_books);
    let back_url = sanitize_internal_redirect(params.return_to.as_deref());
    ctx.insert("back_url", &back_url);

    // CSRF token for position save API
    if let Some(cookie) = jar.get("session") {
        ctx.insert(
            "csrf_token",
            &crate::web::context::generate_csrf_token(cookie.value(), secret),
        );
    }

    match render(&state.tera, "web/reader.html", &ctx) {
        Ok(html) => html.into_response(),
        Err(status) => status.into_response(),
    }
}

/// GET /web/read/:book_id — serve book file inline for the reader
pub async fn web_read_inline(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(book_id): Path<i64>,
) -> Response {
    if !state.config.reader.enable {
        return (StatusCode::NOT_FOUND, "Reader is disabled").into_response();
    }

    let book = match books::get_by_id(&state.db, book_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "Book not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB error").into_response(),
    };

    let root = &state.config.library.root_path;
    let data = match crate::opds::download::read_book_file(
        root,
        &book.path,
        &book.filename,
        book.cat_type,
    ) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to read book {}: {e}", book_id);
            return (StatusCode::NOT_FOUND, "File not found").into_response();
        }
    };

    // Fire-and-forget bookshelf tracking
    let secret = state.config.server.session_secret.as_bytes();
    if let Some(user_id) = jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        let _ = bookshelf::upsert(&state.db, user_id, book_id).await;
    }

    let mime = crate::opds::xml::mime_for_format(&book.format);
    let filename =
        crate::opds::download::title_to_filename(&book.title, &book.format, &book.filename);
    let content_disposition = format!("inline; filename=\"{filename}\"");

    (
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                format!("{mime}; name=\"{filename}\""),
            ),
            (axum::http::header::CONTENT_DISPOSITION, content_disposition),
            (axum::http::header::CONTENT_LENGTH, data.len().to_string()),
        ],
        data,
    )
        .into_response()
}

// ── Reading Position API ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct SavePositionRequest {
    pub book_id: i64,
    pub position: String,
    pub progress: f64,
    pub csrf_token: String,
}

/// POST /web/api/reading-position — save reading position (AJAX JSON)
pub async fn save_reading_position(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(body): axum::Json<SavePositionRequest>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    let user_id = match jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        Some(id) => id,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    if !crate::web::context::validate_csrf(&jar, secret, &body.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let max = state.config.reader.read_history_max;
    match reading_positions::save_position(
        &state.db,
        user_id,
        body.book_id,
        &body.position,
        body.progress,
        max,
    )
    .await
    {
        Ok(()) => axum::Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::warn!("Failed to save reading position: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}

/// GET /web/api/reading-position/:book_id — get saved position (AJAX JSON)
pub async fn get_reading_position(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(book_id): Path<i64>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    let user_id = match jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        Some(id) => id,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    match reading_positions::get_position(&state.db, user_id, book_id).await {
        Ok(Some(pos)) => axum::Json(serde_json::json!({
            "position": pos.position,
            "progress": pos.progress,
        }))
        .into_response(),
        Ok(None) => axum::Json(serde_json::json!({
            "position": null,
            "progress": 0.0,
        }))
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// GET /web/api/reading-history — get recent reading history (AJAX JSON)
pub async fn get_reading_history(State(state): State<AppState>, jar: CookieJar) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    let user_id = match jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        Some(id) => id,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    let recent = reading_positions::get_recent(&state.db, user_id, 10)
        .await
        .unwrap_or_default();
    axum::Json(recent).into_response()
}
