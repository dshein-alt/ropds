use axum::extract::{ConnectInfo, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use std::net::SocketAddr;

use crate::state::AppState;
use crate::web::i18n;

type HmacSha256 = Hmac<Sha256>;

/// Create a signed session cookie value: `{user_id}:{expiry}:{hex_signature}`.
pub fn sign_session(user_id: i64, secret: &[u8], ttl_hours: u64) -> String {
    let expiry = chrono::Utc::now().timestamp() + (ttl_hours * 3600) as i64;
    let payload = format!("{user_id}:{expiry}");
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("{payload}:{sig}")
}

/// Verify a signed session cookie value. Returns user_id if valid and not expired.
pub fn verify_session(cookie_value: &str, secret: &[u8]) -> Option<i64> {
    let parts: Vec<&str> = cookie_value.splitn(3, ':').collect();
    if parts.len() != 3 {
        return None;
    }
    let user_id: i64 = parts[0].parse().ok()?;
    let expiry: i64 = parts[1].parse().ok()?;
    let sig_hex = parts[2];

    // Check expiry
    if chrono::Utc::now().timestamp() > expiry {
        return None;
    }

    // Verify HMAC
    let payload = format!("{}:{}", parts[0], parts[1]);
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());

    let expected = hex::decode(sig_hex).ok()?;
    mac.verify_slice(&expected).ok()?;

    Some(user_id)
}

