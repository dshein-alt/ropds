use super::*;

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
