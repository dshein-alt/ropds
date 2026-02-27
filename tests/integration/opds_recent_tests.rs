use ropds::db;
use ropds::scanner;

use super::*;

#[tokio::test]
async fn opds_recent_feed_returns_recent_books() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["title_only.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/opds/recent/?lang=en").await;
    assert_eq!(resp.status(), 200);

    let xml = body_string(resp).await;
    assert!(xml.contains("<feed"), "should return an OPDS feed");
    assert!(
        xml.contains("/opds/recent/1/?lang=en"),
        "self link should point to recent feed page"
    );
    assert!(
        xml.contains("Lonely Title Book"),
        "recent feed should include newly scanned books"
    );
}
