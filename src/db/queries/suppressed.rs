use crate::db::DbPool;

fn suppression_key(path: &str, filename: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    hasher.update([0u8]);
    hasher.update(filename.as_bytes());
    hex::encode(hasher.finalize())
}

/// Check if a (path, filename) pair is suppressed.
pub async fn is_suppressed(pool: &DbPool, path: &str, filename: &str) -> Result<bool, sqlx::Error> {
    let (count,): (i64,) = match pool.backend() {
        crate::db::DbBackend::Mysql => {
            let sql = pool.sql(
                "SELECT COUNT(*) FROM suppressed_books \
                 WHERE suppressed_key = ? AND path = ? AND filename = ?",
            );
            let key = suppression_key(path, filename);
            sqlx::query_as(&sql)
                .bind(key)
                .bind(path)
                .bind(filename)
                .fetch_one(pool.inner())
                .await?
        }
        _ => {
            let sql =
                pool.sql("SELECT COUNT(*) FROM suppressed_books WHERE path = ? AND filename = ?");
            sqlx::query_as(&sql)
                .bind(path)
                .bind(filename)
                .fetch_one(pool.inner())
                .await?
        }
    };
    Ok(count > 0)
}

/// Insert a suppression record.
pub async fn suppress(pool: &DbPool, path: &str, filename: &str) -> Result<(), sqlx::Error> {
    let sql = match pool.backend() {
        crate::db::DbBackend::Mysql => {
            "INSERT IGNORE INTO suppressed_books (path, filename, suppressed_key) VALUES (?, ?, ?)"
        }
        _ => {
            "INSERT INTO suppressed_books (path, filename) VALUES (?, ?) \
             ON CONFLICT (path, filename) DO NOTHING"
        }
    };
    let sql = pool.sql(sql);
    let query = sqlx::query(&sql).bind(path).bind(filename);
    match pool.backend() {
        crate::db::DbBackend::Mysql => {
            let key = suppression_key(path, filename);
            query.bind(key).execute(pool.inner()).await?;
        }
        _ => {
            query.execute(pool.inner()).await?;
        }
    }
    Ok(())
}
