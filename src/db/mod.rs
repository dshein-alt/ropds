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

/// Install database drivers and create a connection pool.
/// The backend is determined by the URI scheme in `config.url`.
pub async fn create_pool(config: &DatabaseConfig) -> Result<DbPool, sqlx::Error> {
    // Register all compiled-in database drivers
    sqlx::any::install_default_drivers();

    let pool = AnyPoolOptions::new()
        .max_connections(5)
        .connect(&config.url)
        .await?;

    // Apply SQLite-specific pragmas after connecting
    if config.url.starts_with("sqlite") {
        configure_sqlite(&pool).await?;
    }

    run_migrations(&pool).await?;

    Ok(pool)
}

/// Set SQLite pragmas for WAL journal mode and foreign key enforcement.
async fn configure_sqlite(pool: &DbPool) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;
    sqlx::query("PRAGMA foreign_keys=ON").execute(pool).await?;
    Ok(())
}

async fn run_migrations(pool: &DbPool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

#[cfg(test)]
pub async fn create_test_pool() -> DbPool {
    sqlx::any::install_default_drivers();

    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create test pool");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}
