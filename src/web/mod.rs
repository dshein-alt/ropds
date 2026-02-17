pub mod context;
pub mod i18n;
pub mod pagination;
pub mod views;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

pub fn router(_state: AppState) -> Router<AppState> {
    Router::new()
        .route("/", get(views::home))
        .route("/catalogs", get(views::catalogs))
        .route("/books", get(views::books_browse))
        .route("/authors", get(views::authors_browse))
        .route("/series", get(views::series_browse))
        .route("/genres", get(views::genres))
        .route("/search/books", get(views::search_books))
        .route("/search/authors", get(views::search_authors))
        .route("/search/series", get(views::search_series))
        .route("/set-language", get(views::set_language))
}
