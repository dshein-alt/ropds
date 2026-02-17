use crate::db::DbPool;

use crate::db::models::Catalog;

pub async fn get_by_id(pool: &DbPool, id: i64) -> Result<Option<Catalog>, sqlx::Error> {
    sqlx::query_as::<_, Catalog>("SELECT * FROM catalogs WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn get_children(pool: &DbPool, parent_id: i64) -> Result<Vec<Catalog>, sqlx::Error> {
    sqlx::query_as::<_, Catalog>("SELECT * FROM catalogs WHERE parent_id = ? ORDER BY cat_name")
        .bind(parent_id)
        .fetch_all(pool)
        .await
}

pub async fn get_root_catalogs(pool: &DbPool) -> Result<Vec<Catalog>, sqlx::Error> {
    sqlx::query_as::<_, Catalog>(
        "SELECT * FROM catalogs WHERE parent_id IS NULL ORDER BY cat_name",
    )
    .fetch_all(pool)
    .await
}

pub async fn find_by_path(pool: &DbPool, path: &str) -> Result<Option<Catalog>, sqlx::Error> {
    sqlx::query_as::<_, Catalog>("SELECT * FROM catalogs WHERE path = ?")
        .bind(path)
        .fetch_optional(pool)
        .await
}

pub async fn insert(
    pool: &DbPool,
    parent_id: Option<i64>,
    path: &str,
    cat_name: &str,
    cat_type: i32,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO catalogs (parent_id, path, cat_name, cat_type) VALUES (?, ?, ?, ?)",
    )
    .bind(parent_id)
    .bind(path)
    .bind(cat_name)
    .bind(cat_type)
    .execute(pool)
    .await?;
    if let Some(id) = result.last_insert_id() {
        return Ok(id);
    }
    // Fallback: query back by path
    let row: (i64,) = sqlx::query_as("SELECT id FROM catalogs WHERE path = ?")
        .bind(path)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn count(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM catalogs")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}
