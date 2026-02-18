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
    sqlx::query_as::<_, Genre>("SELECT * FROM genres WHERE section = ? ORDER BY subsection")
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

pub async fn link_book(pool: &DbPool, book_id: i64, genre_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO book_genres (book_id, genre_id) VALUES (?, ?)")
        .bind(book_id)
        .bind(genre_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Link a book to a genre by genre code. If the code doesn't match
/// any seeded genre, the link is silently skipped.
pub async fn link_book_by_code(pool: &DbPool, book_id: i64, code: &str) -> Result<(), sqlx::Error> {
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

pub async fn unlink_book(pool: &DbPool, book_id: i64, genre_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM book_genres WHERE book_id = ? AND genre_id = ?")
        .bind(book_id)
        .bind(genre_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Replace all genres for a book: delete existing links, insert new ones.
pub async fn set_book_genres(
    pool: &DbPool,
    book_id: i64,
    genre_ids: &[i64],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM book_genres WHERE book_id = ?")
        .bind(book_id)
        .execute(pool)
        .await?;
    for &genre_id in genre_ids {
        sqlx::query("INSERT OR IGNORE INTO book_genres (book_id, genre_id) VALUES (?, ?)")
            .bind(book_id)
            .bind(genre_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Get genre sections with book counts.
pub async fn get_sections_with_counts(pool: &DbPool) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT g.section, COUNT(DISTINCT bg.book_id) as cnt \
         FROM genres g \
         JOIN book_genres bg ON bg.genre_id = g.id \
         JOIN books b ON b.id = bg.book_id AND b.avail > 0 \
         GROUP BY g.section \
         ORDER BY g.section",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get genres within a section, each with its book count.
pub async fn get_by_section_with_counts(
    pool: &DbPool,
    section: &str,
) -> Result<Vec<(Genre, i64)>, sqlx::Error> {
    let rows: Vec<(i64, String, String, String, i64)> = sqlx::query_as(
        "SELECT g.id, g.code, g.section, g.subsection, COUNT(DISTINCT bg.book_id) as cnt \
         FROM genres g \
         LEFT JOIN book_genres bg ON bg.genre_id = g.id \
         LEFT JOIN books b ON b.id = bg.book_id AND b.avail > 0 \
         WHERE g.section = ? \
         GROUP BY g.id, g.code, g.section, g.subsection \
         ORDER BY g.subsection",
    )
    .bind(section)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, code, section, subsection, cnt)| {
            (
                Genre {
                    id,
                    code,
                    section,
                    subsection,
                },
                cnt,
            )
        })
        .collect())
}
