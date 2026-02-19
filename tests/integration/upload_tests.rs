use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

use ropds::db;
use ropds::db::queries::{authors, books};

use super::*;

/// Build a multipart/form-data body with a file field and a csrf_token field.
fn build_multipart_body(csrf_token: &str, filename: &str, file_data: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----TestBoundary12345";
    let mut body = Vec::new();

    // csrf_token field
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"csrf_token\"\r\n\r\n");
    body.extend_from_slice(csrf_token.as_bytes());
    body.extend_from_slice(b"\r\n");

    // file field
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file_data);
    body.extend_from_slice(b"\r\n");

    // closing boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let content_type = format!("multipart/form-data; boundary={boundary}");
    (content_type, body)
}

/// Upload a test FB2 file, then publish it, verifying the full flow.
#[tokio::test]
async fn upload_file_and_publish() {
    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let upload_dir = tempfile::tempdir().unwrap();
    let config = test_config_with_upload(lib_dir.path(), covers_dir.path(), upload_dir.path());

    let user_id = create_test_user(&pool, "uploader", "password123", true).await;
    let session = session_cookie_value(user_id);
    let csrf = csrf_for_session(&session);

    let state = test_app_state(pool.clone(), config);

    // Read the test FB2 file
    let file_data = std::fs::read(test_data_dir().join("test_book.fb2")).unwrap();

    // Step 1: Upload file via multipart POST
    let (content_type, body) = build_multipart_body(&csrf, "test_book.fb2", &file_data);
    let app = test_router(state.clone());
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/web/upload/file")
        .header("content-type", &content_type)
        .header("cookie", format!("session={session}"))
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 200, "upload should succeed");
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["success"], true);
    let token = json["token"].as_str().expect("should have token");
    assert_eq!(json["meta"]["title"], "Test Book Title");
    assert_eq!(json["meta"]["format"], "fb2");

    // Step 2: Publish with edited metadata
    let publish_body = serde_json::json!({
        "token": token,
        "title": "My Custom Title",
        "authors": ["Custom Author"],
        "genres": [],
        "csrf_token": csrf,
    });
    let app2 = test_router(state);
    let resp2 = post_json(app2, "/web/upload/publish", publish_body, &session).await;
    assert_eq!(resp2.status(), 200, "publish should succeed");
    let body2 = body_string(resp2).await;
    let json2: serde_json::Value = serde_json::from_str(&body2).unwrap();
    assert_eq!(json2["success"], true);
    let book_id = json2["book_id"].as_i64().expect("should return book_id");

    // Step 3: Verify in DB
    let book = books::get_by_id(&pool, book_id).await.unwrap().unwrap();
    assert_eq!(book.title, "My Custom Title");

    let book_authors = authors::get_for_book(&pool, book_id).await.unwrap();
    assert_eq!(book_authors.len(), 1);
    // normalise_author_name reorders "Custom Author" → "Author Custom"
    assert_eq!(book_authors[0].full_name, "Author Custom");

    // Step 4: Verify file exists in library root
    assert!(
        lib_dir.path().join(&book.filename).exists(),
        "published book file should exist in library root"
    );
}

/// Upload with metadata override: verify edited values are stored, not parsed ones.
#[tokio::test]
async fn upload_edit_metadata_on_publish() {
    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let upload_dir = tempfile::tempdir().unwrap();
    let config = test_config_with_upload(lib_dir.path(), covers_dir.path(), upload_dir.path());

    let user_id = create_test_user(&pool, "editor", "password123", true).await;
    let session = session_cookie_value(user_id);
    let csrf = csrf_for_session(&session);

    let state = test_app_state(pool.clone(), config);

    // Upload the full-metadata FB2
    let file_data = std::fs::read(test_data_dir().join("test_book.fb2")).unwrap();
    let (content_type, body) = build_multipart_body(&csrf, "test_book.fb2", &file_data);
    let app = test_router(state.clone());
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/web/upload/file")
        .header("content-type", &content_type)
        .header("cookie", format!("session={session}"))
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let token = json["token"].as_str().unwrap();

    // Publish with completely different title and authors
    let publish_body = serde_json::json!({
        "token": token,
        "title": "Overridden Title",
        "authors": ["New Author One", "New Author Two"],
        "genres": [],
        "csrf_token": csrf,
    });
    let app2 = test_router(state);
    let resp2 = post_json(app2, "/web/upload/publish", publish_body, &session).await;
    assert_eq!(resp2.status(), 200);
    let json2: serde_json::Value = serde_json::from_str(&body_string(resp2).await).unwrap();
    let book_id = json2["book_id"].as_i64().unwrap();

    let book = books::get_by_id(&pool, book_id).await.unwrap().unwrap();
    assert_eq!(book.title, "Overridden Title");

    let book_authors = authors::get_for_book(&pool, book_id).await.unwrap();
    assert_eq!(book_authors.len(), 2, "should have 2 overridden authors");
}

