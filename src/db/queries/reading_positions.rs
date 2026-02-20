use sqlx::FromRow;

use crate::db::DbPool;

#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct ReadingPosition {
    pub id: i64,
    pub user_id: i64,
    pub book_id: i64,
    pub position: String,
    pub progress: f64,
    pub updated_at: String,
}

/// Save or update reading position for a user/book pair.
/// After upsert, prunes entries beyond `max_entries` per user.
pub async fn save_position(
    pool: &DbPool,
    user_id: i64,
    book_id: i64,
    position: &str,
    progress: f64,
    max_entries: i64,
) -> Result<(), sqlx::Error> {
    let raw = match pool.backend() {
        crate::db::DbBackend::Mysql => {
            "INSERT INTO reading_positions (user_id, book_id, position, progress, updated_at) \
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP) \
             ON DUPLICATE KEY UPDATE position = VALUES(position), progress = VALUES(progress), \
             updated_at = CURRENT_TIMESTAMP"
        }
        _ => {
            "INSERT INTO reading_positions (user_id, book_id, position, progress, updated_at) \
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP) \
             ON CONFLICT(user_id, book_id) DO UPDATE SET \
             position = excluded.position, progress = excluded.progress, \
             updated_at = CURRENT_TIMESTAMP"
        }
    };
    let sql = pool.sql(raw);
    sqlx::query(&sql)
        .bind(user_id)
        .bind(book_id)
        .bind(position)
        .bind(progress)
        .execute(pool.inner())
        .await?;

    prune_oldest(pool, user_id, max_entries).await?;

    Ok(())
}

/// Get reading position for a specific user/book pair.
pub async fn get_position(
    pool: &DbPool,
    user_id: i64,
    book_id: i64,
) -> Result<Option<ReadingPosition>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT id, user_id, book_id, position, progress, updated_at \
         FROM reading_positions WHERE user_id = ? AND book_id = ?",
    );
    sqlx::query_as::<_, ReadingPosition>(&sql)
        .bind(user_id)
        .bind(book_id)
        .fetch_optional(pool.inner())
        .await
}

/// Recent reading history entry: position joined with book metadata.
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct RecentRead {
    pub book_id: i64,
    pub title: String,
    pub format: String,
    pub progress: f64,
    pub updated_at: String,
}

/// Get the N most recent reading positions for a user, joined with book info.
/// Used for the reader's "recent books" sidebar.
pub async fn get_recent(
    pool: &DbPool,
    user_id: i64,
    limit: i64,
) -> Result<Vec<RecentRead>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT rp.book_id, b.title, b.format, rp.progress, rp.updated_at \
         FROM reading_positions rp \
         JOIN books b ON b.id = rp.book_id \
         WHERE rp.user_id = ? \
         ORDER BY rp.updated_at DESC \
         LIMIT ?",
    );
    sqlx::query_as::<_, RecentRead>(&sql)
        .bind(user_id)
        .bind(limit)
        .fetch_all(pool.inner())
        .await
}

/// Get the most recently read book_id for a user (for the navbar "Reader" button).
pub async fn get_last_read_book_id(
    pool: &DbPool,
    user_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT book_id FROM reading_positions WHERE user_id = ? \
         ORDER BY updated_at DESC LIMIT 1",
    );
    let row: Option<(i64,)> = sqlx::query_as(&sql)
        .bind(user_id)
        .fetch_optional(pool.inner())
        .await?;
    Ok(row.map(|(id,)| id))
}

