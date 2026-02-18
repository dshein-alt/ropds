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
    sqlx::query_as::<_, Catalog>("SELECT * FROM catalogs WHERE parent_id IS NULL ORDER BY cat_name")
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
    cat_size: i64,
    cat_mtime: &str,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO catalogs (parent_id, path, cat_name, cat_type, cat_size, cat_mtime) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(parent_id)
    .bind(path)
    .bind(cat_name)
    .bind(cat_type)
    .bind(cat_size)
    .bind(cat_mtime)
    .execute(pool)
    .await?;
    if let Some(id) = result.last_insert_id() {
        if id > 0 {
            return Ok(id);
        }
    }
    // Fallback: query back by path (INSERT OR IGNORE returns 0 on conflict)
    let row: (i64,) = sqlx::query_as("SELECT id FROM catalogs WHERE path = ?")
        .bind(path)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn update_archive_meta(
    pool: &DbPool,
    id: i64,
    cat_type: i32,
    cat_size: i64,
    cat_mtime: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE catalogs SET cat_type = ?, cat_size = ?, cat_mtime = ? WHERE id = ?")
        .bind(cat_type)
        .bind(cat_size)
        .bind(cat_mtime)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete catalogs that have no live books and no child catalogs.
/// Repeats until no more empty catalogs are found (prunes leaf-up).
pub async fn delete_empty(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let mut total = 0u64;
    loop {
        let result = sqlx::query(
            "DELETE FROM catalogs WHERE id NOT IN \
             (SELECT DISTINCT catalog_id FROM books WHERE avail > 0) \
             AND id NOT IN \
             (SELECT DISTINCT parent_id FROM catalogs WHERE parent_id IS NOT NULL)",
        )
        .execute(pool)
        .await?;
        let deleted = result.rows_affected();
        if deleted == 0 {
            break;
        }
        total += deleted;
    }
    Ok(total)
}
