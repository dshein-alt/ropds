use super::*;

#[derive(Deserialize)]
pub struct UpdateBookGenresPayload {
    pub book_id: i64,
    pub genre_ids: Vec<i64>,
    #[serde(default)]
    pub csrf_token: String,
}

pub async fn update_book_genres(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<UpdateBookGenresPayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    if let Ok(None) | Err(_) =
        crate::db::queries::books::get_by_id(&state.db, payload.book_id).await
    {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    match crate::db::queries::genres::set_book_genres(
        &state.db,
        payload.book_id,
        &payload.genre_ids,
    )
    .await
    {
        Ok(()) => {
            let locale = jar
                .get("lang")
                .map(|c| c.value().to_string())
                .unwrap_or_else(|| state.config.web.language.clone());
            let updated =
                crate::db::queries::genres::get_for_book(&state.db, payload.book_id, &locale)
                    .await
                    .unwrap_or_default();
            axum::Json(serde_json::json!({
                "ok": true,
                "genres": updated,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to update genres for book {}: {e}", payload.book_id);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}

// ── Book author management (admin-only) ─────────────────────────────

#[derive(Deserialize)]
pub struct UpdateBookAuthorsPayload {
    pub book_id: i64,
    /// Existing author IDs to keep
    #[serde(default)]
    pub author_ids: Vec<i64>,
    /// New author names to add (will be created if they don't exist)
    #[serde(default)]
    pub new_authors: Vec<String>,
    #[serde(default)]
    pub csrf_token: String,
}

pub async fn update_book_authors(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<UpdateBookAuthorsPayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    if let Ok(None) | Err(_) =
        crate::db::queries::books::get_by_id(&state.db, payload.book_id).await
    {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    // Resolve new author names to IDs (create if needed)
    let mut all_ids = payload.author_ids.clone();
    for name in &payload.new_authors {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        match crate::scanner::ensure_author(&state.db, trimmed).await {
            Ok(id) => {
                if !all_ids.contains(&id) {
                    all_ids.push(id);
                }
            }
            Err(e) => {
                tracing::error!("Failed to ensure author '{}': {e}", trimmed);
            }
        }
    }

    // A book must have at least one author
    if all_ids.is_empty() {
        match crate::scanner::ensure_author(&state.db, "Unknown").await {
            Ok(id) => all_ids.push(id),
            Err(e) => {
                tracing::error!("Failed to ensure fallback author: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(serde_json::json!({"ok": false})),
                )
                    .into_response();
            }
        }
    }

    match crate::db::queries::books::set_book_authors_and_update_key(
        &state.db,
        payload.book_id,
        &all_ids,
    )
    .await
    {
        Ok(()) => {
            let updated = crate::db::queries::authors::get_for_book(&state.db, payload.book_id)
                .await
                .unwrap_or_default();
            axum::Json(serde_json::json!({
                "ok": true,
                "authors": updated,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to update authors for book {}: {e}", payload.book_id);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}

// ── Book series management (admin-only) ─────────────────────────────

#[derive(Deserialize)]
pub struct UpdateBookSeriesPayload {
    pub book_id: i64,
    #[serde(default)]
    pub series_name: String,
    #[serde(default)]
    pub series_no: i32,
    #[serde(default)]
    pub csrf_token: String,
}

pub async fn update_book_series(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<UpdateBookSeriesPayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    if let Ok(None) | Err(_) =
        crate::db::queries::books::get_by_id(&state.db, payload.book_id).await
    {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    let name = payload.series_name.trim();
    match crate::db::queries::series::set_book_series(
        &state.db,
        payload.book_id,
        name,
        payload.series_no,
    )
    .await
    {
        Ok(()) => {
            let updated = crate::db::queries::series::get_for_book(&state.db, payload.book_id)
                .await
                .unwrap_or_default();
            let series_json: Vec<serde_json::Value> = updated
                .into_iter()
                .map(|(s, ser_no)| {
                    serde_json::json!({
                        "id": s.id,
                        "ser_name": s.ser_name,
                        "ser_no": ser_no,
                    })
                })
                .collect();
            axum::Json(serde_json::json!({
                "ok": true,
                "series": series_json,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to update series for book {}: {e}", payload.book_id);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct SeriesSearchQuery {
    #[serde(default)]
    pub q: String,
}

pub async fn series_search(
    State(state): State<AppState>,
    Query(params): Query<SeriesSearchQuery>,
) -> Response {
    let term = params.q.trim().to_uppercase();
    if term.len() < 2 {
        return axum::Json(serde_json::json!({"ok": true, "series": []})).into_response();
    }
    let results = crate::db::queries::series::search_by_name(&state.db, &term, 20, 0)
        .await
        .unwrap_or_default();
    let series_json: Vec<serde_json::Value> = results
        .into_iter()
        .map(|s| serde_json::json!({"id": s.id, "ser_name": s.ser_name}))
        .collect();
    axum::Json(serde_json::json!({"ok": true, "series": series_json})).into_response()
}

// ── Book title management (admin-only) ──────────────────────────────

#[derive(Deserialize)]
pub struct UpdateBookTitlePayload {
    pub book_id: i64,
    pub title: String,
    #[serde(default)]
    pub csrf_token: String,
}

pub async fn update_book_title(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<UpdateBookTitlePayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false, "error": "csrf"})),
        )
            .into_response();
    }

    // Validate title
    let title = match validate_book_title(&payload.title) {
        Ok(t) => t,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({"ok": false, "error": err})),
            )
                .into_response();
        }
    };

    // Check book exists
    if let Ok(None) | Err(_) =
        crate::db::queries::books::get_by_id(&state.db, payload.book_id).await
    {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    let search_title = title.to_uppercase();
    let lang_code = crate::scanner::parsers::detect_lang_code(&title);
    match crate::db::queries::books::update_title(
        &state.db,
        payload.book_id,
        &title,
        &search_title,
        lang_code,
    )
    .await
    {
        Ok(()) => axum::Json(serde_json::json!({
            "ok": true,
            "title": title,
        }))
        .into_response(),
        Err(e) => {
            tracing::error!("Failed to update title for book {}: {e}", payload.book_id);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}
