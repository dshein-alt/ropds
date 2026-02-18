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
