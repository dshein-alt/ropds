use ropds::db;
use ropds::db::DbPool;

use super::*;

async fn insert_test_book(pool: &DbPool, title: &str) -> i64 {
    let cat_path = format!("/admin-it-title-{title}");
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
async fn admin_user_endpoints_create_toggle_delete() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let super_id = create_test_user(&pool, "admin-users", "password123", true).await;
    let session = session_cookie_value(super_id);
    let csrf = csrf_for_session(&session);

    let state = test_app_state(pool.clone(), config);
    let app = test_router(state.clone());
    let resp = post_form(
        app,
        "/web/admin/users/create",
        &format!(
            "username=managed-user&password=password123&display_name=Managed+User&csrf_token={csrf}"
        ),
        &session,
    )
    .await;
    assert_eq!(resp.status(), 303);

    let sql = pool.sql("SELECT id, allow_upload FROM users WHERE username = ?");
    let (managed_id, allow_upload): (i64, i32) = sqlx::query_as(&sql)
        .bind("managed-user")
        .fetch_one(pool.inner())
        .await
        .unwrap();
    assert_eq!(allow_upload, 0);

    let app2 = test_router(state.clone());
    let resp2 = post_form(
        app2,
        &format!("/web/admin/users/{managed_id}/upload"),
        &format!("allow_upload=on&csrf_token={csrf}"),
        &session,
    )
    .await;
    assert_eq!(resp2.status(), 303);

    let sql = pool.sql("SELECT allow_upload FROM users WHERE id = ?");
    let (allow_upload,): (i32,) = sqlx::query_as(&sql)
        .bind(managed_id)
        .fetch_one(pool.inner())
        .await
        .unwrap();
    assert_eq!(allow_upload, 1);

    let app3 = test_router(state);
    let resp3 = post_form(
        app3,
        &format!("/web/admin/users/{managed_id}/delete"),
        &format!("csrf_token={csrf}"),
        &session,
    )
    .await;
    assert_eq!(resp3.status(), 303);

    let sql = pool.sql("SELECT COUNT(*) FROM users WHERE id = ?");
    let (count,): (i64,) = sqlx::query_as(&sql)
        .bind(managed_id)
        .fetch_one(pool.inner())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn admin_book_title_endpoint_updates_and_validates() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let super_id = create_test_user(&pool, "admin-title", "password123", true).await;
    let session = session_cookie_value(super_id);
    let csrf = csrf_for_session(&session);

    let book_id = insert_test_book(&pool, "Old Title").await;

    let state = test_app_state(pool.clone(), config);
    let app = test_router(state.clone());
    let resp = post_json(
        app,
        "/web/admin/book-title",
        serde_json::json!({
            "book_id": book_id,
            "title": "Updated Title",
            "csrf_token": csrf,
        }),
        &session,
    )
    .await;
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["title"], "Updated Title");

    let updated = ropds::db::queries::books::get_by_id(&pool, book_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.title, "Updated Title");
    assert_eq!(updated.search_title, "UPDATED TITLE");

    let app2 = test_router(state);
    let resp2 = post_json(
        app2,
        "/web/admin/book-title",
        serde_json::json!({
            "book_id": book_id,
            "title": "   ",
            "csrf_token": csrf_for_session(&session),
        }),
        &session,
    )
    .await;
    assert_eq!(resp2.status(), 400);
    let json2: serde_json::Value = serde_json::from_str(&body_string(resp2).await).unwrap();
    assert_eq!(json2["ok"], false);
    assert_eq!(json2["error"], "title_empty");
}
