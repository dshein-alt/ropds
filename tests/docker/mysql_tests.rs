use super::*;
use ropds::db::models::CatType;
use ropds::db::queries::{authors, books, bookshelf, catalogs, genres};
use ropds::scanner;

// ---------------------------------------------------------------------------
// Migration & schema tests
// ---------------------------------------------------------------------------

/// Verify that MySQL migrations run and seed the 228 built-in genres.
#[tokio::test]
async fn mysql_migrations_run_successfully() {
    let (_container, pool) = start_mysql().await;
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM genres")
        .fetch_one(pool.inner())
        .await
        .unwrap();
    assert_eq!(row.0, 228); // 228 seeded genres
}

/// CURRENT_TIMESTAMP default produces a non-empty TEXT value on MySQL.
#[tokio::test]
async fn mysql_current_timestamp_produces_text() {
    let (_container, pool) = start_mysql().await;
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
// INSERT IGNORE duplicate handling
// ---------------------------------------------------------------------------

/// Inserting a duplicate author (same full_name) returns the original row's ID.
#[tokio::test]
async fn mysql_insert_duplicate_author_returns_same_id() {
    let (_container, pool) = start_mysql().await;
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
async fn mysql_insert_duplicate_catalog_returns_same_id() {
    let (_container, pool) = start_mysql().await;
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
async fn mysql_cyrillic_search_works() {
    let (_container, pool) = start_mysql().await;
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
async fn mysql_bookshelf_upsert_dedup() {
    let (_container, pool) = start_mysql().await;

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
async fn mysql_genre_upsert_translations() {
    let (_container, pool) = start_mysql().await;
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
/// when running against a real MariaDB backend.
#[tokio::test]
async fn mysql_scanner_finds_books_and_metadata() {
    let _lock = SCAN_MUTEX.lock().await;
    let (_container, pool) = start_mysql().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config_with_url(
        lib_dir.path(),
        covers_dir.path(),
        "mysql://root@127.0.0.1:3306/test",
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
async fn mysql_scanner_skips_existing_books() {
    let _lock = SCAN_MUTEX.lock().await;
    let (_container, pool) = start_mysql().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config_with_url(
        lib_dir.path(),
        covers_dir.path(),
        "mysql://root@127.0.0.1:3306/test",
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

/// Title search works correctly on MySQL (LIKE with UPPER).
#[tokio::test]
async fn mysql_book_title_search() {
    let (_container, pool) = start_mysql().await;
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

/// Author linking and orphan cleanup work on MySQL.
#[tokio::test]
async fn mysql_author_link_and_orphan_cleanup() {
    let (_container, pool) = start_mysql().await;
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
