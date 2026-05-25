use ropds::db;
use ropds::db::DbPool;
use ropds::scanner;

use super::*;

/// Insert a catalog row directly and return its id.
async fn insert_cat(pool: &DbPool, parent_id: Option<i64>, path: &str, cat_name: &str) -> i64 {
    let sql = pool.sql("INSERT INTO catalogs (parent_id, path, cat_name) VALUES (?, ?, ?)");
    sqlx::query(&sql)
        .bind(parent_id)
        .bind(path)
        .bind(cat_name)
        .execute(pool.inner())
        .await
        .unwrap();
    let sql = pool.sql("SELECT id FROM catalogs WHERE path = ?");
    let (id,): (i64,) = sqlx::query_as(&sql)
        .bind(path)
        .fetch_one(pool.inner())
        .await
        .unwrap();
    id
}

/// Insert a minimal book row tied to the given catalog.
async fn insert_book_in_cat(pool: &DbPool, catalog_id: i64, title: &str) {
    let search_title = title.to_uppercase();
    let sql = pool.sql(
        "INSERT INTO books (catalog_id, filename, path, format, title, search_title, \
         lang, lang_code, size, avail, cat_type, cover, cover_type) \
         VALUES (?, ?, '/', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
    );
    sqlx::query(&sql)
        .bind(catalog_id)
        .bind(format!("{title}.fb2"))
        .bind(title)
        .bind(&search_title)
        .execute(pool.inner())
        .await
        .unwrap();
}

/// The catalog page shows root-level catalog entries after a scan.
#[tokio::test]
async fn catalog_page_lists_root_catalogs() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    // Create a subdirectory with books
    copy_test_files_to_subdir(lib_dir.path(), "fiction", &["test_book.fb2"]);
    copy_test_files_to_subdir(lib_dir.path(), "science", &["test_book.epub"]);

    scanner::run_scan(&pool, &config).await.unwrap();

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/catalogs").await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(html.contains("fiction"), "should list 'fiction' catalog");
    assert!(html.contains("science"), "should list 'science' catalog");
}

/// Drilling into a catalog by ID shows books inside it.
#[tokio::test]
async fn catalog_drill_down_shows_books() {
    let _lock = SCAN_MUTEX.lock().await;
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    copy_test_files_to_subdir(lib_dir.path(), "mybooks", &["test_book.fb2"]);

    scanner::run_scan(&pool, &config).await.unwrap();

    // Find the catalog ID for "mybooks"
    let cat = ropds::db::queries::catalogs::find_by_path(&pool, "mybooks")
        .await
        .unwrap()
        .expect("mybooks catalog should exist");

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/catalogs?cat_id={}", cat.id)).await;
    assert_eq!(resp.status(), 200);

    let html = body_string(resp).await;
    assert!(
        html.contains("Test Book Title"),
        "should show the book title in catalog view"
    );
}

// ── Root flatten + navigation behavior ──────────────────────────────

/// When the library has a single empty-name root catalog (the filesystem-root
/// catalog created by the scanner), the /web/catalogs view should show that
/// catalog's children + its direct books as if visiting it.
#[tokio::test]
async fn catalog_root_flattens_single_empty_root() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    // Single empty-name top-level catalog with a child + a direct book.
    let lib_root = insert_cat(&pool, None, "/", "").await;
    let _child = insert_cat(&pool, Some(lib_root), "/fiction", "fiction").await;
    insert_book_in_cat(&pool, lib_root, "Rooted Tome").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/catalogs").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;

    assert!(
        html.contains("fiction"),
        "child catalog should be listed at flattened root"
    );
    assert!(
        html.contains("Rooted Tome"),
        "book directly under the empty-name root should appear at /web/catalogs"
    );
    // Breadcrumb should include the Books virtual folder (renamed empty-name root).
    assert!(
        html.contains("Books"),
        "breadcrumb should label the flattened empty-name root as 'Books'"
    );
}

/// When multiple top-level catalogs exist, the root view should NOT flatten;
/// each top-level catalog should appear as its own entry.
#[tokio::test]
async fn catalog_root_does_not_flatten_multiple_roots() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let _a = insert_cat(&pool, None, "/alpha", "alpha").await;
    let b = insert_cat(&pool, None, "/beta", "beta").await;
    let _b_child = insert_cat(&pool, Some(b), "/beta/inner", "inner").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/catalogs").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;

    assert!(html.contains("alpha"), "alpha root should be listed");
    assert!(html.contains("beta"), "beta root should be listed");
    assert!(
        !html.contains("inner"),
        "child of beta must NOT surface at root when no flatten is triggered"
    );
}

