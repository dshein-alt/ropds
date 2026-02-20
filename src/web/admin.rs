use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;

use crate::db::queries::users;
use crate::state::AppState;
use crate::web::auth::verify_session;
use crate::web::context::{build_context, validate_csrf};

/// Middleware: require superuser for admin routes.
pub async fn require_superuser(
    State(state): State<AppState>,
    jar: CookieJar,
    request: Request,
    next: Next,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();

    let is_super = jar
        .get("session")
        .and_then(|c| verify_session(c.value(), secret))
        .map(|uid| async move { users::is_superuser(&state.db, uid).await.unwrap_or(false) });

    let authorized = match is_super {
        Some(fut) => fut.await,
        None => false,
    };

    if !authorized {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    next.run(request).await
}

/// Helper: extract current user_id from session cookie.
fn get_session_user_id(jar: &CookieJar, secret: &[u8]) -> Option<i64> {
    jar.get("session")
        .and_then(|c| verify_session(c.value(), secret))
}

/// Validate password length (8-32 characters).
fn is_valid_password(password: &str) -> bool {
    let len = password.chars().count();
    (8..=32).contains(&len)
}

/// Validate book title: non-empty, max 256 chars, no control characters.
/// Returns the trimmed title on success, or an error message.
pub(crate) fn validate_book_title(title: &str) -> Result<String, &'static str> {
    let trimmed = title.trim().to_string();
    if trimmed.is_empty() {
        return Err("title_empty");
    }
    if trimmed.chars().count() > 256 {
        return Err("title_too_long");
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("title_invalid");
    }
    Ok(trimmed)
}

