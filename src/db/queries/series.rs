use crate::db::{DbBackend, DbPool};

use crate::db::models::Series;

pub async fn get_by_id(pool: &DbPool, id: i64) -> Result<Option<Series>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM series WHERE id = ?");
    sqlx::query_as::<_, Series>(&sql)
        .bind(id)
        .fetch_optional(pool.inner())
        .await
}

pub async fn search_by_name(
    pool: &DbPool,
    term: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Series>, sqlx::Error> {
    let pattern = format!("%{term}%");
    let sql = pool.sql(
        "SELECT * FROM series WHERE search_ser LIKE ? \
         ORDER BY search_ser LIMIT ? OFFSET ?",
    );
    sqlx::query_as::<_, Series>(&sql)
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool.inner())
        .await
}

pub async fn get_by_lang_code_prefix(
    pool: &DbPool,
    lang_code: i32,
    prefix: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Series>, sqlx::Error> {
    let pattern = format!("{prefix}%");
    let sql = pool.sql(
        "SELECT * FROM series WHERE (? = 0 OR lang_code = ?) AND search_ser LIKE ? \
         ORDER BY search_ser LIMIT ? OFFSET ?",
    );
    sqlx::query_as::<_, Series>(&sql)
        .bind(lang_code)
        .bind(lang_code)
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool.inner())
        .await
}

pub async fn find_by_name(pool: &DbPool, ser_name: &str) -> Result<Option<Series>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM series WHERE ser_name = ?");
    sqlx::query_as::<_, Series>(&sql)
        .bind(ser_name)
        .fetch_optional(pool.inner())
        .await
}

pub async fn insert(
    pool: &DbPool,
    ser_name: &str,
    search_ser: &str,
    lang_code: i32,
) -> Result<i64, sqlx::Error> {
    let sql = match pool.backend() {
        DbBackend::Mysql => {
            "INSERT IGNORE INTO series (ser_name, search_ser, lang_code) VALUES (?, ?, ?)"
        }
        _ => {
            "INSERT INTO series (ser_name, search_ser, lang_code) VALUES (?, ?, ?) \
             ON CONFLICT (ser_name) DO NOTHING"
        }
    };
    let sql = pool.sql(sql);
    let result = sqlx::query(&sql)
        .bind(ser_name)
        .bind(search_ser)
        .bind(lang_code)
        .execute(pool.inner())
        .await?;
    if let Some(id) = result.last_insert_id()
        && id > 0
    {
        return Ok(id);
    }
    // Fallback: query back by name (INSERT OR IGNORE returns 0 on conflict)
    let sql = pool.sql("SELECT id FROM series WHERE ser_name = ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(ser_name)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

pub async fn link_book(
    pool: &DbPool,
    book_id: i64,
    series_id: i64,
    ser_no: i32,
) -> Result<(), sqlx::Error> {
    let sql = match pool.backend() {
        DbBackend::Mysql => {
            "INSERT IGNORE INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?)"
        }
        _ => {
            "INSERT INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?) \
             ON CONFLICT (book_id, series_id) DO NOTHING"
        }
    };
    let sql = pool.sql(sql);
    sqlx::query(&sql)
        .bind(book_id)
        .bind(series_id)
        .bind(ser_no)
        .execute(pool.inner())
        .await?;
    Ok(())
}

pub async fn get_for_book(pool: &DbPool, book_id: i64) -> Result<Vec<(Series, i32)>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT s.id, s.ser_name, s.search_ser, s.lang_code, bs.ser_no \
         FROM series s JOIN book_series bs ON bs.series_id = s.id \
         WHERE bs.book_id = ? ORDER BY s.ser_name",
    );
    let rows: Vec<(i64, String, String, i32, i32)> = sqlx::query_as(&sql)
        .bind(book_id)
        .fetch_all(pool.inner())
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, ser_name, search_ser, lang_code, ser_no)| {
            (
                Series {
                    id,
                    ser_name,
                    search_ser,
                    lang_code,
                },
                ser_no,
            )
        })
        .collect())
}

