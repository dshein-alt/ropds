pub mod assets;
pub mod config;
pub mod db;
pub mod djvu;
pub mod opds;
pub mod password;
pub mod pdf;
pub mod scanner;
pub mod scheduler;
pub mod state;
pub mod web;

use axum::Router;
use axum::extract::State;
use axum::response::Json;
use axum::routing::get;

use crate::state::AppState;

async fn health_check(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db_ok = sqlx::query("SELECT 1")
        .execute(state.db.inner())
        .await
        .is_ok();
    Json(serde_json::json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
        "library_root": state.config.library.root_path,
        "database": if db_ok { "connected" } else { "error" },
    }))
}

pub fn build_router(state: AppState) -> Router {
    let router = Router::new()
        .route("/", get(|| async { axum::response::Redirect::to("/web") }))
        .route(
            "/web/",
            get(|| async { axum::response::Redirect::to("/web") }),
        )
        .route("/health", get(health_check))
        .nest("/opds", opds::router(state.clone()))
        .nest("/web", web::router(state.clone()))
        .route("/static/{*path}", get(assets::static_asset));

    router.with_state(state)
}
