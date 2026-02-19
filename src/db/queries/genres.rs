use crate::db::models::{Genre, GenreSection, GenreSectionTranslation, GenreTranslation};
use crate::db::{DbBackend, DbPool};

// ---------------------------------------------------------------------------
// Display queries (language-aware, with English fallback)
// ---------------------------------------------------------------------------

/// Translated genre by ID.
pub async fn get_by_id(pool: &DbPool, id: i64, lang: &str) -> Result<Option<Genre>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT g.id, g.code, \
               COALESCE(gst.name, gst_en.name, g.section) AS section, \
               COALESCE(gt.name, gt_en.name, g.subsection) AS subsection, \
               g.section_id \
         FROM genres g \
         LEFT JOIN genre_sections gs ON gs.id = g.section_id \
         LEFT JOIN genre_section_translations gst ON gst.section_id = gs.id AND gst.lang = ? \
         LEFT JOIN genre_section_translations gst_en ON gst_en.section_id = gs.id AND gst_en.lang = 'en' \
         LEFT JOIN genre_translations gt ON gt.genre_id = g.id AND gt.lang = ? \
         LEFT JOIN genre_translations gt_en ON gt_en.genre_id = g.id AND gt_en.lang = 'en' \
         WHERE g.id = ?",
    );
    sqlx::query_as::<_, Genre>(&sql)
        .bind(lang)
        .bind(lang)
        .bind(id)
        .fetch_optional(pool.inner())
        .await
}

/// Section code for a given section ID.
pub async fn get_section_code(
    pool: &DbPool,
    section_id: i64,
) -> Result<Option<String>, sqlx::Error> {
    let sql = pool.sql("SELECT code FROM genre_sections WHERE id = ?");
    let row: Option<(String,)> = sqlx::query_as(&sql)
        .bind(section_id)
        .fetch_optional(pool.inner())
        .await?;
    Ok(row.map(|r| r.0))
}

/// Genre by code (no translations needed — used by scanner for linking).
pub async fn get_by_code(pool: &DbPool, code: &str) -> Result<Option<Genre>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM genres WHERE code = ?");
    sqlx::query_as::<_, Genre>(&sql)
        .bind(code)
        .fetch_optional(pool.inner())
        .await
}

/// All section codes with translated names. Returns `(code, name)`.
pub async fn get_sections(pool: &DbPool, lang: &str) -> Result<Vec<(String, String)>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT gs.code, COALESCE(gst.name, gst_en.name, gs.code) AS name \
         FROM genre_sections gs \
         LEFT JOIN genre_section_translations gst ON gst.section_id = gs.id AND gst.lang = ? \
         LEFT JOIN genre_section_translations gst_en ON gst_en.section_id = gs.id AND gst_en.lang = 'en' \
         ORDER BY name",
    );
    let rows: Vec<(String, String)> = sqlx::query_as(&sql)
        .bind(lang)
        .fetch_all(pool.inner())
        .await?;
    Ok(rows)
}

/// Translated genres in a section (by section code).
pub async fn get_by_section(
    pool: &DbPool,
    section_code: &str,
    lang: &str,
) -> Result<Vec<Genre>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT g.id, g.code, \
               COALESCE(gst.name, gst_en.name, g.section) AS section, \
               COALESCE(gt.name, gt_en.name, g.subsection) AS subsection, \
               g.section_id \
         FROM genres g \
         JOIN genre_sections gs ON gs.id = g.section_id \
         LEFT JOIN genre_section_translations gst ON gst.section_id = gs.id AND gst.lang = ? \
         LEFT JOIN genre_section_translations gst_en ON gst_en.section_id = gs.id AND gst_en.lang = 'en' \
         LEFT JOIN genre_translations gt ON gt.genre_id = g.id AND gt.lang = ? \
         LEFT JOIN genre_translations gt_en ON gt_en.genre_id = g.id AND gt_en.lang = 'en' \
         WHERE gs.code = ? \
         ORDER BY subsection",
    );
    sqlx::query_as::<_, Genre>(&sql)
        .bind(lang)
        .bind(lang)
        .bind(section_code)
        .fetch_all(pool.inner())
        .await
}

