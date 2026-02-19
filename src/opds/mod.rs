pub mod auth;
pub mod covers;
pub mod download;
pub mod feeds;
pub mod xml;

use axum::Router;
use axum::extract::ConnectInfo;
use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use std::net::SocketAddr;

use crate::state::AppState;

/// Logging middleware for OPDS requests.
async fn opds_logging(request: Request, next: Next) -> Response {
    let start = std::time::Instant::now();
    let addr = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "-".into());
    let method = request.method().clone();
    let uri = request.uri().to_string();

    let response = next.run(request).await;

    let elapsed = start.elapsed();
    let status = response.status().as_u16();
    tracing::info!("{addr} {method} {uri} {status} {elapsed:.1?}",);

    response
}

/// Build the OPDS router with all feed, download, and cover routes.
pub fn router(state: AppState) -> Router<AppState> {
    // Auth-protected routes (feeds, search, download)
    let protected = Router::new()
        // Root feed
        .route("/", get(feeds::root_feed))
        // Catalogs
        .route("/catalogs/", get(feeds::catalogs_feed))
        .route("/catalogs/{cat_id}/", get(feeds::catalogs_feed))
        .route("/catalogs/{cat_id}/{page}/", get(feeds::catalogs_feed))
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
        .route("/books/{lang_code}/{prefix}/", get(feeds::books_feed))
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
        .route("/bookshelf/", get(feeds::bookshelf_feed))
        .route("/bookshelf/{page}/", get(feeds::bookshelf_feed))
        // Download
        .route("/download/{book_id}/{zip_flag}/", get(download::download))
        // Auth middleware
        .layer(middleware::from_fn_with_state(
            state,
            auth::basic_auth_layer,
        ))
        .layer(middleware::from_fn(opds_logging));

    // Public routes (covers don't need auth, used by web UI img tags)
    Router::new()
        .route("/cover/{book_id}/", get(covers::cover))
        .route("/thumb/{book_id}/", get(covers::thumbnail))
        .merge(protected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        Config, DatabaseConfig, LibraryConfig, OpdsConfig, ScannerConfig, ServerConfig,
        UploadConfig, WebConfig,
    };
    use crate::db::DbBackend;
    use crate::db::create_test_pool;
    use crate::web::i18n::Translations;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_router_builds() {
        let config = Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                log_level: "info".to_string(),
                session_secret: "test-secret".to_string(),
                session_ttl_hours: 24,
            },
            library: LibraryConfig {
                root_path: PathBuf::from("/tmp/books"),
                covers_path: PathBuf::from("/tmp/covers"),
                book_extensions: vec!["fb2".to_string(), "epub".to_string(), "zip".to_string()],
                scan_zip: true,
                zip_codepage: "cp866".to_string(),
                inpx_enable: false,
            },
            database: DatabaseConfig {
                url: "sqlite::memory:".to_string(),
            },
            opds: OpdsConfig {
                title: "ROPDS".to_string(),
                subtitle: String::new(),
                max_items: 30,
                split_items: 300,
                auth_required: true,
                show_covers: true,
                alphabet_menu: true,
                hide_doubles: false,
            },
            scanner: ScannerConfig {
                schedule_minutes: vec![0],
                schedule_hours: vec![0],
                schedule_day_of_week: vec![],
                delete_logical: true,
                skip_unchanged: false,
                test_zip: false,
                test_files: false,
                workers_num: 1,
            },
            web: WebConfig {
                language: "en".to_string(),
                theme: "light".to_string(),
            },
            upload: UploadConfig {
                allow_upload: true,
                upload_path: PathBuf::from("/tmp/uploads"),
                max_upload_size_mb: 10,
            },
        };

        let (db, _) = create_test_pool().await;
        let tera = tera::Tera::default();
        let mut translations = Translations::new();
        translations.insert("en".to_string(), serde_json::json!({}));
        let state = AppState::new(
            config,
            db,
            DbBackend::Sqlite,
            tera,
            translations,
            false,
            false,
        );
        let _router = router(state);
    }
}
