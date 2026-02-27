use ropds::db;
use ropds::db::queries::reading_positions;
use ropds::scanner;

use super::*;

/// Helper: set up a scanned library with a test user and return components.
async fn setup_with_user() -> (
    db::DbPool,
    ropds::config::Config,
    i64,
    String,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2", "test_book.epub"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let user_id = create_test_user(&pool, "reader_user", "password123", false).await;
    let session = session_cookie_value(user_id);

    (pool, config, user_id, session, lib_dir, covers_dir)
}

/// Reader page returns 200 for supported formats.
#[tokio::test]
async fn reader_page_for_supported_format() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _user_id, session, _lib, _cov) = setup_with_user().await;

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = get_with_session(app, &format!("/web/reader/{}", book.id), &session).await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(
        html.contains("data-book-id"),
        "should have book data attributes"
    );
    assert!(
        html.contains("data-format"),
        "should have format data attribute"
    );
}

#[tokio::test]
async fn reader_page_uses_return_query_for_back_button() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _user_id, session, _lib, _cov) = setup_with_user().await;

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = get_with_session(
        app,
        &format!(
            "/web/reader/{}?return={}",
            book.id,
            urlencoding::encode("/web/recent?page=2")
        ),
        &session,
    )
    .await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(
        html.contains("recent?page=2"),
        "reader back button should point to return path"
    );
}

/// Reader page returns 404 for nonexistent book.
#[tokio::test]
async fn reader_page_not_found() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _user_id, session, _lib, _cov) = setup_with_user().await;
    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = get_with_session(app, "/web/reader/99999", &session).await;
    assert_eq!(resp.status(), 404);
}

/// Inline serve returns book bytes with correct content-type.
#[tokio::test]
async fn read_inline_serves_book() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _user_id, session, _lib, _cov) = setup_with_user().await;

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = get_with_session(app, &format!("/web/read/{}", book.id), &session).await;
    assert_eq!(resp.status(), 200);

    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        ct.contains("application/fb2+xml"),
        "should serve fb2 mime type, got {ct}"
    );

    let cd = resp
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        cd.starts_with("inline"),
        "should be inline disposition, got {cd}"
    );
}

/// Save and retrieve reading position via API.
#[tokio::test]
async fn position_save_and_get() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _user_id, session, _lib, _cov) = setup_with_user().await;

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    let csrf = csrf_for_session(&session);
    let state = test_app_state(pool, config);

    // Save position
    let app = test_router(state.clone());
    let resp = post_json(
        app,
        "/web/api/reading-position",
        serde_json::json!({
            "book_id": book.id,
            "position": "epubcfi(/6/4!/4/2/1:0)",
            "progress": 0.42,
            "csrf_token": csrf
        }),
        &session,
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(body["ok"], true);

    // Get position
    let app2 = test_router(state);
    let resp2 = get_with_session(
        app2,
        &format!("/web/api/reading-position/{}", book.id),
        &session,
    )
    .await;
    assert_eq!(resp2.status(), 200);
    let body2: serde_json::Value = serde_json::from_str(&body_string(resp2).await).unwrap();
    assert_eq!(body2["position"], "epubcfi(/6/4!/4/2/1:0)");
    assert!((body2["progress"].as_f64().unwrap() - 0.42).abs() < 0.001);
}

/// Position API requires authentication.
#[tokio::test]
async fn position_api_requires_auth() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _user_id, _session, _lib, _cov) = setup_with_user().await;
    let state = test_app_state(pool, config);

    // GET without session
    let app = test_router(state.clone());
    let resp = get(app, "/web/api/reading-position/1").await;
    // With auth_required=false, the session layer doesn't redirect but the handler returns 401
    assert_eq!(resp.status(), 401);

    // POST without session
    let app2 = test_router(state);
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/web/api/reading-position")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "book_id": 1,
                "position": "test",
                "progress": 0.5,
                "csrf_token": "invalid"
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp2 = app2.oneshot(req).await.unwrap();
    assert_eq!(resp2.status(), 401);
}

/// Position save rejects invalid CSRF token.
#[tokio::test]
async fn position_save_rejects_bad_csrf() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _user_id, session, _lib, _cov) = setup_with_user().await;

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = post_json(
        app,
        "/web/api/reading-position",
        serde_json::json!({
            "book_id": book.id,
            "position": "test",
            "progress": 0.5,
            "csrf_token": "wrong-token"
        }),
        &session,
    )
    .await;
    assert_eq!(resp.status(), 403);
}

/// Reading history API returns recent reads.
#[tokio::test]
async fn reading_history_api() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, user_id, session, _lib, _cov) = setup_with_user().await;

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    // Save a position directly
    reading_positions::save_position(&pool, user_id, book.id, "page:5", 0.25, 100)
        .await
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = get_with_session(app, "/web/api/reading-history", &session).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let items = body.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["book_id"], book.id);
    assert!((items[0]["progress"].as_f64().unwrap() - 0.25).abs() < 0.001);
}

/// Reader page is disabled when config.reader.enable = false.
#[tokio::test]
async fn reader_disabled_returns_not_found() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();

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
auth_required = false

[scanner]

[reader]
enable = false
"#,
        lib_dir.path(),
        covers_dir.path()
    ))
    .unwrap();

    copy_test_files(lib_dir.path(), &["test_book.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let user_id = create_test_user(&pool, "reader_off", "password123", false).await;
    let session = session_cookie_value(user_id);

    let book = ropds::db::queries::books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = get_with_session(app, &format!("/web/reader/{}", book.id), &session).await;
    assert_eq!(resp.status(), 404, "reader should be disabled");
}
