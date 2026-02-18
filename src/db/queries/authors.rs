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
        "INSERT OR IGNORE INTO authors (full_name, search_full_name, lang_code) VALUES (?, ?, ?)",
    )
    .bind(full_name)
    .bind(search_full_name)
    .bind(lang_code)
    .execute(pool)
    .await?;
    if let Some(id) = result.last_insert_id() {
        if id > 0 {
            return Ok(id);
        }
    }
    // Fallback: query back by name (INSERT OR IGNORE returns 0 on conflict)
    let row: (i64,) = sqlx::query_as("SELECT id FROM authors WHERE full_name = ?")
        .bind(full_name)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn count(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM authors")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn link_book(pool: &DbPool, book_id: i64, author_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO book_authors (book_id, author_id) VALUES (?, ?)")
        .bind(book_id)
        .bind(author_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn unlink_book(pool: &DbPool, book_id: i64, author_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM book_authors WHERE book_id = ? AND author_id = ?")
        .bind(book_id)
        .bind(author_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Replace all authors for a book: delete existing links, insert new ones,
/// then remove any orphaned authors (no remaining book links).
pub async fn set_book_authors(
    pool: &DbPool,
    book_id: i64,
    author_ids: &[i64],
) -> Result<(), sqlx::Error> {
    // Remember old author IDs before unlinking
    let old_ids: Vec<(i64,)> =
        sqlx::query_as("SELECT author_id FROM book_authors WHERE book_id = ?")
            .bind(book_id)
            .fetch_all(pool)
            .await?;

    sqlx::query("DELETE FROM book_authors WHERE book_id = ?")
        .bind(book_id)
        .execute(pool)
        .await?;
    for &author_id in author_ids {
        sqlx::query("INSERT OR IGNORE INTO book_authors (book_id, author_id) VALUES (?, ?)")
            .bind(book_id)
            .bind(author_id)
            .execute(pool)
            .await?;
    }

    // Clean up orphaned authors that no longer have any books
    for (old_id,) in old_ids {
        if !author_ids.contains(&old_id) {
            delete_if_orphaned(pool, old_id).await?;
        }
    }
    Ok(())
}

/// Delete an author if they have no remaining book links.
pub async fn delete_if_orphaned(pool: &DbPool, author_id: i64) -> Result<(), sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM book_authors WHERE author_id = ?")
        .bind(author_id)
        .fetch_one(pool)
        .await?;
    if row.0 == 0 {
        sqlx::query("DELETE FROM authors WHERE id = ?")
            .bind(author_id)
            .execute(pool)
            .await?;
    }
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

/// Count authors matching a name search (contains).
pub async fn count_by_name_search(pool: &DbPool, term: &str) -> Result<i64, sqlx::Error> {
    let pattern = format!("%{term}%");
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM authors WHERE search_full_name LIKE ?")
        .bind(&pattern)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Alphabet drill-down: get prefix groups for author names.
pub async fn get_name_prefix_groups(
    pool: &DbPool,
    lang_code: i32,
    current_prefix: &str,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let prefix_len = (current_prefix.chars().count() + 1) as i32;
    let like_pattern = format!("{}%", current_prefix);
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT SUBSTR(search_full_name, 1, ?) as prefix, COUNT(*) as cnt \
         FROM authors \
         WHERE (? = 0 OR lang_code = ?) AND search_full_name LIKE ? \
         GROUP BY SUBSTR(search_full_name, 1, ?) \
         ORDER BY prefix",
    )
    .bind(prefix_len)
    .bind(lang_code)
    .bind(lang_code)
    .bind(&like_pattern)
    .bind(prefix_len)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
