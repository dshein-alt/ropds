use crate::db::DbPool;

use crate::db::models::{AvailStatus, Book, CatType};

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
    cat_type: CatType,
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
    .bind(cat_type as i32)
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

pub async fn set_avail_all(pool: &DbPool, avail: AvailStatus) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE books SET avail = ?")
        .bind(avail as i32)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn set_avail(pool: &DbPool, id: i64, avail: AvailStatus) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE books SET avail = ? WHERE id = ?")
        .bind(avail as i32)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_avail_by_path(
    pool: &DbPool,
    path: &str,
    avail: AvailStatus,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE books SET avail = ? WHERE path = ?")
        .bind(avail as i32)
        .bind(path)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn set_avail_for_inpx_dir(
    pool: &DbPool,
    inpx_dir: &str,
    avail: AvailStatus,
) -> Result<u64, sqlx::Error> {
    let result = if inpx_dir.is_empty() {
        sqlx::query("UPDATE books SET avail = ? WHERE cat_type = ?")
            .bind(avail as i32)
            .bind(CatType::Inpx as i32)
            .execute(pool)
            .await?
    } else {
        let pattern = format!("{inpx_dir}/%");
        sqlx::query("UPDATE books SET avail = ? WHERE cat_type = ? AND path LIKE ?")
            .bind(avail as i32)
            .bind(CatType::Inpx as i32)
            .bind(pattern)
            .execute(pool)
            .await?
    };

    Ok(result.rows_affected())
}

/// Mark unverified books as logically deleted (avail=0, hidden from queries).
pub async fn logical_delete_unavailable(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE books SET avail = ? WHERE avail <= ?")
        .bind(AvailStatus::Deleted as i32)
        .bind(AvailStatus::Unverified as i32)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Get IDs of unavailable books (for cover cleanup before physical deletion).
pub async fn get_unavailable_ids(pool: &DbPool) -> Result<Vec<i64>, sqlx::Error> {
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT id FROM books WHERE avail <= ?")
        .bind(AvailStatus::Unverified as i32)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Physically delete unavailable books from the database.
pub async fn physical_delete_unavailable(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM books WHERE avail <= ?")
        .bind(AvailStatus::Unverified as i32)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
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
    lang_code: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE books SET title = ?, search_title = ?, lang_code = ? WHERE id = ?")
        .bind(title)
        .bind(search_title)
        .bind(lang_code)
        .bind(book_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    /// Create a root catalog and return its id. Call once per test pool.
    async fn ensure_catalog(pool: &DbPool) -> i64 {
        sqlx::query("INSERT INTO catalogs (path, cat_name) VALUES ('/test', 'test')")
            .execute(pool)
            .await
            .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM catalogs WHERE path = '/test'")
            .fetch_one(pool)
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
        sqlx::query(
            "INSERT INTO authors (full_name, search_full_name, lang_code) VALUES (?, ?, ?)",
        )
        .bind(full_name)
        .bind(search_name)
        .bind(2)
        .execute(pool)
        .await
        .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM authors WHERE full_name = ?")
            .bind(full_name)
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    async fn insert_test_series(pool: &DbPool, ser_name: &str) -> i64 {
        let search_name = ser_name.to_uppercase();
        sqlx::query("INSERT INTO series (ser_name, search_ser, lang_code) VALUES (?, ?, ?)")
            .bind(ser_name)
            .bind(search_name)
            .bind(2)
            .execute(pool)
            .await
            .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM series WHERE ser_name = ?")
            .bind(ser_name)
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    async fn insert_test_genre(pool: &DbPool, code: &str) -> i64 {
        sqlx::query("INSERT INTO genres (code, section, subsection) VALUES (?, ?, ?)")
            .bind(code)
            .bind("Test section")
            .bind("Test subsection")
            .execute(pool)
            .await
            .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM genres WHERE code = ?")
            .bind(code)
            .fetch_one(pool)
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
        sqlx::query("UPDATE books SET avail = 0 WHERE id = ?")
            .bind(book_id)
            .execute(&pool)
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
            sqlx::query("INSERT INTO book_authors (book_id, author_id) VALUES (?, ?)")
                .bind(book_id)
                .bind(author)
                .execute(&pool)
                .await
                .unwrap();

            sqlx::query("INSERT INTO book_genres (book_id, genre_id) VALUES (?, ?)")
                .bind(book_id)
                .bind(genre)
                .execute(&pool)
                .await
                .unwrap();
        }

        sqlx::query("INSERT INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?)")
            .bind(b1)
            .bind(series)
            .bind(1)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?)")
            .bind(b2)
            .bind(series)
            .bind(2)
            .execute(&pool)
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
}
