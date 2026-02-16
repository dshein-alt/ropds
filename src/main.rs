mod config;
mod db;
mod error;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::extract::State;
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::state::AppState;

async fn health_check(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    Json(serde_json::json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
        "library_root": state.config.library.root_path,
        "database": if db_ok { "connected" } else { "error" },
    }))
}

async fn root() -> &'static str {
    "Rust OPDS Server"
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health_check))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[tokio::main]
async fn main() {
    // Determine config path from CLI arg or default
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config.toml"));

    // Load configuration
    let config = Config::load(&config_path).unwrap_or_else(|e| {
        eprintln!("Error loading config: {e}");
        std::process::exit(1);
    });

    // Setup tracing/logging
    let filter =
        EnvFilter::try_new(&config.server.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Initialize database
    let pool = db::create_pool(&config.database).await.unwrap_or_else(|e| {
        tracing::error!("Failed to initialize database: {e}");
        std::process::exit(1);
    });
    tracing::info!("Database initialized: {}", config.database.url);

    let addr = SocketAddr::new(
        config
            .server
            .host
            .parse()
            .unwrap_or_else(|_| {
                tracing::warn!(
                    "Invalid host '{}', falling back to 0.0.0.0",
                    config.server.host
                );
                "0.0.0.0".parse().unwrap()
            }),
        config.server.port,
    );

    tracing::info!("ropds v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Library root: {}", config.library.root_path.display());
    tracing::info!("Listening on {addr}");

    let state = AppState::new(config, pool);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Failed to bind to {addr}: {e}");
            std::process::exit(1);
        });

    axum::serve(listener, app).await.unwrap_or_else(|e| {
        tracing::error!("Server error: {e}");
        std::process::exit(1);
    });
}
