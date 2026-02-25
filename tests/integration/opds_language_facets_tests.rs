use ropds::db;

use super::*;

/// OPDS language facets feed should list supported locales and link to locale-root endpoints.
#[tokio::test]
async fn opds_language_facets_feed_lists_locale_entries() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/opds/facets/languages/?lang=ru").await;
    assert_eq!(resp.status(), 200);

    let xml = body_string(resp).await;
    assert!(xml.contains("<feed"), "should return an OPDS feed");
    assert!(
        xml.contains("/opds/facets/languages/?lang=ru"),
        "self link should preserve selected locale"
    );
    assert!(
        xml.contains("/opds/lang/en/") && xml.contains("/opds/lang/ru/"),
        "should include root locale links for known locales"
    );
    assert!(
        xml.contains("English") && xml.contains("Русский"),
        "should include locale labels in entries"
    );
    assert!(
        xml.contains("Язык"),
        "feed title should be localized in Russian"
    );
}

/// OPDS language facets route without trailing slash should be accepted.
#[tokio::test]
async fn opds_language_facets_route_without_trailing_slash_works() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/opds/facets/languages?lang=en").await;
    assert_eq!(resp.status(), 200);

    let xml = body_string(resp).await;
    assert!(
        xml.contains("/opds/lang/en/"),
        "route alias should return the same language facet feed"
    );
}

/// OPDS locale-root route should force localized navigation and preserve locale query on links.
#[tokio::test]
async fn opds_root_feed_for_locale_forces_requested_language() {
    let pool = db::create_test_pool().await;
    let lib_dir = tempfile::tempdir().unwrap();
    let covers_dir = tempfile::tempdir().unwrap();
    let config = test_config(lib_dir.path(), covers_dir.path());

    let state = test_app_state(pool, config);
    let app = test_router(state);

    let resp = get(app, "/opds/lang/ru/").await;
    assert_eq!(resp.status(), 200);

    let xml = body_string(resp).await;
    assert!(xml.contains("<feed"), "should return an OPDS feed");
    assert!(
        xml.contains("По авторам") && xml.contains("По сериям"),
        "root entries should be localized in Russian"
    );
    assert!(
        xml.contains("/opds/authors/?lang=ru")
            && xml.contains("/opds/series/?lang=ru")
            && xml.contains("/opds/facets/languages/?lang=ru"),
        "navigation links should preserve forced locale in query string"
    );
}
