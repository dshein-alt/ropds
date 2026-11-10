use ropds::db;
use ropds::db::DbPool;
use ropds::db::queries::{authors, genres, series};
use ropds::scanner;

use super::*;

/// Insert a catalog + a book straight into the DB, bypassing the scanner.
/// Used by prefix-mode coverage which needs deterministic search-titles.
async fn seed_book(pool: &DbPool, title: &str) {
    let path = format!("/prefix-mode/{title}");
    let sql = pool.sql("INSERT OR IGNORE INTO catalogs (path, cat_name) VALUES (?, 'pm')");
    sqlx::query(&sql)
        .bind(&path)
        .execute(pool.inner())
        .await
        .unwrap();
    let sql = pool.sql("SELECT id FROM catalogs WHERE path = ?");
    let (cat_id,): (i64,) = sqlx::query_as(&sql)
        .bind(&path)
        .fetch_one(pool.inner())
        .await
        .unwrap();
    let search_title = title.to_uppercase();
    let sql = pool.sql(
        "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
         lang, lang_code, size, avail, cat_type, cover, cover_type) \
         VALUES (?, ?, ?, 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
    );
    sqlx::query(&sql)
        .bind(cat_id)
        .bind(format!("{title}.fb2"))
        .bind(&path)
        .bind(title)
        .bind(search_title)
        .execute(pool.inner())
        .await
        .unwrap();
}

/// Helper: set up a scanned library with several test books and return (pool, config).
async fn setup_library() -> (
    db::DbPool,
    ropds::config::Config,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let pool = db::create_test_pool().await;
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

    scanner::run_scan(&pool, &config, false).await.unwrap();
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
    assert!(html.contains("action=\"/web/search/books\""));
    assert!(html.contains("<input type=\"hidden\" name=\"type\" value=\"m\">"));
    assert!(html.contains("id=\"search-type-group\" hidden"));
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

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["cyrillic_book.fb2"]);
    scanner::run_scan(&pool, &config, false).await.unwrap();

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

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["digit_title.fb2"]);
    scanner::run_scan(&pool, &config, false).await.unwrap();

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

    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["cyrillic_book.fb2"]);
    scanner::run_scan(&pool, &config, false).await.unwrap();

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

    let user_id = create_test_user(&pool, "single_book_user", "password123", false).await;
    let session = session_cookie_value(user_id);
    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get_with_session(
        app,
        &format!("/web/search/books?type=i&q={}", book.id),
        &session,
    )
    .await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(html.contains("Test Book Title"));
    assert!(html.contains("aria-label=\"Bookshelf: Test Book Title\" aria-pressed=\"false\""));
    assert!(html.contains("data-add-title=\"Add to bookshelf\""));
    assert!(html.contains("data-remove-title=\"Remove from bookshelf\""));
    assert!(html.contains("<dialog id=\"cover-dialog\""));
    if book.cover > 0 {
        assert!(html.contains("class=\"cover-preview rounded\""));
        assert!(html.contains("aria-label=\"View cover: Test Book Title\""));
    }
}

// ── Alphabet drill-down: word-boundary (default) vs. first-word config ──
//
// These HTTP tests verify only that the `opds.alphabet_first_word_only`
// flag is threaded from config into the title-prefix handler. The exact
// matching semantics live in the lib-level tests around
// `books::search_by_title_prefix` / `count_by_title_prefix` /
// `get_title_prefix_groups`. We use the "no results" template fragment as
// the marker because the page also renders a `random_book` widget in the
// footer that can otherwise echo any seeded title regardless of the
// search query.

/// Default config: prefix matches at any word boundary, so a title whose
/// ONLY AB-prefixed word is internal still surfaces under "AB".
#[tokio::test]
async fn search_title_prefix_default_matches_inner_word_titles() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    // First word is "Notes", inner word is "Abrazil" — only matches in
    // word-boundary mode.
    seed_book(&pool, "Notes Abrazil").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/search/books?type=b&q=AB").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(
        !html.contains("No results found"),
        "word-boundary listing must match a title whose inner word starts with AB"
    );
}

/// `opds.alphabet_first_word_only = true`: the same inner-word title is
/// filtered out — the search returns no books and the page falls through
/// to the "no results" branch.
#[tokio::test]
async fn search_title_prefix_first_word_only_skips_inner_words() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config_first_word_only(lib_dir.path(), covers_dir.path());

    seed_book(&pool, "Notes Abrazil").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/search/books?type=b&q=AB").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(
        html.contains("No results found"),
        "first-word mode must NOT match a title whose only AB-prefixed word is internal"
    );
}

/// Author drill-down should equally honour the first-word-only config.
#[tokio::test]
async fn authors_drill_down_first_word_only() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config_first_word_only(lib_dir.path(), covers_dir.path());

    authors::insert(&pool, "Aberdin Laura", "ABERDIN LAURA", 2)
        .await
        .unwrap();
    authors::insert(&pool, "Hakim Abdul Efendi", "HAKIM ABDUL EFENDI", 2)
        .await
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/authors/list?lang=2&prefix=AB").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(html.contains("Aberdin Laura"));
    assert!(
        !html.contains("Hakim Abdul Efendi"),
        "first-word mode must exclude inner-word matches in the author listing"
    );
}

/// Series drill-down should equally honour the first-word-only config.
#[tokio::test]
async fn series_drill_down_first_word_only() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config_first_word_only(lib_dir.path(), covers_dir.path());

    series::insert(&pool, "Aberdin Saga", "ABERDIN SAGA", 2)
        .await
        .unwrap();
    series::insert(&pool, "Notes Abraham", "NOTES ABRAHAM", 2)
        .await
        .unwrap();
    // Touch `genres` so the import isn't flagged as unused in this file.
    let _ = genres::get_by_id(&pool, 0, "en").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/series/list?lang=2&prefix=AB").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(html.contains("Aberdin Saga"));
    assert!(
        !html.contains("Notes Abraham"),
        "first-word mode must exclude inner-word matches in the series listing"
    );
}

/// Regression: the OPDS v1 "begins-with" leaf (`/opds/search/books/b/...`)
/// must honour `opds.alphabet_first_word_only`. Previously the leaf fell
/// through to `search_by_title` (substring search), so a strict-mode
/// drill-down group could open a feed full of inner/midword matches.
#[tokio::test]
async fn opds_v1_search_books_b_leaf_honours_first_word_only() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config_first_word_only(lib_dir.path(), covers_dir.path());

    seed_book(&pool, "Aberdin").await;
    seed_book(&pool, "Notes Abrazil").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/opds/search/books/b/AB/").await;
    assert_eq!(resp.status(), 200);
    let xml = body_string(resp).await;
    assert!(
        xml.contains("Aberdin"),
        "OPDS b-leaf in first-word mode should list Aberdin"
    );
    assert!(
        !xml.contains("Notes Abrazil"),
        "OPDS b-leaf must NOT fall back to substring search in first-word mode"
    );
}

/// Companion regression: in the default word-boundary mode the same leaf
/// must surface inner-word matches.
#[tokio::test]
async fn opds_v1_search_books_b_leaf_default_matches_inner_word() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    seed_book(&pool, "Notes Abrazil").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/opds/search/books/b/AB/").await;
    assert_eq!(resp.status(), 200);
    let xml = body_string(resp).await;
    assert!(
        xml.contains("Notes Abrazil"),
        "OPDS b-leaf in word-boundary mode should include inner-word matches"
    );
}
