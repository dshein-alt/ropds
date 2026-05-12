use crate::db::{DbBackend, DbPool};

use crate::db::models::Author;

pub async fn get_by_id(pool: &DbPool, id: i64) -> Result<Option<Author>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM authors WHERE id = ?");
    sqlx::query_as::<_, Author>(&sql)
        .bind(id)
        .fetch_optional(pool.inner())
        .await
}

pub async fn search_by_name(
    pool: &DbPool,
    term: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Author>, sqlx::Error> {
    let pattern = format!("%{term}%");
    let sql = pool.sql(
        "SELECT * FROM authors WHERE search_full_name LIKE ? \
         ORDER BY search_full_name LIMIT ? OFFSET ?",
    );
    sqlx::query_as::<_, Author>(&sql)
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
) -> Result<Vec<Author>, sqlx::Error> {
    if prefix.is_empty() {
        let sql = pool.sql(
            "SELECT * FROM authors WHERE (? = 0 OR lang_code = ?) \
             ORDER BY search_full_name LIMIT ? OFFSET ?",
        );
        return sqlx::query_as::<_, Author>(&sql)
            .bind(lang_code)
            .bind(lang_code)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool.inner())
            .await;
    }
    // Word-boundary prefix match: either at start of the name or after a space.
    let start_pat = format!("{prefix}%");
    let word_pat = format!("% {prefix}%");
    let sql = pool.sql(
        "SELECT * FROM authors WHERE (? = 0 OR lang_code = ?) \
         AND (search_full_name LIKE ? OR search_full_name LIKE ?) \
         ORDER BY search_full_name LIMIT ? OFFSET ?",
    );
    sqlx::query_as::<_, Author>(&sql)
        .bind(lang_code)
        .bind(lang_code)
        .bind(&start_pat)
        .bind(&word_pat)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool.inner())
        .await
}

pub async fn find_by_name(pool: &DbPool, full_name: &str) -> Result<Option<Author>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM authors WHERE full_name = ?");
    sqlx::query_as::<_, Author>(&sql)
        .bind(full_name)
        .fetch_optional(pool.inner())
        .await
}

