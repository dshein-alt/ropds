//! OAuth 2.0 login and callback handlers for web UI.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use oauth2::TokenResponse;
use oauth2::basic::BasicClient;
use openidconnect::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenUrl,
};
use serde::Deserialize;
use serde_json::Value;

use crate::config::OauthConfig;
use crate::db::models::OAuthIdentity;
use crate::oauth::{ProviderKind, UserInfo};
use crate::state::AppState;
use crate::web::auth::sign_session;

// ── State cookie (CSRF protection) ──────────────────────────────────────────

const STATE_COOKIE: &str = "oauth_state";
const STATE_TTL_SECS: i64 = 600; // 10 minutes
const MAX_USERNAME_ATTEMPTS: u32 = 1000;

/// Create a signed state cookie value: `{state}:{expiry}:{sig}` (reuses HMAC from auth.rs).
pub fn sign_oauth_state(state_value: &str, secret: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let expiry = chrono::Utc::now().timestamp() + STATE_TTL_SECS;
    let payload = format!("{state_value}:{expiry}");
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC key");
    mac.update(payload.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("{payload}:{sig}")
}

/// Verify a state cookie and return whether the state matches and is not expired.
pub fn verify_oauth_state(cookie_value: &str, expected_state: &str, secret: &[u8]) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let parts: Vec<&str> = cookie_value.splitn(3, ':').collect();
    if parts.len() != 3 {
        return false;
    }
    let stored_state = parts[0];
    let expiry: i64 = match parts[1].parse() {
        Ok(e) => e,
        Err(_) => return false,
    };
    let sig_hex = parts[2];

    if stored_state != expected_state {
        return false;
    }
    if chrono::Utc::now().timestamp() > expiry {
        return false;
    }

    let payload = format!("{}:{}", parts[0], parts[1]);
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC key");
    mac.update(payload.as_bytes());
    let expected_bytes = match hex::decode(sig_hex) {
        Ok(b) => b,
        Err(_) => return false,
    };
    mac.verify_slice(&expected_bytes).is_ok()
}

/// Build an oauth2 `BasicClient` for the given provider.
/// Returns `None` if the provider is not configured (empty client_id / keycloak_url).
pub fn build_client(
    provider: ProviderKind,
    cfg: &OauthConfig,
    base_url: &str,
) -> Option<
    BasicClient<
        openidconnect::EndpointSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointSet,
    >,
> {
    let (client_id, client_secret, auth_url, token_url) = match provider {
        ProviderKind::Google => {
            if cfg.google_client_id.is_empty() {
                return None;
            }
            (
                cfg.google_client_id.clone(),
                cfg.google_client_secret.clone(),
                "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
                "https://oauth2.googleapis.com/token".to_string(),
            )
        }
        ProviderKind::Yandex => {
            if cfg.yandex_client_id.is_empty() {
                return None;
            }
            (
                cfg.yandex_client_id.clone(),
                cfg.yandex_client_secret.clone(),
                "https://oauth.yandex.ru/authorize".to_string(),
                "https://oauth.yandex.ru/token".to_string(),
            )
        }
        ProviderKind::Keycloak => {
            if cfg.keycloak_url.is_empty() {
                return None;
            }
            let base = cfg.keycloak_url.trim_end_matches('/');
            let realm = &cfg.keycloak_realm;
            (
                cfg.keycloak_client_id.clone(),
                cfg.keycloak_client_secret.clone(),
                format!("{base}/realms/{realm}/protocol/openid-connect/auth"),
                format!("{base}/realms/{realm}/protocol/openid-connect/token"),
            )
        }
    };

    let redirect_uri = format!("{}/web/oauth/callback/{}", base_url, provider.as_str());

    let auth = match AuthUrl::new(auth_url) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Invalid OAuth auth URL for {}: {e}", provider.as_str());
            return None;
        }
    };
    let token = match TokenUrl::new(token_url) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Invalid OAuth token URL for {}: {e}", provider.as_str());
            return None;
        }
    };
    let redirect = match RedirectUrl::new(redirect_uri) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(
                "Invalid OAuth redirect URL for {} (check server.base_url): {e}",
                provider.as_str()
            );
            return None;
        }
    };

    let client = BasicClient::new(ClientId::new(client_id))
        .set_client_secret(ClientSecret::new(client_secret))
        .set_auth_uri(auth)
        .set_token_uri(token)
        .set_redirect_uri(redirect);

    Some(client)
}

