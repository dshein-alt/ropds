use crate::db::DbPool;

/// Check if a (path, filename) pair is suppressed.
pub async fn is_suppressed(pool: &DbPool, path: &str, filename: &str) -> Result<bool, sqlx::Error> {
    let sql = pool.sql("SELECT COUNT(*) FROM suppressed_books WHERE path = ? AND filename = ?");
    let (count,): (i64,) = sqlx::query_as(&sql)
        .bind(path)
        .bind(filename)
        .fetch_one(pool.inner())
        .await?;
    Ok(count > 0)
}

/// Insert a suppression record.
pub async fn suppress(pool: &DbPool, path: &str, filename: &str) -> Result<(), sqlx::Error> {
    let sql = match pool.backend() {
        crate::db::DbBackend::Mysql => {
            "INSERT IGNORE INTO suppressed_books (path, filename) VALUES (?, ?)"
        }
        _ => {
            "INSERT INTO suppressed_books (path, filename) VALUES (?, ?) \
             ON CONFLICT (path, filename) DO NOTHING"
        }
    };
    let sql = pool.sql(sql);
    sqlx::query(&sql)
        .bind(path)
        .bind(filename)
        .execute(pool.inner())
        .await?;
    Ok(())
}
