use ropds::db;
use ropds::db::queries::{books, reading_positions};
use ropds::scanner;

use super::*;

async fn setup_recent_library() -> (
    db::DbPool,
    ropds::config::Config,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files(
        lib_dir.path(),
        &["test_book.fb2", "test_book.epub", "title_only.fb2"],
    );
    scanner::run_scan(&pool, &config).await.unwrap();

    (pool, config, lib_dir, covers_dir)
}

#[tokio::test]
async fn recent_page_lists_newest_books_first() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _lib, _cov) = setup_recent_library().await;

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
    let app = test_router(state);
    let resp = get(app, "/web/recent").await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    let main = html.split("<footer").next().unwrap_or(&html);
    let new_pos = main
        .find("Lonely Title Book")
        .expect("new title in recent page");
    let old_pos = main
        .find("Test Book Title")
        .expect("old title in recent page");
    assert!(
        new_pos < old_pos,
        "newly added book should appear before older one"
    );
}

#[tokio::test]
async fn home_shows_continue_reading_for_authenticated_user() {
    let _lock = SCAN_MUTEX.lock().await;
    let (pool, config, _lib, _cov) = setup_recent_library().await;

    let user_id = create_test_user(&pool, "continue_user", "password123", false).await;
    let session = session_cookie_value(user_id);

    let first_book = books::find_by_path_and_filename(&pool, "", "test_book.fb2")
        .await
        .unwrap()
        .unwrap();
    let second_book = books::find_by_path_and_filename(&pool, "", "title_only.fb2")
        .await
        .unwrap()
        .unwrap();

    reading_positions::save_position(&pool, user_id, first_book.id, "pos1", 0.25, 100)
        .await
        .unwrap();
    reading_positions::save_position(&pool, user_id, second_book.id, "pos2", 0.77, 100)
        .await
        .unwrap();

    let sql = pool.sql(
        "UPDATE reading_positions SET updated_at = ? \
         WHERE user_id = ? AND book_id = ?",
    );
    sqlx::query(&sql)
        .bind("2024-06-01 00:00:00")
        .bind(user_id)
        .bind(first_book.id)
        .execute(pool.inner())
        .await
        .unwrap();
    sqlx::query(&sql)
        .bind("2024-06-02 00:00:00")
        .bind(user_id)
        .bind(second_book.id)
        .execute(pool.inner())
        .await
        .unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);
    let resp = get_with_session(app, "/web", &session).await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(html.contains("Continue reading"));
    assert!(html.contains(&format!("/web/reader/{}", second_book.id)));
    assert!(html.contains("77%"));
}
