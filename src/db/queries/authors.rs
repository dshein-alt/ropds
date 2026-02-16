use crate::db::DbPool;

use crate::db::models::Author;

pub async fn get_by_id(pool: &DbPool, id: i64) -> Result<Option<Author>, sqlx::Error> {
    sqlx::query_as::<_, Author>("SELECT * FROM authors WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn search_by_name(
    pool: &DbPool,
    term: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Author>, sqlx::Error> {
    let pattern = format!("%{term}%");
    sqlx::query_as::<_, Author>(
        "SELECT * FROM authors WHERE search_full_name LIKE ? \
         ORDER BY search_full_name LIMIT ? OFFSET ?",
    )
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn get_by_lang_code_prefix(
    pool: &DbPool,
    lang_code: i32,
    prefix: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Author>, sqlx::Error> {
    let pattern = format!("{prefix}%");
    sqlx::query_as::<_, Author>(
        "SELECT * FROM authors WHERE lang_code = ? AND search_full_name LIKE ? \
         ORDER BY search_full_name LIMIT ? OFFSET ?",
    )
    .bind(lang_code)
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn find_by_name(pool: &DbPool, full_name: &str) -> Result<Option<Author>, sqlx::Error> {
    sqlx::query_as::<_, Author>("SELECT * FROM authors WHERE full_name = ?")
        .bind(full_name)
        .fetch_optional(pool)
        .await
}

pub async fn insert(
    pool: &DbPool,
    full_name: &str,
    search_full_name: &str,
    lang_code: i32,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO authors (full_name, search_full_name, lang_code) VALUES (?, ?, ?)",
    )
    .bind(full_name)
    .bind(search_full_name)
    .bind(lang_code)
    .execute(pool)
    .await?;
    Ok(result.last_insert_id().unwrap_or(0))
}

pub async fn count(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM authors")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn link_book(
    pool: &DbPool,
    book_id: i64,
    author_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO book_authors (book_id, author_id) VALUES (?, ?)")
        .bind(book_id)
        .bind(author_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_for_book(pool: &DbPool, book_id: i64) -> Result<Vec<Author>, sqlx::Error> {
    sqlx::query_as::<_, Author>(
        "SELECT a.* FROM authors a \
         JOIN book_authors ba ON ba.author_id = a.id \
         WHERE ba.book_id = ? ORDER BY a.full_name",
    )
    .bind(book_id)
    .fetch_all(pool)
    .await
}
