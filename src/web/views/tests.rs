#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        Config, CoversConfig, DatabaseConfig, LibraryConfig, OpdsConfig, ReaderConfig,
        ScannerConfig, ServerConfig, UploadConfig, WebConfig,
    };
    use crate::db::create_test_pool;
    use crate::db::models::CatType;
    use crate::db::queries::books;
    use crate::state::AppState;
    use crate::web::i18n::Translations;
    use axum_extra::extract::cookie::CookieJar;
    use std::path::PathBuf;
    use tempfile::tempdir;

    async fn build_test_state(root_path: PathBuf) -> AppState {
        let config = Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                log_level: "info".to_string(),
                session_secret: "test-secret".to_string(),
                session_ttl_hours: 24,
            },
            library: LibraryConfig {
                root_path,
                covers_path: None,
                cover_max_dimension_px: None,
                cover_jpeg_quality: None,
                book_extensions: vec!["fb2".to_string(), "epub".to_string(), "zip".to_string()],
                scan_zip: true,
                zip_codepage: "cp866".to_string(),
                inpx_enable: false,
            },
            covers: CoversConfig {
                covers_path: PathBuf::from("/tmp/covers"),
                cover_max_dimension_px: 600,
                cover_jpeg_quality: 85,
                show_covers: true,
            },
            database: DatabaseConfig {
                url: "sqlite::memory:".to_string(),
            },
            opds: OpdsConfig {
                title: "ROPDS".to_string(),
                subtitle: String::new(),
                max_items: 30,
                split_items: 300,
                auth_required: true,
                show_covers: None,
                alphabet_menu: true,
                hide_doubles: false,
            },
            scanner: ScannerConfig {
                schedule_minutes: vec![0],
                schedule_hours: vec![0],
                schedule_day_of_week: vec![],
                delete_logical: true,
                skip_unchanged: false,
                test_zip: false,
                test_files: false,
                workers_num: 1,
            },
            web: WebConfig {
                language: "en".to_string(),
                theme: "light".to_string(),
            },
            upload: UploadConfig {
                allow_upload: true,
                upload_path: PathBuf::from("/tmp/uploads"),
                max_upload_size_mb: 10,
            },
            reader: ReaderConfig::default(),
        };

        let db = create_test_pool().await;
        let tera = tera::Tera::default();
        let mut translations = Translations::new();
        translations.insert("en".to_string(), serde_json::json!({"web": {}}));

        AppState::new(config, db, tera, translations, false, false)
    }

    async fn ensure_catalog(pool: &crate::db::DbPool) -> i64 {
        sqlx::query(&pool.sql("INSERT INTO catalogs (path, cat_name) VALUES (?, ?)"))
            .bind("/web-tests")
            .bind("web-tests")
            .execute(pool.inner())
            .await
            .unwrap();
        let row: (i64,) = sqlx::query_as(&pool.sql("SELECT id FROM catalogs WHERE path = ?"))
            .bind("/web-tests")
            .fetch_one(pool.inner())
            .await
            .unwrap();
        row.0
    }

    #[test]
    fn test_default_m() {
        assert_eq!(default_m(), "m".to_string());
    }

    #[test]
    fn test_sanitize_internal_redirect() {
        assert_eq!(sanitize_internal_redirect(Some("/web/books")), "/web/books");
        assert_eq!(
            sanitize_internal_redirect(Some("/web/books?page=2&q=test")),
            "/web/books?page=2&q=test"
        );
        assert_eq!(sanitize_internal_redirect(Some("//evil.example")), "/web");
        assert_eq!(
            sanitize_internal_redirect(Some("https://evil.example")),
            "/web"
        );
        assert_eq!(sanitize_internal_redirect(Some(r"/web\reader")), "/web");
        assert_eq!(sanitize_internal_redirect(None), "/web");
    }

    #[test]
    fn test_parse_bookshelf_sort_variants() {
        let (col, asc) = parse_bookshelf_sort("title", "asc");
        assert!(matches!(col, bookshelf::SortColumn::Title));
        assert!(asc);

        let (col, asc) = parse_bookshelf_sort("author", "desc");
        assert!(matches!(col, bookshelf::SortColumn::Author));
        assert!(!asc);

        let (col, asc) = parse_bookshelf_sort("unknown", "nope");
        assert!(matches!(col, bookshelf::SortColumn::Date));
        assert!(!asc);
    }

    #[test]
    fn test_render_success_and_error() {
        let mut tera = tera::Tera::default();
        tera.add_raw_template("ok.html", "Hello {{ name }}")
            .unwrap();

        let mut ctx = tera::Context::new();
        ctx.insert("name", "World");

        let html = render(&tera, "ok.html", &ctx).unwrap();
        assert_eq!(html.0, "Hello World");

        let err = render(&tera, "missing.html", &ctx).unwrap_err();
        assert_eq!(err, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_set_language_redirect_validation() {
        let jar = CookieJar::new();
        let (jar, redirect) = set_language(
            jar,
            Query(SetLanguageParams {
                lang: "ru".to_string(),
                redirect: Some("/web/books".to_string()),
            }),
        )
        .await;
        assert_eq!(jar.get("lang").unwrap().value(), "ru");
        assert_eq!(redirect.into_response().headers()["location"], "/web/books");

        let (jar, redirect) = set_language(
            jar,
            Query(SetLanguageParams {
                lang: "en".to_string(),
                redirect: Some("//evil.example".to_string()),
            }),
        )
        .await;
        assert_eq!(jar.get("lang").unwrap().value(), "en");
        assert_eq!(redirect.into_response().headers()["location"], "/web");
    }

    #[tokio::test]
    async fn test_web_download_book_not_found() {
        let tmp = tempdir().unwrap();
        let state = build_test_state(tmp.path().to_path_buf()).await;
        let response = web_download(State(state), CookieJar::new(), Path((999_999, 0))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_web_download_missing_file_returns_not_found() {
        let tmp = tempdir().unwrap();
        let state = build_test_state(tmp.path().to_path_buf()).await;
        let catalog_id = ensure_catalog(&state.db).await;
        let book_id = books::insert(
            &state.db,
            catalog_id,
            "missing.fb2",
            "",
            "fb2",
            "Missing File",
            "MISSING FILE",
            "",
            "",
            "en",
            2,
            100,
            CatType::Normal,
            0,
            "",
        )
        .await
        .unwrap();

        let response = web_download(State(state), CookieJar::new(), Path((book_id, 0))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