fn provider_scopes(provider: ProviderKind) -> Vec<Scope> {
    match provider {
        ProviderKind::Google | ProviderKind::Keycloak => vec![
            Scope::new("openid".into()),
            Scope::new("email".into()),
            Scope::new("profile".into()),
        ],
        ProviderKind::Yandex => vec![
            Scope::new("login:email".into()),
            Scope::new("login:info".into()),
        ],
    }
}

/// `GET /web/oauth/login/{provider}`
///
/// Redirects the user to the OAuth provider's authorization page.
pub async fn login(
    State(state): State<AppState>,
    Path(provider_str): Path<String>,
    jar: CookieJar,
) -> Response {
    let provider = match provider_str.parse::<ProviderKind>() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, "Unknown provider").into_response(),
    };

    let base_url = &state.config.server.base_url;
    let client = match build_client(provider, &state.config.oauth, base_url) {
        Some(c) => c,
        None => return (StatusCode::NOT_FOUND, "Provider not configured").into_response(),
    };

    // Build auth URL with scopes and CSRF token
    let (auth_url, csrf_token) = provider_scopes(provider)
        .into_iter()
        .fold(client.authorize_url(CsrfToken::new_random), |req, s| {
            req.add_scope(s)
        })
        .url();

    // Sign the CSRF state and store in cookie
    let secret = state.config.server.session_secret.as_bytes();
    let cookie_val = sign_oauth_state(csrf_token.secret(), secret);
    let cookie = Cookie::build((STATE_COOKIE, cookie_val))
        .path("/web/oauth")
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax)
        .max_age(time::Duration::seconds(STATE_TTL_SECS));

    (jar.add(cookie), Redirect::to(auth_url.as_str())).into_response()
}

// ── Userinfo fetch helper ────────────────────────────────────────────────────

/// Fetch userinfo JSON from the provider's endpoint using the bearer access token.
async fn fetch_userinfo(
    provider: ProviderKind,
    cfg: &OauthConfig,
    access_token: &str,
) -> Result<Value, String> {
    let http = reqwest::Client::new();

    match provider {
        ProviderKind::Google => {
            let resp = http
                .get("https://www.googleapis.com/oauth2/v3/userinfo")
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json::<Value>()
                .await
                .map_err(|e| e.to_string())?;
            Ok(resp)
        }
        ProviderKind::Yandex => {
            let resp = http
                .get("https://login.yandex.ru/info?format=json")
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json::<Value>()
                .await
                .map_err(|e| e.to_string())?;
            Ok(resp)
        }
        ProviderKind::Keycloak => {
            let base = cfg.keycloak_url.trim_end_matches('/');
            let realm = &cfg.keycloak_realm;
            let url = format!("{base}/realms/{realm}/protocol/openid-connect/userinfo");
            let resp = http
                .get(&url)
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json::<Value>()
                .await
                .map_err(|e| e.to_string())?;
            Ok(resp)
        }
    }
}

