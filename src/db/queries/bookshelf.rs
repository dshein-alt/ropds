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
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM bookshelf WHERE user_id = ? AND book_id = ?",
    )
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
    let rows: Vec<(i64,)> =
        sqlx::query_as("SELECT book_id FROM bookshelf WHERE user_id = ?")
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