/// Upload page is forbidden without upload permission.
#[tokio::test]
async fn upload_rejects_unauthorized() {
    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let upload_dir = tempfile::tempdir().unwrap();
    let config = test_config_with_upload(lib_dir.path(), covers_dir.path(), upload_dir.path());

    // Create a non-superuser without upload permission
    let user_id = create_test_user(&pool, "noupload", "password123", false).await;
    // Explicitly remove upload permission
    sqlx::query("UPDATE users SET allow_upload = 0 WHERE id = ?")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let session = session_cookie_value(user_id);

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get_with_session(app, "/web/upload", &session).await;
    assert_eq!(
        resp.status(),
        403,
        "non-upload user should get 403 Forbidden"
    );
}

/// Duplicate filename on publish should be rejected.
#[tokio::test]
async fn upload_duplicate_filename_rejected() {
    let (pool, _) = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let upload_dir = tempfile::tempdir().unwrap();
    let config = test_config_with_upload(lib_dir.path(), covers_dir.path(), upload_dir.path());

    let user_id = create_test_user(&pool, "dupuser", "password123", true).await;
    let session = session_cookie_value(user_id);
    let csrf = csrf_for_session(&session);

    let state = test_app_state(pool.clone(), config);

    let file_data = std::fs::read(test_data_dir().join("test_book.fb2")).unwrap();

    // First upload + publish
    let (ct, body) = build_multipart_body(&csrf, "test_book.fb2", &file_data);
    let app = test_router(state.clone());
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/web/upload/file")
        .header("content-type", &ct)
        .header("cookie", format!("session={session}"))
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let token1 = json["token"].as_str().unwrap().to_string();

    let app2 = test_router(state.clone());
    let resp2 = post_json(
        app2,
        "/web/upload/publish",
        serde_json::json!({"token": token1, "csrf_token": csrf}),
        &session,
    )
    .await;
    assert_eq!(resp2.status(), 200);
    let json2: serde_json::Value = serde_json::from_str(&body_string(resp2).await).unwrap();
    assert_eq!(json2["success"], true, "first publish should succeed");

    // Second upload of same file + publish (duplicate)
    let (ct2, body2) = build_multipart_body(&csrf, "test_book.fb2", &file_data);
    let app3 = test_router(state.clone());
    let req2 = axum::http::Request::builder()
        .method("POST")
        .uri("/web/upload/file")
        .header("content-type", &ct2)
        .header("cookie", format!("session={session}"))
        .body(Body::from(body2))
        .unwrap();
    let resp3 = app3.oneshot(req2).await.unwrap();
    let json3: serde_json::Value =
        serde_json::from_slice(&resp3.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let token2 = json3["token"].as_str().unwrap().to_string();

    let app4 = test_router(state);
    let resp4 = post_json(
        app4,
        "/web/upload/publish",
        serde_json::json!({"token": token2, "csrf_token": csrf}),
        &session,
    )
    .await;
    let status = resp4.status().as_u16();
    let json4: serde_json::Value = serde_json::from_str(&body_string(resp4).await).unwrap();
    // Should fail with a duplicate error
    assert!(
        json4["success"] == false || status == 409 || status == 400,
        "duplicate publish should fail: status={status}, json={json4}"
    );
}
