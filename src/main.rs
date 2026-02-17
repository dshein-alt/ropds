mod config;
mod db;
mod error;
mod opds;
mod pdf;
mod scanner;
mod state;
mod web;

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use axum::extract::State;
use axum::response::Json;
use axum::routing::get;
use clap::Parser;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::state::AppState;
use crate::web::context;

#[derive(Parser)]
#[command(name = "ropds", version, about = "Rust OPDS Server")]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Run a one-shot library scan and exit
    #[arg(long)]
    scan: bool,
}

async fn health_check(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    Json(serde_json::json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
        "library_root": state.config.library.root_path,
        "database": if db_ok { "connected" } else { "error" },
    }))
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { axum::response::Redirect::to("/web") }))
        .route(
            "/web/",
            get(|| async { axum::response::Redirect::to("/web") }),
        )
        .route("/health", get(health_check))
        .nest("/opds", opds::router(state.clone()))
        .nest("/web", web::router(state.clone()))
        .nest_service("/static", ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Load configuration
    let config = Config::load(&cli.config).unwrap_or_else(|e| {
        eprintln!("Error loading config: {e}");
        std::process::exit(1);
    });

    // Setup tracing/logging
    let filter =
        EnvFilter::try_new(&config.server.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    if !pdf::pdftoppm_available() {
        tracing::warn!(
            "`pdftoppm` is not available in PATH; PDF cover/thumbnail generation is disabled"
        );
    }

    // Initialize database
    let pool = db::create_pool(&config.database).await.unwrap_or_else(|e| {
        tracing::error!("Failed to initialize database: {e}");
        std::process::exit(1);
    });
    tracing::info!("Database initialized: {}", config.database.url);

    // Ensure covers directory exists
    if let Err(e) = std::fs::create_dir_all(&config.opds.covers_dir) {
        tracing::error!(
            "Failed to create covers directory {:?}: {e}",
            config.opds.covers_dir
        );
        std::process::exit(1);
    }

    // One-shot scan mode
    if cli.scan {
        tracing::info!("Running one-shot scan...");
        match scanner::run_scan(&pool, &config).await {
            Ok(stats) => {
                tracing::info!(
                    "Scan finished: added={}, skipped={}, deleted={}, errors={}",
                    stats.books_added,
                    stats.books_skipped,
                    stats.books_deleted,
                    stats.errors,
                );
            }
            Err(e) => {
                tracing::error!("Scan failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // Initialize Tera templates
    let mut tera = tera::Tera::new("templates/**/*.html").unwrap_or_else(|e| {
        tracing::error!("Failed to load templates: {e}");
        std::process::exit(1);
    });
    context::register_filters(&mut tera);
    tracing::info!("Templates loaded");

    // Load translations
    let translations = web::i18n::load_translations(std::path::Path::new("locales"))
        .unwrap_or_else(|e| {
            tracing::error!("Failed to load translations: {e}");
            std::process::exit(1);
        });
    tracing::info!(
        "Translations loaded: {:?}",
        translations.keys().collect::<Vec<_>>()
    );

    // Server mode
    let addr = SocketAddr::new(
        config.server.host.parse().unwrap_or_else(|_| {
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

    let state = AppState::new(config, pool, tera, translations);
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
