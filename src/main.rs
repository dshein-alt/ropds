use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use ropds::build_router;
use ropds::config::Config;
use ropds::state::AppState;
use ropds::web::context;

#[derive(Parser)]
#[command(name = "ropds", version, about = "Rust OPDS Server")]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Run a one-shot library scan and exit
    #[arg(long)]
    scan: bool,

    /// Create or update the admin user password and exit
    #[arg(long)]
    set_admin: Option<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Load configuration
    let mut config = Config::load(&cli.config).unwrap_or_else(|e| {
        eprintln!("Error loading config: {e}");
        std::process::exit(1);
    });

    // Auto-generate session secret if not set
    if config.server.session_secret.is_empty() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        config.server.session_secret = format!("ropds-auto-{seed}");
    }

    // Setup tracing/logging
    let filter =
        EnvFilter::try_new(&config.server.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Validate scanner schedule config
    if let Err(e) = ropds::scheduler::validate_config(&config.scanner) {
        tracing::error!("Invalid scanner config: {e}");
        std::process::exit(1);
    }

    let pdf_preview_tool_available = ropds::pdf::pdftoppm_available();
    if !pdf_preview_tool_available {
        tracing::warn!(
            "`pdftoppm` is not available in PATH; PDF cover/thumbnail generation is disabled"
        );
    }
    let pdf_metadata_tool_available = ropds::pdf::pdfinfo_available();
    if !pdf_metadata_tool_available {
        tracing::warn!(
            "`pdfinfo` is not available in PATH; PDF metadata extraction (title/author) is disabled"
        );
    }
    let djvu_preview_tool_available = ropds::djvu::ddjvu_available();
    if !djvu_preview_tool_available {
        tracing::warn!(
            "`ddjvu` is not available in PATH; DJVU cover/thumbnail generation is disabled"
        );
    }

    // Initialize database
    let pool = ropds::db::create_pool(&config.database)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Failed to initialize database: {e}");
            std::process::exit(1);
        });
    tracing::info!("Database initialized: {}", config.database.url);

    // Ensure covers directory exists
    if let Err(e) = std::fs::create_dir_all(&config.library.covers_path) {
        tracing::error!(
            "Failed to create covers directory {:?}: {e}",
            config.library.covers_path
        );
        std::process::exit(1);
    }
    let covers_test = config.library.covers_path.join(".ropds_write_test");
    match std::fs::File::create(&covers_test) {
        Ok(_) => {
            let _ = std::fs::remove_file(&covers_test);
        }
        Err(e) => {
            tracing::error!(
                "Covers path '{}' is not writable: {e}",
                config.library.covers_path.display()
            );
            std::process::exit(1);
        }
    }

    // Validate upload configuration
    if config.upload.allow_upload {
        if config.upload.upload_path.as_os_str().is_empty() {
            tracing::error!("Upload enabled but 'upload_path' is not set in [upload] config");
            std::process::exit(1);
        }

        if !config.upload.upload_path.exists() {
            if let Err(e) = std::fs::create_dir_all(&config.upload.upload_path) {
                tracing::error!(
                    "Upload enabled but failed to create upload_path '{}': {e}",
                    config.upload.upload_path.display()
                );
                std::process::exit(1);
            }
            tracing::info!(
                "Created upload directory: {}",
                config.upload.upload_path.display()
            );
        }

        let test_file = config.upload.upload_path.join(".ropds_write_test");
        match std::fs::File::create(&test_file) {
            Ok(_) => {
                let _ = std::fs::remove_file(&test_file);
            }
            Err(e) => {
                tracing::error!(
                    "Upload enabled but upload_path '{}' is not writable: {e}",
                    config.upload.upload_path.display()
                );
                std::process::exit(1);
            }
        }
        // Also check that root_path (library destination) is writable
        let root_test = config.library.root_path.join(".ropds_write_test");
        match std::fs::File::create(&root_test) {
            Ok(_) => {
                let _ = std::fs::remove_file(&root_test);
            }
            Err(e) => {
                tracing::error!(
                    "Upload enabled but root_path '{}' is not writable: {e}",
                    config.library.root_path.display()
                );
                std::process::exit(1);
            }
        }

        tracing::info!(
            "Upload enabled, upload_path: {}",
            config.upload.upload_path.display()
        );
    }

    // One-shot scan mode
    if cli.scan {
        tracing::info!("Running one-shot scan...");
        match ropds::scanner::run_scan(&pool, &config).await {
            Ok(stats) => {
                tracing::info!(
                    "Scan finished: added={}, skipped={}, deleted={}, archives_scanned={}, archives_skipped={}, errors={}",
                    stats.books_added,
                    stats.books_skipped,
                    stats.books_deleted,
                    stats.archives_scanned,
                    stats.archives_skipped,
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

    // Set admin password mode
    if let Some(ref password) = cli.set_admin {
        if password.len() < 8 || password.len() > 32 {
            tracing::error!("Password must be 8 to 32 characters long");
            std::process::exit(1);
        }
        match set_admin_password(&pool, password).await {
            Ok(created) => {
                if created {
                    tracing::info!("Admin user created");
                } else {
                    tracing::info!("Admin password updated");
                }
            }
            Err(e) => {
                tracing::error!("Failed to set admin password: {e}");
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
    let translations = ropds::web::i18n::load_translations(std::path::Path::new("locales"))
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

    // Start background scan scheduler
    tokio::spawn(ropds::scheduler::run(pool.clone(), config.clone()));

    let state = AppState::new(
        config,
        pool,
        tera,
        translations,
        pdf_preview_tool_available,
        djvu_preview_tool_available,
    );
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Failed to bind to {addr}: {e}");
            std::process::exit(1);
        });

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .unwrap_or_else(|e| {
        tracing::error!("Server error: {e}");
        std::process::exit(1);
    });
}

/// Create the admin user or update its password.
/// Returns `Ok(true)` if a new user was created, `Ok(false)` if updated.
async fn set_admin_password(pool: &ropds::db::DbPool, password: &str) -> Result<bool, sqlx::Error> {
    let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = 'admin'")
        .fetch_optional(pool.inner())
        .await?;

    let hashed = ropds::password::hash(password);

    if let Some((id,)) = existing {
        let sql = pool.sql("UPDATE users SET password_hash = ?, allow_upload = 1, display_name = CASE WHEN display_name = '' THEN 'Administrator' ELSE display_name END WHERE id = ?");
        sqlx::query(&sql)
            .bind(&hashed)
            .bind(id)
            .execute(pool.inner())
            .await?;
        Ok(false)
    } else {
        let sql = pool.sql("INSERT INTO users (username, password_hash, is_superuser, display_name, allow_upload) VALUES ('admin', ?, 1, 'Administrator', 1)");
        sqlx::query(&sql)
            .bind(&hashed)
            .execute(pool.inner())
            .await?;
        Ok(true)
    }
}