// ── Callback handler ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// GET /web/oauth/callback/{provider}
pub async fn callback(
    State(state): State<AppState>,
    Path(provider_str): Path<String>,
    Query(params): Query<CallbackQuery>,
    jar: CookieJar,
) -> Response {
    // 0. Provider lookup
    let provider = match provider_str.parse::<ProviderKind>() {
        Ok(p) => p,
        Err(_) => return (StatusCode::BAD_REQUEST, "Unknown provider").into_response(),
    };

    if let Some(err) = params.error {
        tracing::info!("OAuth error from {provider_str}: {err}");
        return Redirect::to("/web/login?error=oauth").into_response();
    }

    let code = match params.code {
        Some(c) => c,
        None => return Redirect::to("/web/login?error=oauth").into_response(),
    };
    let state_param = match params.state {
        Some(s) => s,
        None => return Redirect::to("/web/login?error=oauth").into_response(),
    };

    // 1. Validate CSRF state cookie
    let secret = state.config.server.session_secret.as_bytes();
    let state_cookie = jar.get(STATE_COOKIE);
    if state_cookie.is_none() {
        tracing::warn!("OAuth callback for {provider_str}: state cookie missing");
    }
    let valid_state = state_cookie
        .map(|c| verify_oauth_state(c.value(), &state_param, secret))
        .unwrap_or(false);
    if !valid_state {
        return (StatusCode::BAD_REQUEST, "Invalid state").into_response();
    }
    let jar = jar.remove(Cookie::build(STATE_COOKIE).path("/web/oauth"));

    // 2. Exchange code for token
    let base_url = &state.config.server.base_url;
    let client = match build_client(provider, &state.config.oauth, base_url) {
        Some(c) => c,
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Provider not configured").into_response();
        }
    };

    let token_result = client
        .exchange_code(AuthorizationCode::new(code))
        .request_async(&reqwest::Client::new())
        .await;
    let access_token = match token_result {
        Ok(t) => t.access_token().secret().clone(),
        Err(e) => {
            tracing::error!("OAuth token exchange failed for {provider_str}: {e}");
            return Redirect::to("/web/login?error=oauth").into_response();
        }
    };

    // 3. Fetch userinfo
    let userinfo_json = match fetch_userinfo(provider, &state.config.oauth, &access_token).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Userinfo fetch failed for {provider_str}: {e}");
            return Redirect::to("/web/login?error=oauth").into_response();
        }
    };

    let userinfo = match crate::oauth::parse_userinfo(provider, &userinfo_json) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Userinfo parse failed for {provider_str}: {e}");
            return Redirect::to("/web/login?error=oauth").into_response();
        }
    };

    // 4. Look up identity
    let identity = crate::db::queries::oauth::find_by_provider(
        &state.db,
        provider.as_str(),
        &userinfo.provider_uid,
    )
    .await;

    match identity {
        Err(e) => {
            tracing::error!("DB error in OAuth callback: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
        }
        Ok(Some(ident)) => handle_existing_identity(ident, &userinfo, &state, jar).await,
        Ok(None) => handle_new_identity(userinfo, &state, jar).await,
    }
}

// ── Helper functions ─────────────────────────────────────────────────────────

async fn handle_existing_identity(
    ident: OAuthIdentity,
    userinfo: &UserInfo,
    state: &AppState,
    jar: CookieJar,
) -> Response {
    match ident.status.as_str() {
        "active" => {
            if userinfo.provider == ProviderKind::Keycloak {
                sync_keycloak_roles(ident.user_id, &userinfo.roles, state).await;
            }
            make_session(ident.user_id, state, jar).await
        }
        "pending" => render_status(state, "web/oauth_pending.html", tera::Context::new()),
        "rejected" => {
            let cooldown_secs = state.config.oauth.rejection_cooldown_hours as i64 * 3600;
            let elapsed = ident
                .rejected_at
                .as_deref()
                .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                .map(|dt| chrono::Utc::now().signed_duration_since(dt).num_seconds())
                .unwrap_or(0);

            if elapsed >= cooldown_secs {
                let _ = crate::db::queries::oauth::update_status_by_id(
                    &state.db, ident.id, "pending", None,
                )
                .await;
                notify_admin_pending(state, userinfo, true).await;
                render_status(state, "web/oauth_pending.html", tera::Context::new())
            } else {
                let retry_at = ident
                    .rejected_at
                    .as_deref()
                    .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                    .map(|dt| dt + chrono::Duration::seconds(cooldown_secs))
                    .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_default();
                let mut ctx = tera::Context::new();
                ctx.insert("retry_at", &retry_at);
                render_status(state, "web/oauth_rejected.html", ctx)
            }
        }
        "banned" => render_status(state, "web/oauth_banned.html", tera::Context::new()),
        _ => Redirect::to("/web/login?error=oauth").into_response(),
    }
}

