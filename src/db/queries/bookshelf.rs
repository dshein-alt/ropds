use crate::db::DbPool;
use crate::db::models::Book;

/// Bookshelf sort column.
pub enum SortColumn {
    Date,
    Title,
    Author,
}

/// Add or update a book on the user's bookshelf.
/// Uses ON CONFLICT to update read_time on re-download.
pub async fn upsert(pool: &DbPool, user_id: i64, book_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO bookshelf (user_id, book_id, read_time) VALUES (?, ?, CURRENT_TIMESTAMP) \
         ON CONFLICT(user_id, book_id) DO UPDATE SET read_time = CURRENT_TIMESTAMP",
    )
    .bind(user_id)
    .bind(book_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get books on user's bookshelf with configurable sorting.
pub async fn get_by_user(
    pool: &DbPool,
    user_id: i64,
    sort: &SortColumn,
    ascending: bool,
    limit: i32,
    offset: i32,
) -> Result<Vec<Book>, sqlx::Error> {
    let dir = if ascending { "ASC" } else { "DESC" };
    let sql = match sort {
        SortColumn::Date => format!(
            "SELECT b.* FROM books b \
             JOIN bookshelf bs ON bs.book_id = b.id \
             WHERE bs.user_id = ? \
             ORDER BY bs.read_time {dir} \
             LIMIT ? OFFSET ?"
        ),
        SortColumn::Title => format!(
            "SELECT b.* FROM books b \
             JOIN bookshelf bs ON bs.book_id = b.id \
             WHERE bs.user_id = ? \
             ORDER BY b.title COLLATE NOCASE {dir} \
             LIMIT ? OFFSET ?"
        ),
        SortColumn::Author => format!(
            "SELECT b.* FROM books b \
             JOIN bookshelf bs ON bs.book_id = b.id \
             LEFT JOIN book_authors ba ON ba.book_id = b.id \
             LEFT JOIN authors a ON a.id = ba.author_id \
             WHERE bs.user_id = ? \
             GROUP BY b.id \
             ORDER BY COALESCE(MIN(a.full_name), '') COLLATE NOCASE {dir} \
             LIMIT ? OFFSET ?"
        ),
    };
    sqlx::query_as::<_, Book>(&sql)
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
}

/// Get read_time values for a set of bookshelf entries.
pub async fn get_read_times(
    pool: &DbPool,
    user_id: i64,
) -> Result<std::collections::HashMap<i64, String>, sqlx::Error> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT book_id, read_time FROM bookshelf WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}

/// Count books on user's bookshelf.
pub async fn count_by_user(pool: &DbPool, user_id: i64) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM bookshelf WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Check if a specific book is on the user's bookshelf.
pub async fn is_on_shelf(pool: &DbPool, user_id: i64, book_id: i64) -> Result<bool, sqlx::Error> {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM bookshelf WHERE user_id = ? AND book_id = ?")
            .bind(user_id)
            .bind(book_id)
            .fetch_one(pool)
            .await?;
    Ok(row.0 > 0)
}