/// Middleware: require a valid session cookie for web routes.
/// Skips auth when `config.opds.auth_required` is false.
pub async fn session_auth_layer(
    State(state): State<AppState>,
    jar: CookieJar,
    request: Request,
    next: Next,
) -> Response {
    if !state.config.opds.auth_required {
        return next.run(request).await;
    }

    let path = request.uri().path().to_string();

    // Allow login page and set-language without auth
    // Note: paths are relative to the nested /web router (prefix already stripped)
    if path == "/login" || path.starts_with("/set-language") {
        return next.run(request).await;
    }

    let secret = state.config.server.session_secret.as_bytes();

    let user_id = jar
        .get("session")
        .and_then(|c| verify_session(c.value(), secret));

    match user_id {
        Some(uid) => {
            // Allow these paths even when password change is required
            if path == "/change-password" || path == "/profile/password" || path == "/logout" {
                return next.run(request).await;
            }

            // Check if user must change password before accessing the app.
            // Fail closed: DB errors are treated as "change required" to avoid
            // bypassing enforcement when the check cannot be trusted.
            let must_change = crate::db::queries::users::password_change_required(&state.db, uid)
                .await
                .unwrap_or(true);

            if must_change {
                let original_path = format!("/web{path}");
                let next_url = urlencoding::encode(&original_path);
                return Redirect::to(&format!("/web/change-password?next={next_url}"))
                    .into_response();
            }

            next.run(request).await
        }
        None => {
            // No valid session — redirect to login
            let original_path = format!("/web{path}");
            let next_url = urlencoding::encode(&original_path);
            Redirect::to(&format!("/web/login?next={next_url}")).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct LoginQuery {
    pub next: Option<String>,
    pub error: Option<String>,
}

/// GET /web/login — render the login form.
pub async fn login_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<LoginQuery>,
) -> impl IntoResponse {
    // If already authenticated, redirect to home
    if state.config.opds.auth_required {
        let secret = state.config.server.session_secret.as_bytes();
        if let Some(cookie) = jar.get("session")
            && verify_session(cookie.value(), secret).is_some()
        {
            return Html(String::new()).into_response();
        }
    }

    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());
    let t = i18n::get_locale(&state.translations, &locale);

    let mut ctx = tera::Context::new();
    ctx.insert("t", t);
    ctx.insert("locale", &locale);
    ctx.insert("app_title", &state.config.opds.title);
    ctx.insert("default_theme", &state.config.web.theme);
    ctx.insert("version", env!("CARGO_PKG_VERSION"));
    ctx.insert("next", &query.next.unwrap_or_default());
    ctx.insert("error", &query.error.unwrap_or_default());

    match state.tera.render("web/login.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    pub next: Option<String>,
}

/// POST /web/login — validate credentials and set session cookie.
pub async fn login_submit(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    axum::Form(form): axum::Form<LoginForm>,
) -> impl IntoResponse {
    let remote = addr.ip().to_string();
    let valid = verify_credentials(&state.db, &form.username, &form.password).await;

    if !valid {
        tracing::info!("{remote} Login failed: user={}", form.username);
        let next_val = form.next.as_deref().unwrap_or_default().to_string();
        let next = urlencoding::encode(&next_val);
        return (
            jar,
            Redirect::to(&format!("/web/login?error=1&next={next}")),
        )
            .into_response();
    }

    // Get user_id for the session
    let user_id = get_user_id(&state.db, &form.username).await.unwrap_or(0);

    tracing::info!("{remote} Login: user={}", form.username);

    // Record login timestamp
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let _ = crate::db::queries::users::update_last_login(&state.db, user_id, &now).await;

    let secret = state.config.server.session_secret.as_bytes();
    let ttl = state.config.server.session_ttl_hours;
    let token = sign_session(user_id, secret, ttl);

    let cookie = Cookie::build(("session", token))
        .path("/web")
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax);

    let redirect_to = form
        .next
        .filter(|n| !n.is_empty() && n.starts_with('/'))
        .unwrap_or_else(|| "/web/bookshelf".to_string());

    (jar.add(cookie), Redirect::to(&redirect_to)).into_response()
}

/// GET /web/logout — clear session and redirect to login.
pub async fn logout(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
) -> impl IntoResponse {
    let remote = addr.ip().to_string();
    let secret = state.config.server.session_secret.as_bytes();
    if let Some(uid) = jar
        .get("session")
        .and_then(|c| verify_session(c.value(), secret))
    {
        let name = crate::db::queries::users::get_username(&state.db, uid)
            .await
            .unwrap_or_else(|_| format!("uid={uid}"));
        tracing::info!("{remote} Logout: user={name}");
    }
    let cookie = Cookie::build(("session", "")).path("/web").http_only(true);
    (jar.remove(cookie), Redirect::to("/web/login"))
}

/// Verify username/password against the users table using argon2.
async fn verify_credentials(pool: &crate::db::DbPool, username: &str, password: &str) -> bool {
    let result: Result<Option<(String,)>, _> =
        sqlx::query_as("SELECT password_hash FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await;

    match result {
        Ok(Some((stored_hash,))) => crate::password::verify(password, &stored_hash),
        _ => false,
    }
}

/// Get user ID by username.
async fn get_user_id(pool: &crate::db::DbPool, username: &str) -> Option<i64> {
    let result: Result<Option<(i64,)>, _> =
        sqlx::query_as("SELECT id FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await;
    result.ok().flatten().map(|(id,)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    #[test]
    fn test_sign_and_verify_session() {
        let secret = b"test-secret-key";
        let token = sign_session(42, secret, 1);
        let user_id = verify_session(&token, secret);
        assert_eq!(user_id, Some(42));
    }

    #[test]
    fn test_verify_wrong_secret() {
        let token = sign_session(42, b"secret-a", 1);
        assert_eq!(verify_session(&token, b"secret-b"), None);
    }

    #[test]
    fn test_verify_expired_session() {
        // Create a token that expired 1 hour ago
        let secret = b"test-secret";
        let expiry = chrono::Utc::now().timestamp() - 3600;
        let payload = format!("42:{expiry}");
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(payload.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());
        let token = format!("{payload}:{sig}");
        assert_eq!(verify_session(&token, secret), None);
    }

    #[test]
    fn test_verify_tampered_token() {
        let secret = b"test-secret";
        let token = sign_session(42, secret, 1);
        // Change user_id
        let tampered = token.replacen("42:", "99:", 1);
        assert_eq!(verify_session(&tampered, secret), None);
    }

    #[test]
    fn test_verify_garbage() {
        assert_eq!(verify_session("garbage", b"secret"), None);
        assert_eq!(verify_session("", b"secret"), None);
        assert_eq!(verify_session("a:b", b"secret"), None);
    }

    #[tokio::test]
    async fn test_verify_credentials_and_get_user_id() {
        let pool = create_test_pool().await;
        let hash = crate::password::hash("password123");
        sqlx::query("INSERT INTO users (username, password_hash, is_superuser) VALUES (?, ?, 0)")
            .bind("alice")
            .bind(hash)
            .execute(&pool)
            .await
            .unwrap();

        assert!(verify_credentials(&pool, "alice", "password123").await);
        assert!(!verify_credentials(&pool, "alice", "wrong-password").await);
        assert!(!verify_credentials(&pool, "missing-user", "password123").await);

        let uid = get_user_id(&pool, "alice").await;
        assert!(uid.is_some());
        assert_eq!(get_user_id(&pool, "missing-user").await, None);
    }
}
