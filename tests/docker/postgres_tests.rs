use super::*;
use ropds::db::models::CatType;
use ropds::db::queries::{authors, books, bookshelf, catalogs, genres};
use ropds::scanner;

// ---------------------------------------------------------------------------
// Migration & schema tests
// ---------------------------------------------------------------------------

/// Verify that PG migrations run and seed the 228 built-in genres.
#[tokio::test]
async fn pg_migrations_run_successfully() {
    let (_container, pool) = start_postgres().await;
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM genres")
        .fetch_one(pool.inner())
        .await
        .unwrap();
    assert_eq!(row.0, 228); // 228 seeded genres
}

/// CURRENT_TIMESTAMP default produces a non-empty TEXT value on PG.
#[tokio::test]
async fn pg_current_timestamp_produces_text() {
    let (_container, pool) = start_postgres().await;
    sqlx::query(
        "INSERT INTO users (username, password_hash, is_superuser) VALUES ('ts_test', 'h', 0)",
    )
    .execute(pool.inner())
    .await
    .unwrap();
    let row: (String,) = sqlx::query_as("SELECT created_at FROM users WHERE username = 'ts_test'")
        .fetch_one(pool.inner())
        .await
        .unwrap();
    assert!(!row.0.is_empty());
}

// ---------------------------------------------------------------------------
// INSERT OR IGNORE / ON CONFLICT duplicate handling
// ---------------------------------------------------------------------------

/// Inserting a duplicate author (same full_name) returns the original row's ID.
#[tokio::test]
async fn pg_insert_duplicate_author_returns_same_id() {
    let (_container, pool) = start_postgres().await;
    let id1 = authors::insert(&pool, "Test Author", "TEST AUTHOR", 2)
        .await
        .unwrap();
    let id2 = authors::insert(&pool, "Test Author", "DIFFERENT", 1)
        .await
        .unwrap();
    assert_eq!(id1, id2);
}

/// Inserting a duplicate catalog (same path) returns the original row's ID.
#[tokio::test]
async fn pg_insert_duplicate_catalog_returns_same_id() {
    let (_container, pool) = start_postgres().await;
    let id1 = catalogs::insert(&pool, None, "/dup", "dup", CatType::Normal, 0, "")
        .await
        .unwrap();
    let id2 = catalogs::insert(&pool, None, "/dup", "dup2", CatType::Zip, 42, "mtime")
        .await
        .unwrap();
    assert_eq!(id1, id2);
}

// ---------------------------------------------------------------------------
// Unicode / Cyrillic search
// ---------------------------------------------------------------------------

/// Cyrillic text round-trips correctly and is searchable via LIKE.
#[tokio::test]
async fn pg_cyrillic_search_works() {
    let (_container, pool) = start_postgres().await;
    let id = authors::insert(&pool, "Толстой Лев", "ТОЛСТОЙ ЛЕВ", 1)
        .await
        .unwrap();
    let found = authors::search_by_name(&pool, "ТОЛСТОЙ", 10, 0)
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, id);
}

// ---------------------------------------------------------------------------
// Bookshelf upsert dedup
// ---------------------------------------------------------------------------

/// Upserting the same (user, book) pair twice results in only one row.
#[tokio::test]
async fn pg_bookshelf_upsert_dedup() {
    let (_container, pool) = start_postgres().await;

    // Create user
    sqlx::query(
        "INSERT INTO users (username, password_hash, is_superuser) VALUES ('shelf_user', 'h', 0)",
    )
    .execute(pool.inner())
    .await
    .unwrap();
    let row: (i64,) = sqlx::query_as("SELECT id FROM users WHERE username = 'shelf_user'")
        .fetch_one(pool.inner())
        .await
        .unwrap();
    let user_id = row.0;

    // Create catalog + book
    let cat_id = catalogs::insert(&pool, None, "/test", "test", CatType::Normal, 0, "")
        .await
        .unwrap();
    let book_id = books::insert(
        &pool,
        cat_id,
        "test.fb2",
        "/test",
        "fb2",
        "Test Book",
        "TEST BOOK",
        "",
        "",
        "en",
        2,
        100,
        CatType::Normal,
        0,
        "",
    )
    .await
    .unwrap();

    // Upsert twice -- should not duplicate
    bookshelf::upsert(&pool, user_id, book_id).await.unwrap();
    bookshelf::upsert(&pool, user_id, book_id).await.unwrap();
    let count = bookshelf::count_by_user(&pool, user_id).await.unwrap();
    assert_eq!(count, 1);
}

// ---------------------------------------------------------------------------
// Genre translation upsert
// ---------------------------------------------------------------------------

/// Upserting a section translation twice with the same (section, lang) key
/// replaces the name rather than creating a duplicate.
#[tokio::test]
async fn pg_genre_upsert_translations() {
    let (_container, pool) = start_postgres().await;
    let section_id = genres::create_section(&pool, "test_section").await.unwrap();
    genres::upsert_section_translation(&pool, section_id, "en", "Test Section")
        .await
        .unwrap();
    genres::upsert_section_translation(&pool, section_id, "en", "Updated Section")
        .await
        .unwrap();
    let translations = genres::get_section_translations(&pool, section_id)
        .await
        .unwrap();
    assert_eq!(translations.len(), 1);
    assert_eq!(translations[0].name, "Updated Section");
}