async fn handle_new_identity(userinfo: UserInfo, state: &AppState, jar: CookieJar) -> Response {
    let base = userinfo
        .display_name
        .as_deref()
        .map(crate::util::slugify_username)
        .unwrap_or_else(|| "user".to_string());

    let opds_password = crate::password::generate_opds_password();
    let opds_hash = crate::password::hash(&opds_password);

    // Insert-first retry loop to avoid race between "username available" check and insert.
    // Candidate order: base, base_2, base_3, ...
    let mut created: Option<(i64, String)> = None;
    for attempt in 1..=MAX_USERNAME_ATTEMPTS {
        let username = username_candidate(&base, attempt);
        let display = userinfo
            .display_name
            .clone()
            .unwrap_or_else(|| username.clone());

        match crate::db::queries::users::create_oauth_user(
            &state.db, &username, &opds_hash, 0, &display,
        )
        .await
        {
            Ok(id) => {
                created = Some((id, username));
                break;
            }
            Err(e) if is_unique_violation(&e) => {
                continue;
            }
            Err(e) => {
                tracing::error!("Failed to create OAuth user: {e}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create user")
                    .into_response();
            }
        }
    }

    let (user_id, _username) = match created {
        Some(v) => v,
        None => {
            tracing::error!(
                "Failed to allocate unique username after {} attempts (base='{}')",
                MAX_USERNAME_ATTEMPTS,
                base
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to assign username",
            )
                .into_response();
        }
    };

    if let Err(e) = crate::db::queries::oauth::create_identity(
        &state.db,
        user_id,
        userinfo.provider.as_str(),
        &userinfo.provider_uid,
        userinfo.email.as_deref(),
        userinfo.display_name.as_deref(),
    )
    .await
    {
        tracing::error!("Failed to create OAuth identity: {e}");
        if let Err(del_err) = crate::db::queries::users::delete(&state.db, user_id).await {
            tracing::error!("Failed to clean up orphaned user {user_id}: {del_err}");
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create identity",
        )
            .into_response();
    }

    let auto_approve =
        userinfo.provider == ProviderKind::Keycloak && state.config.oauth.keycloak_auto_approve;

    if auto_approve {
        let _ = crate::db::queries::oauth::update_status(
            &state.db,
            user_id,
            userinfo.provider.as_str(),
            &userinfo.provider_uid,
            "active",
            None,
        )
        .await;
        sync_keycloak_roles(user_id, &userinfo.roles, state).await;
        make_session(user_id, state, jar).await
    } else {
        notify_admin_pending(state, &userinfo, false).await;
        render_status(state, "web/oauth_pending.html", tera::Context::new())
    }
}

fn username_candidate(base: &str, attempt: u32) -> String {
    if attempt <= 1 {
        base.to_string()
    } else {
        format!("{base}_{attempt}")
    }
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    let sqlstate_match = match err {
        sqlx::Error::Database(db_err) => db_err
            .code()
            .map(|code| {
                code == "23505" || // Postgres unique_violation
                code == "2067" || // SQLite SQLITE_CONSTRAINT_UNIQUE
                code == "1555" || // SQLite SQLITE_CONSTRAINT_PRIMARYKEY
                code == "23000" // MySQL integrity constraint violation class
            })
            .unwrap_or(false),
        _ => false,
    };
    if sqlstate_match {
        return true;
    }

    let msg = err.to_string().to_lowercase();
    msg.contains("unique") || msg.contains("duplicate")
}

