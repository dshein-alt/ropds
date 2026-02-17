pub mod auth;
pub mod covers;
pub mod download;
pub mod feeds;
pub mod xml;

use axum::middleware;
use axum::routing::get;
use axum::Router;

use crate::state::AppState;

/// Build the OPDS router with all feed, download, and cover routes.
pub fn router(state: AppState) -> Router<AppState> {
    // Auth-protected routes (feeds, search, download)
    let protected = Router::new()
        // Root feed
        .route("/", get(feeds::root_feed))
        // Catalogs
        .route("/catalogs/", get(feeds::catalogs_feed))
        .route("/catalogs/{cat_id}/", get(feeds::catalogs_feed))
        // Authors
        .route("/authors/", get(feeds::authors_feed))
        .route("/authors/{lang_code}/", get(feeds::authors_feed))
        .route("/authors/{lang_code}/{prefix}/", get(feeds::authors_feed))
        // Series
        .route("/series/", get(feeds::series_feed))
        .route("/series/{lang_code}/", get(feeds::series_feed))
        .route("/series/{lang_code}/{prefix}/", get(feeds::series_feed))
        // Genres
        .route("/genres/", get(feeds::genres_feed))
        .route("/genres/{section}/", get(feeds::genres_feed))
        // Books by title
        .route("/books/", get(feeds::books_feed))
        .route("/books/{lang_code}/", get(feeds::books_feed))
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
        // Download
        .route("/download/{book_id}/{zip_flag}/", get(download::download))
        // Auth middleware
        .layer(middleware::from_fn_with_state(
            state,
            auth::basic_auth_layer,
        ));

    // Public routes (covers don't need auth, used by web UI img tags)
    Router::new()
        .route("/cover/{book_id}/", get(covers::cover))
        .route("/thumb/{book_id}/", get(covers::thumbnail))
        .merge(protected)
}
