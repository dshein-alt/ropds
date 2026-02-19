#![allow(dead_code)]

#[cfg(feature = "test-postgres")]
pub mod postgres_tests;

#[cfg(feature = "test-mysql")]
pub mod mysql_tests;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use axum::Router;
use axum::body::Body;
use http_body_util::BodyExt;
use tokio::sync::Mutex;
use tower::ServiceExt;

use ropds::config::Config;
use ropds::db::DbPool;
use ropds::state::AppState;
use ropds::web::auth::sign_session;
use ropds::web::context::{generate_csrf_token, register_filters};
use ropds::web::i18n;

/// Global lock to serialize scanner tests (scanner uses a global AtomicBool lock).
pub static SCAN_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

// ---------------------------------------------------------------------------
// Container startup helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "test-postgres")]
pub async fn start_postgres() -> (
    testcontainers_modules::testcontainers::ContainerAsync<
        testcontainers_modules::postgres::Postgres,
    >,
    DbPool,
) {
    use testcontainers_modules::postgres::Postgres;
    use testcontainers_modules::testcontainers::ImageExt;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;

    let container = Postgres::default()
        .with_tag("latest")
        .start()
        .await
        .expect("Failed to start PostgreSQL container");

    let port = container
        .get_host_port_ipv4(5432u16)
        .await
        .expect("Failed to get PG port");

    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = ropds::db::create_test_pool_for(&url).await;
    (container, pool)
}

#[cfg(feature = "test-mysql")]
pub async fn start_mysql() -> (
    testcontainers_modules::testcontainers::ContainerAsync<
        testcontainers_modules::mariadb::Mariadb,
    >,
    DbPool,
) {
    use testcontainers_modules::mariadb::Mariadb;
    use testcontainers_modules::testcontainers::ImageExt;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;

    let container = Mariadb::default()
        .with_tag("latest")
        .start()
        .await
        .expect("Failed to start MariaDB container");

    let port = container
        .get_host_port_ipv4(3306u16)
        .await
        .expect("Failed to get MySQL port");

    // MariaDB default: root user with no password, "test" database
    let url = format!("mysql://root@127.0.0.1:{port}/test");
    let pool = ropds::db::create_test_pool_for(&url).await;
    (container, pool)
}

// ---------------------------------------------------------------------------
// Shared test helpers
// ---------------------------------------------------------------------------

/// Directory containing test book files.
pub fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// Build a Config with a custom database URL (for PG/MySQL containers).
pub fn test_config_with_url(lib_dir: &Path, covers_dir: &Path, db_url: &str) -> Config {
    let toml_str = format!(
        r#"
[server]
session_secret = "test-secret-key-for-integration-tests"

[library]
root_path = {lib_dir:?}
covers_path = {covers_dir:?}

[database]
url = "{db_url}"

[opds]
auth_required = false

[scanner]
"#
    );
    toml::from_str(&toml_str).expect("test config should parse")
}

/// Build an AppState with real Tera templates and translations.
pub fn test_app_state(pool: DbPool, config: Config) -> AppState {
    let templates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/**/*.html");
    let mut tera = tera::Tera::new(templates_dir.to_str().unwrap()).expect("templates should load");
    register_filters(&mut tera);

    let locales_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("locales");
    let translations = i18n::load_translations(&locales_dir).expect("translations should load");

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
    let sql = pool.sql(
        "INSERT INTO users (username, password_hash, is_superuser, display_name, password_change_required, allow_upload) \
         VALUES (?, ?, ?, ?, 0, ?)",
    );
    sqlx::query(&sql)
        .bind(username)
        .bind(&password_hash)
        .bind(su)
        .bind(username)
        .bind(su)
        .execute(pool.inner())
        .await
        .expect("should create test user");

    let sql = pool.sql("SELECT id FROM users WHERE username = ?");
    let row: (i64,) = sqlx::query_as(&sql)
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
