use ropds::db;
use ropds::db::queries::{authors, genres, series};
use ropds::scanner;

use super::*;

/// Helper: set up a scanned library with several test books and return (pool, config).
async fn setup_library() -> (
    db::DbPool,
    ropds::config::Config,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(
        lib_dir.path(),
        &[
            "test_book.fb2",
            "test_book.epub",
            "title_only.fb2",
            "no_cover.fb2",
            "author_no_genre.fb2",
        ],
    );

    scanner::run_scan(&pool, &config, ropds::db::DbBackend::Sqlite)
        .await
        .unwrap();
    (pool, config, lib_dir, covers_dir)
}

/// Search books by title (full-text, type=m).
#[tokio::test]
async fn search_books_by_title() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _lib, _cov) = setup_library().await;
    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/search/books?type=m&q=Test+Book").await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(
        html.contains("Test Book Title"),
        "should find FB2 test book"
    );
    assert!(
        html.contains("EPUB Test Book"),
        "should find EPUB test book"
    );
}

/// Search books by title prefix (type=b).
#[tokio::test]
async fn search_books_by_title_prefix() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _lib, _cov) = setup_library().await;
    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/search/books?type=b&q=Lonely").await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(
        html.contains("Lonely Title Book"),
        "prefix search should find 'Lonely Title Book'"
    );
}

/// Search books by author ID (type=a).
#[tokio::test]
async fn search_books_by_author_id() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _lib, _cov) = setup_library().await;

    // Find author "Doe John" (normalised from "John Doe")
    let author = authors::find_by_name(&pool, "Doe John")
        .await
        .unwrap()
        .expect("author 'Doe John' should exist");

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/search/books?type=a&q={}", author.id)).await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(
        html.contains("Test Book Title"),
        "should show books by this author"
    );
}

/// Search books by series ID (type=s).
#[tokio::test]
async fn search_books_by_series_id() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _lib, _cov) = setup_library().await;

    let ser = series::find_by_name(&pool, "Test Series")
        .await
        .unwrap()
        .expect("'Test Series' should exist");

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/search/books?type=s&q={}", ser.id)).await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(
        html.contains("Test Book Title"),
        "should show books in this series"
    );
}

/// Search books by genre ID (type=g).
#[tokio::test]
async fn search_books_by_genre_id() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _lib, _cov) = setup_library().await;

    // The "detective" genre is used in no_cover.fb2
    let genre = genres::get_by_code(&pool, "detective")
        .await
        .unwrap()
        .expect("'detective' genre should exist");

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/search/books?type=g&q={}", genre.id)).await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(
        html.contains("No Cover Book"),
        "should show 'No Cover Book' under detective genre"
    );
}

/// Browse books by language code and character prefix.
#[tokio::test]
async fn browse_books_by_lang_and_prefix() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _lib, _cov) = setup_library().await;
    let state = test_app_state(pool.clone(), config.clone());

    // lang=2 (Latin) — should show alphabet groups
    let app = test_router(state.clone());
    let resp = get(app, "/web/books?lang=2").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    // Should contain some letter groups (T for "Test Book Title", etc.)
    assert!(html.contains("T"), "should have 'T' letter group");

    // Drill into prefix "T"
    let app2 = test_router(state);
    let resp2 = get(app2, "/web/books?lang=2&chars=T").await;
    assert_eq!(resp2.status(), 200);
    let html2 = body_string(resp2).await;
    assert!(
        html2.contains("Test Book Title") || html2.contains("TE"),
        "should show books or sub-groups starting with T"
    );
}

/// Browse Cyrillic books (lang_code=1).
#[tokio::test]
async fn browse_books_cyrillic() {
    let _lock = SCAN_MUTEX.lock().await;

    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["cyrillic_book.fb2"]);
    scanner::run_scan(&pool, &config, ropds::db::DbBackend::Sqlite)
        .await
        .unwrap();

    let state = test_app_state(pool.clone(), config.clone());

    // lang=1 (Cyrillic) — should show alphabet groups
    let app = test_router(state.clone());
    let resp = get(app, "/web/books?lang=1").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(html.contains("Т"), "should have Cyrillic 'Т' letter group");

    // Drill into prefix
    let app2 = test_router(state);
    let resp2 = get(app2, "/web/books?lang=1&chars=%D0%A2").await;
    assert_eq!(resp2.status(), 200);
    let html2 = body_string(resp2).await;
    assert!(
        html2.contains("Тайна старого дома") || html2.contains("ТА"),
        "should show Cyrillic books or sub-groups starting with Т"
    );
}

/// Browse digit-prefixed books (lang_code=3).
#[tokio::test]
async fn browse_books_digit_prefix() {
    let _lock = SCAN_MUTEX.lock().await;

    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["digit_title.fb2"]);
    scanner::run_scan(&pool, &config, ropds::db::DbBackend::Sqlite)
        .await
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/books?lang=3").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(
        html.contains("4") || html.contains("451 Degree"),
        "should show digit-prefixed books"
    );
}

/// Search Cyrillic book by title substring.
#[tokio::test]
async fn search_cyrillic_book_by_title() {
    let _lock = SCAN_MUTEX.lock().await;

    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["cyrillic_book.fb2"]);
    scanner::run_scan(&pool, &config, ropds::db::DbBackend::Sqlite)
        .await
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(
        app,
        "/web/search/books?type=m&q=%D0%A2%D0%B0%D0%B9%D0%BD%D0%B0",
    )
    .await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(
        html.contains("Тайна старого дома"),
        "should find Cyrillic book by title search"
    );
}

/// Single book lookup by ID (type=i).
#[tokio::test]
async fn search_single_book_by_id() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _lib, _cov) = setup_library().await;

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/search/books?type=i&q={}", book.id)).await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(html.contains("Test Book Title"));
}
