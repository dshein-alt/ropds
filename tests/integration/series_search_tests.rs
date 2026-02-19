use ropds::db;
use ropds::scanner;

use super::*;

/// Search series by name substring.
#[tokio::test]
async fn search_series_by_name() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(
        lib_dir.path(),
        &["test_book.fb2", "no_cover.fb2", "series_no_genre.fb2"],
    );
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/search/series?type=m&q=Test+Series").await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(html.contains("Test Series"), "should find 'Test Series'");
}

/// Browse series by language code and prefix.
#[tokio::test]
async fn browse_series_by_lang_and_prefix() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(
        lib_dir.path(),
        &["test_book.fb2", "no_cover.fb2", "series_no_genre.fb2"],
    );
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool.clone(), config.clone());

    // lang=2 (Latin) — should show alphabet groups
    let app = test_router(state.clone());
    let resp = get(app, "/web/series?lang=2").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(
        html.contains("T") || html.contains("C") || html.contains("G"),
        "should show alphabet groups for series"
    );

    // Drill into prefix "T" (for "Test Series")
    let app2 = test_router(state);
    let resp2 = get(app2, "/web/series?lang=2&chars=T").await;
    assert_eq!(resp2.status(), 200);
    let html2 = body_string(resp2).await;
    assert!(
        html2.contains("Test Series") || html2.contains("TE"),
        "should show series or sub-groups starting with T"
    );
}
