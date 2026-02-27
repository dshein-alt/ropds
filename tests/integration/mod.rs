mod admin_series_tests;
mod admin_user_title_tests;
mod author_search_tests;
mod book_search_tests;
mod bookshelf_tests;
mod catalog_tests;
mod duplicates_tests;
mod opds2_tests;
mod opds_core_tests;
mod opds_language_facets_tests;
mod opds_recent_tests;
mod reader_tests;
mod recent_tests;
mod scanner_tests;
mod series_search_tests;
mod static_tests;
mod upload_tests;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use axum::Router;
use axum::body::Body;
use http_body_util::BodyExt;
use tokio::sync::Mutex;
use tower::ServiceExt;

/// Global lock to serialize scanner tests (SCAN_LOCK is a process-wide AtomicBool).
pub static SCAN_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

use ropds::config::Config;
use ropds::db::DbPool;
use ropds::state::AppState;
use ropds::web::auth::sign_session;
use ropds::web::context::{generate_csrf_token, register_filters};
use ropds::web::i18n;

/// Directory containing test book files.
pub fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// Build a minimal Config pointing at the given temp directories.
pub fn test_config(lib_dir: &Path, covers_dir: &Path) -> Config {
    let toml_str = format!(
        r#"
[server]
session_secret = "test-secret-key-for-integration-tests"

[library]
root_path = {lib_dir:?}

[covers]
covers_path = {covers_dir:?}

[database]
url = "sqlite::memory:"

[opds]
auth_required = false

[scanner]
"#
    );
    toml::from_str(&toml_str).expect("test config should parse")
}

/// Build a Config with upload enabled.
pub fn test_config_with_upload(lib_dir: &Path, covers_dir: &Path, upload_dir: &Path) -> Config {
    let toml_str = format!(
        r#"
[server]
session_secret = "test-secret-key-for-integration-tests"

[library]
root_path = {lib_dir:?}

[covers]
covers_path = {covers_dir:?}

[database]
url = "sqlite::memory:"

[opds]
auth_required = false

[scanner]

[upload]
allow_upload = true
upload_path = {upload_dir:?}
"#
    );
    toml::from_str(&toml_str).expect("test config should parse")
}

/// Build an AppState with real Tera templates and translations.
pub fn test_app_state(pool: DbPool, config: Config) -> AppState {
    let templates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/**/*.html");
    let mut tera = tera::Tera::new(templates_dir.to_str().unwrap()).expect("templates should load");
    register_filters(&mut tera);

    let translations = i18n::load_runtime_translations().expect("translations should load");

    AppState::new(config, pool, tera, translations, false, false)
}

/// Build a full Router from an AppState.
pub fn test_router(state: AppState) -> Router {
    ropds::build_router(state)
}

/// Create a test user in the DB and return the user ID.
pub async fn create_test_user(
    pool: &DbPool,
    username: &str,
    password: &str,
    is_superuser: bool,
) -> i64 {
    let password_hash = ropds::password::hash(password);
    let su = if is_superuser { 1 } else { 0 };
    sqlx::query(
        "INSERT INTO users (username, password_hash, is_superuser, display_name, password_change_required, allow_upload)
         VALUES (?, ?, ?, ?, 0, ?)",
    )
    .bind(username)
    .bind(&password_hash)
    .bind(su)
    .bind(username)
    .bind(su) // superusers get upload permission
    .execute(pool.inner())
    .await
    .expect("should create test user");

    let row: (i64,) = sqlx::query_as("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(pool.inner())
        .await
        .expect("should find created user");
    row.0
}

/// Generate a valid session cookie value for the given user.
pub fn session_cookie_value(user_id: i64) -> String {
    sign_session(user_id, b"test-secret-key-for-integration-tests", 24)
}

/// Generate a CSRF token for a given session cookie value.
pub fn csrf_for_session(session_value: &str) -> String {
    generate_csrf_token(session_value, b"test-secret-key-for-integration-tests")
}

/// Send a GET request and return the response.
pub async fn get(app: Router, path: &str) -> axum::response::Response {
    let req = axum::http::Request::builder()
        .uri(path)
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// Send a GET request with a session cookie.
pub async fn get_with_session(
    app: Router,
    path: &str,
    session_value: &str,
) -> axum::response::Response {
    let req = axum::http::Request::builder()
        .uri(path)
        .header("cookie", format!("session={session_value}"))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// Send a POST form request with a session cookie.
pub async fn post_form(
    app: Router,
    path: &str,
    body: &str,
    session_value: &str,
) -> axum::response::Response {
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("cookie", format!("session={session_value}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// Send a POST JSON request with a session cookie.
pub async fn post_json(
    app: Router,
    path: &str,
    json: serde_json::Value,
    session_value: &str,
) -> axum::response::Response {
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("cookie", format!("session={session_value}"))
        .body(Body::from(serde_json::to_string(&json).unwrap()))
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// Extract response body as a String.
pub async fn body_string(response: axum::response::Response) -> String {
    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Copy specific test data files into a destination directory.
pub fn copy_test_files(dest: &Path, filenames: &[&str]) {
    let src = test_data_dir();
    for name in filenames {
        let from = src.join(name);
        let to = dest.join(name);
        std::fs::copy(&from, &to).unwrap_or_else(|e| panic!("copy {from:?} -> {to:?}: {e}"));
    }
}

/// Copy test files into a subdirectory within the destination.
pub fn copy_test_files_to_subdir(dest: &Path, subdir: &str, filenames: &[&str]) {
    let target = dest.join(subdir);
    std::fs::create_dir_all(&target).unwrap();
    let src = test_data_dir();
    for name in filenames {
        std::fs::copy(src.join(name), target.join(name)).unwrap();
    }
}
