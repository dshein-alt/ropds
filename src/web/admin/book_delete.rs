use super::*;

use crate::db::models::CatType;
use crate::db::queries::{books, suppressed};

use super::user_pages::CsrfForm;

/// POST /web/admin/books/:id/delete -- delete a single book.
pub async fn delete_book(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(book_id): Path<i64>,
    Query(params): Query<DeleteRedirectParams>,
    axum::Form(form): axum::Form<CsrfForm>,
) -> impl IntoResponse {
    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "CSRF validation failed").into_response();
    }

    let book = match books::get_by_id(&state.db, book_id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return Redirect::to(&redirect_url(&params, "error=book_not_found")).into_response();
        }
        Err(e) => {
            tracing::error!("Failed to fetch book {book_id}: {e}");
            return Redirect::to(&redirect_url(&params, "error=db_error")).into_response();
        }
    };

    // For plain files: delete from disk
    if let Ok(CatType::Normal) = CatType::try_from(book.cat_type) {
        let full_path = state
            .config
            .library
            .root_path
            .join(&book.path)
            .join(&book.filename);
        if full_path.exists() {
            if let Err(e) = std::fs::remove_file(&full_path) {
                tracing::warn!("Failed to delete book file {full_path:?}: {e}");
            }
        }
    } else {
        // For ZIP/INPX: add to suppression table
        if let Err(e) = suppressed::suppress(&state.db, &book.path, &book.filename).await {
            tracing::error!("Failed to suppress book {book_id}: {e}");
            return Redirect::to(&redirect_url(&params, "error=db_error")).into_response();
        }
    }

    // Delete cover file if it exists
    if book.cover > 0 && !book.cover_type.is_empty() {
        let cover_path = crate::scanner::cover_storage_path(
            &state.config.covers.covers_path,
            book.id,
            &book.cover_type,
        );
        if cover_path.exists() {
            let _ = std::fs::remove_file(&cover_path);
        }
    }

    // Delete book and all related DB records
    if let Err(e) = books::delete_book_and_relations(&state.db, book_id).await {
        tracing::error!("Failed to delete book {book_id} from DB: {e}");
        return Redirect::to(&redirect_url(&params, "error=db_error")).into_response();
    }

    Redirect::to(&redirect_url(&params, "msg=book_deleted")).into_response()
}

#[derive(Deserialize)]
pub struct DeleteRedirectParams {
    #[serde(default)]
    pub page: Option<i32>,
}

fn redirect_url(params: &DeleteRedirectParams, msg: &str) -> String {
    let page = params.page.unwrap_or(0);
    format!("/web/admin/duplicates?page={page}&{msg}")
}
