use ropds::db;
use ropds::db::DbPool;

use super::*;

async fn insert_dup_book(pool: &DbPool, title: &str, search_title: &str, filename: &str) -> i64 {
    let cat_path = format!("/dup-it-{filename}");
    let sql = pool.sql("INSERT INTO catalogs (path, cat_name) VALUES (?, 'dup-it')");
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

    let sql = pool.sql(
        "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
         lang, lang_code, size, avail, cat_type, cover, cover_type) \
         VALUES (?, ?, '/dup-it', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
    );
    sqlx::query(&sql)
        .bind(catalog_id)
        .bind(filename)
        .bind(title)
        .bind(search_title)
        .execute(pool.inner())
        .await
        .unwrap();

    let sql = pool.sql("SELECT id FROM books WHERE catalog_id = ? AND filename = ?");
    let (book_id,): (i64,) = sqlx::query_as(&sql)
        .bind(catalog_id)
        .bind(filename)
        .fetch_one(pool.inner())
        .await
        .unwrap();
    book_id
}

async fn insert_author(pool: &DbPool, name: &str) -> i64 {
    let search = name.to_uppercase();
    let sql =
        pool.sql("INSERT INTO authors (full_name, search_full_name, lang_code) VALUES (?, ?, 2)");
    sqlx::query(&sql)
        .bind(name)
        .bind(&search)
        .execute(pool.inner())
        .await
        .unwrap();
    let sql = pool.sql("SELECT id FROM authors WHERE full_name = ?");
    let (id,): (i64,) = sqlx::query_as(&sql)
        .bind(name)
        .fetch_one(pool.inner())
        .await
        .unwrap();
    id
}

async fn link_author(pool: &DbPool, book_id: i64, author_id: i64) {
    let sql = pool.sql("INSERT INTO book_authors (book_id, author_id) VALUES (?, ?)");
    sqlx::query(&sql)
        .bind(book_id)
        .bind(author_id)
        .execute(pool.inner())
        .await
        .unwrap();
}

#[tokio::test]
async fn admin_duplicates_page_requires_superuser() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let user_id = create_test_user(&pool, "dup-normal", "password123", false).await;
    let session = session_cookie_value(user_id);

    let state = test_app_state(pool.clone(), config);
    let app = test_router(state);

    let resp = get_with_session(app, "/web/admin/duplicates", &session).await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn admin_duplicates_page_returns_200_for_superuser() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let super_id = create_test_user(&pool, "dup-admin", "password123", true).await;
    let session = session_cookie_value(super_id);

    let state = test_app_state(pool.clone(), config);
    let app = test_router(state);

    let resp = get_with_session(app, "/web/admin/duplicates", &session).await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(html.contains("Duplicate Books"));
}

#[tokio::test]
async fn admin_duplicates_page_shows_groups() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let super_id = create_test_user(&pool, "dup-admin-grp", "password123", true).await;
    let session = session_cookie_value(super_id);

    let author = insert_author(&pool, "Duplicate Author").await;

    // Insert 3 books with the same search_title and author (a duplicate group)
    for i in 0..3 {
        let book = insert_dup_book(
            &pool,
            &format!("Duplicate Title v{i}"),
            "DUPLICATE TITLE",
            &format!("dup-grp-{i}.fb2"),
        )
        .await;
        link_author(&pool, book, author).await;
        db::queries::books::update_author_key(&pool, book)
            .await
            .unwrap();
    }

    // Insert a unique book (not a duplicate)
    let solo = insert_dup_book(&pool, "Unique Solo", "UNIQUE SOLO", "solo.fb2").await;
    link_author(&pool, solo, author).await;
    db::queries::books::update_author_key(&pool, solo)
        .await
        .unwrap();

    let state = test_app_state(pool.clone(), config);
    let app = test_router(state);

    let resp = get_with_session(app, "/web/admin/duplicates", &session).await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;

    // Page shows the duplicate group
    assert!(html.contains("Duplicate Author"));
    assert!(html.contains("1 duplicate groups"));
}

#[tokio::test]
async fn admin_duplicates_page_no_duplicates_message() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let super_id = create_test_user(&pool, "dup-admin-empty", "password123", true).await;
    let session = session_cookie_value(super_id);

    let state = test_app_state(pool.clone(), config);
    let app = test_router(state);

    let resp = get_with_session(app, "/web/admin/duplicates", &session).await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(html.contains("No duplicate groups found."));
}

#[tokio::test]
async fn admin_duplicates_page_different_authors_not_grouped() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let super_id = create_test_user(&pool, "dup-admin-diff", "password123", true).await;
    let session = session_cookie_value(super_id);

    let author_a = insert_author(&pool, "Author Alpha").await;
    let author_b = insert_author(&pool, "Author Beta").await;

    // Two books with same search_title but different authors → NOT duplicates
    let b1 = insert_dup_book(&pool, "Same Title", "SAME TITLE", "diff-a1.fb2").await;
    let b2 = insert_dup_book(&pool, "Same Title", "SAME TITLE", "diff-a2.fb2").await;

    link_author(&pool, b1, author_a).await;
    link_author(&pool, b2, author_b).await;
    db::queries::books::update_author_key(&pool, b1)
        .await
        .unwrap();
    db::queries::books::update_author_key(&pool, b2)
        .await
        .unwrap();

    let state = test_app_state(pool.clone(), config);
    let app = test_router(state);

    let resp = get_with_session(app, "/web/admin/duplicates", &session).await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;
    assert!(html.contains("No duplicate groups found."));
}

#[tokio::test]
async fn set_book_authors_and_update_key_is_atomic() {
    let pool = db::create_test_pool().await;

    let author_a = insert_author(&pool, "Atomic Author A").await;
    let author_b = insert_author(&pool, "Atomic Author B").await;

    let book = insert_dup_book(&pool, "Atomic Book", "ATOMIC BOOK", "atomic.fb2").await;
    link_author(&pool, book, author_a).await;
    db::queries::books::update_author_key(&pool, book)
        .await
        .unwrap();

    // Verify initial state
    let initial = db::queries::books::get_by_id(&pool, book)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(initial.author_key, author_a.to_string());

    // Update authors via the transactional function
    db::queries::books::set_book_authors_and_update_key(&pool, book, &[author_b])
        .await
        .unwrap();

    // Both book_authors and author_key should be updated atomically
    let updated = db::queries::books::get_by_id(&pool, book)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.author_key, author_b.to_string());

    let linked = db::queries::authors::get_for_book(&pool, book)
        .await
        .unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].id, author_b);
}
