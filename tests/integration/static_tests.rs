use http_body_util::BodyExt;
use ropds::db;
use tower::ServiceExt;

use super::*;

#[tokio::test]
async fn serves_static_javascript() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let response = get(app, "/static/js/ropds.js").await;
    assert_eq!(response.status(), 200);

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("javascript"),
        "unexpected content-type: {content_type}"
    );

    let content_length = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .expect("content-length header should be set");

    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        content_length,
        body.len(),
        "content-length should match body"
    );
}

#[tokio::test]
async fn blocks_static_path_traversal() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let response = get(app, "/static/../Cargo.toml").await;
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn serves_compressed_static_javascript_when_requested() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let request = axum::http::Request::builder()
        .uri("/static/js/ropds.js")
        .header("accept-encoding", "gzip")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-encoding")
            .and_then(|value| value.to_str().ok()),
        Some("gzip")
    );
}
