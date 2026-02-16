use crate::db::DbPool;

use crate::db::models::Genre;

pub async fn get_by_id(pool: &DbPool, id: i64) -> Result<Option<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>("SELECT * FROM genres WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn get_by_code(pool: &DbPool, code: &str) -> Result<Option<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>("SELECT * FROM genres WHERE code = ?")
        .bind(code)
        .fetch_optional(pool)
        .await
}

pub async fn get_sections(pool: &DbPool) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT section FROM genres ORDER BY section")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn get_by_section(pool: &DbPool, section: &str) -> Result<Vec<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>(
        "SELECT * FROM genres WHERE section = ? ORDER BY subsection",
    )
    .bind(section)
    .fetch_all(pool)
    .await
}

pub async fn get_all(pool: &DbPool) -> Result<Vec<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>("SELECT * FROM genres ORDER BY section, subsection")
        .fetch_all(pool)
        .await
}

pub async fn count(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM genres")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn link_book(
    pool: &DbPool,
    book_id: i64,
    genre_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO book_genres (book_id, genre_id) VALUES (?, ?)")
        .bind(book_id)
        .bind(genre_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Link a book to a genre by genre code. If the code doesn't match
/// any seeded genre, the link is silently skipped.
pub async fn link_book_by_code(
    pool: &DbPool,
    book_id: i64,
    code: &str,
) -> Result<(), sqlx::Error> {
    if let Some(genre) = get_by_code(pool, code).await? {
        link_book(pool, book_id, genre.id).await?;
    }
    Ok(())
}

pub async fn get_for_book(pool: &DbPool, book_id: i64) -> Result<Vec<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>(
        "SELECT g.* FROM genres g \
         JOIN book_genres bg ON bg.genre_id = g.id \
         WHERE bg.book_id = ? ORDER BY g.section, g.subsection",
    )
    .bind(book_id)
    .fetch_all(pool)
    .await
}