/// All genres with translated names, ordered by section then subsection.
pub async fn get_all(pool: &DbPool, lang: &str) -> Result<Vec<Genre>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT g.id, g.code, \
               COALESCE(gst.name, gst_en.name, g.section) AS section, \
               COALESCE(gt.name, gt_en.name, g.subsection) AS subsection, \
               g.section_id \
         FROM genres g \
         LEFT JOIN genre_sections gs ON gs.id = g.section_id \
         LEFT JOIN genre_section_translations gst ON gst.section_id = gs.id AND gst.lang = ? \
         LEFT JOIN genre_section_translations gst_en ON gst_en.section_id = gs.id AND gst_en.lang = 'en' \
         LEFT JOIN genre_translations gt ON gt.genre_id = g.id AND gt.lang = ? \
         LEFT JOIN genre_translations gt_en ON gt_en.genre_id = g.id AND gt_en.lang = 'en' \
         ORDER BY section, subsection",
    );
    sqlx::query_as::<_, Genre>(&sql)
        .bind(lang)
        .bind(lang)
        .fetch_all(pool.inner())
        .await
}

/// Translated genres linked to a book.
pub async fn get_for_book(
    pool: &DbPool,
    book_id: i64,
    lang: &str,
) -> Result<Vec<Genre>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT g.id, g.code, \
               COALESCE(gst.name, gst_en.name, g.section) AS section, \
               COALESCE(gt.name, gt_en.name, g.subsection) AS subsection, \
               g.section_id \
         FROM genres g \
         JOIN book_genres bg ON bg.genre_id = g.id \
         LEFT JOIN genre_sections gs ON gs.id = g.section_id \
         LEFT JOIN genre_section_translations gst ON gst.section_id = gs.id AND gst.lang = ? \
         LEFT JOIN genre_section_translations gst_en ON gst_en.section_id = gs.id AND gst_en.lang = 'en' \
         LEFT JOIN genre_translations gt ON gt.genre_id = g.id AND gt.lang = ? \
         LEFT JOIN genre_translations gt_en ON gt_en.genre_id = g.id AND gt_en.lang = 'en' \
         WHERE bg.book_id = ? \
         ORDER BY section, subsection",
    );
    sqlx::query_as::<_, Genre>(&sql)
        .bind(lang)
        .bind(lang)
        .bind(book_id)
        .fetch_all(pool.inner())
        .await
}

/// Section codes with translated names and book counts. Returns `(code, name, count)`.
pub async fn get_sections_with_counts(
    pool: &DbPool,
    lang: &str,
) -> Result<Vec<(String, String, i64)>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT gs.code, \
               COALESCE(gst.name, gst_en.name, gs.code) AS name, \
               COUNT(DISTINCT bg.book_id) AS cnt \
         FROM genre_sections gs \
         JOIN genres g ON g.section_id = gs.id \
         JOIN book_genres bg ON bg.genre_id = g.id \
         JOIN books b ON b.id = bg.book_id AND b.avail > 0 \
         LEFT JOIN genre_section_translations gst ON gst.section_id = gs.id AND gst.lang = ? \
         LEFT JOIN genre_section_translations gst_en ON gst_en.section_id = gs.id AND gst_en.lang = 'en' \
         GROUP BY gs.code \
         ORDER BY name",
    );
    let rows: Vec<(String, String, i64)> = sqlx::query_as(&sql)
        .bind(lang)
        .fetch_all(pool.inner())
        .await?;
    Ok(rows)
}

