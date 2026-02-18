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
    hide_doubles: bool,
) -> Result<Vec<Book>, sqlx::Error> {
    if hide_doubles {
        sqlx::query_as::<_, Book>(
            "SELECT * FROM books WHERE catalog_id = ? AND avail > 0 \
             AND id IN (SELECT MIN(id) FROM books WHERE catalog_id = ? AND avail > 0 GROUP BY search_title) \
             ORDER BY search_title LIMIT ? OFFSET ?",
        )
        .bind(catalog_id)
        .bind(catalog_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, Book>(
            "SELECT * FROM books WHERE catalog_id = ? AND avail > 0 ORDER BY search_title LIMIT ? OFFSET ?",
        )
        .bind(catalog_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    }
}

pub async fn get_by_author(
    pool: &DbPool,
    author_id: i64,
    limit: i32,
    offset: i32,
    hide_doubles: bool,
) -> Result<Vec<Book>, sqlx::Error> {
    if hide_doubles {
        sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b \
             JOIN book_authors ba ON ba.book_id = b.id \
             WHERE ba.author_id = ? AND b.avail > 0 \
             AND b.id IN (SELECT MIN(b2.id) FROM books b2 \
               JOIN book_authors ba2 ON ba2.book_id = b2.id \
               WHERE ba2.author_id = ? AND b2.avail > 0 GROUP BY b2.search_title) \
             ORDER BY b.search_title LIMIT ? OFFSET ?",
        )
        .bind(author_id)
        .bind(author_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
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
}

pub async fn get_by_genre(
    pool: &DbPool,
    genre_id: i64,
    limit: i32,
    offset: i32,
    hide_doubles: bool,
) -> Result<Vec<Book>, sqlx::Error> {
    if hide_doubles {
        sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b \
             JOIN book_genres bg ON bg.book_id = b.id \
             WHERE bg.genre_id = ? AND b.avail > 0 \
             AND b.id IN (SELECT MIN(b2.id) FROM books b2 \
               JOIN book_genres bg2 ON bg2.book_id = b2.id \
               WHERE bg2.genre_id = ? AND b2.avail > 0 GROUP BY b2.search_title) \
             ORDER BY b.search_title LIMIT ? OFFSET ?",
        )
        .bind(genre_id)
        .bind(genre_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
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
}

pub async fn get_by_series(
    pool: &DbPool,
    series_id: i64,
    limit: i32,
    offset: i32,
    hide_doubles: bool,
) -> Result<Vec<Book>, sqlx::Error> {
    if hide_doubles {
        sqlx::query_as::<_, Book>(
            "SELECT b.* FROM books b \
             JOIN book_series bs ON bs.book_id = b.id \
             WHERE bs.series_id = ? AND b.avail > 0 \
             AND b.id IN (SELECT MIN(b2.id) FROM books b2 \
               JOIN book_series bs2 ON bs2.book_id = b2.id \
               WHERE bs2.series_id = ? AND b2.avail > 0 GROUP BY b2.search_title) \
             ORDER BY bs.ser_no, b.search_title LIMIT ? OFFSET ?",
        )
        .bind(series_id)
        .bind(series_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
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
}

pub async fn search_by_title(
    pool: &DbPool,
    term: &str,
    limit: i32,
    offset: i32,
    hide_doubles: bool,
) -> Result<Vec<Book>, sqlx::Error> {
    let pattern = format!("%{term}%");
    if hide_doubles {
        sqlx::query_as::<_, Book>(
            "SELECT * FROM books WHERE search_title LIKE ? AND avail > 0 \
             AND id IN (SELECT MIN(id) FROM books WHERE search_title LIKE ? AND avail > 0 GROUP BY search_title) \
             ORDER BY search_title LIMIT ? OFFSET ?",
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
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
}

pub async fn search_by_title_prefix(
    pool: &DbPool,
    prefix: &str,
    limit: i32,
    offset: i32,
    hide_doubles: bool,
) -> Result<Vec<Book>, sqlx::Error> {
    let pattern = format!("{prefix}%");
    if hide_doubles {
        sqlx::query_as::<_, Book>(
            "SELECT * FROM books WHERE search_title LIKE ? AND avail > 0 \
             AND id IN (SELECT MIN(id) FROM books WHERE search_title LIKE ? AND avail > 0 GROUP BY search_title) \
             ORDER BY search_title LIMIT ? OFFSET ?",
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
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
    if let Some(id) = result.last_insert_id() {
        return Ok(id);
    }
    let row: (i64,) = sqlx::query_as("SELECT id FROM books WHERE path = ? AND filename = ?")
        .bind(path)
        .bind(filename)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
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

pub async fn set_avail_by_path(pool: &DbPool, path: &str, avail: i32) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE books SET avail = ? WHERE path = ?")
        .bind(avail)
        .bind(path)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn set_avail_for_inpx_dir(
    pool: &DbPool,
    inpx_dir: &str,
    avail: i32,
) -> Result<u64, sqlx::Error> {
    let result = if inpx_dir.is_empty() {
        sqlx::query("UPDATE books SET avail = ? WHERE cat_type = ?")
            .bind(avail)
            .bind(crate::db::models::CAT_INPX)
            .execute(pool)
            .await?
    } else {
        let pattern = format!("{inpx_dir}/%");
        sqlx::query("UPDATE books SET avail = ? WHERE cat_type = ? AND path LIKE ?")
            .bind(avail)
            .bind(crate::db::models::CAT_INPX)
            .bind(pattern)
            .execute(pool)
            .await?
    };

    Ok(result.rows_affected())
}

/// Mark unverified books as logically deleted (avail=0, hidden from queries).
pub async fn logical_delete_unavailable(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE books SET avail = 0 WHERE avail <= 1")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Get IDs of unavailable books (for cover cleanup before physical deletion).
pub async fn get_unavailable_ids(pool: &DbPool) -> Result<Vec<i64>, sqlx::Error> {
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT id FROM books WHERE avail <= 1")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Physically delete unavailable books from the database.
pub async fn physical_delete_unavailable(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM books WHERE avail <= 1")
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

/// Random available book (for footer).
pub async fn get_random(pool: &DbPool) -> Result<Option<Book>, sqlx::Error> {
    sqlx::query_as::<_, Book>("SELECT * FROM books WHERE avail > 0 ORDER BY ABS(RANDOM()) LIMIT 1")
        .fetch_optional(pool)
        .await
}

/// Count books matching a title search (contains).
pub async fn count_by_title_search(
    pool: &DbPool,
    term: &str,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let pattern = format!("%{term}%");
    let sql = if hide_doubles {
        "SELECT COUNT(DISTINCT search_title) FROM books WHERE search_title LIKE ? AND avail > 0"
    } else {
        "SELECT COUNT(*) FROM books WHERE search_title LIKE ? AND avail > 0"
    };
    let row: (i64,) = sqlx::query_as(sql).bind(&pattern).fetch_one(pool).await?;
    Ok(row.0)
}

/// Count books matching a title-starts-with search.
pub async fn count_by_title_prefix(
    pool: &DbPool,
    prefix: &str,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let pattern = format!("{prefix}%");
    let sql = if hide_doubles {
        "SELECT COUNT(DISTINCT search_title) FROM books WHERE search_title LIKE ? AND avail > 0"
    } else {
        "SELECT COUNT(*) FROM books WHERE search_title LIKE ? AND avail > 0"
    };
    let row: (i64,) = sqlx::query_as(sql).bind(&pattern).fetch_one(pool).await?;
    Ok(row.0)
}

/// Count books by author.
pub async fn count_by_author(
    pool: &DbPool,
    author_id: i64,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let sql = if hide_doubles {
        "SELECT COUNT(DISTINCT b.search_title) FROM books b \
         JOIN book_authors ba ON ba.book_id = b.id \
         WHERE ba.author_id = ? AND b.avail > 0"
    } else {
        "SELECT COUNT(*) FROM books b \
         JOIN book_authors ba ON ba.book_id = b.id \
         WHERE ba.author_id = ? AND b.avail > 0"
    };
    let row: (i64,) = sqlx::query_as(sql).bind(author_id).fetch_one(pool).await?;
    Ok(row.0)
}

/// Count books by genre.
pub async fn count_by_genre(
    pool: &DbPool,
    genre_id: i64,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let sql = if hide_doubles {
        "SELECT COUNT(DISTINCT b.search_title) FROM books b \
         JOIN book_genres bg ON bg.book_id = b.id \
         WHERE bg.genre_id = ? AND b.avail > 0"
    } else {
        "SELECT COUNT(*) FROM books b \
         JOIN book_genres bg ON bg.book_id = b.id \
         WHERE bg.genre_id = ? AND b.avail > 0"
    };
    let row: (i64,) = sqlx::query_as(sql).bind(genre_id).fetch_one(pool).await?;
    Ok(row.0)
}

/// Count books by series.
pub async fn count_by_series(
    pool: &DbPool,
    series_id: i64,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let sql = if hide_doubles {
        "SELECT COUNT(DISTINCT b.search_title) FROM books b \
         JOIN book_series bs ON bs.book_id = b.id \
         WHERE bs.series_id = ? AND b.avail > 0"
    } else {
        "SELECT COUNT(*) FROM books b \
         JOIN book_series bs ON bs.book_id = b.id \
         WHERE bs.series_id = ? AND b.avail > 0"
    };
    let row: (i64,) = sqlx::query_as(sql).bind(series_id).fetch_one(pool).await?;
    Ok(row.0)
}

/// Count books in a catalog.
pub async fn count_by_catalog(
    pool: &DbPool,
    catalog_id: i64,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let sql = if hide_doubles {
        "SELECT COUNT(DISTINCT search_title) FROM books WHERE catalog_id = ? AND avail > 0"
    } else {
        "SELECT COUNT(*) FROM books WHERE catalog_id = ? AND avail > 0"
    };
    let row: (i64,) = sqlx::query_as(sql).bind(catalog_id).fetch_one(pool).await?;
    Ok(row.0)
}

/// Count how many available books share the same search_title as the given book.
pub async fn count_doubles(pool: &DbPool, book_id: i64) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM books \
         WHERE search_title = (SELECT search_title FROM books WHERE id = ?) AND avail > 0",
    )
    .bind(book_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Alphabet drill-down: get prefix groups for book titles.
/// Returns `(prefix_string, count)` pairs.
/// `current_prefix` is the prefix already selected (empty for first level).
/// `lang_code` = 0 means all languages.
pub async fn get_title_prefix_groups(
    pool: &DbPool,
    lang_code: i32,
    current_prefix: &str,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let prefix_len = (current_prefix.chars().count() + 1) as i32;
    let like_pattern = format!("{}%", current_prefix);
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT SUBSTR(search_title, 1, ?) as prefix, COUNT(*) as cnt \
         FROM books \
         WHERE avail > 0 AND (? = 0 OR lang_code = ?) AND search_title LIKE ? \
         GROUP BY SUBSTR(search_title, 1, ?) \
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

pub async fn update_title(
    pool: &DbPool,
    book_id: i64,
    title: &str,
    search_title: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE books SET title = ?, search_title = ? WHERE id = ?")
        .bind(title)
        .bind(search_title)
        .bind(book_id)
        .execute(pool)
        .await?;
    Ok(())
}
