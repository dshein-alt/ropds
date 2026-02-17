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

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"OPDS\"")],
        "Authorization Required",
    )
        .into_response()
}