// ---------------------------------------------------------------------------
// Scanner integration (full pipeline)
// ---------------------------------------------------------------------------

/// The scanner finds books and links metadata (authors, genres, series)
/// when running against a real PostgreSQL backend.
#[tokio::test]
async fn pg_scanner_finds_books_and_metadata() {
    let _lock = SCAN_MUTEX.lock().await;
    let (_container, pool) = start_postgres().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config_with_url(
        lib_dir.path(),
        covers_dir.path(),
        // The URL doesn't matter for the scan itself (pool is already connected),
        // but the Config struct requires a valid-looking URL.
        "postgres://postgres:postgres@127.0.0.1:5432/postgres",
    );

    copy_test_files(lib_dir.path(), &["test_book.fb2"]);

    let stats = scanner::run_scan(&pool, &config).await.unwrap();
    assert!(stats.books_added >= 1);

    // Verify book was found
    let found = books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap();
    assert!(found.is_some());
    let book = found.unwrap();
    assert!(!book.title.is_empty());

    // Verify authors were linked
    let book_authors = authors::get_for_book(&pool, book.id).await.unwrap();
    assert!(!book_authors.is_empty());
}

/// Second scan of the same library skips already-known books.
#[tokio::test]
async fn pg_scanner_skips_existing_books() {
    let _lock = SCAN_MUTEX.lock().await;
    let (_container, pool) = start_postgres().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config_with_url(
        lib_dir.path(),
        covers_dir.path(),
        "postgres://postgres:postgres@127.0.0.1:5432/postgres",
    );

    copy_test_files(lib_dir.path(), &["test_book.fb2", "test_book.epub"]);

    let stats1 = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats1.books_added, 2);

    let stats2 = scanner::run_scan(&pool, &config).await.unwrap();
    assert_eq!(stats2.books_added, 0, "no new books on second scan");
    assert_eq!(stats2.books_skipped, 2, "both books should be skipped");
}

// ---------------------------------------------------------------------------
// Book search
// ---------------------------------------------------------------------------

/// Title search works correctly on PostgreSQL (LIKE with UPPER).
#[tokio::test]
async fn pg_book_title_search() {
    let (_container, pool) = start_postgres().await;
    let cat_id = catalogs::insert(&pool, None, "/search", "search", CatType::Normal, 0, "")
        .await
        .unwrap();

    books::insert(
        &pool,
        cat_id,
        "alpha.fb2",
        "/search",
        "fb2",
        "Alpha Book",
        "ALPHA BOOK",
        "",
        "",
        "en",
        2,
        100,
        CatType::Normal,
        0,
        "",
    )
    .await
    .unwrap();
    books::insert(
        &pool,
        cat_id,
        "beta.fb2",
        "/search",
        "fb2",
        "Beta Book",
        "BETA BOOK",
        "",
        "",
        "en",
        2,
        100,
        CatType::Normal,
        0,
        "",
    )
    .await
    .unwrap();

    let results = books::search_by_title(&pool, "ALPHA", 100, 0, false)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Alpha Book");

    let all = books::search_by_title(&pool, "BOOK", 100, 0, false)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
}

// ---------------------------------------------------------------------------
// Author-book linking
// ---------------------------------------------------------------------------

/// Author linking and orphan cleanup work on PostgreSQL.
#[tokio::test]
async fn pg_author_link_and_orphan_cleanup() {
    let (_container, pool) = start_postgres().await;
    let cat_id = catalogs::insert(&pool, None, "/link", "link", CatType::Normal, 0, "")
        .await
        .unwrap();
    let book_id = books::insert(
        &pool,
        cat_id,
        "link.fb2",
        "/link",
        "fb2",
        "Link Book",
        "LINK BOOK",
        "",
        "",
        "en",
        2,
        100,
        CatType::Normal,
        0,
        "",
    )
    .await
    .unwrap();

    let alice_id = authors::insert(&pool, "Alice", "ALICE", 2).await.unwrap();
    let bob_id = authors::insert(&pool, "Bob", "BOB", 2).await.unwrap();

    authors::link_book(&pool, book_id, alice_id).await.unwrap();
    let linked = authors::get_for_book(&pool, book_id).await.unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].id, alice_id);

    // Replace Alice with Bob -- Alice becomes orphaned and should be deleted.
    authors::set_book_authors(&pool, book_id, &[bob_id])
        .await
        .unwrap();
    let linked = authors::get_for_book(&pool, book_id).await.unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].id, bob_id);

    assert!(
        authors::find_by_name(&pool, "Alice")
            .await
            .unwrap()
            .is_none()
    );
    assert!(authors::find_by_name(&pool, "Bob").await.unwrap().is_some());
}

