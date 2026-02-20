pub mod admin;
pub mod auth;
pub mod context;
pub mod i18n;
pub mod pagination;
pub mod upload;
pub mod views;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};

use crate::state::AppState;

pub fn router(state: AppState) -> Router<AppState> {
    // Body limit for upload: configured max + 1 MB overhead for multipart framing
    let upload_body_limit =
        (state.config.upload.max_upload_size_mb as usize * 1024 * 1024) + 1_048_576;

    let admin_router = Router::new()
        .route("/", get(admin::admin_page))
        .route("/users/create", post(admin::create_user))
        .route("/users/{id}/password", post(admin::change_password))
        .route("/users/{id}/delete", post(admin::delete_user))
        .route("/users/{id}/upload", post(admin::toggle_upload))
        .route("/book-genres", post(admin::update_book_genres))
        .route("/book-authors", post(admin::update_book_authors))
        .route("/book-series", post(admin::update_book_series))
        .route("/series-search", get(admin::series_search))
        .route("/book-title", post(admin::update_book_title))
        .route("/scan", post(admin::scan_now))
        .route("/scan-status", get(admin::scan_status))
        .route("/genres", get(admin::genres_admin_json))
        .route("/genre-translation", post(admin::upsert_genre_translation))
        .route(
            "/genre-translation/delete",
            post(admin::delete_genre_translation),
        )
        .route("/genre", post(admin::create_genre))
        .route("/genre/delete", post(admin::delete_genre))
        .route("/section", post(admin::create_section))
        .route("/section/delete", post(admin::delete_section))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            admin::require_superuser,
        ));

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
        .route("/login", get(auth::login_page).post(auth::login_submit))
        .route("/logout", get(auth::logout))
        .route(
            "/change-password",
            get(admin::change_password_page).post(admin::change_password_submit),
        )
        .route("/profile", get(admin::profile_page))
        .route("/profile/password", post(admin::profile_change_password))
        .route(
            "/profile/display-name",
            post(admin::profile_update_display_name),
        )
        .route("/download/{book_id}/{zip_flag}", get(views::web_download))
        .route("/bookshelf", get(views::bookshelf_page))
        .route("/bookshelf/cards", get(views::bookshelf_cards))
        .route("/bookshelf/toggle", post(views::bookshelf_toggle))
        .route("/bookshelf/clear", post(views::bookshelf_clear))
        .route("/api/genres", get(views::genres_json))
        .route("/upload", get(upload::upload_page))
        .route(
            "/upload/file",
            post(upload::upload_file).layer(DefaultBodyLimit::max(upload_body_limit)),
        )
        .route("/upload/cover/{token}", get(upload::upload_cover))
        .route("/upload/publish", post(upload::publish))
        .nest("/admin", admin_router)
        .layer(middleware::from_fn_with_state(
            state,
            auth::session_auth_layer,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        Config, CoversConfig, DatabaseConfig, LibraryConfig, OpdsConfig, ScannerConfig,
        ServerConfig, UploadConfig, WebConfig,
    };
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
                covers_path: None,
                cover_max_dimension_px: None,
                cover_jpeg_quality: None,
                book_extensions: vec!["fb2".to_string(), "epub".to_string(), "zip".to_string()],
                scan_zip: true,
                zip_codepage: "cp866".to_string(),
                inpx_enable: false,
            },
            covers: CoversConfig {
                covers_path: PathBuf::from("/tmp/covers"),
                cover_max_dimension_px: 600,
                cover_jpeg_quality: 85,
                show_covers: true,
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
                show_covers: None,
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

        let pool = create_test_pool().await;
        let tera = tera::Tera::default();
        let mut translations = Translations::new();
        translations.insert("en".to_string(), serde_json::json!({"admin": {}}));

        let state = AppState::new(config, pool, tera, translations, false, false);
        let _router = router(state);
    }
}
