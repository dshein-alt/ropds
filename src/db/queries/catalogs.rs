use sqlx::SqlitePool;

use crate::db::models::Catalog;

pub async fn get_by_id(pool: &SqlitePool, id: i64) -> Result<Option<Catalog>, sqlx::Error> {
    sqlx::query_as::<_, Catalog>("SELECT * FROM catalogs WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn get_children(pool: &SqlitePool, parent_id: i64) -> Result<Vec<Catalog>, sqlx::Error> {
    sqlx::query_as::<_, Catalog>("SELECT * FROM catalogs WHERE parent_id = ? ORDER BY cat_name")
        .bind(parent_id)
        .fetch_all(pool)
        .await
}

pub async fn get_root_catalogs(pool: &SqlitePool) -> Result<Vec<Catalog>, sqlx::Error> {
    sqlx::query_as::<_, Catalog>(
        "SELECT * FROM catalogs WHERE parent_id IS NULL ORDER BY cat_name",
    )
    .fetch_all(pool)
    .await
}

pub async fn find_by_path(pool: &SqlitePool, path: &str) -> Result<Option<Catalog>, sqlx::Error> {
    sqlx::query_as::<_, Catalog>("SELECT * FROM catalogs WHERE path = ?")
        .bind(path)
        .fetch_optional(pool)
        .await
}

pub async fn insert(
    pool: &SqlitePool,
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
    Ok(result.last_insert_rowid())
}

pub async fn count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM catalogs")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}