/// Delete reading positions beyond the `keep` most recent for a user.
async fn prune_oldest(pool: &DbPool, user_id: i64, keep: i64) -> Result<(), sqlx::Error> {
    // Count first to avoid unnecessary delete
    let count_sql = pool.sql("SELECT COUNT(*) FROM reading_positions WHERE user_id = ?");
    let (count,): (i64,) = sqlx::query_as(&count_sql)
        .bind(user_id)
        .fetch_one(pool.inner())
        .await?;

    if count <= keep {
        return Ok(());
    }

    // Delete all except the `keep` most recent.
    // LIMIT doesn't support bind parameters in all backends, so we format it directly.
    let raw = match pool.backend() {
        crate::db::DbBackend::Mysql => {
            // MySQL doesn't support LIMIT in subqueries with IN, use nested subquery
            format!(
                "DELETE FROM reading_positions WHERE user_id = ? AND id NOT IN \
                 (SELECT id FROM (SELECT id FROM reading_positions WHERE user_id = ? \
                 ORDER BY updated_at DESC LIMIT {keep}) AS tmp)"
            )
        }
        _ => {
            format!(
                "DELETE FROM reading_positions WHERE user_id = ? AND id NOT IN \
                 (SELECT id FROM reading_positions WHERE user_id = ? \
                 ORDER BY updated_at DESC LIMIT {keep})"
            )
        }
    };
    let sql = pool.sql(&raw);
    sqlx::query(&sql)
        .bind(user_id)
        .bind(user_id)
        .execute(pool.inner())
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    async fn insert_user(pool: &DbPool, username: &str) -> i64 {
        let sql = pool
            .sql("INSERT INTO users (username, password_hash, is_superuser) VALUES (?, 'h', 0)");
        sqlx::query(&sql)
            .bind(username)
            .execute(pool.inner())
            .await
            .unwrap();
        let sql = pool.sql("SELECT id FROM users WHERE username = ?");
        let row: (i64,) = sqlx::query_as(&sql)
            .bind(username)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        row.0
    }

    async fn ensure_catalog(pool: &DbPool) -> i64 {
        let sql = pool.sql("INSERT INTO catalogs (path, cat_name) VALUES ('/rp_test', 'rp_test')");
        sqlx::query(&sql).execute(pool.inner()).await.unwrap();
        let sql = pool.sql("SELECT id FROM catalogs WHERE path = '/rp_test'");
        let row: (i64,) = sqlx::query_as(&sql).fetch_one(pool.inner()).await.unwrap();
        row.0
    }

    async fn insert_book(pool: &DbPool, catalog_id: i64, title: &str) -> i64 {
        let search_title = title.to_uppercase();
        let sql = pool.sql(
            "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
             lang, lang_code, size, avail, cat_type, cover, cover_type) \
             VALUES (?, ?, '/rp_test', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
        );
        sqlx::query(&sql)
            .bind(catalog_id)
            .bind(format!("{title}.fb2"))
            .bind(title)
            .bind(search_title)
            .execute(pool.inner())
            .await
            .unwrap();
        let sql = pool.sql("SELECT id FROM books WHERE catalog_id = ? AND title = ?");
        let row: (i64,) = sqlx::query_as(&sql)
            .bind(catalog_id)
            .bind(title)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        row.0
    }

    #[tokio::test]
    async fn test_save_and_get_position() {
        let pool = create_test_pool().await;
        let user_id = insert_user(&pool, "reader1").await;
        let cat_id = ensure_catalog(&pool).await;
        let book_id = insert_book(&pool, cat_id, "Test Book").await;

        save_position(&pool, user_id, book_id, "epubcfi(/6/4!/4/1:0)", 0.25, 100)
            .await
            .unwrap();

        let pos = get_position(&pool, user_id, book_id).await.unwrap();
        assert!(pos.is_some());
        let pos = pos.unwrap();
        assert_eq!(pos.position, "epubcfi(/6/4!/4/1:0)");
        assert!((pos.progress - 0.25).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_upsert_updates_existing() {
        let pool = create_test_pool().await;
        let user_id = insert_user(&pool, "reader2").await;
        let cat_id = ensure_catalog(&pool).await;
        let book_id = insert_book(&pool, cat_id, "Update Book").await;

        save_position(&pool, user_id, book_id, "cfi1", 0.1, 100)
            .await
            .unwrap();

        save_position(&pool, user_id, book_id, "cfi2", 0.5, 100)
            .await
            .unwrap();

        let pos = get_position(&pool, user_id, book_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pos.position, "cfi2");
        assert!((pos.progress - 0.5).abs() < f64::EPSILON);

        let sql = pool.sql("SELECT COUNT(*) FROM reading_positions WHERE user_id = ?");
        let (count,): (i64,) = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_get_position_missing() {
        let pool = create_test_pool().await;
        let user_id = insert_user(&pool, "reader3").await;

        let pos = get_position(&pool, user_id, 99999).await.unwrap();
        assert!(pos.is_none());
    }

    #[tokio::test]
    async fn test_cap_at_configured_max() {
        let pool = create_test_pool().await;
        let user_id = insert_user(&pool, "reader4").await;
        let cat_id = ensure_catalog(&pool).await;

        // Insert 101 books and positions
        for i in 0..101 {
            let book_id = insert_book(&pool, cat_id, &format!("Cap Book {i:03}")).await;
            let sql = pool.sql(
                "INSERT INTO reading_positions (user_id, book_id, position, progress, updated_at) \
                 VALUES (?, ?, ?, 0.0, ?)",
            );
            sqlx::query(&sql)
                .bind(user_id)
                .bind(book_id)
                .bind(format!("pos{i}"))
                .bind(format!("2026-01-01 00:{:02}:00", i))
                .execute(pool.inner())
                .await
                .unwrap();
        }

        let sql = pool.sql("SELECT COUNT(*) FROM reading_positions WHERE user_id = ?");
        let (count,): (i64,) = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        assert_eq!(count, 101);

        // Trigger pruning with max_entries=100
        let sql = pool.sql(
            "SELECT book_id FROM reading_positions WHERE user_id = ? ORDER BY updated_at DESC LIMIT 1",
        );
        let (last_book_id,): (i64,) = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_one(pool.inner())
            .await
            .unwrap();

        save_position(&pool, user_id, last_book_id, "updated", 0.99, 100)
            .await
            .unwrap();

        let sql = pool.sql("SELECT COUNT(*) FROM reading_positions WHERE user_id = ?");
        let (count,): (i64,) = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        assert_eq!(count, 100);

        // Oldest entry (pos0) should be pruned
        let sql = pool
            .sql("SELECT COUNT(*) FROM reading_positions WHERE user_id = ? AND position = 'pos0'");
        let (count,): (i64,) = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        assert_eq!(count, 0);

        // Now test with a smaller cap (e.g., 50): should prune down to 50
        save_position(&pool, user_id, last_book_id, "cap50", 0.5, 50)
            .await
            .unwrap();
        let sql = pool.sql("SELECT COUNT(*) FROM reading_positions WHERE user_id = ?");
        let (count,): (i64,) = sqlx::query_as(&sql)
            .bind(user_id)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        assert_eq!(count, 50, "should respect non-100 max_entries cap");
    }

    #[tokio::test]
    async fn test_multi_user_isolation() {
        let pool = create_test_pool().await;
        let user1 = insert_user(&pool, "iso_user1").await;
        let user2 = insert_user(&pool, "iso_user2").await;
        let cat_id = ensure_catalog(&pool).await;
        let book_id = insert_book(&pool, cat_id, "Shared Book").await;

        save_position(&pool, user1, book_id, "cfi_user1", 0.3, 100)
            .await
            .unwrap();
        save_position(&pool, user2, book_id, "cfi_user2", 0.7, 100)
            .await
            .unwrap();

        let pos1 = get_position(&pool, user1, book_id).await.unwrap().unwrap();
        assert_eq!(pos1.position, "cfi_user1");
        assert!((pos1.progress - 0.3).abs() < f64::EPSILON);

        let pos2 = get_position(&pool, user2, book_id).await.unwrap().unwrap();
        assert_eq!(pos2.position, "cfi_user2");
        assert!((pos2.progress - 0.7).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_get_recent_and_last_read() {
        let pool = create_test_pool().await;
        let user_id = insert_user(&pool, "recent_user").await;
        let cat_id = ensure_catalog(&pool).await;
        let b1 = insert_book(&pool, cat_id, "Recent Book A").await;
        let b2 = insert_book(&pool, cat_id, "Recent Book B").await;
        let b3 = insert_book(&pool, cat_id, "Recent Book C").await;

        save_position(&pool, user_id, b1, "cfi1", 0.1, 100)
            .await
            .unwrap();
        save_position(&pool, user_id, b2, "cfi2", 0.5, 100)
            .await
            .unwrap();
        save_position(&pool, user_id, b3, "cfi3", 0.9, 100)
            .await
            .unwrap();

        // get_recent returns most recent first
        let recent = get_recent(&pool, user_id, 10).await.unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].book_id, b3);
        assert_eq!(recent[1].book_id, b2);
        assert_eq!(recent[2].book_id, b1);
        assert_eq!(recent[0].title, "Recent Book C");

        // get_recent with limit=2 returns only 2
        let recent = get_recent(&pool, user_id, 2).await.unwrap();
        assert_eq!(recent.len(), 2);

        // get_last_read_book_id returns most recent
        let last = get_last_read_book_id(&pool, user_id).await.unwrap();
        assert_eq!(last, Some(b3));

        // No reading history for unknown user
        let last = get_last_read_book_id(&pool, 99999).await.unwrap();
        assert!(last.is_none());
    }
}
