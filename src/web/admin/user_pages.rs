use super::*;

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
        &state.config.covers.covers_path.display().to_string(),
    );
    ctx.insert(
        "cfg_cover_max_dimension_px",
        &state.config.covers.cover_max_dimension_px,
    );
    ctx.insert(
        "cfg_cover_jpeg_quality",
        &state.config.covers.cover_jpeg_quality,
    );
    ctx.insert("cfg_show_covers", &state.config.covers.show_covers);

    ctx.insert("cfg_opds_title", &state.config.opds.title);
    ctx.insert("cfg_opds_subtitle", &state.config.opds.subtitle);
    ctx.insert("cfg_max_items", &state.config.opds.max_items);
    ctx.insert("cfg_split_items", &state.config.opds.split_items);
    ctx.insert("cfg_auth_required", &state.config.opds.auth_required);
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

    // OAuth access requests (for Access Requests accordion)
    let pending_identities = crate::db::queries::oauth::list_by_status(&state.db, "pending")
        .await
        .unwrap_or_default();
    let mut pending: Vec<serde_json::Value> = Vec::with_capacity(pending_identities.len());
    for item in pending_identities {
        let source_username = users::get_username(&state.db, item.user_id)
            .await
            .unwrap_or_default();
        pending.push(serde_json::json!({
            "id": item.id,
            "user_id": item.user_id,
            "provider": item.provider,
            "provider_uid": item.provider_uid,
            "email": item.email,
            "display_name": item.display_name,
            "status": item.status,
            "rejected_at": item.rejected_at,
            "created_at": item.created_at,
            "source_username": source_username,
        }));
    }
    let rejected = crate::db::queries::oauth::list_by_status(&state.db, "rejected")
        .await
        .unwrap_or_default();
    let banned = crate::db::queries::oauth::list_by_status(&state.db, "banned")
        .await
        .unwrap_or_default();
    ctx.insert("pending", &pending);
    ctx.insert("rejected", &rejected);
    ctx.insert("banned", &banned);

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
    if !is_valid_username(username) {
        return Redirect::to("/web/admin?error=username_invalid").into_response();
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
    let user_id = match get_session_user_id(&jar, secret) {
        Some(id) => id,
        None => return Redirect::to("/web/login").into_response(),
    };

    let mut ctx = build_context(&state, &jar, "profile").await;

    // Check if user has an active OAuth identity (for OPDS Access card)
    let identities = crate::db::queries::oauth::list_for_user(&state.db, user_id)
        .await
        .unwrap_or_default();
    let is_oauth_user = identities.iter().any(|i| i.status == "active");
    ctx.insert("is_oauth_user", &is_oauth_user);
    let base = &state.config.server.base_url;
    ctx.insert("opds_url", &format!("{base}/opds"));
    ctx.insert("opds_v2_url", &format!("{base}/opds/v2"));

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
pub struct OpdsResetForm {
    pub csrf_token: String,
}

/// POST /web/profile/opds-reset
/// Generates a new random OPDS password for OAuth users. Shows it once.
pub async fn opds_password_reset(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<OpdsResetForm>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    let user_id = match get_session_user_id(&jar, secret) {
        Some(id) => id,
        None => return Redirect::to("/web/login").into_response(),
    };

    // Only for OAuth users (must have at least one active oauth_identity).
    let identities = crate::db::queries::oauth::list_for_user(&state.db, user_id)
        .await
        .unwrap_or_default();
    let is_oauth_user = identities.iter().any(|i| i.status == "active");
    if !is_oauth_user {
        return (StatusCode::FORBIDDEN, "Not an OAuth user").into_response();
    }

    let new_password = crate::password::generate_opds_password();
    let new_hash = crate::password::hash(&new_password);

    if let Err(e) = users::update_password(&state.db, user_id, &new_hash).await {
        tracing::error!("OPDS password reset failed: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "Reset failed").into_response();
    }

    // Return the new password as JSON for inline display.
    let mut response = axum::Json(serde_json::json!({"password": new_password})).into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        axum::http::header::PRAGMA,
        axum::http::HeaderValue::from_static("no-cache"),
    );
    response
}
