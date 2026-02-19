use crate::db::{DbBackend, DbPool};

use crate::db::models::Author;

pub async fn get_by_id(pool: &DbPool, id: i64) -> Result<Option<Author>, sqlx::Error> {
    sqlx::query_as::<_, Author>("SELECT * FROM authors WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn search_by_name(
    pool: &DbPool,
    term: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Author>, sqlx::Error> {
    let pattern = format!("%{term}%");
    sqlx::query_as::<_, Author>(
        "SELECT * FROM authors WHERE search_full_name LIKE ? \
         ORDER BY search_full_name LIMIT ? OFFSET ?",
    )
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn get_by_lang_code_prefix(
    pool: &DbPool,
    lang_code: i32,
    prefix: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Author>, sqlx::Error> {
    let pattern = format!("{prefix}%");
    sqlx::query_as::<_, Author>(
        "SELECT * FROM authors WHERE lang_code = ? AND search_full_name LIKE ? \
         ORDER BY search_full_name LIMIT ? OFFSET ?",
    )
    .bind(lang_code)
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn find_by_name(pool: &DbPool, full_name: &str) -> Result<Option<Author>, sqlx::Error> {
    sqlx::query_as::<_, Author>("SELECT * FROM authors WHERE full_name = ?")
        .bind(full_name)
        .fetch_optional(pool)
        .await
}

pub async fn insert(
    pool: &DbPool,
    full_name: &str,
    search_full_name: &str,
    lang_code: i32,
    backend: DbBackend,
) -> Result<i64, sqlx::Error> {
    let sql = match backend {
        DbBackend::Mysql => {
            "INSERT IGNORE INTO authors (full_name, search_full_name, lang_code) VALUES (?, ?, ?)"
        }
        _ => {
            "INSERT INTO authors (full_name, search_full_name, lang_code) VALUES (?, ?, ?) \
             ON CONFLICT (full_name) DO NOTHING"
        }
    };
    let result = sqlx::query(sql)
        .bind(full_name)
        .bind(search_full_name)
        .bind(lang_code)
        .execute(pool)
        .await?;
    if let Some(id) = result.last_insert_id()
        && id > 0
    {
        return Ok(id);
    }
    // Fallback: query back by name (INSERT OR IGNORE returns 0 on conflict)
    let row: (i64,) = sqlx::query_as("SELECT id FROM authors WHERE full_name = ?")
        .bind(full_name)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn link_book(
    pool: &DbPool,
    book_id: i64,
    author_id: i64,
    backend: DbBackend,
) -> Result<(), sqlx::Error> {
    let sql = match backend {
        DbBackend::Mysql => "INSERT IGNORE INTO book_authors (book_id, author_id) VALUES (?, ?)",
        _ => {
            "INSERT INTO book_authors (book_id, author_id) VALUES (?, ?) \
             ON CONFLICT (book_id, author_id) DO NOTHING"
        }
    };
    sqlx::query(sql)
        .bind(book_id)
        .bind(author_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Replace all authors for a book: delete existing links, insert new ones,
/// then remove any orphaned authors (no remaining book links).
pub async fn set_book_authors(
    pool: &DbPool,
    book_id: i64,
    author_ids: &[i64],
    backend: DbBackend,
) -> Result<(), sqlx::Error> {
    // Remember old author IDs before unlinking
    let old_ids: Vec<(i64,)> =
        sqlx::query_as("SELECT author_id FROM book_authors WHERE book_id = ?")
            .bind(book_id)
            .fetch_all(pool)
            .await?;

    sqlx::query("DELETE FROM book_authors WHERE book_id = ?")
        .bind(book_id)
        .execute(pool)
        .await?;
    let sql = match backend {
        DbBackend::Mysql => "INSERT IGNORE INTO book_authors (book_id, author_id) VALUES (?, ?)",
        _ => {
            "INSERT INTO book_authors (book_id, author_id) VALUES (?, ?) \
             ON CONFLICT (book_id, author_id) DO NOTHING"
        }
    };
    for &author_id in author_ids {
        sqlx::query(sql)
            .bind(book_id)
            .bind(author_id)
            .execute(pool)
            .await?;
    }

    // Clean up orphaned authors that no longer have any books
    for (old_id,) in old_ids {
        if !author_ids.contains(&old_id) {
            delete_if_orphaned(pool, old_id).await?;
        }
    }
    Ok(())
}

/// Delete an author if they have no remaining book links.
pub async fn delete_if_orphaned(pool: &DbPool, author_id: i64) -> Result<(), sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM book_authors WHERE author_id = ?")
        .bind(author_id)
        .fetch_one(pool)
        .await?;
    if row.0 == 0 {
        sqlx::query("DELETE FROM authors WHERE id = ?")
            .bind(author_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn get_for_book(pool: &DbPool, book_id: i64) -> Result<Vec<Author>, sqlx::Error> {
    sqlx::query_as::<_, Author>(
        "SELECT a.* FROM authors a \
         JOIN book_authors ba ON ba.author_id = a.id \
         WHERE ba.book_id = ? ORDER BY a.full_name",
    )
    .bind(book_id)
    .fetch_all(pool)
    .await
}

/// Count authors matching a name search (contains).
pub async fn count_by_name_search(pool: &DbPool, term: &str) -> Result<i64, sqlx::Error> {
    let pattern = format!("%{term}%");
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM authors WHERE search_full_name LIKE ?")
        .bind(&pattern)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Alphabet drill-down: get prefix groups for author names.
pub async fn get_name_prefix_groups(
    pool: &DbPool,
    lang_code: i32,
    current_prefix: &str,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let prefix_len = (current_prefix.chars().count() + 1) as i32;
    let like_pattern = format!("{}%", current_prefix);
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT SUBSTR(search_full_name, 1, ?) as prefix, COUNT(*) as cnt \
         FROM authors \
         WHERE (? = 0 OR lang_code = ?) AND search_full_name LIKE ? \
         GROUP BY SUBSTR(search_full_name, 1, ?) \
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    async fn ensure_catalog(pool: &DbPool) -> i64 {
        sqlx::query("INSERT INTO catalogs (path, cat_name) VALUES ('/authors', 'authors')")
            .execute(pool)
            .await
            .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM catalogs WHERE path = '/authors'")
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    async fn insert_test_book(pool: &DbPool, catalog_id: i64, title: &str) -> i64 {
        let search_title = title.to_uppercase();
        sqlx::query(
            "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
             lang, lang_code, size, avail, cat_type, cover, cover_type) \
             VALUES (?, ?, '/authors', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
        )
        .bind(catalog_id)
        .bind(format!("{title}.fb2"))
        .bind(title)
        .bind(search_title)
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
    async fn test_insert_search_count_and_prefix_groups() {
        let (pool, _) = create_test_pool().await;

        let alice = insert(&pool, "Alice Smith", "ALICE SMITH", 2, DbBackend::Sqlite)
            .await
            .unwrap();
        let _alina = insert(&pool, "Alina West", "ALINA WEST", 2, DbBackend::Sqlite)
            .await
            .unwrap();
        let _cyr = insert(&pool, "Алиса", "АЛИСА", 1, DbBackend::Sqlite)
            .await
            .unwrap();

        let found = get_by_id(&pool, alice).await.unwrap().unwrap();
        assert_eq!(found.full_name, "Alice Smith");

        let by_name = find_by_name(&pool, "Alice Smith").await.unwrap().unwrap();
        assert_eq!(by_name.id, alice);

        let search = search_by_name(&pool, "ALI", 100, 0).await.unwrap();
        assert_eq!(search.len(), 2);

        let count = count_by_name_search(&pool, "ALI").await.unwrap();
        assert_eq!(count, 2);

        let prefix = get_by_lang_code_prefix(&pool, 2, "AL", 100, 0)
            .await
            .unwrap();
        assert_eq!(prefix.len(), 2);

        let groups = get_name_prefix_groups(&pool, 2, "A").await.unwrap();
        assert_eq!(groups, vec![("AL".to_string(), 2)]);
    }

    #[tokio::test]
    async fn test_insert_duplicate_returns_same_id() {
        let (pool, _) = create_test_pool().await;

        let id1 = insert(&pool, "Same Name", "SAME NAME", 2, DbBackend::Sqlite)
            .await
            .unwrap();
        let id2 = insert(&pool, "Same Name", "DIFFERENT SEARCH", 1, DbBackend::Sqlite)
            .await
            .unwrap();
        assert_eq!(id1, id2);
    }

    #[tokio::test]
    async fn test_link_and_set_book_authors_with_orphan_cleanup() {
        let (pool, _) = create_test_pool().await;
        let catalog_id = ensure_catalog(&pool).await;
        let book_id = insert_test_book(&pool, catalog_id, "Book One").await;

        let alice_id = insert(&pool, "Alice", "ALICE", 2, DbBackend::Sqlite)
            .await
            .unwrap();
        let bob_id = insert(&pool, "Bob", "BOB", 2, DbBackend::Sqlite)
            .await
            .unwrap();

        link_book(&pool, book_id, alice_id, DbBackend::Sqlite)
            .await
            .unwrap();
        let linked = get_for_book(&pool, book_id).await.unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].id, alice_id);

        set_book_authors(&pool, book_id, &[bob_id], DbBackend::Sqlite)
            .await
            .unwrap();
        let linked = get_for_book(&pool, book_id).await.unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].id, bob_id);

        // Alice became orphaned and should have been deleted.
        assert!(find_by_name(&pool, "Alice").await.unwrap().is_none());
        assert!(find_by_name(&pool, "Bob").await.unwrap().is_some());

        // Bob is still linked, so explicit orphan cleanup must keep him.
        delete_if_orphaned(&pool, bob_id).await.unwrap();
        assert!(find_by_name(&pool, "Bob").await.unwrap().is_some());
    }
}