/// Translated genres within a section (by code), each with its book count.
pub async fn get_by_section_with_counts(
    pool: &DbPool,
    section_code: &str,
    lang: &str,
) -> Result<Vec<(Genre, i64)>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT g.id, g.code, \
               COALESCE(gst.name, gst_en.name, g.section) AS section, \
               COALESCE(gt.name, gt_en.name, g.subsection) AS subsection, \
               g.section_id, \
               COUNT(DISTINCT bg.book_id) AS cnt \
         FROM genres g \
         JOIN genre_sections gs ON gs.id = g.section_id \
         LEFT JOIN book_genres bg ON bg.genre_id = g.id \
         LEFT JOIN books b ON b.id = bg.book_id AND b.avail > 0 \
         LEFT JOIN genre_section_translations gst ON gst.section_id = gs.id AND gst.lang = ? \
         LEFT JOIN genre_section_translations gst_en ON gst_en.section_id = gs.id AND gst_en.lang = 'en' \
         LEFT JOIN genre_translations gt ON gt.genre_id = g.id AND gt.lang = ? \
         LEFT JOIN genre_translations gt_en ON gt_en.genre_id = g.id AND gt_en.lang = 'en' \
         WHERE gs.code = ? \
         GROUP BY g.id, g.code \
         ORDER BY subsection",
    );
    let rows: Vec<(i64, String, String, String, i64, i64)> = sqlx::query_as(&sql)
        .bind(lang)
        .bind(lang)
        .bind(section_code)
        .fetch_all(pool.inner())
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, code, section, subsection, section_id, cnt)| {
            (
                Genre {
                    id,
                    code,
                    section,
                    subsection,
                    section_id: Some(section_id),
                },
                cnt,
            )
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Link / unlink (no translations needed)
// ---------------------------------------------------------------------------

pub async fn link_book(pool: &DbPool, book_id: i64, genre_id: i64) -> Result<(), sqlx::Error> {
    let sql = match pool.backend() {
        DbBackend::Mysql => "INSERT IGNORE INTO book_genres (book_id, genre_id) VALUES (?, ?)",
        _ => {
            "INSERT INTO book_genres (book_id, genre_id) VALUES (?, ?) \
             ON CONFLICT (book_id, genre_id) DO NOTHING"
        }
    };
    let sql = pool.sql(sql);
    sqlx::query(&sql)
        .bind(book_id)
        .bind(genre_id)
        .execute(pool.inner())
        .await?;
    Ok(())
}

/// Link a book to a genre by genre code. If the code doesn't match
/// any seeded genre, the link is silently skipped.
pub async fn link_book_by_code(pool: &DbPool, book_id: i64, code: &str) -> Result<(), sqlx::Error> {
    if let Some(genre) = get_by_code(pool, code).await? {
        link_book(pool, book_id, genre.id).await?;
    }
    Ok(())
}

/// Replace all genres for a book: delete existing links, insert new ones.
pub async fn set_book_genres(
    pool: &DbPool,
    book_id: i64,
    genre_ids: &[i64],
) -> Result<(), sqlx::Error> {
    let del = pool.sql("DELETE FROM book_genres WHERE book_id = ?");
    sqlx::query(&del)
        .bind(book_id)
        .execute(pool.inner())
        .await?;
    let sql = match pool.backend() {
        DbBackend::Mysql => "INSERT IGNORE INTO book_genres (book_id, genre_id) VALUES (?, ?)",
        _ => {
            "INSERT INTO book_genres (book_id, genre_id) VALUES (?, ?) \
             ON CONFLICT (book_id, genre_id) DO NOTHING"
        }
    };
    let sql = pool.sql(sql);
    for &genre_id in genre_ids {
        sqlx::query(&sql)
            .bind(book_id)
            .bind(genre_id)
            .execute(pool.inner())
            .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Admin CRUD: sections
// ---------------------------------------------------------------------------

pub async fn get_all_sections(pool: &DbPool) -> Result<Vec<GenreSection>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM genre_sections ORDER BY code");
    sqlx::query_as::<_, GenreSection>(&sql)
        .fetch_all(pool.inner())
        .await
}

pub async fn create_section(pool: &DbPool, code: &str) -> Result<i64, sqlx::Error> {
    let sql = pool.sql("INSERT INTO genre_sections (code) VALUES (?)");
    let result = sqlx::query(&sql).bind(code).execute(pool.inner()).await?;
    if let Some(id) = result.last_insert_id() {
        return Ok(id);
    }
    let sql = pool.sql("SELECT id FROM genre_sections WHERE code = ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(code)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

pub async fn delete_section(pool: &DbPool, section_id: i64) -> Result<(), sqlx::Error> {
    let sql = pool.sql("DELETE FROM genre_sections WHERE id = ?");
    sqlx::query(&sql)
        .bind(section_id)
        .execute(pool.inner())
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Admin CRUD: section translations
// ---------------------------------------------------------------------------

pub async fn get_section_translations(
    pool: &DbPool,
    section_id: i64,
) -> Result<Vec<GenreSectionTranslation>, sqlx::Error> {
    let sql =
        pool.sql("SELECT * FROM genre_section_translations WHERE section_id = ? ORDER BY lang");
    sqlx::query_as::<_, GenreSectionTranslation>(&sql)
        .bind(section_id)
        .fetch_all(pool.inner())
        .await
}

pub async fn upsert_section_translation(
    pool: &DbPool,
    section_id: i64,
    lang: &str,
    name: &str,
) -> Result<(), sqlx::Error> {
    let sql = match pool.backend() {
        DbBackend::Mysql => {
            "INSERT INTO genre_section_translations (section_id, lang, name) VALUES (?, ?, ?) \
             ON DUPLICATE KEY UPDATE name = VALUES(name)"
        }
        _ => {
            "INSERT INTO genre_section_translations (section_id, lang, name) VALUES (?, ?, ?) \
             ON CONFLICT (section_id, lang) DO UPDATE SET name = excluded.name"
        }
    };
    let sql = pool.sql(sql);
    sqlx::query(&sql)
        .bind(section_id)
        .bind(lang)
        .bind(name)
        .execute(pool.inner())
        .await?;
    Ok(())
}

pub async fn delete_section_translation(
    pool: &DbPool,
    section_id: i64,
    lang: &str,
) -> Result<(), sqlx::Error> {
    let sql = pool.sql("DELETE FROM genre_section_translations WHERE section_id = ? AND lang = ?");
    sqlx::query(&sql)
        .bind(section_id)
        .bind(lang)
        .execute(pool.inner())
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Admin CRUD: genres
// ---------------------------------------------------------------------------

pub async fn create_genre(pool: &DbPool, code: &str, section_id: i64) -> Result<i64, sqlx::Error> {
    let sql = pool
        .sql("INSERT INTO genres (code, section, subsection, section_id) VALUES (?, '', '', ?)");
    let result = sqlx::query(&sql)
        .bind(code)
        .bind(section_id)
        .execute(pool.inner())
        .await?;
    if let Some(id) = result.last_insert_id() {
        return Ok(id);
    }
    let sql = pool.sql("SELECT id FROM genres WHERE code = ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(code)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

pub async fn delete_genre(pool: &DbPool, genre_id: i64) -> Result<(), sqlx::Error> {
    let sql = pool.sql("DELETE FROM genres WHERE id = ?");
    sqlx::query(&sql)
        .bind(genre_id)
        .execute(pool.inner())
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Admin CRUD: genre translations
// ---------------------------------------------------------------------------

pub async fn get_genre_translations(
    pool: &DbPool,
    genre_id: i64,
) -> Result<Vec<GenreTranslation>, sqlx::Error> {
    let sql = pool.sql("SELECT * FROM genre_translations WHERE genre_id = ? ORDER BY lang");
    sqlx::query_as::<_, GenreTranslation>(&sql)
        .bind(genre_id)
        .fetch_all(pool.inner())
        .await
}

pub async fn upsert_genre_translation(
    pool: &DbPool,
    genre_id: i64,
    lang: &str,
    name: &str,
) -> Result<(), sqlx::Error> {
    let sql = match pool.backend() {
        DbBackend::Mysql => {
            "INSERT INTO genre_translations (genre_id, lang, name) VALUES (?, ?, ?) \
             ON DUPLICATE KEY UPDATE name = VALUES(name)"
        }
        _ => {
            "INSERT INTO genre_translations (genre_id, lang, name) VALUES (?, ?, ?) \
             ON CONFLICT (genre_id, lang) DO UPDATE SET name = excluded.name"
        }
    };
    let sql = pool.sql(sql);
    sqlx::query(&sql)
        .bind(genre_id)
        .bind(lang)
        .bind(name)
        .execute(pool.inner())
        .await?;
    Ok(())
}

pub async fn delete_genre_translation(
    pool: &DbPool,
    genre_id: i64,
    lang: &str,
) -> Result<(), sqlx::Error> {
    let sql = pool.sql("DELETE FROM genre_translations WHERE genre_id = ? AND lang = ?");
    sqlx::query(&sql)
        .bind(genre_id)
        .bind(lang)
        .execute(pool.inner())
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Admin helpers
// ---------------------------------------------------------------------------

/// Languages that have at least one genre or section translation.
pub async fn get_available_languages(pool: &DbPool) -> Result<Vec<String>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT DISTINCT lang FROM ( \
             SELECT lang FROM genre_section_translations \
             UNION \
             SELECT lang FROM genre_translations \
         ) ORDER BY lang",
    );
    let rows: Vec<(String,)> = sqlx::query_as(&sql).fetch_all(pool.inner()).await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Full admin dump: all genres with all translations (for admin genres page).
/// Returns JSON-friendly tuples: `(genre_id, code, section_code, translations)`.
pub async fn get_all_admin(
    pool: &DbPool,
) -> Result<Vec<(i64, String, String, Vec<GenreTranslation>)>, sqlx::Error> {
    // Fetch genres with section code
    let sql = pool.sql(
        "SELECT g.id, g.code, COALESCE(gs.code, '') AS section_code \
         FROM genres g \
         LEFT JOIN genre_sections gs ON gs.id = g.section_id \
         ORDER BY section_code, g.code",
    );
    let genres: Vec<(i64, String, String)> = sqlx::query_as(&sql).fetch_all(pool.inner()).await?;

    let mut result = Vec::with_capacity(genres.len());
    for (id, code, section_code) in genres {
        let translations = get_genre_translations(pool, id).await?;
        result.push((id, code, section_code, translations));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::db::models::CatType;
    use crate::db::queries::books;

    async fn ensure_catalog(pool: &DbPool) -> i64 {
        let sql = pool
            .sql("INSERT INTO catalogs (path, cat_name) VALUES ('/genres-test', 'genres-test')");
        sqlx::query(&sql).execute(pool.inner()).await.unwrap();
        let sql = pool.sql("SELECT id FROM catalogs WHERE path = '/genres-test'");
        let row: (i64,) = sqlx::query_as(&sql).fetch_one(pool.inner()).await.unwrap();
        row.0
    }

    async fn insert_test_book(pool: &DbPool, catalog_id: i64, filename: &str) -> i64 {
        books::insert(
            pool,
            catalog_id,
            filename,
            "/genres-test",
            "fb2",
            filename,
            &filename.to_uppercase(),
            "",
            "",
            "en",
            2,
            1000,
            CatType::Normal,
            0,
            "",
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_display_queries_and_count_queries() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;

        let section_id = create_section(&pool, "ut_section_a").await.unwrap();
        let genre_id = create_genre(&pool, "ut_genre_a", section_id).await.unwrap();
        upsert_section_translation(&pool, section_id, "en", "Section A")
            .await
            .unwrap();
        upsert_section_translation(&pool, section_id, "ru", "Раздел А")
            .await
            .unwrap();
        upsert_genre_translation(&pool, genre_id, "en", "Genre A")
            .await
            .unwrap();
        upsert_genre_translation(&pool, genre_id, "ru", "Жанр А")
            .await
            .unwrap();

        let b1 = insert_test_book(&pool, cat, "genre-a-1.fb2").await;
        let b2 = insert_test_book(&pool, cat, "genre-a-2.fb2").await;
        link_book(&pool, b1, genre_id).await.unwrap();
        link_book(&pool, b2, genre_id).await.unwrap();

        assert_eq!(
            get_section_code(&pool, section_id).await.unwrap(),
            Some("ut_section_a".to_string())
        );

        let by_code = get_by_code(&pool, "ut_genre_a").await.unwrap().unwrap();
        assert_eq!(by_code.id, genre_id);

        let by_id_ru = get_by_id(&pool, genre_id, "ru").await.unwrap().unwrap();
        assert_eq!(by_id_ru.section, "Раздел А");
        assert_eq!(by_id_ru.subsection, "Жанр А");

        let by_id_fallback = get_by_id(&pool, genre_id, "de").await.unwrap().unwrap();
        assert_eq!(by_id_fallback.section, "Section A");
        assert_eq!(by_id_fallback.subsection, "Genre A");

        let sections = get_sections(&pool, "de").await.unwrap();
        let section = sections
            .iter()
            .find(|(code, _)| code == "ut_section_a")
            .unwrap();
        assert_eq!(section.1, "Section A");

        let by_section = get_by_section(&pool, "ut_section_a", "de").await.unwrap();
        assert_eq!(by_section.len(), 1);
        assert_eq!(by_section[0].code, "ut_genre_a");
        assert_eq!(by_section[0].subsection, "Genre A");

        let all = get_all(&pool, "de").await.unwrap();
        assert!(all.iter().any(|g| g.code == "ut_genre_a"));

        let for_book = get_for_book(&pool, b1, "de").await.unwrap();
        assert_eq!(for_book.len(), 1);
        assert_eq!(for_book[0].code, "ut_genre_a");

        let sections_with_counts = get_sections_with_counts(&pool, "de").await.unwrap();
        let section_count = sections_with_counts
            .iter()
            .find(|(code, _, _)| code == "ut_section_a")
            .unwrap();
        assert_eq!(section_count.2, 2);

        let by_section_with_counts = get_by_section_with_counts(&pool, "ut_section_a", "de")
            .await
            .unwrap();
        assert_eq!(by_section_with_counts.len(), 1);
        assert_eq!(by_section_with_counts[0].0.code, "ut_genre_a");
        assert_eq!(by_section_with_counts[0].1, 2);
    }

    #[tokio::test]
    async fn test_linking_and_set_book_genres() {
        let pool = create_test_pool().await;
        let cat = ensure_catalog(&pool).await;

        let section_id = create_section(&pool, "ut_section_b").await.unwrap();
        let g1 = create_genre(&pool, "ut_genre_b1", section_id)
            .await
            .unwrap();
        let g2 = create_genre(&pool, "ut_genre_b2", section_id)
            .await
            .unwrap();
        upsert_genre_translation(&pool, g1, "en", "Genre B1")
            .await
            .unwrap();
        upsert_genre_translation(&pool, g2, "en", "Genre B2")
            .await
            .unwrap();

        let book_id = insert_test_book(&pool, cat, "linking.fb2").await;
        link_book_by_code(&pool, book_id, "ut_genre_b1")
            .await
            .unwrap();
        link_book_by_code(&pool, book_id, "missing_genre_code")
            .await
            .unwrap();

        let linked = get_for_book(&pool, book_id, "en").await.unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].code, "ut_genre_b1");

        link_book(&pool, book_id, g1).await.unwrap();
        let linked = get_for_book(&pool, book_id, "en").await.unwrap();
        assert_eq!(linked.len(), 1);

        set_book_genres(&pool, book_id, &[g1, g2]).await.unwrap();
        let mut linked_codes: Vec<String> = get_for_book(&pool, book_id, "en")
            .await
            .unwrap()
            .into_iter()
            .map(|g| g.code)
            .collect();
        linked_codes.sort();
        assert_eq!(
            linked_codes,
            vec!["ut_genre_b1".to_string(), "ut_genre_b2".to_string()]
        );
    }

    #[tokio::test]
    async fn test_admin_crud_translations_and_languages() {
        let pool = create_test_pool().await;

        let section_id = create_section(&pool, "ut_section_c").await.unwrap();
        assert!(
            get_all_sections(&pool)
                .await
                .unwrap()
                .iter()
                .any(|s| s.id == section_id && s.code == "ut_section_c")
        );

        upsert_section_translation(&pool, section_id, "en", "Section C")
            .await
            .unwrap();
        upsert_section_translation(&pool, section_id, "ru", "Раздел C")
            .await
            .unwrap();
        upsert_section_translation(&pool, section_id, "en", "Section C Updated")
            .await
            .unwrap();
        let section_translations = get_section_translations(&pool, section_id).await.unwrap();
        assert_eq!(section_translations.len(), 2);
        assert!(
            section_translations
                .iter()
                .any(|t| t.lang == "en" && t.name == "Section C Updated")
        );

        let genre_id = create_genre(&pool, "ut_genre_c", section_id).await.unwrap();
        upsert_genre_translation(&pool, genre_id, "en", "Genre C")
            .await
            .unwrap();
        upsert_genre_translation(&pool, genre_id, "ru", "Жанр C")
            .await
            .unwrap();
        upsert_genre_translation(&pool, genre_id, "ru", "Жанр C Updated")
            .await
            .unwrap();
        let genre_translations = get_genre_translations(&pool, genre_id).await.unwrap();
        assert_eq!(genre_translations.len(), 2);
        assert!(
            genre_translations
                .iter()
                .any(|t| t.lang == "ru" && t.name == "Жанр C Updated")
        );

        let langs = get_available_languages(&pool).await.unwrap();
        assert!(langs.iter().any(|lang| lang == "en"));
        assert!(langs.iter().any(|lang| lang == "ru"));

        delete_genre_translation(&pool, genre_id, "ru")
            .await
            .unwrap();
        assert_eq!(
            get_genre_translations(&pool, genre_id).await.unwrap().len(),
            1
        );

        delete_section_translation(&pool, section_id, "ru")
            .await
            .unwrap();
        assert_eq!(
            get_section_translations(&pool, section_id)
                .await
                .unwrap()
                .len(),
            1
        );

        delete_genre(&pool, genre_id).await.unwrap();
        assert!(get_by_code(&pool, "ut_genre_c").await.unwrap().is_none());

        delete_section(&pool, section_id).await.unwrap();
        assert!(get_section_code(&pool, section_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_get_all_admin_contains_created_genres() {
        let pool = create_test_pool().await;

        let section_id = create_section(&pool, "ut_section_d").await.unwrap();
        let g1 = create_genre(&pool, "ut_genre_d1", section_id)
            .await
            .unwrap();
        let g2 = create_genre(&pool, "ut_genre_d2", section_id)
            .await
            .unwrap();
        upsert_genre_translation(&pool, g1, "en", "Genre D1")
            .await
            .unwrap();
        upsert_genre_translation(&pool, g1, "ru", "Жанр D1")
            .await
            .unwrap();
        upsert_genre_translation(&pool, g2, "en", "Genre D2")
            .await
            .unwrap();

        let admin_rows = get_all_admin(&pool).await.unwrap();

        let d1 = admin_rows
            .iter()
            .find(|(_, code, _, _)| code == "ut_genre_d1")
            .unwrap();
        assert_eq!(d1.2, "ut_section_d");
        assert!(d1.3.iter().any(|t| t.lang == "en" && t.name == "Genre D1"));
        assert!(d1.3.iter().any(|t| t.lang == "ru" && t.name == "Жанр D1"));

        let d2 = admin_rows
            .iter()
            .find(|(_, code, _, _)| code == "ut_genre_d2")
            .unwrap();
        assert_eq!(d2.2, "ut_section_d");
        assert!(d2.3.iter().any(|t| t.lang == "en" && t.name == "Genre D2"));
    }
}
