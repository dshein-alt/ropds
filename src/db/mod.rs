pub mod models;
pub mod queries;

use sqlx::any::AnyPoolOptions;

use crate::config::DatabaseConfig;

/// Type alias for the database pool. All query modules use this instead
/// of a concrete pool type, allowing runtime backend selection via URI:
///   - `sqlite://path.db`  → SQLite
///   - `postgres://...`    → PostgreSQL
///   - `mysql://...`       → MySQL / MariaDB
pub type DbPool = sqlx::AnyPool;

/// Database backend detected from the connection URL scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbBackend {
    Sqlite,
    Postgres,
    Mysql,
}

impl DbBackend {
    pub fn from_url(url: &str) -> Self {
        if url.starts_with("postgres") {
            DbBackend::Postgres
        } else if url.starts_with("mysql") {
            DbBackend::Mysql
        } else {
            DbBackend::Sqlite
        }
    }
}

/// Install database drivers and create a connection pool.
/// The backend is determined by the URI scheme in `config.url`.
pub async fn create_pool(config: &DatabaseConfig) -> Result<(DbPool, DbBackend), sqlx::Error> {
    // Register all compiled-in database drivers
    sqlx::any::install_default_drivers();

    let backend = DbBackend::from_url(&config.url);
    let pool = AnyPoolOptions::new()
        .max_connections(5)
        .connect(&config.url)
        .await?;

    // Apply SQLite-specific pragmas after connecting
    if backend == DbBackend::Sqlite {
        configure_sqlite(&pool).await?;
    }

    run_migrations(&pool, backend).await?;

    Ok((pool, backend))
}

/// Set SQLite pragmas for WAL journal mode and foreign key enforcement.
async fn configure_sqlite(pool: &DbPool) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;
    sqlx::query("PRAGMA foreign_keys=ON").execute(pool).await?;
    Ok(())
}

async fn run_migrations(pool: &DbPool, backend: DbBackend) -> Result<(), sqlx::Error> {
    let migrator = match backend {
        DbBackend::Sqlite => sqlx::migrate!("./migrations"),
        DbBackend::Postgres => sqlx::migrate!("./migrations_pg"),
        DbBackend::Mysql => sqlx::migrate!("./migrations_mysql"),
    };
    migrator.run(pool).await?;
    Ok(())
}

/// Create an in-memory SQLite pool for testing, with all migrations applied.
pub async fn create_test_pool() -> (DbPool, DbBackend) {
    sqlx::any::install_default_drivers();

    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create test pool");

    run_migrations(&pool, DbBackend::Sqlite)
        .await
        .expect("Failed to run migrations");

    (pool, DbBackend::Sqlite)
}

/// Create a test pool for any backend (used by Docker integration tests).
pub async fn create_test_pool_for(url: &str) -> (DbPool, DbBackend) {
    sqlx::any::install_default_drivers();
    let backend = DbBackend::from_url(url);
    let pool = AnyPoolOptions::new()
        .max_connections(5)
        .connect(url)
        .await
        .expect("Failed to create test pool");
    if backend == DbBackend::Sqlite {
        configure_sqlite(&pool)
            .await
            .expect("Failed to configure SQLite");
    }
    run_migrations(&pool, backend)
        .await
        .expect("Failed to run migrations");
    (pool, backend)
}