/// Count series matching a name search (contains).
pub async fn count_by_name_search(pool: &DbPool, term: &str) -> Result<i64, sqlx::Error> {
    let pattern = format!("%{term}%");
    let sql = pool.sql("SELECT COUNT(*) FROM series WHERE search_ser LIKE ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(&pattern)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

/// Alphabet drill-down: get prefix groups for series names.
pub async fn get_name_prefix_groups(
    pool: &DbPool,
    lang_code: i32,
    current_prefix: &str,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let prefix_len = (current_prefix.chars().count() + 1) as i32;
    let like_pattern = format!("{}%", current_prefix);
    let sql = pool.sql(
        "SELECT SUBSTR(search_ser, 1, ?) as prefix, COUNT(*) as cnt \
         FROM series \
         WHERE (? = 0 OR lang_code = ?) AND search_ser LIKE ? \
         GROUP BY SUBSTR(search_ser, 1, ?) \
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

/// Delete a series if it has no remaining book links.
pub async fn delete_if_orphaned(pool: &DbPool, series_id: i64) -> Result<(), sqlx::Error> {
    let sql = pool.sql("SELECT COUNT(*) FROM book_series WHERE series_id = ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(series_id)
        .fetch_one(pool.inner())
        .await?;
    if row.0 == 0 {
        let sql = pool.sql("DELETE FROM series WHERE id = ?");
        sqlx::query(&sql)
            .bind(series_id)
            .execute(pool.inner())
            .await?;
    }
    Ok(())
}

/// Replace the series for a book: delete existing link, optionally set a new one,
/// then remove any orphaned series (no remaining book links).
/// Empty `series_name` removes the series without assigning a new one.
pub async fn set_book_series(
    pool: &DbPool,
    book_id: i64,
    series_name: &str,
    ser_no: i32,
) -> Result<(), sqlx::Error> {
    // Remember old series IDs before unlinking
    let sql = pool.sql("SELECT series_id FROM book_series WHERE book_id = ?");
    let old_ids: Vec<(i64,)> = sqlx::query_as(&sql)
        .bind(book_id)
        .fetch_all(pool.inner())
        .await?;

    let sql = pool.sql("DELETE FROM book_series WHERE book_id = ?");
    sqlx::query(&sql)
        .bind(book_id)
        .execute(pool.inner())
        .await?;

    let new_series_id = if !series_name.is_empty() {
        let series_id = crate::scanner::ensure_series(pool, series_name)
            .await
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        link_book(pool, book_id, series_id, ser_no).await?;
        Some(series_id)
    } else {
        None
    };

    // Clean up orphaned series that no longer have any books
    for (old_id,) in old_ids {
        if new_series_id != Some(old_id) {
            delete_if_orphaned(pool, old_id).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    async fn ensure_catalog(pool: &DbPool) -> i64 {
        let sql = pool.sql("INSERT INTO catalogs (path, cat_name) VALUES ('/series', 'series')");
        sqlx::query(&sql).execute(pool.inner()).await.unwrap();
        let sql = pool.sql("SELECT id FROM catalogs WHERE path = '/series'");
        let row: (i64,) = sqlx::query_as(&sql).fetch_one(pool.inner()).await.unwrap();
        row.0
    }

    async fn insert_test_book(pool: &DbPool, catalog_id: i64, title: &str) -> i64 {
        let search_title = title.to_uppercase();
        let sql = pool.sql(
            "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
             lang, lang_code, size, avail, cat_type, cover, cover_type) \
             VALUES (?, ?, '/series', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
        );
        sqlx::query(&sql)
            .bind(catalog_id)
            .bind(format!("{title}.fb2"))
            .bind(title)
            .bind(search_title)
            .execute(pool.inner())
            .await
            .unwrap();
        let sql = pool.sql("SELECT id FROM books WHERE catalog_id = ? AND title = ?");
        let row: (i64,) = sqlx::query_as(&sql)
            .bind(catalog_id)
            .bind(title)
            .fetch_one(pool.inner())
            .await
            .unwrap();
        row.0
    }

    #[tokio::test]
    async fn test_insert_search_count_and_prefix_groups() {
        let pool = create_test_pool().await;

        let alpha = insert(&pool, "Alpha Saga", "ALPHA SAGA", 2).await.unwrap();
        let _alpine = insert(&pool, "Alpine Arc", "ALPINE ARC", 2).await.unwrap();
        let _cyr = insert(&pool, "Альфа", "АЛЬФА", 1).await.unwrap();

        let found = get_by_id(&pool, alpha).await.unwrap().unwrap();
        assert_eq!(found.ser_name, "Alpha Saga");

        let by_name = find_by_name(&pool, "Alpha Saga").await.unwrap().unwrap();
        assert_eq!(by_name.id, alpha);

        let search = search_by_name(&pool, "ALP", 100, 0).await.unwrap();
        assert_eq!(search.len(), 2);

        let count = count_by_name_search(&pool, "ALP").await.unwrap();
        assert_eq!(count, 2);

        let prefix = get_by_lang_code_prefix(&pool, 2, "AL", 100, 0)
            .await
            .unwrap();
        assert_eq!(prefix.len(), 2);
        let all_langs = get_by_lang_code_prefix(&pool, 0, "", 100, 0).await.unwrap();
        assert_eq!(all_langs.len(), 3);

        let groups = get_name_prefix_groups(&pool, 2, "A").await.unwrap();
        assert_eq!(groups, vec![("AL".to_string(), 2)]);
    }

    #[tokio::test]
    async fn test_insert_duplicate_returns_same_id() {
        let pool = create_test_pool().await;

        let id1 = insert(&pool, "Shared Series", "SHARED SERIES", 2)
            .await
            .unwrap();
        let id2 = insert(&pool, "Shared Series", "OTHER", 1).await.unwrap();
        assert_eq!(id1, id2);
    }

    #[tokio::test]
    async fn test_link_book_and_get_for_book() {
        let pool = create_test_pool().await;
        let catalog_id = ensure_catalog(&pool).await;
        let book_id = insert_test_book(&pool, catalog_id, "Linked Book").await;

        let z_id = insert(&pool, "Zeta", "ZETA", 2).await.unwrap();
        let a_id = insert(&pool, "Alpha", "ALPHA", 2).await.unwrap();
        link_book(&pool, book_id, z_id, 7).await.unwrap();
        link_book(&pool, book_id, a_id, 3).await.unwrap();

        let linked = get_for_book(&pool, book_id).await.unwrap();
        assert_eq!(linked.len(), 2);
        // Ordered by series name in SQL: Alpha, then Zeta.
        assert_eq!(linked[0].0.id, a_id);
        assert_eq!(linked[0].1, 3);
        assert_eq!(linked[1].0.id, z_id);
        assert_eq!(linked[1].1, 7);
    }

    #[tokio::test]
    async fn test_set_book_series_assign_update_remove() {
        let pool = create_test_pool().await;
        let catalog_id = ensure_catalog(&pool).await;
        let book_id = insert_test_book(&pool, catalog_id, "Series Test").await;

        // Assign a series
        set_book_series(&pool, book_id, "Foundation", 1)
            .await
            .unwrap();
        let linked = get_for_book(&pool, book_id).await.unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].0.ser_name, "Foundation");
        assert_eq!(linked[0].1, 1);

        // Update to a different series — old series should be orphan-cleaned
        set_book_series(&pool, book_id, "Dune", 3).await.unwrap();
        let linked = get_for_book(&pool, book_id).await.unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].0.ser_name, "Dune");
        assert_eq!(linked[0].1, 3);
        // Foundation should be deleted (orphaned)
        assert!(find_by_name(&pool, "Foundation").await.unwrap().is_none());

        // Remove series entirely
        set_book_series(&pool, book_id, "", 0).await.unwrap();
        let linked = get_for_book(&pool, book_id).await.unwrap();
        assert!(linked.is_empty());
        // Dune should also be cleaned up
        assert!(find_by_name(&pool, "Dune").await.unwrap().is_none());
    }
}