/// When an empty-name root coexists with other top-level catalogs, the
/// "Books" virtual folder must appear BEFORE other roots in the listing.
#[tokio::test]
async fn catalog_root_pins_books_to_top() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let _lib = insert_cat(&pool, None, "/", "").await;
    let _z = insert_cat(&pool, None, "/zzz", "zzz").await;
    let _a = insert_cat(&pool, None, "/aaa", "aaa").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/catalogs").await;
    assert_eq!(resp.status(), 200);
    let html = body_string(resp).await;

    let books_pos = html.find("Books").expect("Books entry present");
    let aaa_pos = html.find(">aaa<").or_else(|| html.find("aaa")).unwrap();
    let zzz_pos = html.find(">zzz<").or_else(|| html.find("zzz")).unwrap();
    assert!(
        books_pos < aaa_pos && books_pos < zzz_pos,
        "Books virtual folder should appear before other root catalogs in listing"
    );
}

/// At /web/catalogs (cat_id=0) the persistent `..` element must be present but
/// inert — no anchor wrapping it.
#[tokio::test]
async fn catalog_navigation_dotdot_inert_at_root() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let _ = insert_cat(&pool, None, "/some", "some").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/web/catalogs").await;
    let html = body_string(resp).await;

    // The .. row should render as a disabled span, never as a link.
    assert!(
        html.contains(r#"aria-disabled="true""#),
        "`..` row at root must be marked aria-disabled"
    );
    assert!(
        !html.contains(r#"href="/web/catalogs""#)
            || html.matches(r#"href="/web/catalogs""#).count() <= 1,
        "no extra `..` link to /web/catalogs at root view"
    );
}

/// Inside a nested catalog, `..` must link to the parent catalog by id.
#[tokio::test]
async fn catalog_navigation_dotdot_links_to_parent() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let parent = insert_cat(&pool, None, "/parent", "parent").await;
    let child = insert_cat(&pool, Some(parent), "/parent/child", "child").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/catalogs?cat_id={child}")).await;
    let html = body_string(resp).await;

    let expected = format!(r#"href="/web/catalogs?cat_id={parent}""#);
    assert!(
        html.contains(&expected),
        "`..` should link to parent (cat_id={parent}); rendered HTML missing it"
    );
}

/// At a top-level catalog (parent_id is NULL), `..` should still be clickable
/// and lead back to the root /web/catalogs view.
#[tokio::test]
async fn catalog_navigation_dotdot_top_level_returns_to_root() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let top = insert_cat(&pool, None, "/top", "top").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/catalogs?cat_id={top}")).await;
    let html = body_string(resp).await;

    // Look for the dotdot anchor specifically (not the breadcrumb home link).
    // Both target /web/catalogs, so we only check that the link exists and the
    // disabled-span variant does NOT.
    assert!(
        html.contains(r#"href="/web/catalogs""#),
        "`..` from a top-level catalog should link to /web/catalogs"
    );
    assert!(
        !html.contains(r#"aria-disabled="true""#),
        "`..` should NOT be disabled when viewing a top-level catalog"
    );
}

/// From a catalog whose parent is the empty-name library-root (auto-flattened
/// at cat_id=0), `..` should target /web/catalogs (clean URL), NOT the
/// library-root catalog id.
#[tokio::test]
async fn catalog_navigation_dotdot_skips_flattened_root() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let lib_root = insert_cat(&pool, None, "/", "").await;
    let child = insert_cat(&pool, Some(lib_root), "/fiction", "fiction").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/catalogs?cat_id={child}")).await;
    let html = body_string(resp).await;

    assert!(
        html.contains(r#"href="/web/catalogs""#),
        "`..` should target /web/catalogs when parent is the flattened lib-root"
    );
    let forbidden = format!(r#"href="/web/catalogs?cat_id={lib_root}""#);
    // The breadcrumb may link to the lib-root id; ensure the dotdot link does
    // not duplicate that — we check no anchor targets it for the dotdot row by
    // requiring at most ONE occurrence (the breadcrumb).
    assert!(
        html.matches(&forbidden).count() <= 1,
        "`..` should not point at the flattened library-root cat_id"
    );
}

/// Regression: when an empty-name root coexists with another top-level catalog,
/// auto-flatten is OFF. `..` from a child of the empty-name root must preserve
/// parent context (link to cat_id=<empty-name-root>), NOT collapse to
/// /web/catalogs which would lose the navigation path.
#[tokio::test]
async fn catalog_navigation_dotdot_preserves_parent_when_no_flatten() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    // Multi-root setup → no auto-flatten.
    let books_root = insert_cat(&pool, None, "/", "").await;
    let _other_root = insert_cat(&pool, None, "/other", "other").await;
    let child = insert_cat(&pool, Some(books_root), "/fiction", "fiction").await;

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, &format!("/web/catalogs?cat_id={child}")).await;
    let html = body_string(resp).await;

    let expected = format!(r#"href="/web/catalogs?cat_id={books_root}""#);
    assert!(
        html.contains(&expected),
        "`..` should link to the empty-name root's cat_id (={books_root}) when \
         flatten is OFF (multi-root case), preserving parent context"
    );
}
