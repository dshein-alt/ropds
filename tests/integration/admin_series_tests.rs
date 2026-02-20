use ropds::db;
use ropds::db::DbPool;
use ropds::db::queries::series;

use super::*;

async fn insert_test_book(pool: &DbPool, title: &str) -> i64 {
    let cat_path = format!("/admin-it-{title}");
    let sql = pool.sql("INSERT INTO catalogs (path, cat_name) VALUES (?, 'admin-it')");
    sqlx::query(&sql)
        .bind(&cat_path)
        .execute(pool.inner())
        .await
        .unwrap();

    let sql = pool.sql("SELECT id FROM catalogs WHERE path = ?");
    let (catalog_id,): (i64,) = sqlx::query_as(&sql)
        .bind(&cat_path)
        .fetch_one(pool.inner())
        .await
        .unwrap();

    let search_title = title.to_uppercase();
    let sql = pool.sql(
        "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
         lang, lang_code, size, avail, cat_type, cover, cover_type) \
         VALUES (?, ?, '/admin-it', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
    );
    sqlx::query(&sql)
        .bind(catalog_id)
        .bind(format!("{title}.fb2"))
        .bind(title)
        .bind(search_title)
        .execute(pool.inner())
        .await
        .unwrap();

    let sql = pool.sql("SELECT id FROM books WHERE catalog_id = ? AND title = ?");
    let (book_id,): (i64,) = sqlx::query_as(&sql)
        .bind(catalog_id)
        .bind(title)
        .fetch_one(pool.inner())
        .await
        .unwrap();
    book_id
}

#[tokio::test]
async fn admin_series_endpoints_update_search_and_remove() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let user_id = create_test_user(&pool, "admin-series", "password123", true).await;
    let session = session_cookie_value(user_id);
    let csrf = csrf_for_session(&session);

    let book_id = insert_test_book(&pool, "Admin API Series").await;

    let state = test_app_state(pool.clone(), config);
    let app = test_router(state.clone());
    let resp = post_json(
        app,
        "/web/admin/book-series",
        serde_json::json!({
            "book_id": book_id,
            "series_name": "Foundation",
            "series_no": 4,
            "csrf_token": csrf,
        }),
        &session,
    )
    .await;
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["series"][0]["ser_name"], "Foundation");
    assert_eq!(json["series"][0]["ser_no"], 4);

    let linked = series::get_for_book(&pool, book_id).await.unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].0.ser_name, "Foundation");
    assert_eq!(linked[0].1, 4);

    let app2 = test_router(state.clone());
    let resp2 = get_with_session(app2, "/web/admin/series-search?q=fo", &session).await;
    assert_eq!(resp2.status(), 200);
    let json2: serde_json::Value = serde_json::from_str(&body_string(resp2).await).unwrap();
    assert_eq!(json2["ok"], true);
    let results = json2["series"].as_array().unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().any(|s| s["ser_name"] == "Foundation"));

    let app3 = test_router(state);
    let resp3 = post_json(
        app3,
        "/web/admin/book-series",
        serde_json::json!({
            "book_id": book_id,
            "series_name": "",
            "series_no": 0,
            "csrf_token": csrf_for_session(&session),
        }),
        &session,
    )
    .await;
    assert_eq!(resp3.status(), 200);
    let json3: serde_json::Value = serde_json::from_str(&body_string(resp3).await).unwrap();
    assert_eq!(json3["ok"], true);
    assert_eq!(json3["series"].as_array().unwrap().len(), 0);

    let linked = series::get_for_book(&pool, book_id).await.unwrap();
    assert!(linked.is_empty());
    assert!(
        series::find_by_name(&pool, "Foundation")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn admin_series_endpoints_forbidden_for_non_superuser() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());
    let book_id = insert_test_book(&pool, "No Access Series").await;

    let user_id = create_test_user(&pool, "user-series", "password123", false).await;
    let session = session_cookie_value(user_id);

    let state = test_app_state(pool, config);
    let app = test_router(state.clone());
    let resp = get_with_session(app, "/web/admin/series-search?q=fo", &session).await;
    assert_eq!(resp.status(), 403);

    let app2 = test_router(state);
    let resp2 = post_json(
        app2,
        "/web/admin/book-series",
        serde_json::json!({
            "book_id": book_id,
            "series_name": "Denied",
            "series_no": 1,
            "csrf_token": csrf_for_session(&session),
        }),
        &session,
    )
    .await;
    assert_eq!(resp2.status(), 403);
}
