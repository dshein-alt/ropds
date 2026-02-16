use crate::db::DbPool;

use crate::db::models::Book;

pub async fn get_by_id(pool: &DbPool, id: i64) -> Result<Option<Book>, sqlx::Error> {
    sqlx::query_as::<_, Book>("SELECT * FROM books WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn get_by_catalog(
    pool: &DbPool,
    catalog_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<Book>, sqlx::Error> {
    sqlx::query_as::<_, Book>(
        "SELECT * FROM books WHERE catalog_id = ? AND avail > 0 ORDER BY search_title LIMIT ? OFFSET ?",
    )
    .bind(catalog_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn get_by_author(
    pool: &DbPool,
    author_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<Book>, sqlx::Error> {
    sqlx::query_as::<_, Book>(
        "SELECT b.* FROM books b \
         JOIN book_authors ba ON ba.book_id = b.id \
         WHERE ba.author_id = ? AND b.avail > 0 \
         ORDER BY b.search_title LIMIT ? OFFSET ?",
    )
    .bind(author_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn get_by_genre(
    pool: &DbPool,
    genre_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<Book>, sqlx::Error> {
    sqlx::query_as::<_, Book>(
        "SELECT b.* FROM books b \
         JOIN book_genres bg ON bg.book_id = b.id \
         WHERE bg.genre_id = ? AND b.avail > 0 \
         ORDER BY b.search_title LIMIT ? OFFSET ?",
    )
    .bind(genre_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn get_by_series(
    pool: &DbPool,
    series_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<Book>, sqlx::Error> {
    sqlx::query_as::<_, Book>(
        "SELECT b.* FROM books b \
         JOIN book_series bs ON bs.book_id = b.id \
         WHERE bs.series_id = ? AND b.avail > 0 \
         ORDER BY bs.ser_no, b.search_title LIMIT ? OFFSET ?",
    )
    .bind(series_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn search_by_title(
    pool: &DbPool,
    term: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Book>, sqlx::Error> {
    let pattern = format!("%{term}%");
    sqlx::query_as::<_, Book>(
        "SELECT * FROM books WHERE search_title LIKE ? AND avail > 0 \
         ORDER BY search_title LIMIT ? OFFSET ?",
    )
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn find_by_path_and_filename(
    pool: &DbPool,
    path: &str,
    filename: &str,
) -> Result<Option<Book>, sqlx::Error> {
    sqlx::query_as::<_, Book>("SELECT * FROM books WHERE path = ? AND filename = ?")
        .bind(path)
        .bind(filename)
        .fetch_optional(pool)
        .await
}

pub async fn insert(
    pool: &DbPool,
    catalog_id: i64,
    filename: &str,
    path: &str,
    format: &str,
    title: &str,
    search_title: &str,
    annotation: &str,
    docdate: &str,
    lang: &str,
    lang_code: i32,
    size: i64,
    cat_type: i32,
    cover: i32,
    cover_type: &str,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
         annotation, docdate, lang, lang_code, size, avail, cat_type, cover, cover_type) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 2, ?, ?, ?)",
    )
    .bind(catalog_id)
    .bind(filename)
    .bind(path)
    .bind(format)
    .bind(title)
    .bind(search_title)
    .bind(annotation)
    .bind(docdate)
    .bind(lang)
    .bind(lang_code)
    .bind(size)
    .bind(cat_type)
    .bind(cover)
    .bind(cover_type)
    .execute(pool)
    .await?;
    Ok(result.last_insert_id().unwrap_or(0))
}

pub async fn set_avail_all(pool: &DbPool, avail: i32) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE books SET avail = ?")
        .bind(avail)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn set_avail(pool: &DbPool, id: i64, avail: i32) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE books SET avail = ? WHERE id = ?")
        .bind(avail)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_unavailable(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM books WHERE avail = 0")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn count(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM books WHERE avail > 0")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}
