use super::*;

pub async fn genres_admin_json(State(state): State<AppState>) -> Response {
    let sections = crate::db::queries::genres::get_all_sections(&state.db)
        .await
        .unwrap_or_default();

    let languages = crate::db::queries::genres::get_available_languages(&state.db)
        .await
        .unwrap_or_default();

    let all_genres = crate::db::queries::genres::get_all_admin(&state.db)
        .await
        .unwrap_or_default();

    let mut section_data = Vec::new();
    for section in &sections {
        let translations =
            crate::db::queries::genres::get_section_translations(&state.db, section.id)
                .await
                .unwrap_or_default();

        let genre_items: Vec<serde_json::Value> = all_genres
            .iter()
            .filter(|(_, _, sc, _)| sc == &section.code)
            .map(|(id, code, _, trans)| {
                serde_json::json!({
                    "id": id,
                    "code": code,
                    "translations": trans.iter().map(|t| serde_json::json!({
                        "lang": t.lang,
                        "name": t.name,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();

        section_data.push(serde_json::json!({
            "id": section.id,
            "code": section.code,
            "translations": translations.iter().map(|t| serde_json::json!({
                "lang": t.lang,
                "name": t.name,
            })).collect::<Vec<serde_json::Value>>(),
            "genres": genre_items,
        }));
    }

    axum::Json(serde_json::json!({
        "sections": section_data,
        "languages": languages,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct UpsertTranslationPayload {
    #[serde(default)]
    pub section_id: Option<i64>,
    #[serde(default)]
    pub genre_id: Option<i64>,
    pub lang: String,
    pub name: String,
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/genre-translation — upsert a section or genre translation.
pub async fn upsert_genre_translation(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<UpsertTranslationPayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    let lang = payload.lang.trim();
    let name = payload.name.trim();
    if lang.is_empty() || name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"ok": false, "error": "lang and name required"})),
        )
            .into_response();
    }

    let result = if let Some(section_id) = payload.section_id {
        crate::db::queries::genres::upsert_section_translation(&state.db, section_id, lang, name)
            .await
    } else if let Some(genre_id) = payload.genre_id {
        crate::db::queries::genres::upsert_genre_translation(&state.db, genre_id, lang, name).await
    } else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(
                serde_json::json!({"ok": false, "error": "section_id or genre_id required"}),
            ),
        )
            .into_response();
    };

    match result {
        Ok(()) => axum::Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::error!("Failed to upsert translation: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct DeleteTranslationPayload {
    #[serde(default)]
    pub section_id: Option<i64>,
    #[serde(default)]
    pub genre_id: Option<i64>,
    pub lang: String,
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/genre-translation/delete — delete a section or genre translation.
pub async fn delete_genre_translation(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<DeleteTranslationPayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    let result = if let Some(section_id) = payload.section_id {
        crate::db::queries::genres::delete_section_translation(&state.db, section_id, &payload.lang)
            .await
    } else if let Some(genre_id) = payload.genre_id {
        crate::db::queries::genres::delete_genre_translation(&state.db, genre_id, &payload.lang)
            .await
    } else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    };

    match result {
        Ok(()) => axum::Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::error!("Failed to delete translation: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct CreateGenrePayload {
    pub code: String,
    pub section_id: i64,
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/genre — create a new genre in a section.
pub async fn create_genre(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<CreateGenrePayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    let code = payload.code.trim();
    if code.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"ok": false, "error": "code required"})),
        )
            .into_response();
    }

    match crate::db::queries::genres::create_genre(&state.db, code, payload.section_id).await {
        Ok(id) => axum::Json(serde_json::json!({"ok": true, "id": id})).into_response(),
        Err(e) if e.to_string().contains("UNIQUE constraint") => (
            StatusCode::CONFLICT,
            axum::Json(serde_json::json!({"ok": false, "error": "duplicate"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to create genre: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct DeleteGenrePayload {
    pub genre_id: i64,
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/genre/delete — delete a genre.
pub async fn delete_genre(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<DeleteGenrePayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    match crate::db::queries::genres::delete_genre(&state.db, payload.genre_id).await {
        Ok(()) => axum::Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::error!("Failed to delete genre: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct CreateSectionPayload {
    pub code: String,
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/section — create a new genre section.
pub async fn create_section(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<CreateSectionPayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    let code = payload.code.trim();
    if code.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"ok": false, "error": "code required"})),
        )
            .into_response();
    }

    match crate::db::queries::genres::create_section(&state.db, code).await {
        Ok(id) => axum::Json(serde_json::json!({"ok": true, "id": id})).into_response(),
        Err(e) if e.to_string().contains("UNIQUE constraint") => (
            StatusCode::CONFLICT,
            axum::Json(serde_json::json!({"ok": false, "error": "duplicate"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to create section: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct DeleteSectionPayload {
    pub section_id: i64,
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/section/delete — delete a genre section.
pub async fn delete_section(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Json(payload): axum::Json<DeleteSectionPayload>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &payload.csrf_token) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"ok": false})),
        )
            .into_response();
    }

    match crate::db::queries::genres::delete_section(&state.db, payload.section_id).await {
        Ok(()) => axum::Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::error!("Failed to delete section: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"ok": false})),
            )
                .into_response()
        }
    }
}
