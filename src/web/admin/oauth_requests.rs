use super::*;

use super::user_pages::CsrfForm;
use crate::db::queries::oauth;

/// GET /web/admin/oauth-requests — redirect to main admin page which now
/// includes the Access Requests accordion.
pub async fn page() -> Response {
    Redirect::to("/web/admin").into_response()
}

/// POST /web/admin/oauth-requests/{id}/approve
#[derive(Deserialize)]
pub struct ApproveForm {
    #[serde(default)]
    pub csrf_token: String,
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    pub link_user_id: Option<i64>,
    #[serde(default)]
    pub new_username: String,
}

fn deserialize_optional_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    match raw.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => v.parse::<i64>().map(Some).map_err(serde::de::Error::custom),
    }
}

pub async fn approve(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    axum::Form(form): axum::Form<ApproveForm>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    let identity = match oauth::get_by_id(&state.db, id).await {
        Ok(Some(i)) => i,
        Ok(None) => return Redirect::to("/web/admin?error=db_error").into_response(),
        Err(e) => {
            tracing::error!("Load identity failed for {id}: {e}");
            return Redirect::to("/web/admin?error=db_error").into_response();
        }
    };

    // Optional link to an existing local account before approval, otherwise keep
    // "new user" flow and optionally allow username override.
    if let Some(target_user_id) = form.link_user_id.filter(|v| *v > 0) {
        // Validate target user exists.
        match crate::db::queries::users::get_by_id(&state.db, target_user_id).await {
            Ok(Some(_)) => {}
            Ok(None) => return Redirect::to("/web/admin?error=db_error").into_response(),
            Err(e) => {
                tracing::error!("Load link target user failed for id {target_user_id}: {e}");
                return Redirect::to("/web/admin?error=db_error").into_response();
            }
        }

        if target_user_id != identity.user_id {
            if let Err(e) = oauth::reassign_user_by_id(&state.db, id, target_user_id).await {
                tracing::error!(
                    "Failed to link OAuth identity {id} from user {} to user {target_user_id}: {e}",
                    identity.user_id
                );
                return Redirect::to("/web/admin?error=db_error").into_response();
            }

            // Cleanup temporary source user if it no longer owns any OAuth identities.
            match oauth::count_for_user(&state.db, identity.user_id).await {
                Ok(0) => {
                    if let Err(e) =
                        crate::db::queries::users::delete(&state.db, identity.user_id).await
                    {
                        tracing::warn!(
                            "OAuth identity linked but failed to remove now-orphaned source user {}: {e}",
                            identity.user_id
                        );
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        "OAuth identity linked but failed to count identities for source user {}: {e}",
                        identity.user_id
                    );
                }
            }
        }
    } else {
        let requested_username = form.new_username.trim();
        if requested_username.is_empty() {
            return Redirect::to("/web/admin?error=username_empty").into_response();
        }
        if !is_valid_username(requested_username) {
            return Redirect::to("/web/admin?error=username_invalid").into_response();
        }

        let source_user =
            match crate::db::queries::users::get_by_id(&state.db, identity.user_id).await {
                Ok(Some(u)) => u,
                Ok(None) => return Redirect::to("/web/admin?error=db_error").into_response(),
                Err(e) => {
                    tracing::error!("Load source user failed for identity {id}: {e}");
                    return Redirect::to("/web/admin?error=db_error").into_response();
                }
            };

        if source_user.username != requested_username {
            match crate::db::queries::users::get_id_by_username(&state.db, requested_username).await
            {
                Ok(Some(existing_id)) if existing_id != identity.user_id => {
                    return Redirect::to("/web/admin?error=username_exists").into_response();
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("Check username failed for '{requested_username}': {e}");
                    return Redirect::to("/web/admin?error=db_error").into_response();
                }
            }

            if let Err(e) = crate::db::queries::users::update_username(
                &state.db,
                identity.user_id,
                requested_username,
            )
            .await
            {
                let error_text = e.to_string().to_lowercase();
                if error_text.contains("unique") || error_text.contains("duplicate") {
                    return Redirect::to("/web/admin?error=username_exists").into_response();
                }
                tracing::error!(
                    "Update username failed for user {} to '{}': {e}",
                    identity.user_id,
                    requested_username
                );
                return Redirect::to("/web/admin?error=db_error").into_response();
            }
        }
    }

    if let Err(e) = oauth::update_status_by_id(&state.db, id, "active", None).await {
        tracing::error!("Approve failed for identity {id}: {e}");
        return Redirect::to("/web/admin?error=db_error").into_response();
    }

    Redirect::to("/web/admin?msg=approved").into_response()
}

/// POST /web/admin/oauth-requests/{id}/reject
pub async fn reject(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    axum::Form(form): axum::Form<CsrfForm>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = oauth::update_status_by_id(&state.db, id, "rejected", Some(&now)).await {
        tracing::error!("Reject failed for identity {id}: {e}");
        return Redirect::to("/web/admin?msg=error").into_response();
    }

    Redirect::to("/web/admin?msg=rejected").into_response()
}

/// POST /web/admin/oauth-requests/{id}/ban
pub async fn ban(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    axum::Form(form): axum::Form<CsrfForm>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    if let Err(e) = oauth::update_status_by_id(&state.db, id, "banned", None).await {
        tracing::error!("Ban failed for identity {id}: {e}");
        return Redirect::to("/web/admin?msg=error").into_response();
    }

    Redirect::to("/web/admin?msg=banned").into_response()
}

/// POST /web/admin/oauth-requests/{id}/reinstate
pub async fn reinstate(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    axum::Form(form): axum::Form<CsrfForm>,
) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    if let Err(e) = oauth::update_status_by_id(&state.db, id, "active", None).await {
        tracing::error!("Reinstate failed for identity {id}: {e}");
        return Redirect::to("/web/admin?msg=error").into_response();
    }

    Redirect::to("/web/admin?msg=reinstated").into_response()
}
