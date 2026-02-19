pub mod models;
pub mod queries;

use std::borrow::Cow;
use std::fmt;

use sqlx::any::AnyPoolOptions;

use crate::config::DatabaseConfig;

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

/// Database pool wrapping `sqlx::AnyPool` with backend metadata.
///
/// Provides `sql()` for automatic `?` → `$N` placeholder rewriting
/// on PostgreSQL, and `inner()` for raw pool access in sqlx calls.
#[derive(Clone)]
pub struct DbPool {
    inner: sqlx::AnyPool,
    backend: DbBackend,
}

impl fmt::Debug for DbPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbPool")
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}

impl DbPool {
    pub fn new(inner: sqlx::AnyPool, backend: DbBackend) -> Self {
        Self { inner, backend }
    }

    /// Get raw pool reference for use in `sqlx::query(...).execute(pool.inner())`.
    pub fn inner(&self) -> &sqlx::AnyPool {
        &self.inner
    }

    /// Get the database backend type.
    pub fn backend(&self) -> DbBackend {
        self.backend
    }

    /// Rewrite SQL placeholders for the current backend.
    ///
    /// PostgreSQL requires `$1, $2, ...` instead of `?`.
    /// Returns borrowed string unchanged for SQLite/MySQL.
    pub fn sql<'a>(&self, query: &'a str) -> Cow<'a, str> {
        rewrite_placeholders(query, self.backend)
    }
}

/// Rewrite `?` placeholders to `$1, $2, ...` for PostgreSQL.
/// Skips `?` inside single-quoted string literals.
/// Returns `Cow::Borrowed` when no rewriting is needed.
fn rewrite_placeholders<'a>(sql: &'a str, backend: DbBackend) -> Cow<'a, str> {
    if backend != DbBackend::Postgres || !sql.contains('?') {
        return Cow::Borrowed(sql);
    }
    let mut result = String::with_capacity(sql.len() + 20);
    let mut n = 0u32;
    let mut in_quote = false;
    for ch in sql.chars() {
        if ch == '\'' {
            in_quote = !in_quote;
            result.push(ch);
        } else if ch == '?' && !in_quote {
            n += 1;
            result.push('$');
            // Inline number formatting (avoids allocation)
            if n < 10 {
                result.push((b'0' + n as u8) as char);
            } else {
                result.push_str(&n.to_string());
            }
        } else {
            result.push(ch);
        }
    }
    Cow::Owned(result)
}

/// Install database drivers and create a connection pool.
/// The backend is determined by the URI scheme in `config.url`.
pub async fn create_pool(config: &DatabaseConfig) -> Result<DbPool, sqlx::Error> {
    sqlx::any::install_default_drivers();

    let backend = DbBackend::from_url(&config.url);
    let pool = AnyPoolOptions::new()
        .max_connections(5)
        .connect(&config.url)
        .await?;

    if backend == DbBackend::Sqlite {
        configure_sqlite(&pool).await?;
    }

    run_migrations(&pool, backend).await?;

    Ok(DbPool::new(pool, backend))
}

/// Set SQLite pragmas for WAL journal mode and foreign key enforcement.
async fn configure_sqlite(pool: &sqlx::AnyPool) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;
    sqlx::query("PRAGMA foreign_keys=ON").execute(pool).await?;
    Ok(())
}

async fn run_migrations(pool: &sqlx::AnyPool, backend: DbBackend) -> Result<(), sqlx::Error> {
    let migrator = match backend {
        DbBackend::Sqlite => sqlx::migrate!("./migrations"),
        DbBackend::Postgres => sqlx::migrate!("./migrations_pg"),
        DbBackend::Mysql => sqlx::migrate!("./migrations_mysql"),
    };
    migrator.run(pool).await?;
    Ok(())
}

/// Create an in-memory SQLite pool for testing, with all migrations applied.
pub async fn create_test_pool() -> DbPool {
    sqlx::any::install_default_drivers();

    let pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create test pool");

    run_migrations(&pool, DbBackend::Sqlite)
        .await
        .expect("Failed to run migrations");

    DbPool::new(pool, DbBackend::Sqlite)
}

/// Create a test pool for any backend (used by Docker integration tests).
pub async fn create_test_pool_for(url: &str) -> DbPool {
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
    DbPool::new(pool, backend)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test if $N placeholders work across all backends through AnyPool.
    #[tokio::test]
    async fn test_dollar_placeholders_with_sqlite() {
        let pool = create_test_pool().await;
        // Test: does $1 work with SQLite through AnyPool?
        let row: (i64,) = sqlx::query_as("SELECT $1")
            .bind(42i64)
            .fetch_one(pool.inner())
            .await
            .expect("$1 placeholder should work with SQLite");
        assert_eq!(row.0, 42);

        // Test with multiple placeholders
        let row: (i64, String) = sqlx::query_as("SELECT $1, $2")
            .bind(7i64)
            .bind("hello".to_string())
            .fetch_one(pool.inner())
            .await
            .expect("$1, $2 placeholders should work with SQLite");
        assert_eq!(row.0, 7);
        assert_eq!(row.1, "hello");
    }

    #[test]
    fn test_rewrite_placeholders_sqlite() {
        let sql = "SELECT * FROM foo WHERE id = ? AND name = ?";
        assert!(matches!(
            rewrite_placeholders(sql, DbBackend::Sqlite),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn test_rewrite_placeholders_mysql() {
        let sql = "SELECT * FROM foo WHERE id = ? AND name = ?";
        assert!(matches!(
            rewrite_placeholders(sql, DbBackend::Mysql),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn test_rewrite_placeholders_postgres() {
        let sql = "SELECT * FROM foo WHERE id = ? AND name = ?";
        let result = rewrite_placeholders(sql, DbBackend::Postgres);
        assert_eq!(result, "SELECT * FROM foo WHERE id = $1 AND name = $2");
    }

    #[test]
    fn test_rewrite_skips_quoted_question_marks() {
        let sql = "SELECT * FROM foo WHERE name = '?' AND id = ?";
        let result = rewrite_placeholders(sql, DbBackend::Postgres);
        assert_eq!(result, "SELECT * FROM foo WHERE name = '?' AND id = $1");
    }

    #[test]
    fn test_rewrite_no_placeholders() {
        let sql = "SELECT COUNT(*) FROM foo";
        assert!(matches!(
            rewrite_placeholders(sql, DbBackend::Postgres),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn test_rewrite_many_placeholders() {
        let sql = "INSERT INTO t (a,b,c,d,e,f,g,h,i,j,k) VALUES (?,?,?,?,?,?,?,?,?,?,?)";
        let result = rewrite_placeholders(sql, DbBackend::Postgres);
        assert_eq!(
            result,
            "INSERT INTO t (a,b,c,d,e,f,g,h,i,j,k) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"
        );
    }
}
