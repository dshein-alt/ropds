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
        // Accept `postgres://`, `postgresql://`, `mysql://`, and `mariadb://`.
        // Anything else (including `sqlite://` and bare file paths) is SQLite.
        if url.starts_with("postgres") {
            DbBackend::Postgres
        } else if url.starts_with("mysql") || url.starts_with("mariadb") {
            DbBackend::Mysql
        } else {
            DbBackend::Sqlite
        }
    }
}

/// Redact URI userinfo before logging a database connection string.
///
/// Keeps the scheme, host, and path intact while replacing any userinfo
/// (`username` or `username:password`) with `***`.
pub fn redact_database_url(url: &str) -> Cow<'_, str> {
    let Some(scheme_end) = url.find("://") else {
        return Cow::Borrowed(url);
    };

    let authority_start = scheme_end + 3;
    let remainder = &url[authority_start..];
    let authority_len = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_len];

    let Some(userinfo_end) = authority.rfind('@') else {
        return Cow::Borrowed(url);
    };

    let mut redacted = String::with_capacity(url.len());
    redacted.push_str(&url[..authority_start]);
    redacted.push_str("***");
    redacted.push('@');
    redacted.push_str(&authority[userinfo_end + 1..]);
    redacted.push_str(&remainder[authority_len..]);
    Cow::Owned(redacted)
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

/// Set SQLite pragmas for WAL journal mode, lock wait timeout, and foreign key enforcement.
async fn configure_sqlite(pool: &sqlx::AnyPool) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;
    // Wait up to 30s on SQLite file locks before failing writes during heavy scans.
    sqlx::query("PRAGMA busy_timeout=30000")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA foreign_keys=ON").execute(pool).await?;
    Ok(())
}

async fn run_migrations(pool: &sqlx::AnyPool, backend: DbBackend) -> Result<(), sqlx::Error> {
    let migrator = match backend {
        DbBackend::Sqlite => sqlx::migrate!("./migrations/sqlite"),
        DbBackend::Postgres => sqlx::migrate!("./migrations/pg"),
        DbBackend::Mysql => sqlx::migrate!("./migrations/mysql"),
    };
    migrator.run(pool).await?;
    Ok(())
}

/// Prepare the target database for the SQLite to target data migration: create
/// it if missing, run a safety preflight, apply every migration, and then
/// clear every user table so the target is truly empty of data (including
/// seed rows inserted by migrations).
///
/// Refuses to proceed if user data exists without `_sqlx_migrations`, so that
/// an externally-populated database cannot be mistakenly "initialized" on top.
///
/// This is the migration-prep flag. Fresh installs that do NOT migrate from a
/// SQLite dump should start the server normally without `--init-db`; that path
/// applies migrations and leaves seed data in place.
pub async fn init_db(config: &DatabaseConfig) -> Result<(), sqlx::Error> {
    sqlx::any::install_default_drivers();
    let backend = DbBackend::from_url(&config.url);

    // Try the target URL directly first. If it connects, the database already
    // exists — avoids needing access to the admin DB (e.g. 'postgres') which
    // non-privileged roles typically can't reach per pg_hba.conf.
    let pool = match AnyPoolOptions::new()
        .max_connections(1)
        .connect(&config.url)
        .await
    {
        Ok(p) => p,
        Err(direct_err) => {
            tracing::info!(
                "Direct connection failed ({direct_err}); attempting to create database"
            );
            match backend {
                DbBackend::Postgres => {
                    use sqlx::Postgres;
                    use sqlx::migrate::MigrateDatabase;
                    if !Postgres::database_exists(&config.url).await? {
                        Postgres::create_database(&config.url).await?;
                    }
                }
                DbBackend::Mysql => {
                    use sqlx::MySql;
                    use sqlx::migrate::MigrateDatabase;
                    if !MySql::database_exists(&config.url).await? {
                        MySql::create_database(&config.url).await?;
                    }
                }
                DbBackend::Sqlite => {
                    use sqlx::Sqlite;
                    use sqlx::migrate::MigrateDatabase;
                    if !Sqlite::database_exists(&config.url).await? {
                        Sqlite::create_database(&config.url).await?;
                    }
                }
            }
            AnyPoolOptions::new()
                .max_connections(1)
                .connect(&config.url)
                .await?
        }
    };

    if backend == DbBackend::Sqlite {
        configure_sqlite(&pool).await?;
    }

    preflight_init(&pool, backend).await?;
    run_migrations(&pool, backend).await?;
    clear_data_tables(&pool, backend).await?;
    Ok(())
}

