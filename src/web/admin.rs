use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;

use crate::db::queries::users;
use crate::state::AppState;
use crate::web::auth::verify_session;
use crate::web::context::build_context;

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
    let current_user_id = get_session_user_id(&jar, secret).unwrap_or(0);
    ctx.insert("current_user_id", &current_user_id);

    // Server config sections (read-only display)
    ctx.insert("cfg_uptime", &format_uptime(state.started_at.elapsed().as_secs(), &ctx));
    ctx.insert("cfg_host", &state.config.server.host);
    ctx.insert("cfg_port", &state.config.server.port);
    ctx.insert("cfg_log_level", &state.config.server.log_level);

    ctx.insert("cfg_root_path", &state.config.library.root_path.display().to_string());
    ctx.insert("cfg_book_extensions", &state.config.library.book_extensions.join(", "));
    ctx.insert("cfg_scan_zip", &state.config.library.scan_zip);
    ctx.insert("cfg_zip_codepage", &state.config.library.zip_codepage);
    ctx.insert("cfg_inpx_enable", &state.config.library.inpx_enable);

    ctx.insert("cfg_opds_title", &state.config.opds.title);
    ctx.insert("cfg_opds_subtitle", &state.config.opds.subtitle);
    ctx.insert("cfg_max_items", &state.config.opds.max_items);
    ctx.insert("cfg_split_items", &state.config.opds.split_items);
    ctx.insert("cfg_auth_required", &state.config.opds.auth_required);
    ctx.insert("cfg_show_covers", &state.config.opds.show_covers);
    ctx.insert("cfg_alphabet_menu", &state.config.opds.alphabet_menu);
    ctx.insert("cfg_hide_doubles", &state.config.opds.hide_doubles);
    ctx.insert("cfg_cache_time", &state.config.opds.cache_time);
    ctx.insert("cfg_covers_dir", &state.config.opds.covers_dir.display().to_string());

    // Scanner config
    ctx.insert("cfg_schedule_minutes", &state.config.scanner.schedule_minutes);
    ctx.insert("cfg_schedule_hours", &state.config.scanner.schedule_hours);
    ctx.insert("cfg_schedule_days", &state.config.scanner.schedule_day_of_week);
    ctx.insert("cfg_delete_logical", &state.config.scanner.delete_logical);

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
}

/// POST /web/admin/users/create
pub async fn create_user(
    State(state): State<AppState>,
    axum::Form(form): axum::Form<CreateUserForm>,
) -> impl IntoResponse {
    // Validate
    let username = form.username.trim();
    if username.is_empty() {
        return Redirect::to("/web/admin?error=username_empty").into_response();
    }
    if form.password.chars().count() < 8 || form.password.chars().count() > 32 {
        return Redirect::to("/web/admin?error=password_short").into_response();
    }

    let is_super = if form.is_superuser.is_some() { 1 } else { 0 };
    let hash = crate::password::hash(&form.password);

    match users::create(&state.db, username, &hash, is_super).await {
        Ok(_) => Redirect::to("/web/admin?msg=user_created").into_response(),
        Err(_) => Redirect::to("/web/admin?error=username_exists").into_response(),
    }
}

/// POST /web/admin/users/:id/password
pub async fn change_password(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    axum::Form(form): axum::Form<ChangePasswordForm>,
) -> impl IntoResponse {
    if form.password.chars().count() < 8 || form.password.chars().count() > 32 {
        return Redirect::to("/web/admin?error=password_short");
    }

    let hash = crate::password::hash(&form.password);
    let _ = users::update_password(&state.db, user_id, &hash).await;
    Redirect::to("/web/admin?msg=password_changed")
}

#[derive(Deserialize)]
pub struct ChangePasswordForm {
    pub password: String,
}

/// POST /web/admin/users/:id/delete
pub async fn delete_user(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    // Prevent self-deletion
    let secret = state.config.server.session_secret.as_bytes();
    if let Some(current_id) = get_session_user_id(&jar, secret) {
        if current_id == user_id {
            return Redirect::to("/web/admin?error=cannot_delete_self");
        }
    }

    let _ = users::delete(&state.db, user_id).await;
    Redirect::to("/web/admin?msg=user_deleted")
}

/// GET /web/profile — render profile page for authenticated users.
pub async fn profile_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Html<String>, StatusCode> {
    let ctx = build_context(&state, &jar, "profile").await;
    match state.tera.render("web/profile.html", &ctx) {
        Ok(html) => Ok(Html(html)),
        Err(e) => {
            tracing::error!("Template error: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// POST /web/profile/password — change own password.
pub async fn profile_change_password(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<ChangePasswordForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    let user_id = match get_session_user_id(&jar, secret) {
        Some(id) => id,
        None => return Redirect::to("/web/login").into_response(),
    };

    if form.password.chars().count() < 8 || form.password.chars().count() > 32 {
        return Redirect::to("/web/profile?error=password_short").into_response();
    }

    let hash = crate::password::hash(&form.password);
    let _ = users::update_password(&state.db, user_id, &hash).await;
    Redirect::to("/web/profile?msg=password_changed").into_response()
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