async fn sync_keycloak_roles(user_id: i64, roles: &[String], state: &AppState) {
    let cfg = &state.config.oauth;
    let allow_upload = if cfg.keycloak_role_upload.is_empty() {
        None
    } else {
        Some(roles.contains(&cfg.keycloak_role_upload) as i32)
    };
    let is_superuser = if cfg.keycloak_role_admin.is_empty() {
        None
    } else {
        Some(roles.contains(&cfg.keycloak_role_admin) as i32)
    };

    if allow_upload.is_none() && is_superuser.is_none() {
        return;
    }

    match (allow_upload, is_superuser) {
        (Some(u), Some(a)) => {
            if let Err(e) = sqlx::query(
                &state
                    .db
                    .sql("UPDATE users SET allow_upload = ?, is_superuser = ? WHERE id = ?"),
            )
            .bind(u)
            .bind(a)
            .bind(user_id)
            .execute(state.db.inner())
            .await
            {
                tracing::warn!(
                    "Keycloak role sync failed (allow_upload + is_superuser) for user {user_id}: {e}"
                );
            }
        }
        (Some(u), None) => {
            if let Err(e) = sqlx::query(
                &state
                    .db
                    .sql("UPDATE users SET allow_upload = ? WHERE id = ?"),
            )
            .bind(u)
            .bind(user_id)
            .execute(state.db.inner())
            .await
            {
                tracing::warn!("Keycloak role sync failed (allow_upload) for user {user_id}: {e}");
            }
        }
        (None, Some(a)) => {
            if let Err(e) = sqlx::query(
                &state
                    .db
                    .sql("UPDATE users SET is_superuser = ? WHERE id = ?"),
            )
            .bind(a)
            .bind(user_id)
            .execute(state.db.inner())
            .await
            {
                tracing::warn!("Keycloak role sync failed (is_superuser) for user {user_id}: {e}");
            }
        }
        _ => {}
    }
}

async fn notify_admin_pending(state: &AppState, userinfo: &UserInfo, is_reapply: bool) {
    let cfg = &state.config;
    if !cfg.oauth.notify_admin_email {
        return;
    }
    if !crate::email::is_email_configured(&cfg.smtp) {
        return;
    }

    let subject = if is_reapply {
        "ROPDS: OAuth re-application (was rejected)".to_string()
    } else {
        "ROPDS: New OAuth access request".to_string()
    };
    let body = format!(
        "Provider: {}\nDisplay name: {}\nEmail: {}\n\nReview: {}/web/admin\n",
        userinfo.provider.as_str(),
        userinfo.display_name.as_deref().unwrap_or("-"),
        userinfo.email.as_deref().unwrap_or("-"),
        cfg.server.base_url,
    );
    crate::email::send_async(cfg.smtp.clone(), cfg.smtp.send_to.clone(), subject, body);
}

async fn make_session(user_id: i64, state: &AppState, jar: CookieJar) -> Response {
    // Keep OAuth login behavior consistent with password login: record last_login.
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if let Err(e) = crate::db::queries::users::update_last_login(&state.db, user_id, &now).await {
        tracing::warn!("Failed to update last_login for OAuth user {user_id}: {e}");
    }

    let secret = state.config.server.session_secret.as_bytes();
    let ttl = state.config.server.session_ttl_hours;
    let token = sign_session(user_id, secret, ttl);
    let cookie = Cookie::build(("session", token))
        .path("/web")
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax);
    (jar.add(cookie), Redirect::to("/web/bookshelf")).into_response()
}

fn render_status(state: &AppState, template: &str, mut ctx: tera::Context) -> Response {
    ctx.insert("locale", &state.config.web.language);
    ctx.insert("default_theme", &state.config.web.theme);
    ctx.insert("app_title", &state.config.opds.title);
    ctx.insert("version", env!("CARGO_PKG_VERSION"));
    match state.tera.render(template, &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error rendering {template}: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_cookie_roundtrip() {
        let secret = b"test-secret";
        let cookie = sign_oauth_state("my-state", secret);
        assert!(verify_oauth_state(&cookie, "my-state", secret));
    }

    #[test]
    fn test_state_cookie_wrong_state() {
        let secret = b"test-secret";
        let cookie = sign_oauth_state("state-a", secret);
        assert!(!verify_oauth_state(&cookie, "state-b", secret));
    }

    #[test]
    fn test_state_cookie_wrong_secret() {
        let cookie = sign_oauth_state("state", b"secret-a");
        assert!(!verify_oauth_state(&cookie, "state", b"secret-b"));
    }

    #[test]
    fn test_username_candidate_sequence() {
        assert_eq!(username_candidate("user", 1), "user");
        assert_eq!(username_candidate("user", 2), "user_2");
        assert_eq!(username_candidate("user", 9), "user_9");
    }
}