/// Clear every user table (all tables except `_sqlx_migrations`) so the target
/// is truly empty, ready for the SQLite to target data migration script.
///
/// This is the migration-prep semantic of `--init-db`: after this call, the
/// schema and `_sqlx_migrations` are in place, but no data rows remain.
/// Seed data inserted by migrations (genres, counters, etc.) is wiped and is
/// expected to come from the SQLite source during data copy.
async fn clear_data_tables(pool: &sqlx::AnyPool, backend: DbBackend) -> Result<(), sqlx::Error> {
    let tables: Vec<String> = list_user_tables(pool, backend)
        .await?
        .into_iter()
        .filter(|t| t != "_sqlx_migrations")
        .collect();
    if tables.is_empty() {
        return Ok(());
    }

    let mut conn = pool.acquire().await?;
    match backend {
        DbBackend::Postgres => {
            let quoted: Vec<String> = tables
                .iter()
                .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
                .collect();
            let sql = format!(
                "TRUNCATE TABLE {} RESTART IDENTITY CASCADE",
                quoted.join(", ")
            );
            sqlx::query(&sql).execute(&mut *conn).await?;
        }
        DbBackend::Mysql => {
            sqlx::query("SET FOREIGN_KEY_CHECKS = 0")
                .execute(&mut *conn)
                .await?;
            for t in &tables {
                let safe = t.replace('`', "``");
                let sql = format!("TRUNCATE TABLE `{safe}`");
                sqlx::query(&sql).execute(&mut *conn).await?;
            }
            sqlx::query("SET FOREIGN_KEY_CHECKS = 1")
                .execute(&mut *conn)
                .await?;
        }
        DbBackend::Sqlite => {
            // SQLite has no multi-table TRUNCATE; DELETE is transactional and
            // cheap enough for ROPDS-sized seed data.
            for t in &tables {
                let safe = t.replace('"', "\"\"");
                let sql = format!("DELETE FROM \"{safe}\"");
                sqlx::query(&sql).execute(&mut *conn).await?;
            }
            // Reset AUTOINCREMENT counters if the sqlite_sequence table exists.
            let _ = sqlx::query("DELETE FROM sqlite_sequence")
                .execute(&mut *conn)
                .await;
        }
    }
    tracing::info!("Cleared {} data table(s) for migration prep", tables.len());
    Ok(())
}

async fn preflight_init(pool: &sqlx::AnyPool, backend: DbBackend) -> Result<(), sqlx::Error> {
    let tables = list_user_tables(pool, backend).await?;
    let has_sqlx = tables.iter().any(|t| t == "_sqlx_migrations");
    let data_tables: Vec<&String> = tables
        .iter()
        .filter(|t| t.as_str() != "_sqlx_migrations")
        .collect();

    if !has_sqlx && !data_tables.is_empty() {
        let names: Vec<&str> = data_tables.iter().map(|s| s.as_str()).collect();
        return Err(sqlx::Error::Configuration(
            format!(
                "target database has tables ({}) but no _sqlx_migrations; \
                 refusing to initialize to avoid corrupting existing data",
                names.join(", ")
            )
            .into(),
        ));
    }

    // `--init-db` clears data tables at the end of its run. If any user table
    // already contains rows, the database is either a live install or an
    // already-migrated target — either way, wiping it would destroy data.
    // Refuse before applying migrations or clearing anything.
    let mut populated: Vec<(String, i64)> = Vec::new();
    for t in &data_tables {
        let quoted = match backend {
            DbBackend::Postgres | DbBackend::Sqlite => {
                format!("\"{}\"", t.replace('"', "\"\""))
            }
            DbBackend::Mysql => format!("`{}`", t.replace('`', "``")),
        };
        let sql = format!("SELECT COUNT(*) FROM {quoted}");
        let (count,): (i64,) = sqlx::query_as(&sql).fetch_one(pool).await?;
        if count > 0 {
            populated.push((t.to_string(), count));
        }
    }
    if !populated.is_empty() {
        let detail: Vec<String> = populated
            .iter()
            .map(|(t, c)| format!("{t} ({c} rows)"))
            .collect();
        return Err(sqlx::Error::Configuration(
            format!(
                "target database already contains data — refusing to run \
                 `--init-db` (which would wipe every data table). \
                 Populated tables: {}. \
                 If you really mean to reset this database, truncate these \
                 tables (or drop and recreate the database) manually, then \
                 re-run `--init-db`.",
                detail.join(", ")
            )
            .into(),
        ));
    }
    Ok(())
}

async fn list_user_tables(
    pool: &sqlx::AnyPool,
    backend: DbBackend,
) -> Result<Vec<String>, sqlx::Error> {
    let query = match backend {
        DbBackend::Postgres => {
            "SELECT table_name::text FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
             ORDER BY table_name"
        }
        DbBackend::Mysql => {
            "SELECT CAST(table_name AS CHAR) FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' \
             ORDER BY table_name"
        }
        DbBackend::Sqlite => {
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
             ORDER BY name"
        }
    };
    let rows: Vec<(String,)> = sqlx::query_as(query).fetch_all(pool).await?;
    Ok(rows.into_iter().map(|(n,)| n).collect())
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

    #[test]
    fn test_redact_database_url_with_password() {
        let url = "postgres://ropds:hunter2@db.example.com:5432/ropds";
        assert_eq!(
            redact_database_url(url),
            "postgres://***@db.example.com:5432/ropds"
        );
    }

    #[test]
    fn test_redact_database_url_with_username_only() {
        let url = "mysql://ropds@db.internal:3306/ropds?ssl-mode=REQUIRED";
        assert_eq!(
            redact_database_url(url),
            "mysql://***@db.internal:3306/ropds?ssl-mode=REQUIRED"
        );
    }

    #[test]
    fn test_redact_database_url_without_userinfo() {
        let url = "sqlite://ropds.db";
        assert!(matches!(redact_database_url(url), Cow::Borrowed(_)));
        assert_eq!(redact_database_url(url), "sqlite://ropds.db");
    }

    #[test]
    fn test_db_backend_from_url() {
        assert_eq!(
            DbBackend::from_url("postgres://u:p@h:5432/d"),
            DbBackend::Postgres
        );
        assert_eq!(
            DbBackend::from_url("postgresql://u:p@h:5432/d"),
            DbBackend::Postgres
        );
        assert_eq!(
            DbBackend::from_url("mysql://u:p@h:3306/d"),
            DbBackend::Mysql
        );
        assert_eq!(
            DbBackend::from_url("mariadb://u:p@h:3306/d"),
            DbBackend::Mysql
        );
        assert_eq!(DbBackend::from_url("sqlite://ropds.db"), DbBackend::Sqlite);
        assert_eq!(DbBackend::from_url("sqlite::memory:"), DbBackend::Sqlite);
    }
}
