use ropds::db;
use ropds::db::queries::bookshelf;
use ropds::scanner;

use super::*;

/// Helper: set up a scanned library with a test user and return (pool, config, user_id, session).
async fn setup_with_user() -> (
    db::DbPool,
    ropds::config::Config,
    i64,
    String,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2", "test_book.epub"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let user_id = create_test_user(&pool, "testuser", "password123", false).await;
    let session = session_cookie_value(user_id);

    (pool, config, user_id, session, lib_dir, covers_dir)
}

/// Add a book to the bookshelf via toggle, then verify it appears.
#[tokio::test]
async fn bookshelf_add_and_list() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, user_id, session, _lib, _cov) = setup_with_user().await;

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    let csrf = csrf_for_session(&session);

    let state = test_app_state(pool.clone(), config);

    // Toggle book onto shelf
    let app = test_router(state.clone());
    let body = format!("book_id={}&csrf_token={}", book.id, csrf);
    let resp = post_form(app, "/web/bookshelf/toggle", &body, &session).await;
    // Should redirect (302) or return JSON on AJAX
    let status = resp.status().as_u16();
    assert!(
        status == 200 || status == 302 || status == 303,
        "toggle should succeed, got {status}"
    );

    // Verify via DB
    assert!(
        bookshelf::is_on_shelf(&pool, user_id, book.id)
            .await
            .unwrap(),
        "book should be on shelf"
    );

    // Verify via web page
    let app2 = test_router(state);
    let resp2 = get_with_session(app2, "/web/bookshelf", &session).await;
    assert_eq!(resp2.status(), 200);
    let html = body_string(resp2).await;
    assert!(
        html.contains("Test Book Title"),
        "bookshelf page should show the book"
    );
}

/// Toggle a book off the bookshelf.
#[tokio::test]
async fn bookshelf_remove() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, user_id, session, _lib, _cov) = setup_with_user().await;

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    // Add to shelf directly via DB
    bookshelf::upsert(&pool, user_id, book.id).await.unwrap();
    assert!(
        bookshelf::is_on_shelf(&pool, user_id, book.id)
            .await
            .unwrap()
    );

    // Toggle off via web
    let csrf = csrf_for_session(&session);
    let state = test_app_state(pool.clone(), config);
    let app = test_router(state);
    let body = format!("book_id={}&csrf_token={}", book.id, csrf);
    let resp = post_form(app, "/web/bookshelf/toggle", &body, &session).await;
    let status = resp.status().as_u16();
    assert!(status == 200 || status == 302 || status == 303);

    // Verify removed
    assert!(
        !bookshelf::is_on_shelf(&pool, user_id, book.id)
            .await
            .unwrap(),
        "book should be removed from shelf"
    );
}

/// Clear all books from the bookshelf.
#[tokio::test]
async fn bookshelf_clear_all() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, user_id, session, _lib, _cov) = setup_with_user().await;

    // Add both books to shelf
    let book1 = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();
    let book2 = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.epub")
        .await
        .unwrap()
        .unwrap();
    bookshelf::upsert(&pool, user_id, book1.id).await.unwrap();
    bookshelf::upsert(&pool, user_id, book2.id).await.unwrap();
    assert_eq!(bookshelf::count_by_user(&pool, user_id).await.unwrap(), 2);

    // Clear via web
    let csrf = csrf_for_session(&session);
    let state = test_app_state(pool.clone(), config);
    let app = test_router(state);
    let body = format!("csrf_token={csrf}");
    let resp = post_form(app, "/web/bookshelf/clear", &body, &session).await;
    let status = resp.status().as_u16();
    assert!(status == 200 || status == 302 || status == 303);

    assert_eq!(
        bookshelf::count_by_user(&pool, user_id).await.unwrap(),
        0,
        "bookshelf should be empty after clear"
    );
}

/// Bookshelf supports sorting by title and date.
#[tokio::test]
async fn bookshelf_sorting() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, user_id, session, _lib, _cov) = setup_with_user().await;

    let book1 = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();
    let book2 = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.epub")
        .await
        .unwrap()
        .unwrap();
    bookshelf::upsert(&pool, user_id, book1.id).await.unwrap();
    bookshelf::upsert(&pool, user_id, book2.id).await.unwrap();

    let state = test_app_state(pool, config);

    // Sort by title ascending
    let app = test_router(state.clone());
    let resp = get_with_session(app, "/web/bookshelf?sort=title&dir=asc", &session).await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    // Both books should appear
    assert!(html.contains("Test Book Title") || html.contains("EPUB Test Book"));

    // Sort by date descending
    let app2 = test_router(state);
    let resp2 = get_with_session(app2, "/web/bookshelf?sort=date&dir=desc", &session).await;
    assert_eq!(resp2.status(), 200);
}

/// Bookshelf requires authentication when auth_required is true.
#[tokio::test]
async fn bookshelf_requires_auth() {
    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();

    // Config with auth REQUIRED
    let config: ropds::config::Config = toml::from_str(&format!(
        r#"
[server]
session_secret = "test-secret-key-for-integration-tests"

[library]
root_path = {:?}
covers_path = {:?}

[database]
url = "sqlite::memory:"

[opds]
auth_required = true

[scanner]
"#,
        lib_dir.path(),
        covers_dir.path()
    ))
    .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    // No session cookie — should redirect to login
    let resp = get(app, "/web/bookshelf").await;
    let status = resp.status().as_u16();
    assert!(
        status == 302 || status == 303,
        "should redirect unauthenticated user, got {status}"
    );
}
