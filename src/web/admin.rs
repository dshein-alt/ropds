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

mod book_edit;
mod genres;
mod scan;
mod user_pages;

pub use book_edit::*;
pub use genres::*;
pub use scan::*;
pub use user_pages::*;

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

include!("admin/tests.rs");
