use ropds::db;
use ropds::scanner;

use super::*;

/// The catalog page shows root-level catalog entries after a scan.
#[tokio::test]
async fn catalog_page_lists_root_catalogs() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    // Create a subdirectory with books
    copy_test_files_to_subdir(lib_dir.path(), "fiction", &["test_book.fb2"]);
    copy_test_files_to_subdir(lib_dir.path(), "science", &["test_book.epub"]);

    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/catalogs").await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(html.contains("fiction"), "should list 'fiction' catalog");
    assert!(html.contains("science"), "should list 'science' catalog");
}

/// Drilling into a catalog by ID shows books inside it.
#[tokio::test]
async fn catalog_drill_down_shows_books() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files_to_subdir(lib_dir.path(), "mybooks", &["test_book.fb2"]);

    scanner::run_scan(&pool, &config).await.unwrap();

    // Find the catalog ID for "mybooks"
    let cat = ropds::db::queries::catalogs::find_by_path(&pool, "mybooks")
        .await
        .unwrap()
        .expect("mybooks catalog should exist");

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/catalogs?cat_id={}", cat.id)).await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(
        html.contains("Test Book Title"),
        "should show the book title in catalog view"
    );
}
