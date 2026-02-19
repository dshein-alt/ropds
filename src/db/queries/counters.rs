use crate::db::DbPool;

use crate::db::models::Counter;

pub async fn get_all(pool: &DbPool) -> Result<Vec<Counter>, sqlx::Error> {
    sqlx::query_as::<_, Counter>("SELECT * FROM counters ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn set(pool: &DbPool, name: &str, value: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE counters SET value = ?, updated_at = CURRENT_TIMESTAMP WHERE name = ?")
        .bind(value)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

/// Recalculate all counters from actual table counts.
pub async fn update_all(pool: &DbPool) -> Result<(), sqlx::Error> {
    let books: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM books WHERE avail > 0")
        .fetch_one(pool)
        .await?;
    let catalogs: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM catalogs")
        .fetch_one(pool)
        .await?;
    let authors: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM authors")
        .fetch_one(pool)
        .await?;
    let genres: (i64,) = sqlx::query_as("SELECT COUNT(DISTINCT genre_id) FROM book_genres")
        .fetch_one(pool)
        .await?;
    let series: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM series")
        .fetch_one(pool)
        .await?;

    set(pool, "allbooks", books.0).await?;
    set(pool, "allcatalogs", catalogs.0).await?;
    set(pool, "allauthors", authors.0).await?;
    set(pool, "allgenres", genres.0).await?;
    set(pool, "allseries", series.0).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    async fn get_counter_value(pool: &DbPool, name: &str) -> i64 {
        let row: (i64,) = sqlx::query_as("SELECT value FROM counters WHERE name = ?")
            .bind(name)
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    async fn ensure_catalog(pool: &DbPool) -> i64 {
        sqlx::query("INSERT INTO catalogs (path, cat_name) VALUES ('/counters', 'counters')")
            .execute(pool)
            .await
            .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM catalogs WHERE path = '/counters'")
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    async fn insert_test_book(pool: &DbPool, catalog_id: i64, title: &str, avail: i32) -> i64 {
        let search_title = title.to_uppercase();
        sqlx::query(
            "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
             lang, lang_code, size, avail, cat_type, cover, cover_type) \
             VALUES (?, ?, '/counters', 'fb2', ?, ?, 'en', 2, 100, ?, 0, 0, '')",
        )
        .bind(catalog_id)
        .bind(format!("{title}.fb2"))
        .bind(title)
        .bind(search_title)
        .bind(avail)
        .execute(pool)
        .await
        .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM books WHERE catalog_id = ? AND title = ?")
            .bind(catalog_id)
            .bind(title)
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    #[tokio::test]
    async fn test_get_all_and_set() {
        let pool = create_test_pool().await;

        let all = get_all(&pool).await.unwrap();
        assert!(all.len() >= 5);
        assert!(all.iter().any(|c| c.name == "allbooks"));

        set(&pool, "allbooks", 123).await.unwrap();
        assert_eq!(get_counter_value(&pool, "allbooks").await, 123);
    }

    #[tokio::test]
    async fn test_update_all_recalculates_values() {
        let pool = create_test_pool().await;

        let catalog_id = ensure_catalog(&pool).await;
        let book1 = insert_test_book(&pool, catalog_id, "Live", 2).await;
        let book2 = insert_test_book(&pool, catalog_id, "Deleted", 0).await;

        sqlx::query(
            "INSERT INTO authors (full_name, search_full_name, lang_code) VALUES \
             ('Author One', 'AUTHOR ONE', 2), ('Author Two', 'AUTHOR TWO', 2)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO series (ser_name, search_ser, lang_code) VALUES ('Series A', 'SERIES A', 2)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO genres (code, section, subsection) VALUES \
             ('g1', 'sec', 'sub1'), ('g2', 'sec', 'sub2')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let g1: (i64,) = sqlx::query_as("SELECT id FROM genres WHERE code = 'g1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let g2: (i64,) = sqlx::query_as("SELECT id FROM genres WHERE code = 'g2'")
            .fetch_one(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO book_genres (book_id, genre_id) VALUES (?, ?), (?, ?)")
            .bind(book1)
            .bind(g1.0)
            .bind(book2)
            .bind(g2.0)
            .execute(&pool)
            .await
            .unwrap();

        update_all(&pool).await.unwrap();

        assert_eq!(get_counter_value(&pool, "allbooks").await, 1);
        assert_eq!(get_counter_value(&pool, "allcatalogs").await, 1);
        assert_eq!(get_counter_value(&pool, "allauthors").await, 2);
        assert_eq!(get_counter_value(&pool, "allgenres").await, 2);
        assert_eq!(get_counter_value(&pool, "allseries").await, 1);
    }
}
