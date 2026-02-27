use axum_extra::extract::cookie::CookieJar;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::Duration;
use tera::Context;

use crate::db::models::Author;
use crate::db::queries::{authors, books, counters, reading_positions};
use crate::state::AppState;
use crate::web::i18n;

type HmacSha256 = Hmac<Sha256>;
const CONTEXT_STATS_CACHE_KEY: &str = "web:context:stats";
const CONTEXT_STATS_TTL: Duration = Duration::from_secs(30);

/// Generate a CSRF token tied to the session value.
pub fn generate_csrf_token(session_value: &str, secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(b"csrf:");
    mac.update(session_value.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Validate a submitted CSRF token against the session.
pub fn validate_csrf(jar: &CookieJar, secret: &[u8], submitted: &str) -> bool {
    jar.get("session")
        .map(|c| generate_csrf_token(c.value(), secret) == submitted)
        .unwrap_or(false)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Stats {
    pub allbooks: i64,
    pub allauthors: i64,
    pub allgenres: i64,
    pub allseries: i64,
}

#[derive(Debug, Serialize)]
pub struct RandomBook {
    pub id: i64,
    pub title: String,
    pub cover: i32,
    pub annotation: String,
    pub authors: Vec<Author>,
}

/// Build a Tera context with all shared variables.
pub async fn build_context(state: &AppState, jar: &CookieJar, active_page: &str) -> Context {
    let mut ctx = Context::new();

    // Locale
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());
    let t = i18n::get_locale(&state.translations, &locale);
    let reader_read_badge = t
        .get("reader")
        .and_then(|v| v.get("read_badge"))
        .and_then(|v| v.as_str())
        .unwrap_or("read");
    ctx.insert("t", t);
    ctx.insert("locale", &locale);
    ctx.insert("available_locales", &["en", "ru"]);
    ctx.insert("reader_read_badge", reader_read_badge);

    // Theme (server only knows the default; JS handles runtime switching)
    let theme = &state.config.web.theme;
    ctx.insert("default_theme", theme);

    // Active page for navbar highlighting
    ctx.insert("active_page", active_page);
    // Navbar search target: title | author | series
    ctx.insert("search_target", "title");

    // App config
    ctx.insert("app_title", &state.config.opds.title);
    ctx.insert("show_covers", &state.config.covers.show_covers);
    ctx.insert("alphabet_menu", &state.config.opds.alphabet_menu);
    ctx.insert("split_items", &state.config.opds.split_items);
    ctx.insert("auth_required", &state.config.opds.auth_required);

    // Auth state for navbar (admin link / profile link) + CSRF token
    let secret = state.config.server.session_secret.as_bytes();
    let mut is_superuser: i32 = 0;
    let mut is_authenticated: i32 = 0;
    let mut display_name = String::new();
    let mut username = String::new();
    let mut user_allow_upload: i32 = 0;
    let mut last_read_book_id: i64 = 0;
    if let Some(cookie) = jar.get("session")
        && let Some(user_id) = crate::web::auth::verify_session(cookie.value(), secret)
    {
        is_authenticated = 1;
        if let Ok(Some(user)) = crate::db::queries::users::get_by_id(&state.db, user_id).await {
            if user.is_superuser == 1 {
                is_superuser = 1;
            }
            display_name = user.display_name;
            username = user.username;
            user_allow_upload = user.allow_upload;
        }
        // Last read book for Reader navbar button
        if state.config.reader.enable
            && let Ok(Some(bid)) =
                reading_positions::get_last_read_book_id(&state.db, user_id).await
        {
            last_read_book_id = bid;
        }
        ctx.insert("csrf_token", &generate_csrf_token(cookie.value(), secret));
    }
    ctx.insert("is_superuser", &is_superuser);
    ctx.insert("is_authenticated", &is_authenticated);
    ctx.insert("display_name", &display_name);
    ctx.insert("username", &username);

    // Upload permission: global config AND (admin OR user has allow_upload)
    let can_upload =
        state.config.upload.allow_upload && (is_superuser == 1 || user_allow_upload == 1);
    ctx.insert("can_upload", &can_upload);

    // Reader: navbar button links to last read book (opens in new tab)
    ctx.insert("reader_enabled", &state.config.reader.enable);
    ctx.insert("last_read_book_id", &last_read_book_id);

    // Stats from counters table
    let stats = if let Some(cached) = state.get_cached::<Stats>(CONTEXT_STATS_CACHE_KEY) {
        cached
    } else {
        let counters_list = counters::get_all(&state.db).await.unwrap_or_default();
        let computed = Stats {
            allbooks: counters_list
                .iter()
                .find(|c| c.name == "allbooks")
                .map(|c| c.value)
                .unwrap_or(0),
            allauthors: counters_list
                .iter()
                .find(|c| c.name == "allauthors")
                .map(|c| c.value)
                .unwrap_or(0),
            allgenres: counters_list
                .iter()
                .find(|c| c.name == "allgenres")
                .map(|c| c.value)
                .unwrap_or(0),
            allseries: counters_list
                .iter()
                .find(|c| c.name == "allseries")
                .map(|c| c.value)
                .unwrap_or(0),
        };
        state.set_cached(CONTEXT_STATS_CACHE_KEY, CONTEXT_STATS_TTL, &computed);
        computed
    };
    ctx.insert("stats", &stats);

    // Random book for footer
    if let Ok(Some(book)) = books::get_random(&state.db).await {
        let book_authors = authors::get_for_book(&state.db, book.id)
            .await
            .unwrap_or_default();
        let rb = RandomBook {
            id: book.id,
            title: book.title,
            cover: book.cover,
            annotation: book.annotation.chars().take(300).collect(),
            authors: book_authors,
        };
        ctx.insert("random_book", &rb);
    }

    // Version
    ctx.insert("version", env!("CARGO_PKG_VERSION"));

    ctx
}

/// Register custom Tera filters.
pub fn register_filters(tera: &mut tera::Tera) {
    tera.register_filter("filesizeformat", filesizeformat);
}

/// Tera filter: format bytes as human-readable file size.
fn filesizeformat(
    value: &tera::Value,
    _args: &std::collections::HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let bytes = value.as_i64().unwrap_or(0);
    let result = if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    };
    Ok(tera::Value::String(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_extra::extract::cookie::{Cookie, CookieJar};

    #[test]
    fn test_csrf_token_deterministic() {
        let secret = b"test-secret";
        let t1 = generate_csrf_token("session123", secret);
        let t2 = generate_csrf_token("session123", secret);
        assert_eq!(t1, t2);
    }

    #[test]
    fn test_csrf_token_differs_by_session() {
        let secret = b"test-secret";
        let t1 = generate_csrf_token("session-a", secret);
        let t2 = generate_csrf_token("session-b", secret);
        assert_ne!(t1, t2);
    }

    #[test]
    fn test_validate_csrf_valid() {
        let secret = b"test-secret";
        let session = "42:9999999999:abcdef";
        let token = generate_csrf_token(session, secret);
        let jar = CookieJar::new().add(Cookie::new("session", session.to_string()));
        assert!(validate_csrf(&jar, secret, &token));
    }

    #[test]
    fn test_validate_csrf_invalid() {
        let secret = b"test-secret";
        let session = "42:9999999999:abcdef";
        let jar = CookieJar::new().add(Cookie::new("session", session.to_string()));
        assert!(!validate_csrf(&jar, secret, "wrong-token"));
    }

    #[test]
    fn test_validate_csrf_no_session() {
        let secret = b"test-secret";
        let jar = CookieJar::new();
        assert!(!validate_csrf(&jar, secret, "any-token"));
    }
}