// ---------------------------------------------------------------------------
// Duplicate detection & author_key
// ---------------------------------------------------------------------------

/// `update_author_key`, `count_doubles`, and `get_duplicate_groups` work on PG.
/// This covers the `||` concatenation and `STRING_AGG` backfill used on PG.
#[tokio::test]
async fn pg_author_key_and_duplicate_detection() {
    let (_container, pool) = start_postgres().await;
    let cat_id = catalogs::insert(&pool, None, "/dup", "dup", CatType::Normal, 0, "")
        .await
        .unwrap();

    let alice = authors::insert(&pool, "Alice", "ALICE", 2).await.unwrap();
    let bob = authors::insert(&pool, "Bob", "BOB", 2).await.unwrap();

    // Book 1 + 2: same title, same author (Alice) → duplicates
    let b1 = books::insert(
        &pool,
        cat_id,
        "dup1.fb2",
        "/dup",
        "fb2",
        "Same Book",
        "SAME BOOK",
        "",
        "",
        "en",
        2,
        100,
        CatType::Normal,
        0,
        "",
    )
    .await
    .unwrap();
    let b2 = books::insert(
        &pool,
        cat_id,
        "dup2.fb2",
        "/dup",
        "fb2",
        "Same Book v2",
        "SAME BOOK",
        "",
        "",
        "en",
        2,
        200,
        CatType::Normal,
        0,
        "",
    )
    .await
    .unwrap();

    // Book 3: same title, different author (Bob) → NOT a duplicate of b1/b2
    let b3 = books::insert(
        &pool,
        cat_id,
        "dup3.fb2",
        "/dup",
        "fb2",
        "Same Book v3",
        "SAME BOOK",
        "",
        "",
        "en",
        2,
        300,
        CatType::Normal,
        0,
        "",
    )
    .await
    .unwrap();

    authors::link_book(&pool, b1, alice).await.unwrap();
    authors::link_book(&pool, b2, alice).await.unwrap();
    authors::link_book(&pool, b3, bob).await.unwrap();

    for &id in &[b1, b2, b3] {
        books::update_author_key(&pool, id).await.unwrap();
    }

    // Verify author_key format
    let book1 = books::get_by_id(&pool, b1).await.unwrap().unwrap();
    assert_eq!(book1.author_key, alice.to_string());
    let book3 = books::get_by_id(&pool, b3).await.unwrap().unwrap();
    assert_eq!(book3.author_key, bob.to_string());

    // count_doubles: b1 sees 2 (b1+b2), b3 sees 1 (only itself)
    assert_eq!(books::count_doubles(&pool, b1).await.unwrap(), 2);
    assert_eq!(books::count_doubles(&pool, b3).await.unwrap(), 1);

    // Duplicate groups: only 1 group (b1+b2)
    let count = books::count_duplicate_groups(&pool).await.unwrap();
    assert_eq!(count, 1);

    let groups = books::get_duplicate_groups(&pool, 100, 0).await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].search_title, "SAME BOOK");
    assert_eq!(groups[0].cnt, 2);

    // Books in group
    let in_group = books::get_books_in_group(&pool, &groups[0].search_title, &groups[0].author_key)
        .await
        .unwrap();
    assert_eq!(in_group.len(), 2);

    // hide_doubles with COUNT(DISTINCT ...) using || on PG
    assert_eq!(
        books::count_by_catalog(&pool, cat_id, true).await.unwrap(),
        2, // b1+b2 dedup to 1, plus b3 = 2
    );
    assert_eq!(
        books::count_by_catalog(&pool, cat_id, false).await.unwrap(),
        3,
    );
}

/// Transactional `set_book_authors_and_update_key` works on PG.
#[tokio::test]
async fn pg_set_book_authors_and_update_key() {
    let (_container, pool) = start_postgres().await;
    let cat_id = catalogs::insert(&pool, None, "/txn", "txn", CatType::Normal, 0, "")
        .await
        .unwrap();

    let alice = authors::insert(&pool, "Alice T", "ALICE T", 2)
        .await
        .unwrap();
    let bob = authors::insert(&pool, "Bob T", "BOB T", 2).await.unwrap();

    let book = books::insert(
        &pool,
        cat_id,
        "txn.fb2",
        "/txn",
        "fb2",
        "Txn Book",
        "TXN BOOK",
        "",
        "",
        "en",
        2,
        100,
        CatType::Normal,
        0,
        "",
    )
    .await
    .unwrap();

    // Initial: set authors to [alice]
    books::set_book_authors_and_update_key(&pool, book, &[alice])
        .await
        .unwrap();
    let b = books::get_by_id(&pool, book).await.unwrap().unwrap();
    assert_eq!(b.author_key, alice.to_string());

    // Update: set authors to [bob]
    books::set_book_authors_and_update_key(&pool, book, &[bob])
        .await
        .unwrap();
    let b = books::get_by_id(&pool, book).await.unwrap().unwrap();
    assert_eq!(b.author_key, bob.to_string());

    let linked = authors::get_for_book(&pool, book).await.unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].id, bob);
}
