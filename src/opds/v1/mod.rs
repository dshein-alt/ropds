pub mod feeds;
pub mod helpers;
pub mod xml;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

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

/// Build OPDS 1.2 (Atom XML) routes.
pub fn router() -> Router<AppState> {
    Router::new()
        // Root feed
        .route("/", get(feeds::root_feed))
        .route("/lang/{locale}/", get(feeds::root_feed_for_locale))
        // Catalogs
        .route("/catalogs/", get(feeds::catalogs_root))
        .route("/catalogs/{cat_id}/", get(feeds::catalogs_feed))
        .route("/catalogs/{cat_id}/{page}/", get(feeds::catalogs_feed))
        // Authors
        .route("/authors/", get(feeds::authors_root))
        .route("/authors/{lang_code}/", get(feeds::authors_feed))
        .route("/authors/{lang_code}/{prefix}/", get(feeds::authors_feed))
        .route(
            "/authors/{lang_code}/{prefix}/list/",
            get(feeds::authors_list),
        )
        .route(
            "/authors/{lang_code}/{prefix}/list/{page}/",
            get(feeds::authors_list),
        )
        // Series
        .route("/series/", get(feeds::series_root))
        .route("/series/{lang_code}/", get(feeds::series_feed))
        .route("/series/{lang_code}/{prefix}/", get(feeds::series_feed))
        .route(
            "/series/{lang_code}/{prefix}/list/",
            get(feeds::series_list),
        )
        .route(
            "/series/{lang_code}/{prefix}/list/{page}/",
            get(feeds::series_list),
        )
        // Genres
        .route("/genres/", get(feeds::genres_root))
        .route("/genres/{section}/", get(feeds::genres_by_section))
        // OPDS facets
        .route("/facets/languages", get(feeds::language_facets_feed))
        .route("/facets/languages/", get(feeds::language_facets_feed))
        // Books by title
        .route("/books/", get(feeds::books_root))
        .route("/books/{lang_code}/", get(feeds::books_feed))
        .route("/books/{lang_code}/{prefix}/", get(feeds::books_feed))
        // Recently added
        .route("/recent/", get(feeds::recent_root))
        .route("/recent/{page}/", get(feeds::recent_feed))
        // OpenSearch
        .route("/search/", get(feeds::opensearch))
        // Search type selection
        .route("/search/{terms}/", get(feeds::search_types_feed))
        // Book search
        .route(
            "/search/books/{search_type}/{terms}/",
            get(feeds::search_books_feed),
        )
        .route(
            "/search/books/{search_type}/{terms}/{page}/",
            get(feeds::search_books_feed),
        )
        // Author search
        .route(
            "/search/authors/{search_type}/{terms}/",
            get(feeds::search_authors_feed),
        )
        .route(
            "/search/authors/{search_type}/{terms}/{page}/",
            get(feeds::search_authors_feed),
        )
        // Series search
        .route(
            "/search/series/{search_type}/{terms}/",
            get(feeds::search_series_feed),
        )
        .route(
            "/search/series/{search_type}/{terms}/{page}/",
            get(feeds::search_series_feed),
        )
        // Bookshelf
        .route("/bookshelf/", get(feeds::bookshelf_root))
        .route("/bookshelf/{page}/", get(feeds::bookshelf_feed))
}