/// GET /web/admin — render admin panel.
pub async fn admin_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "admin").await;

    // Load all users (view struct excludes password_hash)
    let all_users = users::get_all_views(&state.db).await.unwrap_or_default();
    ctx.insert("users", &all_users);

    // Current user id (to prevent self-delete in template)
    let secret = state.config.server.session_secret.as_bytes();
    let current_user_id = match get_session_user_id(&jar, secret) {
        Some(id) => id,
        None => {
            tracing::warn!(
                "admin_page reached without valid session — middleware misconfiguration?"
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    ctx.insert("current_user_id", &current_user_id);

    // Server config sections (read-only display)
    ctx.insert(
        "cfg_uptime",
        &format_uptime(state.started_at.elapsed().as_secs(), &ctx),
    );
    ctx.insert("cfg_host", &state.config.server.host);
    ctx.insert("cfg_port", &state.config.server.port);
    ctx.insert("cfg_log_level", &state.config.server.log_level);
    ctx.insert(
        "cfg_pdf_preview_tool_available",
        &state.pdf_preview_tool_available,
    );
    ctx.insert(
        "cfg_djvu_preview_tool_available",
        &state.djvu_preview_tool_available,
    );

    ctx.insert(
        "cfg_root_path",
        &state.config.library.root_path.display().to_string(),
    );
    ctx.insert(
        "cfg_book_extensions",
        &state.config.library.book_extensions.join(", "),
    );
    ctx.insert("cfg_scan_zip", &state.config.library.scan_zip);
    ctx.insert("cfg_zip_codepage", &state.config.library.zip_codepage);
    ctx.insert("cfg_inpx_enable", &state.config.library.inpx_enable);
    ctx.insert(
        "cfg_covers_path",
        &state.config.library.covers_path.display().to_string(),
    );

    ctx.insert("cfg_opds_title", &state.config.opds.title);
    ctx.insert("cfg_opds_subtitle", &state.config.opds.subtitle);
    ctx.insert("cfg_max_items", &state.config.opds.max_items);
    ctx.insert("cfg_split_items", &state.config.opds.split_items);
    ctx.insert("cfg_auth_required", &state.config.opds.auth_required);
    ctx.insert("cfg_show_covers", &state.config.opds.show_covers);
    ctx.insert("cfg_alphabet_menu", &state.config.opds.alphabet_menu);
    ctx.insert("cfg_hide_doubles", &state.config.opds.hide_doubles);

    // Upload config
    ctx.insert("cfg_upload_allow_upload", &state.config.upload.allow_upload);
    ctx.insert(
        "cfg_upload_path",
        &state.config.upload.upload_path.display().to_string(),
    );
    ctx.insert(
        "cfg_upload_max_size_mb",
        &state.config.upload.max_upload_size_mb,
    );

    // Scanner config
    ctx.insert(
        "cfg_schedule_minutes",
        &state.config.scanner.schedule_minutes,
    );
    ctx.insert("cfg_schedule_hours", &state.config.scanner.schedule_hours);
    ctx.insert(
        "cfg_schedule_days",
        &state.config.scanner.schedule_day_of_week,
    );
    ctx.insert("cfg_delete_logical", &state.config.scanner.delete_logical);
    ctx.insert("is_scanning", &crate::scanner::is_scanning());

    match state.tera.render("web/admin.html", &ctx) {
        Ok(html) => Ok(Html(html)),
        Err(e) => {
            tracing::error!("Template error: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
pub struct CreateUserForm {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub is_superuser: Option<String>, // checkbox: present = "on", absent = None
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/users/create
pub async fn create_user(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<CreateUserForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    // Validate
    let username = form.username.trim();
    if username.is_empty() {
        return Redirect::to("/web/admin?error=username_empty").into_response();
    }
    if !is_valid_password(&form.password) {
        return Redirect::to("/web/admin?error=password_short").into_response();
    }

    let is_super = if form.is_superuser.is_some() { 1 } else { 0 };
    let hash = crate::password::hash(&form.password);
    let display_name = form.display_name.trim();

    match users::create(&state.db, username, &hash, is_super, display_name).await {
        Ok(_) => Redirect::to("/web/admin?msg=user_created").into_response(),
        Err(_) => Redirect::to("/web/admin?error=username_exists").into_response(),
    }
}

/// POST /web/admin/users/:id/password
pub async fn change_password(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(user_id): Path<i64>,
    axum::Form(form): axum::Form<ChangePasswordForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    if !is_valid_password(&form.password) {
        return Redirect::to("/web/admin?error=password_short").into_response();
    }

    let hash = crate::password::hash(&form.password);
    if let Err(e) = users::update_password(&state.db, user_id, &hash).await {
        tracing::error!("Failed to update password for user {user_id}: {e}");
        return Redirect::to("/web/admin?error=db_error").into_response();
    }

    Redirect::to("/web/admin?msg=password_changed").into_response()
}

#[derive(Deserialize)]
pub struct ChangePasswordForm {
    pub password: String,
    #[serde(default)]
    pub csrf_token: String,
}

#[derive(Deserialize)]
pub struct CsrfForm {
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/users/:id/delete
pub async fn delete_user(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(user_id): Path<i64>,
    axum::Form(form): axum::Form<CsrfForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    // Prevent self-deletion
    if let Some(current_id) = get_session_user_id(&jar, secret)
        && current_id == user_id
    {
        return Redirect::to("/web/admin?error=cannot_delete_self").into_response();
    }

    match users::delete(&state.db, user_id).await {
        Ok(_) => Redirect::to("/web/admin?msg=user_deleted").into_response(),
        Err(e) => {
            tracing::error!("Failed to delete user {user_id}: {e}");
            Redirect::to("/web/admin?error=db_error").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct ToggleUploadForm {
    #[serde(default)]
    pub allow_upload: Option<String>, // checkbox: present = "on", absent = None
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/users/:id/upload
pub async fn toggle_upload(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(user_id): Path<i64>,
    axum::Form(form): axum::Form<ToggleUploadForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    // Prevent toggling superuser upload permission (they always have it)
    if users::is_superuser(&state.db, user_id)
        .await
        .unwrap_or(false)
    {
        return Redirect::to("/web/admin").into_response();
    }

    let allow = if form.allow_upload.is_some() { 1 } else { 0 };
    match users::update_allow_upload(&state.db, user_id, allow).await {
        Ok(_) => Redirect::to("/web/admin?msg=upload_toggled").into_response(),
        Err(e) => {
            tracing::error!("Failed to toggle upload for user {user_id}: {e}");
            Redirect::to("/web/admin?error=db_error").into_response()
        }
    }
}

/// GET /web/profile — render profile page for authenticated users.
pub async fn profile_page(State(state): State<AppState>, jar: CookieJar) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if get_session_user_id(&jar, secret).is_none() {
        return Redirect::to("/web/login").into_response();
    }

    let ctx = build_context(&state, &jar, "profile").await;
    match state.tera.render("web/profile.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct DisplayNameForm {
    pub display_name: String,
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/profile/display-name — update own display name.
pub async fn profile_update_display_name(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<DisplayNameForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    let user_id = match get_session_user_id(&jar, secret) {
        Some(id) => id,
        None => return Redirect::to("/web/login").into_response(),
    };

    let display_name = form.display_name.trim();
    if let Err(e) = users::update_display_name(&state.db, user_id, display_name).await {
        tracing::error!("Failed to update display name for user {user_id}: {e}");
        return Redirect::to("/web/profile?error=db_error").into_response();
    }

    Redirect::to("/web/profile?msg=display_name_changed").into_response()
}

/// POST /web/profile/password — change own password.
pub async fn profile_change_password(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<ChangePasswordForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    let user_id = match get_session_user_id(&jar, secret) {
        Some(id) => id,
        None => return Redirect::to("/web/login").into_response(),
    };

    if !is_valid_password(&form.password) {
        return Redirect::to("/web/profile?error=password_short").into_response();
    }

    let hash = crate::password::hash(&form.password);
    if let Err(e) = users::update_password(&state.db, user_id, &hash).await {
        tracing::error!("Failed to update password for user {user_id}: {e}");
        return Redirect::to("/web/profile?error=db_error").into_response();
    }

    // Clear forced password change flag
    let _ = users::clear_password_change_required(&state.db, user_id).await;

    Redirect::to("/web/profile?msg=password_changed").into_response()
}

#[derive(Deserialize)]
pub struct ChangePasswordPageQuery {
    pub next: Option<String>,
    pub error: Option<String>,
    pub msg: Option<String>,
}

/// GET /web/change-password — forced password change page.
pub async fn change_password_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<ChangePasswordPageQuery>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if get_session_user_id(&jar, secret).is_none() {
        return Redirect::to("/web/login").into_response();
    }

    let mut ctx = build_context(&state, &jar, "change-password").await;
    ctx.insert("next", &query.next.unwrap_or_default());
    ctx.insert("error", &query.error.unwrap_or_default());
    ctx.insert("msg", &query.msg.unwrap_or_default());

    match state.tera.render("web/change_password.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct ChangePasswordSubmitForm {
    pub password: String,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub csrf_token: String,
}

// ── Book genre management (admin-only) ──────────────────────────────

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

    match crate::db::queries::authors::set_book_authors(&state.db, payload.book_id, &all_ids).await
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

/// POST /web/change-password — submit forced password change.
pub async fn change_password_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<ChangePasswordSubmitForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    let user_id = match get_session_user_id(&jar, secret) {
        Some(id) => id,
        None => return Redirect::to("/web/login").into_response(),
    };

    let next_param = form
        .next
        .as_deref()
        .filter(|n| !n.is_empty() && n.starts_with('/'))
        .map(|n| format!("&next={}", urlencoding::encode(n)))
        .unwrap_or_default();

    if !is_valid_password(&form.password) {
        return Redirect::to(&format!(
            "/web/change-password?error=password_short{next_param}"
        ))
        .into_response();
    }

    let hash = crate::password::hash(&form.password);
    if let Err(e) = users::update_password(&state.db, user_id, &hash).await {
        tracing::error!("Failed to update password for user {user_id}: {e}");
        return Redirect::to(&format!("/web/change-password?error=db_error{next_param}"))
            .into_response();
    }

    // Clear the forced password change flag
    if let Err(e) = users::clear_password_change_required(&state.db, user_id).await {
        tracing::error!("Failed to clear password_change_required for user {user_id}: {e}");
        return Redirect::to(&format!("/web/change-password?error=db_error{next_param}"))
            .into_response();
    }

    // Redirect to original destination or home
    let redirect_to = form
        .next
        .filter(|n| !n.is_empty() && n.starts_with('/'))
        .unwrap_or_else(|| "/web".to_string());

    Redirect::to(&redirect_to).into_response()
}

#[derive(Deserialize)]
pub struct ScanForm {
    #[serde(default)]
    pub csrf_token: String,
}

/// POST /web/admin/scan — trigger a manual scan.
pub async fn scan_now(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<ScanForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    if crate::scanner::is_scanning() {
        return Redirect::to("/web/admin?error=scan_already_running").into_response();
    }

    let pool = state.db.clone();
    let config = (*state.config).clone();
    tokio::spawn(async move {
        match crate::scanner::run_scan(&pool, &config).await {
            Ok(ref stats) => {
                tracing::info!(
                    "Manual scan finished: {} added, {} skipped, {} deleted, {} errors",
                    stats.books_added,
                    stats.books_skipped,
                    stats.books_deleted,
                    stats.errors,
                );
                crate::scanner::store_scan_result(crate::scanner::ScanResult {
                    ok: true,
                    stats: Some(stats.clone()),
                    error: None,
                });
            }
            Err(ref e) => {
                tracing::error!("Manual scan failed: {e}");
                crate::scanner::store_scan_result(crate::scanner::ScanResult {
                    ok: false,
                    stats: None,
                    error: Some(e.to_string()),
                });
            }
        }
    });

    Redirect::to("/web/admin?msg=scan_started").into_response()
}

/// GET /web/admin/scan-status — returns JSON scan status for polling.
pub async fn scan_status() -> impl IntoResponse {
    let scanning = crate::scanner::is_scanning();
    let mut resp = serde_json::json!({ "scanning": scanning });
    if !scanning && let Some(result) = crate::scanner::take_last_scan_result() {
        resp["result"] = serde_json::to_value(result).unwrap_or_default();
    }
    axum::Json(resp)
}

// ── Genre translation management (admin-only) ──────────────────────

/// GET /web/admin/genres — JSON dump of all sections/genres/translations.
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

/// Format elapsed seconds as human-readable uptime using translations from context.
fn format_uptime(total_secs: u64, ctx: &tera::Context) -> String {
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;

    // Extract translation keys from context (t.admin.uptime_days, etc.)
    let t = ctx.get("t").and_then(|v| v.as_object());
    let admin = t.and_then(|t| t.get("admin")).and_then(|v| v.as_object());
    let label = |key: &str, fallback: &str| -> String {
        admin
            .and_then(|a| a.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or(fallback)
            .to_string()
    };

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days} {}", label("uptime_days", "d")));
    }
    if hours > 0 {
        parts.push(format!("{hours} {}", label("uptime_hours", "h")));
    }
    parts.push(format!("{minutes} {}", label("uptime_minutes", "min")));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        Config, DatabaseConfig, LibraryConfig, OpdsConfig, ScannerConfig, ServerConfig,
        UploadConfig, WebConfig,
    };
    use crate::db::{DbPool, create_test_pool};
    use crate::web::auth::sign_session;
    use crate::web::context::generate_csrf_token;
    use axum_extra::extract::cookie::{Cookie, CookieJar};
    use http_body_util::BodyExt;
    use std::path::PathBuf;

    fn test_state(pool: DbPool) -> AppState {
        let config = Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                log_level: "info".to_string(),
                session_secret: "test-secret".to_string(),
                session_ttl_hours: 24,
            },
            library: LibraryConfig {
                root_path: PathBuf::from("/tmp/books"),
                covers_path: PathBuf::from("/tmp/covers"),
                book_extensions: vec!["fb2".to_string()],
                scan_zip: true,
                zip_codepage: "cp866".to_string(),
                inpx_enable: false,
            },
            database: DatabaseConfig {
                url: "sqlite::memory:".to_string(),
            },
            opds: OpdsConfig {
                title: "ROPDS".to_string(),
                subtitle: String::new(),
                max_items: 30,
                split_items: 300,
                auth_required: true,
                show_covers: true,
                alphabet_menu: true,
                hide_doubles: false,
            },
            scanner: ScannerConfig {
                schedule_minutes: vec![0],
                schedule_hours: vec![0],
                schedule_day_of_week: vec![],
                delete_logical: true,
                skip_unchanged: false,
                test_zip: false,
                test_files: false,
                workers_num: 1,
            },
            web: WebConfig {
                language: "en".to_string(),
                theme: "light".to_string(),
            },
            upload: UploadConfig {
                allow_upload: true,
                upload_path: PathBuf::from("/tmp/uploads"),
                max_upload_size_mb: 10,
            },
        };

        let tera = tera::Tera::default();
        let mut translations = crate::web::i18n::Translations::new();
        translations.insert("en".to_string(), serde_json::json!({}));
        AppState::new(config, pool, tera, translations, false, false)
    }

    async fn insert_test_book(pool: &DbPool, title: &str) -> i64 {
        let cat_path = format!("/admin-{title}");
        let sql = pool.sql("INSERT INTO catalogs (path, cat_name) VALUES (?, 'admin')");
        sqlx::query(&sql)
            .bind(&cat_path)
            .execute(pool.inner())
            .await
            .unwrap();

        let sql = pool.sql("SELECT id FROM catalogs WHERE path = ?");
        let (catalog_id,): (i64,) = sqlx::query_as(&sql)
            .bind(&cat_path)
            .fetch_one(pool.inner())
            .await
            .unwrap();

        let search_title = title.to_uppercase();
        let sql = pool.sql(
            "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
             lang, lang_code, size, avail, cat_type, cover, cover_type) \
             VALUES (?, ?, '/admin', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
        );
        sqlx::query(&sql)
            .bind(catalog_id)
            .bind(format!("{title}.fb2"))
            .bind(title)
            .bind(search_title)
            .execute(pool.inner())
            .await
            .unwrap();

        let sql = pool.sql("SELECT id FROM books WHERE catalog_id = ? AND title = ?");
        let (book_id,): (i64,) = sqlx::query_as(&sql)
            .bind(catalog_id)
            .bind(title)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        book_id
    }

    async fn response_json(resp: Response) -> serde_json::Value {
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&body).unwrap()
    }

    #[test]
    fn test_is_valid_password_boundaries() {
        assert!(!is_valid_password("1234567"));
        assert!(is_valid_password("12345678"));
        assert!(is_valid_password(&"x".repeat(32)));
        assert!(!is_valid_password(&"x".repeat(33)));
    }

    #[test]
    fn test_validate_book_title_rules() {
        assert_eq!(
            validate_book_title("  The Title  ").unwrap(),
            "The Title".to_string()
        );
        assert_eq!(validate_book_title("   ").unwrap_err(), "title_empty");
        assert_eq!(
            validate_book_title(&"a".repeat(257)).unwrap_err(),
            "title_too_long"
        );
        assert_eq!(
            validate_book_title("abc\u{0007}def").unwrap_err(),
            "title_invalid"
        );
    }

    #[test]
    fn test_get_session_user_id_valid_and_invalid() {
        let secret = b"session-secret-for-tests";
        let token = sign_session(42, secret, 1);
        let jar = CookieJar::new().add(Cookie::new("session", token));
        assert_eq!(get_session_user_id(&jar, secret), Some(42));

        let invalid = CookieJar::new().add(Cookie::new("session", "bad-token"));
        assert_eq!(get_session_user_id(&invalid, secret), None);
    }

    #[test]
    fn test_format_uptime_with_translations() {
        let mut ctx = tera::Context::new();
        let t = serde_json::json!({
            "admin": {
                "uptime_days": "days",
                "uptime_hours": "hours",
                "uptime_minutes": "mins"
            }
        });
        ctx.insert("t", &t);

        assert_eq!(format_uptime(90_061, &ctx), "1 days 1 hours 1 mins");
        assert_eq!(format_uptime(3_600, &ctx), "1 hours 0 mins");
    }

    #[test]
    fn test_format_uptime_fallback_labels() {
        let ctx = tera::Context::new();
        assert_eq!(format_uptime(172_920, &ctx), "2 d 2 min");
    }

    #[tokio::test]
    async fn test_update_book_series_handler_assign_and_remove() {
        let pool = create_test_pool().await;
        let state = test_state(pool.clone());
        let book_id = insert_test_book(&pool, "series-handler").await;

        let secret = state.config.server.session_secret.as_bytes();
        let session = sign_session(1, secret, 24);
        let csrf_token = generate_csrf_token(&session, secret);
        let jar = CookieJar::new().add(Cookie::new("session", session.clone()));

        let resp = update_book_series(
            State(state.clone()),
            jar.clone(),
            axum::Json(UpdateBookSeriesPayload {
                book_id,
                series_name: "Foundation".to_string(),
                series_no: 2,
                csrf_token: csrf_token.clone(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["ok"], true);
        assert_eq!(json["series"][0]["ser_name"], "Foundation");
        assert_eq!(json["series"][0]["ser_no"], 2);

        let linked = crate::db::queries::series::get_for_book(&pool, book_id)
            .await
            .unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].0.ser_name, "Foundation");
        assert_eq!(linked[0].1, 2);

        let resp = update_book_series(
            State(state),
            jar,
            axum::Json(UpdateBookSeriesPayload {
                book_id,
                series_name: String::new(),
                series_no: 0,
                csrf_token,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["ok"], true);
        assert_eq!(json["series"].as_array().unwrap().len(), 0);

        let linked = crate::db::queries::series::get_for_book(&pool, book_id)
            .await
            .unwrap();
        assert!(linked.is_empty());
        assert!(
            crate::db::queries::series::find_by_name(&pool, "Foundation")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_series_search_handler_short_and_match() {
        let pool = create_test_pool().await;
        let state = test_state(pool.clone());
        let book_id = insert_test_book(&pool, "series-search").await;
        crate::db::queries::series::set_book_series(&pool, book_id, "Foundations", 1)
            .await
            .unwrap();

        let resp = series_search(
            State(state.clone()),
            Query(SeriesSearchQuery { q: "f".into() }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["ok"], true);
        assert_eq!(json["series"].as_array().unwrap().len(), 0);

        let resp = series_search(State(state), Query(SeriesSearchQuery { q: "fo".into() })).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["ok"], true);
        let results = json["series"].as_array().unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0]["ser_name"], "Foundations");
    }
}
