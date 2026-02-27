use axum::body::Body;
use base64::Engine;
use ropds::db;
use ropds::db::queries::books;
use ropds::scanner;
use tower::ServiceExt;

use super::*;

fn basic_auth(username: &str, password: &str) -> String {
    let raw = format!("{username}:{password}");
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
    )
}

#[tokio::test]
async fn opds_requires_basic_auth_when_enabled() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let mut config = test_config(lib_dir.path(), covers_dir.path());
    config.opds.auth_required = true;

    create_test_user(&pool, "opds-auth", "password123", false).await;

    let state = test_app_state(pool, config);
    let app = test_router(state.clone());
    let resp = get(app, "/opds/books/").await;
    assert_eq!(resp.status(), 401);
    assert_eq!(
        resp.headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok()),
        Some("Basic realm=\"OPDS\"")
    );

    let app2 = test_router(state);
    let req = axum::http::Request::builder()
        .uri("/opds/books/")
        .header("authorization", basic_auth("opds-auth", "password123"))
        .body(Body::empty())
        .unwrap();
    let resp2 = app2.oneshot(req).await.unwrap();
    assert_eq!(resp2.status(), 200);
}

#[tokio::test]
async fn opds_recent_feed_has_prev_next_navigation() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let mut config = test_config(lib_dir.path(), covers_dir.path());
    config.opds.max_items = 1;

    copy_test_files(lib_dir.path(), &["test_book.fb2", "title_only.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let old_book = books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();
    let new_book = books::find_by_path_and_filename(&pool, "", "title_only.fb2")
        .await
        .unwrap()
        .unwrap();

    let sql = pool.sql("UPDATE books SET reg_date = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind("2024-01-01 00:00:00")
        .bind(old_book.id)
        .execute(pool.inner())
        .await
        .unwrap();
    sqlx::query(&sql)
        .bind("2024-05-01 00:00:00")
        .bind(new_book.id)
        .execute(pool.inner())
        .await
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state.clone());
    let resp = get(app, "/opds/recent/1/?lang=en").await;
    assert_eq!(resp.status(), 200);
    let xml = body_string(resp).await;
    assert!(xml.contains("/opds/recent/2/?lang=en"));

    let app2 = test_router(state);
    let resp2 = get(app2, "/opds/recent/2/?lang=en").await;
    assert_eq!(resp2.status(), 200);
    let xml2 = body_string(resp2).await;
    assert!(xml2.contains("/opds/recent/1/?lang=en"));
}

#[tokio::test]
async fn opds_book_search_returns_matching_entries() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["title_only.fb2", "test_book.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/opds/search/books/m/Lonely/").await;
    assert_eq!(resp.status(), 200);
    let xml = body_string(resp).await;
    assert!(xml.contains("<feed"), "should return an OPDS feed");
    assert!(
        xml.contains("Lonely Title Book"),
        "should include matched title"
    );
    assert!(
        xml.contains("/opds/download/") || xml.contains("rel=\"http://opds-spec.org/acquisition\""),
        "search results should include acquisition link"
    );
}
