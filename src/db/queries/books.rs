use crate::db::{DbBackend, DbPool};

use crate::db::models::{AvailStatus, Book, CatType};

pub async fn get_by_id(pool: &DbPool, id: i64) -> Result<Option<Book>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM books WHERE id = ?");
    sqlx::query_as::<_, Book>(&sql)
        .bind(id)
        .fetch_optional(pool.inner())
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
        let sql = pool.sql(
            "SELECT * FROM books WHERE catalog_id = ? AND avail > 0 \
             AND id IN (SELECT MIN(id) FROM books WHERE catalog_id = ? AND avail > 0 GROUP BY search_title, author_key) \
             ORDER BY search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(catalog_id)
            .bind(catalog_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await
    } else {
        let sql = pool.sql(
            "SELECT * FROM books WHERE catalog_id = ? AND avail > 0 ORDER BY search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(catalog_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
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
        let sql = pool.sql(
            "SELECT b.* FROM books b \
             JOIN book_authors ba ON ba.book_id = b.id \
             WHERE ba.author_id = ? AND b.avail > 0 \
             AND b.id IN (SELECT MIN(b2.id) FROM books b2 \
               JOIN book_authors ba2 ON ba2.book_id = b2.id \
               WHERE ba2.author_id = ? AND b2.avail > 0 GROUP BY b2.search_title, b2.author_key) \
             ORDER BY b.search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(author_id)
            .bind(author_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await
    } else {
        let sql = pool.sql(
            "SELECT b.* FROM books b \
             JOIN book_authors ba ON ba.book_id = b.id \
             WHERE ba.author_id = ? AND b.avail > 0 \
             ORDER BY b.search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(author_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
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
        let sql = pool.sql(
            "SELECT b.* FROM books b \
             JOIN book_genres bg ON bg.book_id = b.id \
             WHERE bg.genre_id = ? AND b.avail > 0 \
             AND b.id IN (SELECT MIN(b2.id) FROM books b2 \
               JOIN book_genres bg2 ON bg2.book_id = b2.id \
               WHERE bg2.genre_id = ? AND b2.avail > 0 GROUP BY b2.search_title, b2.author_key) \
             ORDER BY b.search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(genre_id)
            .bind(genre_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await
    } else {
        let sql = pool.sql(
            "SELECT b.* FROM books b \
             JOIN book_genres bg ON bg.book_id = b.id \
             WHERE bg.genre_id = ? AND b.avail > 0 \
             ORDER BY b.search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(genre_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
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
        let sql = pool.sql(
            "SELECT b.* FROM books b \
             JOIN book_series bs ON bs.book_id = b.id \
             WHERE bs.series_id = ? AND b.avail > 0 \
             AND b.id IN (SELECT MIN(b2.id) FROM books b2 \
               JOIN book_series bs2 ON bs2.book_id = b2.id \
               WHERE bs2.series_id = ? AND b2.avail > 0 GROUP BY b2.search_title, b2.author_key) \
             ORDER BY bs.ser_no, b.search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(series_id)
            .bind(series_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await
    } else {
        let sql = pool.sql(
            "SELECT b.* FROM books b \
             JOIN book_series bs ON bs.book_id = b.id \
             WHERE bs.series_id = ? AND b.avail > 0 \
             ORDER BY bs.ser_no, b.search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(series_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
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
        let sql = pool.sql(
            "SELECT * FROM books WHERE search_title LIKE ? AND avail > 0 \
             AND id IN (SELECT MIN(id) FROM books WHERE search_title LIKE ? AND avail > 0 GROUP BY search_title, author_key) \
             ORDER BY search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await
    } else {
        let sql = pool.sql(
            "SELECT * FROM books WHERE search_title LIKE ? AND avail > 0 \
             ORDER BY search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
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
        let sql = pool.sql(
            "SELECT * FROM books WHERE search_title LIKE ? AND avail > 0 \
             AND id IN (SELECT MIN(id) FROM books WHERE search_title LIKE ? AND avail > 0 GROUP BY search_title, author_key) \
             ORDER BY search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await
    } else {
        let sql = pool.sql(
            "SELECT * FROM books WHERE search_title LIKE ? AND avail > 0 \
             ORDER BY search_title LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await
    }
}

pub async fn find_by_path_and_filename(
    pool: &DbPool,
    path: &str,
    filename: &str,
) -> Result<Option<Book>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM books WHERE path = ? AND filename = ?");
    sqlx::query_as::<_, Book>(&sql)
        .bind(path)
        .bind(filename)
        .fetch_optional(pool.inner())
        .await
}

#[allow(clippy::too_many_arguments)]
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
    cat_type: CatType,
    cover: i32,
    cover_type: &str,
) -> Result<i64, sqlx::Error> {
    let sql = pool.sql(
        "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
         annotation, docdate, lang, lang_code, size, avail, cat_type, cover, cover_type) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 2, ?, ?, ?)",
    );
    let result = sqlx::query(&sql)
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
        .bind(cat_type as i32)
        .bind(cover)
        .bind(cover_type)
        .execute(pool.inner())
        .await?;
    if let Some(id) = result.last_insert_id() {
        return Ok(id);
    }
    let sql = pool.sql("SELECT id FROM books WHERE path = ? AND filename = ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(path)
        .bind(filename)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

pub async fn set_avail_all(pool: &DbPool, avail: AvailStatus) -> Result<u64, sqlx::Error> {
    let sql = pool.sql("UPDATE books SET avail = ?");
    let result = sqlx::query(&sql)
        .bind(avail as i32)
        .execute(pool.inner())
        .await?;
    Ok(result.rows_affected())
}

pub async fn set_avail(pool: &DbPool, id: i64, avail: AvailStatus) -> Result<(), sqlx::Error> {
    let sql = pool.sql("UPDATE books SET avail = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind(avail as i32)
        .bind(id)
        .execute(pool.inner())
        .await?;
    Ok(())
}

pub async fn set_avail_by_path(
    pool: &DbPool,
    path: &str,
    avail: AvailStatus,
) -> Result<u64, sqlx::Error> {
    let sql = pool.sql("UPDATE books SET avail = ? WHERE path = ?");
    let result = sqlx::query(&sql)
        .bind(avail as i32)
        .bind(path)
        .execute(pool.inner())
        .await?;
    Ok(result.rows_affected())
}

pub async fn set_avail_for_inpx_dir(
    pool: &DbPool,
    inpx_dir: &str,
    avail: AvailStatus,
) -> Result<u64, sqlx::Error> {
    let result = if inpx_dir.is_empty() {
        let sql = pool.sql("UPDATE books SET avail = ? WHERE cat_type = ?");
        sqlx::query(&sql)
            .bind(avail as i32)
            .bind(CatType::Inpx as i32)
            .execute(pool.inner())
            .await?
    } else {
        let pattern = format!("{inpx_dir}/%");
        let sql = pool.sql("UPDATE books SET avail = ? WHERE cat_type = ? AND path LIKE ?");
        sqlx::query(&sql)
            .bind(avail as i32)
            .bind(CatType::Inpx as i32)
            .bind(pattern)
            .execute(pool.inner())
            .await?
    };

    Ok(result.rows_affected())
}

/// Mark unverified books as logically deleted (avail=0, hidden from queries).
pub async fn logical_delete_unavailable(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let sql = pool.sql("UPDATE books SET avail = ? WHERE avail <= ?");
    let result = sqlx::query(&sql)
        .bind(AvailStatus::Deleted as i32)
        .bind(AvailStatus::Unverified as i32)
        .execute(pool.inner())
        .await?;
    Ok(result.rows_affected())
}

/// Get IDs of unavailable books (for cover cleanup before physical deletion).
pub async fn get_unavailable_ids(pool: &DbPool) -> Result<Vec<i64>, sqlx::Error> {
    let sql = pool.sql("SELECT id FROM books WHERE avail <= ?");
    let rows: Vec<(i64,)> = sqlx::query_as(&sql)
        .bind(AvailStatus::Unverified as i32)
        .fetch_all(pool.inner())
        .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Physically delete unavailable books from the database.
pub async fn physical_delete_unavailable(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let sql = pool.sql("DELETE FROM books WHERE avail <= ?");
    let result = sqlx::query(&sql)
        .bind(AvailStatus::Unverified as i32)
        .execute(pool.inner())
        .await?;
    Ok(result.rows_affected())
}

/// Random available book (for footer).
pub async fn get_random(pool: &DbPool) -> Result<Option<Book>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM books WHERE avail > 0 ORDER BY ABS(RANDOM()) LIMIT 1");
    sqlx::query_as::<_, Book>(&sql)
        .fetch_optional(pool.inner())
        .await
}

/// Recently added books, newest first.
pub async fn get_recent_added(
    pool: &DbPool,
    limit: i32,
    offset: i32,
    hide_doubles: bool,
) -> Result<Vec<Book>, sqlx::Error> {
    if hide_doubles {
        let sql = pool.sql(
            "SELECT * FROM books WHERE avail > 0 \
             AND id IN (SELECT MAX(id) FROM books WHERE avail > 0 GROUP BY search_title, author_key) \
             ORDER BY reg_date DESC, id DESC LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await
    } else {
        let sql = pool.sql(
            "SELECT * FROM books WHERE avail > 0 ORDER BY reg_date DESC, id DESC LIMIT ? OFFSET ?",
        );
        sqlx::query_as::<_, Book>(&sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await
    }
}

/// Count available books in the recently added view.
pub async fn count_recent_added(pool: &DbPool, hide_doubles: bool) -> Result<i64, sqlx::Error> {
    let sql = if hide_doubles {
        "SELECT COUNT(*) FROM (SELECT 1 FROM books WHERE avail > 0 \
         GROUP BY search_title, author_key) AS t"
    } else {
        "SELECT COUNT(*) FROM books WHERE avail > 0"
    };
    let sql = pool.sql(sql);
    let row: (i64,) = sqlx::query_as(&sql).fetch_one(pool.inner()).await?;
    Ok(row.0)
}

/// Count books matching a title search (contains).
pub async fn count_by_title_search(
    pool: &DbPool,
    term: &str,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let pattern = format!("%{term}%");
    let sql = if hide_doubles {
        "SELECT COUNT(*) FROM (SELECT 1 FROM books \
         WHERE search_title LIKE ? AND avail > 0 \
         GROUP BY search_title, author_key) AS t"
    } else {
        "SELECT COUNT(*) FROM books WHERE search_title LIKE ? AND avail > 0"
    };
    let sql = pool.sql(sql);
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(&pattern)
        .fetch_one(pool.inner())
        .await?;
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
        "SELECT COUNT(*) FROM (SELECT 1 FROM books \
         WHERE search_title LIKE ? AND avail > 0 \
         GROUP BY search_title, author_key) AS t"
    } else {
        "SELECT COUNT(*) FROM books WHERE search_title LIKE ? AND avail > 0"
    };
    let sql = pool.sql(sql);
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(&pattern)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

/// Count books by author.
pub async fn count_by_author(
    pool: &DbPool,
    author_id: i64,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let sql = if hide_doubles {
        "SELECT COUNT(*) FROM (SELECT 1 FROM books b \
         JOIN book_authors ba ON ba.book_id = b.id \
         WHERE ba.author_id = ? AND b.avail > 0 \
         GROUP BY b.search_title, b.author_key) AS t"
    } else {
        "SELECT COUNT(*) FROM books b \
         JOIN book_authors ba ON ba.book_id = b.id \
         WHERE ba.author_id = ? AND b.avail > 0"
    };
    let sql = pool.sql(sql);
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(author_id)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

/// Count books by genre.
pub async fn count_by_genre(
    pool: &DbPool,
    genre_id: i64,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let sql = if hide_doubles {
        "SELECT COUNT(*) FROM (SELECT 1 FROM books b \
         JOIN book_genres bg ON bg.book_id = b.id \
         WHERE bg.genre_id = ? AND b.avail > 0 \
         GROUP BY b.search_title, b.author_key) AS t"
    } else {
        "SELECT COUNT(*) FROM books b \
         JOIN book_genres bg ON bg.book_id = b.id \
         WHERE bg.genre_id = ? AND b.avail > 0"
    };
    let sql = pool.sql(sql);
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(genre_id)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

/// Count books by series.
pub async fn count_by_series(
    pool: &DbPool,
    series_id: i64,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let sql = if hide_doubles {
        "SELECT COUNT(*) FROM (SELECT 1 FROM books b \
         JOIN book_series bs ON bs.book_id = b.id \
         WHERE bs.series_id = ? AND b.avail > 0 \
         GROUP BY b.search_title, b.author_key) AS t"
    } else {
        "SELECT COUNT(*) FROM books b \
         JOIN book_series bs ON bs.book_id = b.id \
         WHERE bs.series_id = ? AND b.avail > 0"
    };
    let sql = pool.sql(sql);
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(series_id)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

/// Count books in a catalog.
pub async fn count_by_catalog(
    pool: &DbPool,
    catalog_id: i64,
    hide_doubles: bool,
) -> Result<i64, sqlx::Error> {
    let sql = if hide_doubles {
        "SELECT COUNT(*) FROM (SELECT 1 FROM books \
         WHERE catalog_id = ? AND avail > 0 \
         GROUP BY search_title, author_key) AS t"
    } else {
        "SELECT COUNT(*) FROM books WHERE catalog_id = ? AND avail > 0"
    };
    let sql = pool.sql(sql);
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(catalog_id)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

/// Count how many available books share the same search_title and author_key as the given book.
pub async fn count_doubles(pool: &DbPool, book_id: i64) -> Result<i64, sqlx::Error> {
    let sql = pool.sql(
        "SELECT COUNT(*) FROM books \
         WHERE search_title = (SELECT search_title FROM books WHERE id = ?) \
         AND author_key = (SELECT author_key FROM books WHERE id = ?) \
         AND avail > 0",
    );
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(book_id)
        .bind(book_id)
        .fetch_one(pool.inner())
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
    let sql = pool.sql(
        "SELECT SUBSTR(search_title, 1, ?) as prefix, COUNT(*) as cnt \
         FROM books \
         WHERE avail > 0 AND (? = 0 OR lang_code = ?) AND search_title LIKE ? \
         GROUP BY SUBSTR(search_title, 1, ?) \
         ORDER BY prefix",
    );
    let rows: Vec<(String, i64)> = sqlx::query_as(&sql)
        .bind(prefix_len)
        .bind(lang_code)
        .bind(lang_code)
        .bind(&like_pattern)
        .bind(prefix_len)
        .fetch_all(pool.inner())
        .await?;
    Ok(rows)
}

pub async fn update_title(
    pool: &DbPool,
    book_id: i64,
    title: &str,
    search_title: &str,
    lang_code: i32,
) -> Result<(), sqlx::Error> {
    let sql = pool.sql("UPDATE books SET title = ?, search_title = ?, lang_code = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind(title)
        .bind(search_title)
        .bind(lang_code)
        .bind(book_id)
        .execute(pool.inner())
        .await?;
    Ok(())
}

// ── Duplicate detection queries ──────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DuplicateGroup {
    pub search_title: String,
    pub author_key: String,
    pub cnt: i64,
}

/// Get groups of books that share the same search_title and author_key.
/// Returns groups ordered by count descending, then by search_title.
pub async fn get_duplicate_groups(
    pool: &DbPool,
    limit: i32,
    offset: i32,
) -> Result<Vec<DuplicateGroup>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT search_title, author_key, COUNT(*) as cnt \
         FROM books WHERE avail > 0 \
         GROUP BY search_title, author_key \
         HAVING COUNT(*) > 1 \
         ORDER BY cnt DESC, search_title \
         LIMIT ? OFFSET ?",
    );
    sqlx::query_as::<_, DuplicateGroup>(&sql)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool.inner())
        .await
}

/// Count the number of duplicate groups (groups with more than one book
/// sharing the same search_title and author_key).
pub async fn count_duplicate_groups(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let sql = pool.sql(
        "SELECT COUNT(*) FROM (\
         SELECT 1 FROM books WHERE avail > 0 \
         GROUP BY search_title, author_key \
         HAVING COUNT(*) > 1\
         ) AS t",
    );
    let row: (i64,) = sqlx::query_as(&sql).fetch_one(pool.inner()).await?;
    Ok(row.0)
}

/// Get all available books in a duplicate group identified by search_title and author_key.
pub async fn get_books_in_group(
    pool: &DbPool,
    search_title: &str,
    author_key: &str,
) -> Result<Vec<Book>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT * FROM books \
         WHERE search_title = ? AND author_key = ? AND avail > 0 \
         ORDER BY id",
    );
    sqlx::query_as::<_, Book>(&sql)
        .bind(search_title)
        .bind(author_key)
        .fetch_all(pool.inner())
        .await
}

/// Recompute and store `author_key` for a book.
///
/// The key is the sorted, comma-separated list of author IDs linked to the
/// book (e.g. `"3,17,42"`).  It is used together with `search_title` to
/// detect duplicate editions.
pub async fn update_author_key(pool: &DbPool, book_id: i64) -> Result<(), sqlx::Error> {
    let authors = crate::db::queries::authors::get_for_book(pool, book_id).await?;
    let mut ids: Vec<i64> = authors.iter().map(|a| a.id).collect();
    ids.sort_unstable();
    let key: String = ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = pool.sql("UPDATE books SET author_key = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind(&key)
        .bind(book_id)
        .execute(pool.inner())
        .await?;
    Ok(())
}

/// Atomically replace all authors for a book and recompute `author_key`.
///
/// Runs `set_book_authors` + `update_author_key` in a single transaction so
/// that a failure in either step rolls back both, avoiding inconsistent state.
pub async fn set_book_authors_and_update_key(
    pool: &DbPool,
    book_id: i64,
    author_ids: &[i64],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.inner().begin().await?;

    // ── set_book_authors logic ──────────────────────────────────────
    let sql = pool.sql("SELECT author_id FROM book_authors WHERE book_id = ?");
    let old_ids: Vec<(i64,)> = sqlx::query_as(&sql)
        .bind(book_id)
        .fetch_all(&mut *tx)
        .await?;

    let sql = pool.sql("DELETE FROM book_authors WHERE book_id = ?");
    sqlx::query(&sql).bind(book_id).execute(&mut *tx).await?;

    let link_sql = match pool.backend() {
        DbBackend::Mysql => "INSERT IGNORE INTO book_authors (book_id, author_id) VALUES (?, ?)",
        _ => {
            "INSERT INTO book_authors (book_id, author_id) VALUES (?, ?) \
             ON CONFLICT (book_id, author_id) DO NOTHING"
        }
    };
    let link_sql = pool.sql(link_sql);
    for &author_id in author_ids {
        sqlx::query(&link_sql)
            .bind(book_id)
            .bind(author_id)
            .execute(&mut *tx)
            .await?;
    }

    // ── update_author_key logic ─────────────────────────────────────
    let author_sql = pool.sql(
        "SELECT a.id FROM authors a \
         JOIN book_authors ba ON ba.author_id = a.id \
         WHERE ba.book_id = ? ORDER BY a.id",
    );
    let rows: Vec<(i64,)> = sqlx::query_as(&author_sql)
        .bind(book_id)
        .fetch_all(&mut *tx)
        .await?;
    let key: String = rows
        .iter()
        .map(|(id,)| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let update_sql = pool.sql("UPDATE books SET author_key = ? WHERE id = ?");
    sqlx::query(&update_sql)
        .bind(&key)
        .bind(book_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    // ── orphan cleanup (outside transaction — non-critical) ─────────
    for (old_id,) in old_ids {
        if !author_ids.contains(&old_id) {
            if let Err(e) = crate::db::queries::authors::delete_if_orphaned(pool, old_id).await {
                tracing::warn!(author_id = old_id, error = %e, "orphan author cleanup failed");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    /// Create a root catalog and return its id. Call once per test pool.
    async fn ensure_catalog(pool: &DbPool) -> i64 {
        let sql = pool.sql("INSERT INTO catalogs (path, cat_name) VALUES (?, ?)");
        sqlx::query(&sql)
            .bind("/test")
            .bind("test")
            .execute(pool.inner())
            .await
            .unwrap();
        let sql = pool.sql("SELECT id FROM catalogs WHERE path = ?");
        let row: (i64,) = sqlx::query_as(&sql)
            .bind("/test")
            .fetch_one(pool.inner())
            .await
            .unwrap();
        row.0
    }

    async fn insert_test_book(pool: &DbPool, catalog_id: i64, title: &str, lang_code: i32) -> i64 {
        let search_title = title.to_uppercase();
        insert(
            pool,
            catalog_id,
            &format!("{title}.fb2"),
            "/test",
            "fb2",
            title,
            &search_title,
            "",   // annotation
            "",   // docdate
            "ru", // lang
            lang_code,
            1000, // size
            CatType::Normal,
            0,  // cover
            "", // cover_type
        )
        .await
        .unwrap()
    }

    async fn insert_test_book_custom(
        pool: &DbPool,
        catalog_id: i64,
        filename: &str,
        path: &str,
        title: &str,
        search_title: &str,
        cat_type: CatType,
    ) -> i64 {
        insert(
            pool,
            catalog_id,
            filename,
            path,
            "fb2",
            title,
            search_title,
            "",
            "",
            "ru",
            2,
            1000,
            cat_type,
            0,
            "",
        )
        .await
        .unwrap()
    }

    async fn insert_test_author(pool: &DbPool, full_name: &str) -> i64 {
        let search_name = full_name.to_uppercase();
        let sql = pool
            .sql("INSERT INTO authors (full_name, search_full_name, lang_code) VALUES (?, ?, ?)");
        sqlx::query(&sql)
            .bind(full_name)
            .bind(search_name)
            .bind(2)
            .execute(pool.inner())
            .await
            .unwrap();
        let sql = pool.sql("SELECT id FROM authors WHERE full_name = ?");
        let row: (i64,) = sqlx::query_as(&sql)
            .bind(full_name)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        row.0
    }

    async fn insert_test_series(pool: &DbPool, ser_name: &str) -> i64 {
        let search_name = ser_name.to_uppercase();
        let sql = pool.sql("INSERT INTO series (ser_name, search_ser, lang_code) VALUES (?, ?, ?)");
        sqlx::query(&sql)
            .bind(ser_name)
            .bind(search_name)
            .bind(2)
            .execute(pool.inner())
            .await
            .unwrap();
        let sql = pool.sql("SELECT id FROM series WHERE ser_name = ?");
        let row: (i64,) = sqlx::query_as(&sql)
            .bind(ser_name)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        row.0
    }

    async fn insert_test_genre(pool: &DbPool, code: &str) -> i64 {
        let sql = pool.sql("INSERT INTO genres (code, section, subsection) VALUES (?, ?, ?)");
        sqlx::query(&sql)
            .bind(code)
            .bind("Test section")
            .bind("Test subsection")
            .execute(pool.inner())
            .await
            .unwrap();
        let sql = pool.sql("SELECT id FROM genres WHERE code = ?");
        let row: (i64,) = sqlx::query_as(&sql)
            .bind(code)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        row.0
    }

    #[tokio::test]
    async fn test_title_prefix_groups_empty() {
        let pool = create_test_pool().await;
        let groups = get_title_prefix_groups(&pool, 0, "").await.unwrap();
        assert!(groups.is_empty());
    }

    #[tokio::test]
    async fn test_title_prefix_groups_basic() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        insert_test_book(&pool, cat, "Alpha", 2).await;
        insert_test_book(&pool, cat, "Beta", 2).await;
        insert_test_book(&pool, cat, "Charlie", 2).await;

        let groups = get_title_prefix_groups(&pool, 0, "").await.unwrap();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0], ("A".to_string(), 1));
        assert_eq!(groups[1], ("B".to_string(), 1));
        assert_eq!(groups[2], ("C".to_string(), 1));
    }

    #[tokio::test]
    async fn test_title_prefix_groups_lang_filter() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        insert_test_book(&pool, cat, "Альфа", 1).await; // Cyrillic
        insert_test_book(&pool, cat, "Бета", 1).await; // Cyrillic
        insert_test_book(&pool, cat, "Alpha", 2).await; // Latin

        // lang_code=1 (Cyrillic) — only 2 groups
        let groups = get_title_prefix_groups(&pool, 1, "").await.unwrap();
        assert_eq!(groups.len(), 2);

        // lang_code=2 (Latin) — only 1 group
        let groups = get_title_prefix_groups(&pool, 2, "").await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "A");

        // lang_code=0 (all) — all 3 groups
        let groups = get_title_prefix_groups(&pool, 0, "").await.unwrap();
        assert_eq!(groups.len(), 3);
    }

    #[tokio::test]
    async fn test_title_prefix_groups_drill_down() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        insert_test_book(&pool, cat, "Aa book", 2).await;
        insert_test_book(&pool, cat, "Ab book", 2).await;
        insert_test_book(&pool, cat, "Ac book", 2).await;
        insert_test_book(&pool, cat, "Ba book", 2).await;

        // Top level: "A" with count=3, "B" with count=1
        let groups = get_title_prefix_groups(&pool, 0, "").await.unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], ("A".to_string(), 3));
        assert_eq!(groups[1], ("B".to_string(), 1));

        // Drill into "A": 3 sub-groups
        let groups = get_title_prefix_groups(&pool, 0, "A").await.unwrap();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, "AA");
        assert_eq!(groups[1].0, "AB");
        assert_eq!(groups[2].0, "AC");
    }

    #[tokio::test]
    async fn test_title_prefix_groups_deep_drill_down() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        insert_test_book(&pool, cat, "Abc one", 2).await;
        insert_test_book(&pool, cat, "Abd two", 2).await;
        insert_test_book(&pool, cat, "Abe three", 2).await;

        // Level 1: all under "A"
        let groups = get_title_prefix_groups(&pool, 0, "").await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], ("A".to_string(), 3));

        // Level 2: all under "AB"
        let groups = get_title_prefix_groups(&pool, 0, "A").await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], ("AB".to_string(), 3));

        // Level 3: three distinct 3-char prefixes
        let groups = get_title_prefix_groups(&pool, 0, "AB").await.unwrap();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, "ABC");
        assert_eq!(groups[1].0, "ABD");
        assert_eq!(groups[2].0, "ABE");
    }

    #[tokio::test]
    async fn test_title_prefix_groups_count_aggregation() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        // 3 books starting with "A", 2 with "B", 1 with "C"
        insert_test_book(&pool, cat, "Alpha", 2).await;
        insert_test_book(&pool, cat, "Another", 2).await;
        insert_test_book(&pool, cat, "Again", 2).await;
        insert_test_book(&pool, cat, "Beta", 2).await;
        insert_test_book(&pool, cat, "Bravo", 2).await;
        insert_test_book(&pool, cat, "Charlie", 2).await;

        let groups = get_title_prefix_groups(&pool, 0, "").await.unwrap();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0], ("A".to_string(), 3));
        assert_eq!(groups[1], ("B".to_string(), 2));
        assert_eq!(groups[2], ("C".to_string(), 1));
    }

    #[tokio::test]
    async fn test_title_prefix_groups_excludes_unavailable() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let book_id = insert_test_book(&pool, cat, "Alpha", 2).await;
        insert_test_book(&pool, cat, "Beta", 2).await;

        // Mark one book as unavailable
        let sql = pool.sql("UPDATE books SET avail = 0 WHERE id = ?");
        sqlx::query(&sql)
            .bind(book_id)
            .execute(pool.inner())
            .await
            .unwrap();

        let groups = get_title_prefix_groups(&pool, 0, "").await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "B");
    }

    #[tokio::test]
    async fn test_search_by_title_prefix() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        insert_test_book(&pool, cat, "Alpha", 2).await;
        insert_test_book(&pool, cat, "Another", 2).await;
        insert_test_book(&pool, cat, "Beta", 2).await;

        // Prefix "A" matches "Alpha" and "Another"
        let results = search_by_title_prefix(&pool, "A", 100, 0, false)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        // Prefix "AL" matches only "Alpha"
        let results = search_by_title_prefix(&pool, "AL", 100, 0, false)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Alpha");

        // Prefix "B" matches only "Beta"
        let results = search_by_title_prefix(&pool, "B", 100, 0, false)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Beta");

        // Prefix "Z" matches nothing
        let results = search_by_title_prefix(&pool, "Z", 100, 0, false)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_search_by_title_prefix_pagination() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        insert_test_book(&pool, cat, "Aa", 2).await;
        insert_test_book(&pool, cat, "Ab", 2).await;
        insert_test_book(&pool, cat, "Ac", 2).await;
        insert_test_book(&pool, cat, "Ad", 2).await;

        // Page 1: limit 2, offset 0
        let page1 = search_by_title_prefix(&pool, "A", 2, 0, false)
            .await
            .unwrap();
        assert_eq!(page1.len(), 2);

        // Page 2: limit 2, offset 2
        let page2 = search_by_title_prefix(&pool, "A", 2, 2, false)
            .await
            .unwrap();
        assert_eq!(page2.len(), 2);

        // No overlap between pages
        assert_ne!(page1[0].id, page2[0].id);
    }

    #[tokio::test]
    async fn test_title_prefix_groups_lang_filter_with_drill_down() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        // Two Cyrillic books with different second chars
        insert_test_book(&pool, cat, "Альфа", 1).await;
        insert_test_book(&pool, cat, "Абвгд", 1).await;
        // One Latin book starting with "A"
        insert_test_book(&pool, cat, "Alpha", 2).await;

        // Drill into Cyrillic "А" — should see 2 sub-prefixes
        let groups = get_title_prefix_groups(&pool, 1, "А").await.unwrap();
        assert_eq!(groups.len(), 2);

        // Drill into Latin "A" — should see 1 sub-prefix
        let groups = get_title_prefix_groups(&pool, 2, "A").await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "AL");
    }

    #[tokio::test]
    async fn test_get_by_catalog_and_find_by_path_with_doubles() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let alpha_a = insert_test_book_custom(
            &pool,
            cat,
            "alpha-a.fb2",
            "/test/a",
            "Alpha A",
            "ALPHA",
            CatType::Normal,
        )
        .await;
        insert_test_book_custom(
            &pool,
            cat,
            "alpha-b.fb2",
            "/test/b",
            "Alpha B",
            "ALPHA",
            CatType::Normal,
        )
        .await;
        let beta = insert_test_book_custom(
            &pool,
            cat,
            "beta.fb2",
            "/test/c",
            "Beta",
            "BETA",
            CatType::Normal,
        )
        .await;

        // Availability filter should exclude this row from listing queries.
        set_avail(&pool, beta, AvailStatus::Deleted).await.unwrap();

        let all_rows = get_by_catalog(&pool, cat, 100, 0, false).await.unwrap();
        assert_eq!(all_rows.len(), 2);

        let deduped_rows = get_by_catalog(&pool, cat, 100, 0, true).await.unwrap();
        assert_eq!(deduped_rows.len(), 1);
        assert_eq!(deduped_rows[0].search_title, "ALPHA");

        let found = find_by_path_and_filename(&pool, "/test/a", "alpha-a.fb2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, alpha_a);
        assert_eq!(found.title, "Alpha A");
    }

    #[tokio::test]
    async fn test_get_by_author_genre_series_and_counts() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let author = insert_test_author(&pool, "Test Author").await;
        let genre = insert_test_genre(&pool, "books_q_tests").await;
        let series = insert_test_series(&pool, "Test Saga").await;

        let b1 = insert_test_book_custom(
            &pool,
            cat,
            "book-1.fb2",
            "/test/series",
            "First",
            "DUPLICATE",
            CatType::Normal,
        )
        .await;
        let b2 = insert_test_book_custom(
            &pool,
            cat,
            "book-2.fb2",
            "/test/series",
            "Second",
            "DUPLICATE",
            CatType::Normal,
        )
        .await;

        for book_id in [b1, b2] {
            let sql = pool.sql("INSERT INTO book_authors (book_id, author_id) VALUES (?, ?)");
            sqlx::query(&sql)
                .bind(book_id)
                .bind(author)
                .execute(pool.inner())
                .await
                .unwrap();

            let sql = pool.sql("INSERT INTO book_genres (book_id, genre_id) VALUES (?, ?)");
            sqlx::query(&sql)
                .bind(book_id)
                .bind(genre)
                .execute(pool.inner())
                .await
                .unwrap();
        }

        let sql = pool.sql("INSERT INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?)");
        sqlx::query(&sql)
            .bind(b1)
            .bind(series)
            .bind(1)
            .execute(pool.inner())
            .await
            .unwrap();
        let sql = pool.sql("INSERT INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?)");
        sqlx::query(&sql)
            .bind(b2)
            .bind(series)
            .bind(2)
            .execute(pool.inner())
            .await
            .unwrap();

        assert_eq!(
            get_by_author(&pool, author, 100, 0, false)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            get_by_author(&pool, author, 100, 0, true)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            get_by_genre(&pool, genre, 100, 0, false)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            get_by_genre(&pool, genre, 100, 0, true)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            get_by_series(&pool, series, 100, 0, false)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            get_by_series(&pool, series, 100, 0, true)
                .await
                .unwrap()
                .len(),
            1
        );

        assert_eq!(count_by_author(&pool, author, false).await.unwrap(), 2);
        assert_eq!(count_by_author(&pool, author, true).await.unwrap(), 1);
        assert_eq!(count_by_genre(&pool, genre, false).await.unwrap(), 2);
        assert_eq!(count_by_genre(&pool, genre, true).await.unwrap(), 1);
        assert_eq!(count_by_series(&pool, series, false).await.unwrap(), 2);
        assert_eq!(count_by_series(&pool, series, true).await.unwrap(), 1);
        assert_eq!(count_by_catalog(&pool, cat, false).await.unwrap(), 2);
        assert_eq!(count_by_catalog(&pool, cat, true).await.unwrap(), 1);
        assert_eq!(count_doubles(&pool, b1).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_search_title_and_update_title() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let b1 = insert_test_book_custom(
            &pool,
            cat,
            "first.fb2",
            "/test/search",
            "The First",
            "FOO BAR",
            CatType::Normal,
        )
        .await;
        insert_test_book_custom(
            &pool,
            cat,
            "second.fb2",
            "/test/search",
            "The Second",
            "FOO BAR",
            CatType::Normal,
        )
        .await;

        assert_eq!(
            search_by_title(&pool, "FOO", 100, 0, false)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            search_by_title(&pool, "FOO", 100, 0, true)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(count_by_title_search(&pool, "FOO", false).await.unwrap(), 2);
        assert_eq!(count_by_title_search(&pool, "FOO", true).await.unwrap(), 1);
        assert_eq!(count_by_title_prefix(&pool, "FO", false).await.unwrap(), 2);
        assert_eq!(count_by_title_prefix(&pool, "FO", true).await.unwrap(), 1);

        update_title(&pool, b1, "Updated", "UPDATED", 3)
            .await
            .unwrap();
        let row = get_by_id(&pool, b1).await.unwrap().unwrap();
        assert_eq!(row.title, "Updated");
        assert_eq!(row.search_title, "UPDATED");
        assert_eq!(row.lang_code, 3);
    }

    #[tokio::test]
    async fn test_availability_helpers_and_cleanup_flow() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;

        let normal = insert_test_book_custom(
            &pool,
            cat,
            "normal.fb2",
            "/library/normal",
            "Normal",
            "NORMAL",
            CatType::Normal,
        )
        .await;
        let inpx_a = insert_test_book_custom(
            &pool,
            cat,
            "inpx-a.fb2",
            "/inpx/main/a",
            "Inpx A",
            "INPX_A",
            CatType::Inpx,
        )
        .await;
        let inpx_b = insert_test_book_custom(
            &pool,
            cat,
            "inpx-b.fb2",
            "/inpx/other/b",
            "Inpx B",
            "INPX_B",
            CatType::Inpx,
        )
        .await;

        let updated = set_avail_by_path(&pool, "/inpx/main/a", AvailStatus::Unverified)
            .await
            .unwrap();
        assert_eq!(updated, 1);

        let updated = set_avail_for_inpx_dir(&pool, "/inpx/main", AvailStatus::Unverified)
            .await
            .unwrap();
        assert_eq!(updated, 1);
        assert_eq!(
            get_by_id(&pool, inpx_a).await.unwrap().unwrap().avail,
            AvailStatus::Unverified as i32
        );
        assert_eq!(
            get_by_id(&pool, inpx_b).await.unwrap().unwrap().avail,
            AvailStatus::Confirmed as i32
        );

        let updated = set_avail_for_inpx_dir(&pool, "", AvailStatus::Unverified)
            .await
            .unwrap();
        assert_eq!(updated, 2);

        set_avail(&pool, normal, AvailStatus::Deleted)
            .await
            .unwrap();
        let marked_deleted = logical_delete_unavailable(&pool).await.unwrap();
        assert_eq!(marked_deleted, 3);

        let mut unavailable_ids = get_unavailable_ids(&pool).await.unwrap();
        unavailable_ids.sort_unstable();
        let mut expected = vec![normal, inpx_a, inpx_b];
        expected.sort_unstable();
        assert_eq!(unavailable_ids, expected);

        let physically_deleted = physical_delete_unavailable(&pool).await.unwrap();
        assert_eq!(physically_deleted, 3);
        assert!(get_by_id(&pool, normal).await.unwrap().is_none());
        assert!(get_by_id(&pool, inpx_a).await.unwrap().is_none());
        assert!(get_by_id(&pool, inpx_b).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_set_avail_all_and_get_random() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let first = insert_test_book(&pool, cat, "Random One", 2).await;
        insert_test_book(&pool, cat, "Random Two", 2).await;

        let random = get_random(&pool).await.unwrap().unwrap();
        assert!(random.id == first || random.id > 0);

        let updated = set_avail_all(&pool, AvailStatus::Deleted).await.unwrap();
        assert_eq!(updated, 2);
        assert!(get_random(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_recent_added_queries_order_and_count() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let old_id = insert_test_book_custom(
            &pool,
            cat,
            "old.fb2",
            "/test/recent",
            "Old Title",
            "OLD TITLE",
            CatType::Normal,
        )
        .await;
        let new_id = insert_test_book_custom(
            &pool,
            cat,
            "new.fb2",
            "/test/recent",
            "New Title",
            "NEW TITLE",
            CatType::Normal,
        )
        .await;
        let dup_old = insert_test_book_custom(
            &pool,
            cat,
            "dup-old.fb2",
            "/test/recent",
            "Dup Old",
            "DUPLICATE",
            CatType::Normal,
        )
        .await;
        let dup_new = insert_test_book_custom(
            &pool,
            cat,
            "dup-new.fb2",
            "/test/recent",
            "Dup New",
            "DUPLICATE",
            CatType::Normal,
        )
        .await;

        // Make ordering deterministic for assertions.
        let sql = pool.sql("UPDATE books SET reg_date = ? WHERE id = ?");
        sqlx::query(&sql)
            .bind("2024-01-01 10:00:00")
            .bind(old_id)
            .execute(pool.inner())
            .await
            .unwrap();
        sqlx::query(&sql)
            .bind("2024-02-01 10:00:00")
            .bind(new_id)
            .execute(pool.inner())
            .await
            .unwrap();
        sqlx::query(&sql)
            .bind("2024-01-15 10:00:00")
            .bind(dup_old)
            .execute(pool.inner())
            .await
            .unwrap();
        sqlx::query(&sql)
            .bind("2024-03-01 10:00:00")
            .bind(dup_new)
            .execute(pool.inner())
            .await
            .unwrap();

        let all_recent = get_recent_added(&pool, 10, 0, false).await.unwrap();
        assert_eq!(all_recent.len(), 4);
        assert_eq!(all_recent[0].id, dup_new);
        assert_eq!(all_recent[1].id, new_id);

        let deduped_recent = get_recent_added(&pool, 10, 0, true).await.unwrap();
        assert_eq!(deduped_recent.len(), 3);
        assert_eq!(deduped_recent[0].id, dup_new);

        assert_eq!(count_recent_added(&pool, false).await.unwrap(), 4);
        assert_eq!(count_recent_added(&pool, true).await.unwrap(), 3);
    }

    // ── author_key & duplicate detection tests ──────────────────────────

    async fn link_author(pool: &DbPool, book_id: i64, author_id: i64) {
        let sql = pool.sql("INSERT INTO book_authors (book_id, author_id) VALUES (?, ?)");
        sqlx::query(&sql)
            .bind(book_id)
            .bind(author_id)
            .execute(pool.inner())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_update_author_key_format() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let book = insert_test_book(&pool, cat, "KeyTest", 2).await;
        let a1 = insert_test_author(&pool, "Zoe").await;
        let a2 = insert_test_author(&pool, "Alice").await;

        link_author(&pool, book, a1).await;
        link_author(&pool, book, a2).await;

        update_author_key(&pool, book).await.unwrap();
        let row = get_by_id(&pool, book).await.unwrap().unwrap();

        // IDs must be sorted numerically and comma-separated
        let mut expected_ids = vec![a1, a2];
        expected_ids.sort_unstable();
        let expected = expected_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(row.author_key, expected);
    }

    #[tokio::test]
    async fn test_update_author_key_no_authors() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let book = insert_test_book(&pool, cat, "NoAuth", 2).await;

        update_author_key(&pool, book).await.unwrap();
        let row = get_by_id(&pool, book).await.unwrap().unwrap();
        assert_eq!(row.author_key, "");
    }

    #[tokio::test]
    async fn test_hide_doubles_considers_author_key() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let author_a = insert_test_author(&pool, "Author A").await;
        let author_b = insert_test_author(&pool, "Author B").await;

        // Two books with the same search_title but different authors
        let b1 = insert_test_book_custom(
            &pool,
            cat,
            "dup-a.fb2",
            "/test/dup",
            "Same Title",
            "SAME TITLE",
            CatType::Normal,
        )
        .await;
        let b2 = insert_test_book_custom(
            &pool,
            cat,
            "dup-b.fb2",
            "/test/dup",
            "Same Title",
            "SAME TITLE",
            CatType::Normal,
        )
        .await;

        link_author(&pool, b1, author_a).await;
        link_author(&pool, b2, author_b).await;
        update_author_key(&pool, b1).await.unwrap();
        update_author_key(&pool, b2).await.unwrap();

        // Without hide_doubles: both visible
        let all = get_by_catalog(&pool, cat, 100, 0, false).await.unwrap();
        assert_eq!(all.len(), 2);

        // With hide_doubles: still both visible (different author_key)
        let deduped = get_by_catalog(&pool, cat, 100, 0, true).await.unwrap();
        assert_eq!(deduped.len(), 2);
    }

    #[tokio::test]
    async fn test_hide_doubles_same_author_deduplicates() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let author = insert_test_author(&pool, "Shared Author").await;

        // Two books with the same search_title AND same author
        let b1 = insert_test_book_custom(
            &pool,
            cat,
            "dup-1.fb2",
            "/test/dup",
            "Dup Title",
            "DUP TITLE",
            CatType::Normal,
        )
        .await;
        let b2 = insert_test_book_custom(
            &pool,
            cat,
            "dup-2.fb2",
            "/test/dup",
            "Dup Title 2",
            "DUP TITLE",
            CatType::Normal,
        )
        .await;

        link_author(&pool, b1, author).await;
        link_author(&pool, b2, author).await;
        update_author_key(&pool, b1).await.unwrap();
        update_author_key(&pool, b2).await.unwrap();

        // Without hide_doubles: both visible
        let all = get_by_catalog(&pool, cat, 100, 0, false).await.unwrap();
        assert_eq!(all.len(), 2);

        // With hide_doubles: deduplicated to one (same search_title + author_key)
        let deduped = get_by_catalog(&pool, cat, 100, 0, true).await.unwrap();
        assert_eq!(deduped.len(), 1);
    }

    #[tokio::test]
    async fn test_count_doubles_considers_author_key() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let author_a = insert_test_author(&pool, "Writer A").await;
        let author_b = insert_test_author(&pool, "Writer B").await;

        // b1 and b2: same title, same author → duplicates
        let b1 = insert_test_book_custom(
            &pool,
            cat,
            "d1.fb2",
            "/test/cnt",
            "Title",
            "TITLE",
            CatType::Normal,
        )
        .await;
        let b2 = insert_test_book_custom(
            &pool,
            cat,
            "d2.fb2",
            "/test/cnt",
            "Title2",
            "TITLE",
            CatType::Normal,
        )
        .await;
        // b3: same title, different author → NOT a duplicate of b1/b2
        let b3 = insert_test_book_custom(
            &pool,
            cat,
            "d3.fb2",
            "/test/cnt",
            "Title3",
            "TITLE",
            CatType::Normal,
        )
        .await;

        for b in [b1, b2] {
            link_author(&pool, b, author_a).await;
        }
        link_author(&pool, b3, author_b).await;

        for b in [b1, b2, b3] {
            update_author_key(&pool, b).await.unwrap();
        }

        // b1 should see 2 doubles (b1 and b2 share title+author)
        assert_eq!(count_doubles(&pool, b1).await.unwrap(), 2);
        // b3 should see only 1 (itself — different author)
        assert_eq!(count_doubles(&pool, b3).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_duplicate_groups_detection() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let author = insert_test_author(&pool, "Group Author").await;

        // Group 1: 3 books, same title + author
        for i in 0..3 {
            let b = insert_test_book_custom(
                &pool,
                cat,
                &format!("grp1-{i}.fb2"),
                "/test/grp",
                &format!("Group One v{i}"),
                "GROUP ONE",
                CatType::Normal,
            )
            .await;
            link_author(&pool, b, author).await;
            update_author_key(&pool, b).await.unwrap();
        }

        // Group 2: 2 books, same title + author
        for i in 0..2 {
            let b = insert_test_book_custom(
                &pool,
                cat,
                &format!("grp2-{i}.fb2"),
                "/test/grp",
                &format!("Group Two v{i}"),
                "GROUP TWO",
                CatType::Normal,
            )
            .await;
            link_author(&pool, b, author).await;
            update_author_key(&pool, b).await.unwrap();
        }

        // Singleton (not a duplicate group)
        let solo = insert_test_book_custom(
            &pool,
            cat,
            "solo.fb2",
            "/test/grp",
            "Solo",
            "SOLO",
            CatType::Normal,
        )
        .await;
        link_author(&pool, solo, author).await;
        update_author_key(&pool, solo).await.unwrap();

        let count = count_duplicate_groups(&pool).await.unwrap();
        assert_eq!(count, 2);

        let groups = get_duplicate_groups(&pool, 100, 0).await.unwrap();
        assert_eq!(groups.len(), 2);
        // Ordered by count DESC
        assert_eq!(groups[0].cnt, 3);
        assert_eq!(groups[0].search_title, "GROUP ONE");
        assert_eq!(groups[1].cnt, 2);
        assert_eq!(groups[1].search_title, "GROUP TWO");

        // get_books_in_group returns the individual books
        let books_in_g1 = get_books_in_group(&pool, &groups[0].search_title, &groups[0].author_key)
            .await
            .unwrap();
        assert_eq!(books_in_g1.len(), 3);

        let books_in_g2 = get_books_in_group(&pool, &groups[1].search_title, &groups[1].author_key)
            .await
            .unwrap();
        assert_eq!(books_in_g2.len(), 2);
    }

    #[tokio::test]
    async fn test_hide_doubles_count_queries_with_author_key() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;
        let author_a = insert_test_author(&pool, "Count Author A").await;
        let author_b = insert_test_author(&pool, "Count Author B").await;
        let genre = insert_test_genre(&pool, "count_test_genre").await;
        let series = insert_test_series(&pool, "Count Saga").await;

        // Two books: same search_title, different authors
        let b1 = insert_test_book_custom(
            &pool,
            cat,
            "cnt-1.fb2",
            "/test/count",
            "Count Title",
            "COUNT TITLE",
            CatType::Normal,
        )
        .await;
        let b2 = insert_test_book_custom(
            &pool,
            cat,
            "cnt-2.fb2",
            "/test/count",
            "Count Title 2",
            "COUNT TITLE",
            CatType::Normal,
        )
        .await;

        link_author(&pool, b1, author_a).await;
        link_author(&pool, b2, author_b).await;
        update_author_key(&pool, b1).await.unwrap();
        update_author_key(&pool, b2).await.unwrap();

        // Link both to genre and series
        for b in [b1, b2] {
            let sql = pool.sql("INSERT INTO book_genres (book_id, genre_id) VALUES (?, ?)");
            sqlx::query(&sql)
                .bind(b)
                .bind(genre)
                .execute(pool.inner())
                .await
                .unwrap();

            let sql =
                pool.sql("INSERT INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?)");
            sqlx::query(&sql)
                .bind(b)
                .bind(series)
                .bind(1)
                .execute(pool.inner())
                .await
                .unwrap();
        }

        // Different author_key → hide_doubles should keep both
        assert_eq!(count_by_catalog(&pool, cat, true).await.unwrap(), 2);
        assert_eq!(
            count_by_title_search(&pool, "COUNT", true).await.unwrap(),
            2
        );
        assert_eq!(count_by_title_prefix(&pool, "CO", true).await.unwrap(), 2);
        assert_eq!(count_by_genre(&pool, genre, true).await.unwrap(), 2);
        assert_eq!(count_by_series(&pool, series, true).await.unwrap(), 2);
        assert_eq!(count_recent_added(&pool, true).await.unwrap(), 2);
    }
}
