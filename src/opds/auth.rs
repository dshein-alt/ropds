use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;

use crate::state::AppState;

/// Axum middleware layer for HTTP Basic Authentication.
///
/// When `config.opds.auth_required` is true, all OPDS requests must
/// carry a valid `Authorization: Basic ...` header. Credentials are
/// checked against the `users` table.
pub async fn basic_auth_layer(
    state: axum::extract::State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !state.config.opds.auth_required {
        return next.run(request).await;
    }

    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(value) if value.starts_with("Basic ") => {
            let encoded = &value[6..];
            let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded) {
                Ok(d) => d,
                Err(_) => return unauthorized_response(),
            };
            let credentials = match String::from_utf8(decoded) {
                Ok(s) => s,
                Err(_) => return unauthorized_response(),
            };

            let (username, password) = match credentials.split_once(':') {
                Some((u, p)) => (u, p),
                None => return unauthorized_response(),
            };

            // Check credentials against DB
            match verify_credentials(&state.db, username, password).await {
                true => next.run(request).await,
                false => unauthorized_response(),
            }
        }
        _ => unauthorized_response(),
    }
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

/// Extract user_id from Basic Auth headers.
///
/// Parses `Authorization: Basic <base64>`, decodes the credentials,
/// splits on `:` to get the username, and looks up the user ID in the
/// database. Returns `None` if any step fails.
///
/// Used by download handlers to track bookshelf reads; will also be
/// used by feeds (Task 8).
pub async fn get_user_id_from_headers(
    pool: &crate::db::DbPool,
    headers: &axum::http::HeaderMap,
) -> Option<i64> {
    let auth = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = auth.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let credentials = String::from_utf8(decoded).ok()?;
    let (username, _password) = credentials.split_once(':')?;

    let result: Result<Option<(i64,)>, _> =
        sqlx::query_as("SELECT id FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await;
    result.ok().flatten().map(|(id,)| id)
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"OPDS\"")],
        "Authorization Required",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use axum::http::HeaderMap;

    fn auth_header(username: &str, password: &str) -> String {
        let raw = format!("{username}:{password}");
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
        )
    }

    #[tokio::test]
    async fn test_verify_credentials_and_get_user_id_from_headers() {
        let pool = create_test_pool().await;
        let hash = crate::password::hash("secret123");
        sqlx::query("INSERT INTO users (username, password_hash, is_superuser) VALUES (?, ?, 0)")
            .bind("alice")
            .bind(hash)
            .execute(&pool)
            .await
            .unwrap();

        assert!(verify_credentials(&pool, "alice", "secret123").await);
        assert!(!verify_credentials(&pool, "alice", "wrong").await);
        assert!(!verify_credentials(&pool, "missing", "secret123").await);

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            auth_header("alice", "secret123").parse().unwrap(),
        );
        assert!(get_user_id_from_headers(&pool, &headers).await.is_some());
    }

    #[tokio::test]
    async fn test_get_user_id_from_headers_invalid_inputs() {
        let pool = create_test_pool().await;
        let mut headers = HeaderMap::new();

        assert_eq!(get_user_id_from_headers(&pool, &headers).await, None);

        headers.insert(header::AUTHORIZATION, "Bearer abc".parse().unwrap());
        assert_eq!(get_user_id_from_headers(&pool, &headers).await, None);

        headers.insert(header::AUTHORIZATION, "Basic ???".parse().unwrap());
        assert_eq!(get_user_id_from_headers(&pool, &headers).await, None);
    }

    #[test]
    fn test_unauthorized_response() {
        let response = unauthorized_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Basic realm=\"OPDS\""
        );
    }
}
