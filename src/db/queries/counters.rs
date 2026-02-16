use crate::db::DbPool;

use crate::db::models::Counter;

pub async fn get(pool: &DbPool, name: &str) -> Result<Option<Counter>, sqlx::Error> {
    sqlx::query_as::<_, Counter>("SELECT * FROM counters WHERE name = ?")
        .bind(name)
        .fetch_optional(pool)
        .await
}

pub async fn get_all(pool: &DbPool) -> Result<Vec<Counter>, sqlx::Error> {
    sqlx::query_as::<_, Counter>("SELECT * FROM counters ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn set(pool: &DbPool, name: &str, value: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE counters SET value = ?, updated_at = CURRENT_TIMESTAMP WHERE name = ?",
    )
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
    let genres: (i64,) =
        sqlx::query_as("SELECT COUNT(DISTINCT genre_id) FROM book_genres")
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