/// Get set of book IDs on the user's bookshelf (for bulk star rendering).
pub async fn get_book_ids_for_user(
    pool: &DbPool,
    user_id: i64,
) -> Result<std::collections::HashSet<i64>, sqlx::Error> {
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT book_id FROM bookshelf WHERE user_id = ?")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Remove a single book from the user's bookshelf.
pub async fn delete_one(pool: &DbPool, user_id: i64, book_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM bookshelf WHERE user_id = ? AND book_id = ?")
        .bind(user_id)
        .bind(book_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Clear all books from the user's bookshelf.
pub async fn clear_all(pool: &DbPool, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM bookshelf WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    async fn insert_user(pool: &DbPool, username: &str) -> i64 {
        sqlx::query("INSERT INTO users (username, password_hash, is_superuser) VALUES (?, 'h', 0)")
            .bind(username)
            .execute(pool)
            .await
            .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM users WHERE username = ?")
            .bind(username)
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    async fn ensure_catalog(pool: &DbPool) -> i64 {
        sqlx::query("INSERT INTO catalogs (path, cat_name) VALUES ('/bookshelf', 'bookshelf')")
            .execute(pool)
            .await
            .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM catalogs WHERE path = '/bookshelf'")
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    async fn insert_book(pool: &DbPool, catalog_id: i64, title: &str) -> i64 {
        let search_title = title.to_uppercase();
        sqlx::query(
            "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
             lang, lang_code, size, avail, cat_type, cover, cover_type) \
             VALUES (?, ?, '/bookshelf', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
        )
        .bind(catalog_id)
        .bind(format!("{title}.fb2"))
        .bind(title)
        .bind(search_title)
        .execute(pool)
        .await
        .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM books WHERE catalog_id = ? AND title = ?")
            .bind(catalog_id)
            .bind(title)
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    #[tokio::test]
    async fn test_bookshelf_lifecycle_and_upsert_dedup() {
        let (pool, _) = create_test_pool().await;
        let user_id = insert_user(&pool, "shelf_user").await;
        let catalog_id = ensure_catalog(&pool).await;
        let b1 = insert_book(&pool, catalog_id, "Book One").await;
        let b2 = insert_book(&pool, catalog_id, "Book Two").await;

        upsert(&pool, user_id, b1).await.unwrap();
        upsert(&pool, user_id, b1).await.unwrap(); // should not duplicate
        upsert(&pool, user_id, b2).await.unwrap();

        assert_eq!(count_by_user(&pool, user_id).await.unwrap(), 2);
        assert!(is_on_shelf(&pool, user_id, b1).await.unwrap());
        assert!(!is_on_shelf(&pool, user_id, 99999).await.unwrap());

        let ids = get_book_ids_for_user(&pool, user_id).await.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&b1));
        assert!(ids.contains(&b2));

        let read_times = get_read_times(&pool, user_id).await.unwrap();
        assert_eq!(read_times.len(), 2);
        assert!(read_times.contains_key(&b1));

        delete_one(&pool, user_id, b1).await.unwrap();
        assert_eq!(count_by_user(&pool, user_id).await.unwrap(), 1);

        clear_all(&pool, user_id).await.unwrap();
        assert_eq!(count_by_user(&pool, user_id).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_get_by_user_sorting_variants() {
        let (pool, _) = create_test_pool().await;
        let user_id = insert_user(&pool, "sort_user").await;
        let catalog_id = ensure_catalog(&pool).await;

        let b_alpha = insert_book(&pool, catalog_id, "Alpha Book").await;
        let b_zulu = insert_book(&pool, catalog_id, "Zulu Book").await;

        // Author links for author sorting.
        sqlx::query(
            "INSERT INTO authors (full_name, search_full_name, lang_code) VALUES \
             ('Charlie', 'CHARLIE', 2), ('Alice', 'ALICE', 2)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let a_charlie: (i64,) =
            sqlx::query_as("SELECT id FROM authors WHERE full_name = 'Charlie'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let a_alice: (i64,) = sqlx::query_as("SELECT id FROM authors WHERE full_name = 'Alice'")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO book_authors (book_id, author_id) VALUES (?, ?), (?, ?)")
            .bind(b_alpha)
            .bind(a_charlie.0)
            .bind(b_zulu)
            .bind(a_alice.0)
            .execute(&pool)
            .await
            .unwrap();

        upsert(&pool, user_id, b_alpha).await.unwrap();
        upsert(&pool, user_id, b_zulu).await.unwrap();
        sqlx::query("UPDATE bookshelf SET read_time = ? WHERE user_id = ? AND book_id = ?")
            .bind("2026-01-01 00:00:00")
            .bind(user_id)
            .bind(b_alpha)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE bookshelf SET read_time = ? WHERE user_id = ? AND book_id = ?")
            .bind("2026-01-02 00:00:00")
            .bind(user_id)
            .bind(b_zulu)
            .execute(&pool)
            .await
            .unwrap();

        let by_title = get_by_user(&pool, user_id, &SortColumn::Title, true, 10, 0)
            .await
            .unwrap();
        assert_eq!(by_title[0].id, b_alpha);
        assert_eq!(by_title[1].id, b_zulu);

        let by_author = get_by_user(&pool, user_id, &SortColumn::Author, true, 10, 0)
            .await
            .unwrap();
        assert_eq!(by_author[0].id, b_zulu); // Alice
        assert_eq!(by_author[1].id, b_alpha); // Charlie

        let by_date_desc = get_by_user(&pool, user_id, &SortColumn::Date, false, 10, 0)
            .await
            .unwrap();
        assert_eq!(by_date_desc[0].id, b_zulu);
        assert_eq!(by_date_desc[1].id, b_alpha);
    }
}
