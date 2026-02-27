use axum::body::Body;
use base64::Engine;
use ropds::db;
use ropds::scanner;
use serde_json::Value;
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
async fn opds_v2_root_returns_json_navigation() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = get(app, "/opds/v2/?lang=en").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/opds+json; charset=utf-8")
    );

    let body = body_string(resp).await;
    let doc: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(doc["metadata"]["title"], "ROPDS");
    let nav = doc["navigation"].as_array().unwrap();
    assert!(
        nav.iter()
            .any(|item| item["href"] == "/opds/v2/catalogs/?lang=en"),
        "root navigation should include OPDS 2.0 catalogs endpoint"
    );
    assert!(
        nav.iter()
            .any(|item| item["href"] == "/opds/v2/authors/?lang=en"),
        "root navigation should include OPDS 2.0 authors endpoint"
    );
}

#[tokio::test]
async fn opds_v2_recent_feed_returns_publications() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["title_only.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/opds/v2/recent/?lang=en").await;
    assert_eq!(resp.status(), 200);
    let body = body_string(resp).await;
    let doc: Value = serde_json::from_str(&body).unwrap();

    let pubs = doc["publications"].as_array().unwrap();
    assert!(!pubs.is_empty(), "recent feed should include publications");
    assert!(
        pubs.iter()
            .any(|p| p["metadata"]["title"] == "Lonely Title Book"),
        "recent feed should include scanned test book"
    );
}

#[tokio::test]
async fn opds_v2_search_books_includes_acquisition_links() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["title_only.fb2", "test_book.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/opds/v2/search/books/m/Lonely/").await;
    assert_eq!(resp.status(), 200);
    let body = body_string(resp).await;
    let doc: Value = serde_json::from_str(&body).unwrap();
    let pubs = doc["publications"].as_array().unwrap();
    assert!(
        !pubs.is_empty(),
        "search should return matching publications"
    );

    let has_acquisition = pubs.iter().any(|p| {
        p["links"].as_array().is_some_and(|links| {
            links
                .iter()
                .any(|l| l["rel"] == "http://opds-spec.org/acquisition/open-access")
        })
    });
    assert!(
        has_acquisition,
        "search result should include OPDS acquisition link"
    );
}

#[tokio::test]
async fn opds_v2_authors_root_returns_alphabet_navigation() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = get(app, "/opds/v2/authors/?lang=en").await;
    assert_eq!(resp.status(), 200);
    let body = body_string(resp).await;
    let doc: Value = serde_json::from_str(&body).unwrap();

    let nav = doc["navigation"].as_array().unwrap();
    assert!(
        nav.iter()
            .any(|item| item["href"] == "/opds/v2/authors/1/?lang=en"),
        "authors root should include cyrillic bucket"
    );
}

#[tokio::test]
async fn opds_v2_genres_and_language_facets_routes_work() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(lib_dir.path(), &["test_book.fb2"]);
    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state.clone());
    let resp = get(app, "/opds/v2/genres/?lang=en").await;
    assert_eq!(resp.status(), 200);
    let body = body_string(resp).await;
    let doc: Value = serde_json::from_str(&body).unwrap();
    assert!(
        !doc["navigation"].as_array().unwrap().is_empty(),
        "genres root should return at least one section"
    );

    let app2 = test_router(state);
    let resp2 = get(app2, "/opds/v2/facets/languages/?lang=en").await;
    assert_eq!(resp2.status(), 200);
    let body2 = body_string(resp2).await;
    let doc2: Value = serde_json::from_str(&body2).unwrap();
    assert!(
        doc2["navigation"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["href"] == "/opds/v2/lang/en/"),
        "language facets should include locale path links"
    );
}

#[tokio::test]
async fn opds_v2_bookshelf_requires_auth_when_enabled() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let mut config = test_config(lib_dir.path(), covers_dir.path());
    config.opds.auth_required = true;

    create_test_user(&pool, "opds2-auth", "password123", false).await;

    let state = test_app_state(pool, config);
    let app = test_router(state.clone());
    let resp = get(app, "/opds/v2/bookshelf/").await;
    assert_eq!(resp.status(), 401);

    let app2 = test_router(state);
    let req = axum::http::Request::builder()
        .uri("/opds/v2/bookshelf/?lang=en")
        .header("authorization", basic_auth("opds2-auth", "password123"))
        .body(Body::empty())
        .unwrap();
    let resp2 = app2.oneshot(req).await.unwrap();
    assert_eq!(resp2.status(), 200);
    assert_eq!(
        resp2
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/opds+json; charset=utf-8")
    );
}
