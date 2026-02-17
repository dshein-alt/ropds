pub mod admin;
pub mod auth;
pub mod context;
pub mod i18n;
pub mod pagination;
pub mod views;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};

use crate::state::AppState;

pub fn router(state: AppState) -> Router<AppState> {
    let admin_router = Router::new()
        .route("/", get(admin::admin_page))
        .route("/users/create", post(admin::create_user))
        .route("/users/{id}/password", post(admin::change_password))
        .route("/users/{id}/delete", post(admin::delete_user))
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
        .nest("/admin", admin_router)
        .layer(middleware::from_fn_with_state(
            state,
            auth::session_auth_layer,
        ))
}