pub async fn insert(
    pool: &DbPool,
    full_name: &str,
    search_full_name: &str,
    lang_code: i32,
) -> Result<i64, sqlx::Error> {
    let sql = match pool.backend() {
        DbBackend::Mysql => {
            "INSERT IGNORE INTO authors (full_name, search_full_name, lang_code) VALUES (?, ?, ?)"
        }
        _ => {
            "INSERT INTO authors (full_name, search_full_name, lang_code) VALUES (?, ?, ?) \
             ON CONFLICT (full_name) DO NOTHING"
        }
    };
    let sql = pool.sql(sql);
    let result = sqlx::query(&sql)
        .bind(full_name)
        .bind(search_full_name)
        .bind(lang_code)
        .execute(pool.inner())
        .await?;
    if let Some(id) = result.last_insert_id()
        && id > 0
    {
        return Ok(id);
    }
    // Fallback: query back by name (INSERT OR IGNORE returns 0 on conflict)
    let sql = pool.sql("SELECT id FROM authors WHERE full_name = ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(full_name)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

pub async fn link_book(pool: &DbPool, book_id: i64, author_id: i64) -> Result<(), sqlx::Error> {
    let sql = match pool.backend() {
        DbBackend::Mysql => "INSERT IGNORE INTO book_authors (book_id, author_id) VALUES (?, ?)",
        _ => {
            "INSERT INTO book_authors (book_id, author_id) VALUES (?, ?) \
             ON CONFLICT (book_id, author_id) DO NOTHING"
        }
    };
    let sql = pool.sql(sql);
    sqlx::query(&sql)
        .bind(book_id)
        .bind(author_id)
        .execute(pool.inner())
        .await?;
    Ok(())
}

/// Replace all authors for a book: delete existing links, insert new ones,
/// then remove any orphaned authors (no remaining book links).
pub async fn set_book_authors(
    pool: &DbPool,
    book_id: i64,
    author_ids: &[i64],
) -> Result<(), sqlx::Error> {
    // Remember old author IDs before unlinking
    let sql = pool.sql("SELECT author_id FROM book_authors WHERE book_id = ?");
    let old_ids: Vec<(i64,)> = sqlx::query_as(&sql)
        .bind(book_id)
        .fetch_all(pool.inner())
        .await?;

    let sql = pool.sql("DELETE FROM book_authors WHERE book_id = ?");
    sqlx::query(&sql)
        .bind(book_id)
        .execute(pool.inner())
        .await?;
    let sql = match pool.backend() {
        DbBackend::Mysql => "INSERT IGNORE INTO book_authors (book_id, author_id) VALUES (?, ?)",
        _ => {
            "INSERT INTO book_authors (book_id, author_id) VALUES (?, ?) \
             ON CONFLICT (book_id, author_id) DO NOTHING"
        }
    };
    let sql = pool.sql(sql);
    for &author_id in author_ids {
        sqlx::query(&sql)
            .bind(book_id)
            .bind(author_id)
            .execute(pool.inner())
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
    let sql = pool.sql("SELECT COUNT(*) FROM book_authors WHERE author_id = ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(author_id)
        .fetch_one(pool.inner())
        .await?;
    if row.0 == 0 {
        let sql = pool.sql("DELETE FROM authors WHERE id = ?");
        sqlx::query(&sql)
            .bind(author_id)
            .execute(pool.inner())
            .await?;
    }
    Ok(())
}

pub async fn get_for_book(pool: &DbPool, book_id: i64) -> Result<Vec<Author>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT a.* FROM authors a \
         JOIN book_authors ba ON ba.author_id = a.id \
         WHERE ba.book_id = ? ORDER BY a.full_name",
    );
    sqlx::query_as::<_, Author>(&sql)
        .bind(book_id)
        .fetch_all(pool.inner())
        .await
}

/// Count authors matching a name search (contains).
pub async fn count_by_name_search(pool: &DbPool, term: &str) -> Result<i64, sqlx::Error> {
    let pattern = format!("%{term}%");
    let sql = pool.sql("SELECT COUNT(*) FROM authors WHERE search_full_name LIKE ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(&pattern)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

/// Count authors matching a word-boundary prefix, scoped by language.
///
/// Mirrors the WHERE clause of [`get_by_lang_code_prefix`] so totals stay
/// in sync with the paginated listing used by the alphabet drill-down.
pub async fn count_by_lang_code_prefix(
    pool: &DbPool,
    lang_code: i32,
    prefix: &str,
) -> Result<i64, sqlx::Error> {
    if prefix.is_empty() {
        let sql = pool.sql("SELECT COUNT(*) FROM authors WHERE ? = 0 OR lang_code = ?");
        let row: (i64,) = sqlx::query_as(&sql)
            .bind(lang_code)
            .bind(lang_code)
            .fetch_one(pool.inner())
            .await?;
        return Ok(row.0);
    }
    let start_pat = format!("{prefix}%");
    let word_pat = format!("% {prefix}%");
    let sql = pool.sql(
        "SELECT COUNT(*) FROM authors WHERE (? = 0 OR lang_code = ?) \
         AND (search_full_name LIKE ? OR search_full_name LIKE ?)",
    );
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(lang_code)
        .bind(lang_code)
        .bind(&start_pat)
        .bind(&word_pat)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

/// Alphabet drill-down: get prefix groups for author names.
///
/// The current prefix is matched at any word boundary inside `search_full_name`
/// (start of the string or immediately after a space). For each matching row,
/// every distinct word-initial extension of `current_prefix` (one more character
/// than the current prefix, capped at the word length) contributes one count.
pub async fn get_name_prefix_groups(
    pool: &DbPool,
    lang_code: i32,
    current_prefix: &str,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let names: Vec<(String,)> = if current_prefix.is_empty() {
        let sql = pool.sql("SELECT search_full_name FROM authors WHERE ? = 0 OR lang_code = ?");
        sqlx::query_as(&sql)
            .bind(lang_code)
            .bind(lang_code)
            .fetch_all(pool.inner())
            .await?
    } else {
        let start_pat = format!("{}%", current_prefix);
        let word_pat = format!("% {}%", current_prefix);
        let sql = pool.sql(
            "SELECT search_full_name FROM authors \
             WHERE (? = 0 OR lang_code = ?) \
             AND (search_full_name LIKE ? OR search_full_name LIKE ?)",
        );
        sqlx::query_as(&sql)
            .bind(lang_code)
            .bind(lang_code)
            .bind(&start_pat)
            .bind(&word_pat)
            .fetch_all(pool.inner())
            .await?
    };
    Ok(aggregate_word_prefix_groups(
        names.iter().map(|(n,)| n.as_str()),
        current_prefix,
    ))
}

/// Aggregate word-boundary prefix groups for a sequence of uppercased names.
///
/// Each input row contributes once per distinct word-initial extension that
/// starts with `current_prefix`. The extension is the matching word's first
/// `current_prefix.chars().count() + 1` characters, or the whole word when it
/// is exactly as long as `current_prefix`.
pub(crate) fn aggregate_word_prefix_groups<'a, I>(
    names: I,
    current_prefix: &str,
) -> Vec<(String, i64)>
where
    I: IntoIterator<Item = &'a str>,
{
    use std::collections::{BTreeMap, BTreeSet};
    let prefix_chars: Vec<char> = current_prefix.chars().collect();
    let prefix_len = prefix_chars.len();
    let next_len = prefix_len + 1;
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for name in names {
        let mut row_prefixes: BTreeSet<String> = BTreeSet::new();
        for word in name.split_whitespace() {
            let word_chars: Vec<char> = word.chars().collect();
            if word_chars.len() < prefix_len {
                continue;
            }
            if word_chars[..prefix_len] != prefix_chars[..] {
                continue;
            }
            let take = word_chars.len().min(next_len);
            let extended: String = word_chars[..take].iter().collect();
            row_prefixes.insert(extended);
        }
        for extended in row_prefixes {
            *counts.entry(extended).or_insert(0) += 1;
        }
    }
    counts.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    #[test]
    fn aggregate_word_prefix_groups_empty_prefix_uses_first_letter_per_word() {
        let names = [
            "JOHNS ABRAHAM",
            "ABERDIN LAURA",
            "HAKIM ABDUL EFENDI",
            "CASEY GABRIEL",
            "LAMABAD CAFIR",
        ];
        let groups = aggregate_word_prefix_groups(names.iter().copied(), "");
        let map: std::collections::HashMap<_, _> = groups.into_iter().collect();
        // A appears as the first letter of a word in three of the rows.
        assert_eq!(map.get("A"), Some(&3));
        // L appears in "LAURA" and "LAMABAD".
        assert_eq!(map.get("L"), Some(&2));
        // C appears in "CASEY" and "CAFIR".
        assert_eq!(map.get("C"), Some(&2));
        assert_eq!(map.get("J"), Some(&1));
        assert_eq!(map.get("H"), Some(&1));
        assert_eq!(map.get("E"), Some(&1));
        assert_eq!(map.get("G"), Some(&1));
    }

    #[test]
    fn aggregate_word_prefix_groups_excludes_inner_substrings() {
        // "GABRIEL" contains "AB" but not at a word boundary; aggregation
        // operates on word starts only and must skip it.
        let names = ["GABRIEL CASEY"];
        let groups = aggregate_word_prefix_groups(names.iter().copied(), "AB");
        assert!(groups.is_empty());
    }

    #[test]
    fn aggregate_word_prefix_groups_short_word_keeps_full_word() {
        // When a word is exactly as long as the prefix it cannot be extended;
        // mirror the original SUBSTR semantics by returning the word as-is.
        let names = ["AB SOMETHING"];
        let groups = aggregate_word_prefix_groups(names.iter().copied(), "AB");
        assert_eq!(groups, vec![("AB".to_string(), 1)]);
    }

    async fn ensure_catalog(pool: &DbPool) -> i64 {
        let sql = pool.sql("INSERT INTO catalogs (path, cat_name) VALUES ('/authors', 'authors')");
        sqlx::query(&sql).execute(pool.inner()).await.unwrap();
        let sql = pool.sql("SELECT id FROM catalogs WHERE path = '/authors'");
        let row: (i64,) = sqlx::query_as(&sql).fetch_one(pool.inner()).await.unwrap();
        row.0
    }

    async fn insert_test_book(pool: &DbPool, catalog_id: i64, title: &str) -> i64 {
        let search_title = title.to_uppercase();
        let sql = pool.sql(
            "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
             lang, lang_code, size, avail, cat_type, cover, cover_type) \
             VALUES (?, ?, '/authors', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
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

        let alice = insert(&pool, "Alice Smith", "ALICE SMITH", 2)
            .await
            .unwrap();
        let _alina = insert(&pool, "Alina West", "ALINA WEST", 2).await.unwrap();
        let _cyr = insert(&pool, "Алиса", "АЛИСА", 1).await.unwrap();

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
        let all_langs = get_by_lang_code_prefix(&pool, 0, "", 100, 0).await.unwrap();
        assert_eq!(all_langs.len(), 3);

        let groups = get_name_prefix_groups(&pool, 2, "A").await.unwrap();
        assert_eq!(groups, vec![("AL".to_string(), 2)]);
    }

    #[tokio::test]
    async fn test_word_boundary_prefix_listing_and_groups() {
        let pool = create_test_pool().await;
        // Names already in normalized "search" form (uppercased; for two-part
        // names the scanner reorders to "Last First" before uppercasing).
        insert(&pool, "Johns Abraham", "JOHNS ABRAHAM", 2)
            .await
            .unwrap();
        insert(&pool, "Aberdin Laura", "ABERDIN LAURA", 2)
            .await
            .unwrap();
        insert(&pool, "Hakim Abdul Efendi", "HAKIM ABDUL EFENDI", 2)
            .await
            .unwrap();
        // Inner substring "AB" but never at a word boundary — must not match.
        insert(&pool, "Casey Gabriel", "CASEY GABRIEL", 2)
            .await
            .unwrap();
        insert(&pool, "Lamabad Cafir", "LAMABAD CAFIR", 2)
            .await
            .unwrap();

        // Listing for prefix "AB" — three authors with a word starting with AB.
        let by_ab = get_by_lang_code_prefix(&pool, 2, "AB", 100, 0)
            .await
            .unwrap();
        let names: Vec<&str> = by_ab.iter().map(|a| a.full_name.as_str()).collect();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"Johns Abraham"));
        assert!(names.contains(&"Aberdin Laura"));
        assert!(names.contains(&"Hakim Abdul Efendi"));

        // Listing for prefix "ab" — case-insensitive uppercasing happens at the
        // call site, so we mirror it here.
        let by_ab_ci = get_by_lang_code_prefix(&pool, 2, &"ab".to_uppercase(), 100, 0)
            .await
            .unwrap();
        assert_eq!(by_ab_ci.len(), 3);

        // Drill-down from "A" — sub-group "AB" should aggregate the three rows
        // (their AB-extensions are ABR, ABE and ABD respectively).
        let groups = get_name_prefix_groups(&pool, 2, "A").await.unwrap();
        let ab_total: i64 = groups
            .iter()
            .filter(|(p, _)| p.starts_with("AB"))
            .map(|(_, c)| *c)
            .sum();
        assert_eq!(ab_total, 3);

        // Drill into "AB" — three distinct sub-prefixes, one per author.
        let groups = get_name_prefix_groups(&pool, 2, "AB").await.unwrap();
        let prefixes: Vec<&str> = groups.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(prefixes, vec!["ABD", "ABE", "ABR"]);
        for (_, count) in &groups {
            assert_eq!(*count, 1);
        }

        // count_by_name_search is unrelated to drill-down (uses substring),
        // so a substring "AB" matches all five rows above.
        let total = count_by_name_search(&pool, "AB").await.unwrap();
        assert_eq!(total, 5);
    }

    #[tokio::test]
    async fn test_insert_duplicate_returns_same_id() {
        let pool = create_test_pool().await;

        let id1 = insert(&pool, "Same Name", "SAME NAME", 2).await.unwrap();
        let id2 = insert(&pool, "Same Name", "DIFFERENT SEARCH", 1)
            .await
            .unwrap();
        assert_eq!(id1, id2);
    }

    #[tokio::test]
    async fn test_link_and_set_book_authors_with_orphan_cleanup() {
        let pool = create_test_pool().await;
        let catalog_id = ensure_catalog(&pool).await;
        let book_id = insert_test_book(&pool, catalog_id, "Book One").await;

        let alice_id = insert(&pool, "Alice", "ALICE", 2).await.unwrap();
        let bob_id = insert(&pool, "Bob", "BOB", 2).await.unwrap();

        link_book(&pool, book_id, alice_id).await.unwrap();
        let linked = get_for_book(&pool, book_id).await.unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].id, alice_id);

        set_book_authors(&pool, book_id, &[bob_id]).await.unwrap();
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
