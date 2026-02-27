pub mod feeds;
pub mod helpers;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

/// Build OPDS 2.0 (JSON) routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v2", get(feeds::root_feed))
        .route("/v2/", get(feeds::root_feed))
        .route("/v2/lang/{locale}/", get(feeds::root_feed_for_locale))
        .route("/v2/catalogs/", get(feeds::catalogs_root))
        .route("/v2/catalogs/{cat_id}/", get(feeds::catalogs_feed))
        .route("/v2/catalogs/{cat_id}/{page}/", get(feeds::catalogs_feed))
        .route("/v2/authors/", get(feeds::authors_root))
        .route("/v2/authors/{lang_code}/", get(feeds::authors_feed))
        .route("/v2/authors/{lang_code}/{prefix}/", get(feeds::authors_feed))
        .route(
            "/v2/authors/{lang_code}/{prefix}/list/",
            get(feeds::authors_list),
        )
        .route(
            "/v2/authors/{lang_code}/{prefix}/list/{page}/",
            get(feeds::authors_list),
        )
        .route("/v2/series/", get(feeds::series_root))
        .route("/v2/series/{lang_code}/", get(feeds::series_feed))
        .route("/v2/series/{lang_code}/{prefix}/", get(feeds::series_feed))
        .route(
            "/v2/series/{lang_code}/{prefix}/list/",
            get(feeds::series_list),
        )
        .route(
            "/v2/series/{lang_code}/{prefix}/list/{page}/",
            get(feeds::series_list),
        )
        .route("/v2/genres/", get(feeds::genres_root))
        .route("/v2/genres/{section}/", get(feeds::genres_by_section))
        .route("/v2/facets/languages", get(feeds::language_facets_feed))
        .route("/v2/facets/languages/", get(feeds::language_facets_feed))
        .route("/v2/recent/", get(feeds::recent_root))
        .route("/v2/recent/{page}/", get(feeds::recent_feed))
        .route("/v2/bookshelf/", get(feeds::bookshelf_root))
        .route("/v2/bookshelf/{page}/", get(feeds::bookshelf_feed))
        .route("/v2/search/{terms}/", get(feeds::search_books_default))
        .route(
            "/v2/search/books/{search_type}/{terms}/",
            get(feeds::search_books_feed),
        )
        .route(
            "/v2/search/books/{search_type}/{terms}/{page}/",
            get(feeds::search_books_feed),
        )
}

#[derive(serde::Deserialize, Default)]
pub struct LangQuery {
    pub lang: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct CatalogsParams {
    pub cat_id: i64,
    pub page: Option<i32>,
}

#[derive(serde::Deserialize)]
pub struct AuthorsParams {
    pub lang_code: i32,
    pub prefix: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct AuthorsListParams {
    pub lang_code: i32,
    pub prefix: String,
    pub page: Option<i32>,
}

#[derive(serde::Deserialize)]
pub struct SearchBooksParams {
    pub search_type: String,
    pub terms: String,
    pub page: Option<i32>,
}
