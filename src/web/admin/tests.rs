#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        Config, CoversConfig, DatabaseConfig, LibraryConfig, OpdsConfig, ReaderConfig,
        ScannerConfig, ServerConfig, UploadConfig, WebConfig,
    };
    use crate::db::{DbPool, create_test_pool};
    use crate::web::auth::sign_session;
    use crate::web::context::generate_csrf_token;
    use axum_extra::extract::cookie::{Cookie, CookieJar};
    use http_body_util::BodyExt;
    use std::path::PathBuf;

    fn test_state(pool: DbPool) -> AppState {
        let config = Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                log_level: "info".to_string(),
                session_secret: "test-secret".to_string(),
                session_ttl_hours: 24,
            },
            library: LibraryConfig {
                root_path: PathBuf::from("/tmp/books"),
                covers_path: None,
                cover_max_dimension_px: None,
                cover_jpeg_quality: None,
                book_extensions: vec!["fb2".to_string()],
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

        let tera = tera::Tera::default();
        let mut translations = crate::web::i18n::Translations::new();
        translations.insert("en".to_string(), serde_json::json!({}));
        AppState::new(config, pool, tera, translations, false, false)
    }

    async fn insert_test_book(pool: &DbPool, title: &str) -> i64 {
        let cat_path = format!("/admin-{title}");
        let sql = pool.sql("INSERT INTO catalogs (path, cat_name) VALUES (?, 'admin')");
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
             VALUES (?, ?, '/admin', 'fb2', ?, ?, 'en', 2, 100, 2, 0, 0, '')",
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

    async fn response_json(resp: Response) -> serde_json::Value {
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&body).unwrap()
    }

    #[test]
    fn test_is_valid_password_boundaries() {
        assert!(!is_valid_password("1234567"));
        assert!(is_valid_password("12345678"));
        assert!(is_valid_password(&"x".repeat(32)));
        assert!(!is_valid_password(&"x".repeat(33)));
    }

    #[test]
    fn test_validate_book_title_rules() {
        assert_eq!(
            validate_book_title("  The Title  ").unwrap(),
            "The Title".to_string()
        );
        assert_eq!(validate_book_title("   ").unwrap_err(), "title_empty");
        assert_eq!(
            validate_book_title(&"a".repeat(257)).unwrap_err(),
            "title_too_long"
        );
        assert_eq!(
            validate_book_title("abc\u{0007}def").unwrap_err(),
            "title_invalid"
        );
    }

    #[test]
    fn test_get_session_user_id_valid_and_invalid() {
        let secret = b"session-secret-for-tests";
        let token = sign_session(42, secret, 1);
        let jar = CookieJar::new().add(Cookie::new("session", token));
        assert_eq!(get_session_user_id(&jar, secret), Some(42));

        let invalid = CookieJar::new().add(Cookie::new("session", "bad-token"));
        assert_eq!(get_session_user_id(&invalid, secret), None);
    }

    #[test]
    fn test_format_uptime_with_translations() {
        let mut ctx = tera::Context::new();
        let t = serde_json::json!({
            "admin": {
                "uptime_days": "days",
                "uptime_hours": "hours",
                "uptime_minutes": "mins"
            }
        });
        ctx.insert("t", &t);

        assert_eq!(format_uptime(90_061, &ctx), "1 days 1 hours 1 mins");
        assert_eq!(format_uptime(3_600, &ctx), "1 hours 0 mins");
    }

    #[test]
    fn test_format_uptime_fallback_labels() {
        let ctx = tera::Context::new();
        assert_eq!(format_uptime(172_920, &ctx), "2 d 2 min");
    }

    #[tokio::test]
    async fn test_update_book_series_handler_assign_and_remove() {
        let pool = create_test_pool().await;
        let state = test_state(pool.clone());
        let book_id = insert_test_book(&pool, "series-handler").await;

        let secret = state.config.server.session_secret.as_bytes();
        let session = sign_session(1, secret, 24);
        let csrf_token = generate_csrf_token(&session, secret);
        let jar = CookieJar::new().add(Cookie::new("session", session.clone()));

        let resp = update_book_series(
            State(state.clone()),
            jar.clone(),
            axum::Json(UpdateBookSeriesPayload {
                book_id,
                series_name: "Foundation".to_string(),
                series_no: 2,
                csrf_token: csrf_token.clone(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["ok"], true);
        assert_eq!(json["series"][0]["ser_name"], "Foundation");
        assert_eq!(json["series"][0]["ser_no"], 2);

        let linked = crate::db::queries::series::get_for_book(&pool, book_id)
            .await
            .unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].0.ser_name, "Foundation");
        assert_eq!(linked[0].1, 2);

        let resp = update_book_series(
            State(state),
            jar,
            axum::Json(UpdateBookSeriesPayload {
                book_id,
                series_name: String::new(),
                series_no: 0,
                csrf_token,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["ok"], true);
        assert_eq!(json["series"].as_array().unwrap().len(), 0);

        let linked = crate::db::queries::series::get_for_book(&pool, book_id)
            .await
            .unwrap();
        assert!(linked.is_empty());
        assert!(
            crate::db::queries::series::find_by_name(&pool, "Foundation")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_series_search_handler_short_and_match() {
        let pool = create_test_pool().await;
        let state = test_state(pool.clone());
        let book_id = insert_test_book(&pool, "series-search").await;
        crate::db::queries::series::set_book_series(&pool, book_id, "Foundations", 1)
            .await
            .unwrap();

        let resp = series_search(
            State(state.clone()),
            Query(SeriesSearchQuery { q: "f".into() }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["ok"], true);
        assert_eq!(json["series"].as_array().unwrap().len(), 0);

        let resp = series_search(State(state), Query(SeriesSearchQuery { q: "fo".into() })).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["ok"], true);
        let results = json["series"].as_array().unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0]["ser_name"], "Foundations");
    }
}
