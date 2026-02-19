use crate::db::DbPool;

use crate::db::models::{CatType, Catalog};

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
    cat_type: CatType,
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
    .bind(cat_type as i32)
    .bind(cat_size)
    .bind(cat_mtime)
    .execute(pool)
    .await?;
    if let Some(id) = result.last_insert_id()
        && id > 0
    {
        return Ok(id);
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
    cat_type: CatType,
    cat_size: i64,
    cat_mtime: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE catalogs SET cat_type = ?, cat_size = ?, cat_mtime = ? WHERE id = ?")
        .bind(cat_type as i32)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    async fn insert_test_book(pool: &DbPool, catalog_id: i64, title: &str, avail: i32) -> i64 {
        let search_title = title.to_uppercase();
        sqlx::query(
            "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
             lang, lang_code, size, avail, cat_type, cover, cover_type) \
             VALUES (?, ?, '/catalogs', 'fb2', ?, ?, 'en', 2, 100, ?, ?, 0, '')",
        )
        .bind(catalog_id)
        .bind(format!("{title}.fb2"))
        .bind(title)
        .bind(search_title)
        .bind(avail)
        .bind(CatType::Normal as i32)
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
    async fn test_insert_and_lookup_hierarchy() {
        let pool = create_test_pool().await;

        let root_id = insert(&pool, None, "/root", "root", CatType::Normal, 0, "")
            .await
            .unwrap();
        let child_id = insert(
            &pool,
            Some(root_id),
            "/root/child",
            "child",
            CatType::Normal,
            0,
            "",
        )
        .await
        .unwrap();

        let root = get_by_id(&pool, root_id).await.unwrap().unwrap();
        assert_eq!(root.path, "/root");

        let child = find_by_path(&pool, "/root/child").await.unwrap().unwrap();
        assert_eq!(child.id, child_id);
        assert_eq!(child.parent_id, Some(root_id));

        let roots = get_root_catalogs(&pool).await.unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, root_id);

        let children = get_children(&pool, root_id).await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child_id);
    }

    #[tokio::test]
    async fn test_insert_duplicate_returns_same_id() {
        let pool = create_test_pool().await;

        let id1 = insert(&pool, None, "/dup", "dup", CatType::Normal, 0, "")
            .await
            .unwrap();
        let id2 = insert(&pool, None, "/dup", "dup2", CatType::Zip, 42, "mtime")
            .await
            .unwrap();

        assert_eq!(id1, id2);
    }

    #[tokio::test]
    async fn test_update_archive_meta() {
        let pool = create_test_pool().await;

        let id = insert(
            &pool,
            None,
            "/archive.zip",
            "archive.zip",
            CatType::Zip,
            10,
            "old",
        )
        .await
        .unwrap();
        update_archive_meta(&pool, id, CatType::Inpx, 99, "2026-02-19 10:30:00")
            .await
            .unwrap();

        let cat = get_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(cat.cat_type, CatType::Inpx as i32);
        assert_eq!(cat.cat_size, 99);
        assert_eq!(cat.cat_mtime, "2026-02-19 10:30:00");
    }

    #[tokio::test]
    async fn test_delete_empty_prunes_tree_and_keeps_non_empty() {
        let pool = create_test_pool().await;

        let a = insert(&pool, None, "/a", "a", CatType::Normal, 0, "")
            .await
            .unwrap();
        let _b = insert(&pool, Some(a), "/a/b", "b", CatType::Normal, 0, "")
            .await
            .unwrap();

        let keep = insert(&pool, None, "/keep", "keep", CatType::Normal, 0, "")
            .await
            .unwrap();
        let _book_id = insert_test_book(&pool, keep, "Live Book", 2).await;

        let deleted = delete_empty(&pool).await.unwrap();
        assert_eq!(deleted, 2);

        assert!(find_by_path(&pool, "/a").await.unwrap().is_none());
        assert!(find_by_path(&pool, "/a/b").await.unwrap().is_none());
        assert!(find_by_path(&pool, "/keep").await.unwrap().is_some());
    }
}
