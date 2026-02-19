use ropds::db;
use ropds::scanner;

use super::*;

/// Search authors by name substring.
#[tokio::test]
async fn search_authors_by_name() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2", "no_cover.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/search/authors?type=m&q=Doe").await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(html.contains("Doe"), "should find author matching 'Doe'");
}

/// Browse authors by language code and prefix.
#[tokio::test]
async fn browse_authors_by_lang_and_prefix() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(
        lib_dir.path(),
        &["test_book.fb2", "no_cover.fb2", "author_no_genre.fb2"],
    );
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool.clone(), config.clone());

    // lang=2 (Latin) — should show alphabet groups
    let app = test_router(state.clone());
    let resp = get(app, "/web/authors?lang=2").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    // Should contain letter groups for authors
    assert!(
        html.contains("D") || html.contains("S") || html.contains("L"),
        "should show alphabet groups for Latin authors"
    );

    // Drill into prefix "D" (for Doe)
    let app2 = test_router(state);
    let resp2 = get(app2, "/web/authors?lang=2&chars=D").await;
    assert_eq!(resp2.status(), 200);
    let html2 = body_string(resp2).await;
    assert!(
        html2.contains("Doe") || html2.contains("DO"),
        "should show authors or sub-groups starting with D"
    );
}

/// Search Cyrillic author by name.
#[tokio::test]
async fn search_cyrillic_author() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["cyrillic_book.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    // Search by Cyrillic name substring "Иванов"
    let resp = get(
        app,
        "/web/search/authors?type=m&q=%D0%98%D0%B2%D0%B0%D0%BD%D0%BE%D0%B2",
    )
    .await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(
        html.contains("Иванов"),
        "should find Cyrillic author 'Иванов'"
    );
}

/// Browse Cyrillic authors (lang=1).
#[tokio::test]
async fn browse_cyrillic_authors() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["cyrillic_book.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/authors?lang=1").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(
        html.contains("И"),
        "should show Cyrillic 'И' letter group for Иванов"
    );
}
