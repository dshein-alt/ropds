use crate::db::DbPool;
use crate::db::models::{Genre, GenreSection, GenreSectionTranslation, GenreTranslation};

// ---------------------------------------------------------------------------
// Display queries (language-aware, with English fallback)
// ---------------------------------------------------------------------------

/// Translated genre by ID.
pub async fn get_by_id(pool: &DbPool, id: i64, lang: &str) -> Result<Option<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>(
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
    )
    .bind(lang)
    .bind(lang)
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Genre by code (no translations needed — used by scanner for linking).
pub async fn get_by_code(pool: &DbPool, code: &str) -> Result<Option<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>("SELECT * FROM genres WHERE code = ?")
        .bind(code)
        .fetch_optional(pool)
        .await
}

/// All section codes with translated names. Returns `(code, name)`.
pub async fn get_sections(pool: &DbPool, lang: &str) -> Result<Vec<(String, String)>, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT gs.code, COALESCE(gst.name, gst_en.name, gs.code) AS name \
         FROM genre_sections gs \
         LEFT JOIN genre_section_translations gst ON gst.section_id = gs.id AND gst.lang = ? \
         LEFT JOIN genre_section_translations gst_en ON gst_en.section_id = gs.id AND gst_en.lang = 'en' \
         ORDER BY name",
    )
    .bind(lang)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Translated genres in a section (by section code).
pub async fn get_by_section(
    pool: &DbPool,
    section_code: &str,
    lang: &str,
) -> Result<Vec<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>(
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
    )
    .bind(lang)
    .bind(lang)
    .bind(section_code)
    .fetch_all(pool)
    .await
}

/// All genres with translated names, ordered by section then subsection.
pub async fn get_all(pool: &DbPool, lang: &str) -> Result<Vec<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>(
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
    )
    .bind(lang)
    .bind(lang)
    .fetch_all(pool)
    .await
}

/// Translated genres linked to a book.
pub async fn get_for_book(
    pool: &DbPool,
    book_id: i64,
    lang: &str,
) -> Result<Vec<Genre>, sqlx::Error> {
    sqlx::query_as::<_, Genre>(
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
    )
    .bind(lang)
    .bind(lang)
    .bind(book_id)
    .fetch_all(pool)
    .await
}

/// Section codes with translated names and book counts. Returns `(code, name, count)`.
pub async fn get_sections_with_counts(
    pool: &DbPool,
    lang: &str,
) -> Result<Vec<(String, String, i64)>, sqlx::Error> {
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
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
    )
    .bind(lang)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Translated genres within a section (by code), each with its book count.
pub async fn get_by_section_with_counts(
    pool: &DbPool,
    section_code: &str,
    lang: &str,
) -> Result<Vec<(Genre, i64)>, sqlx::Error> {
    let rows: Vec<(i64, String, String, String, i64, i64)> = sqlx::query_as(
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
    )
    .bind(lang)
    .bind(lang)
    .bind(section_code)
    .fetch_all(pool)
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
    sqlx::query("INSERT OR IGNORE INTO book_genres (book_id, genre_id) VALUES (?, ?)")
        .bind(book_id)
        .bind(genre_id)
        .execute(pool)
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
    sqlx::query("DELETE FROM book_genres WHERE book_id = ?")
        .bind(book_id)
        .execute(pool)
        .await?;
    for &genre_id in genre_ids {
        sqlx::query("INSERT OR IGNORE INTO book_genres (book_id, genre_id) VALUES (?, ?)")
            .bind(book_id)
            .bind(genre_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Admin CRUD: sections
// ---------------------------------------------------------------------------

pub async fn get_all_sections(pool: &DbPool) -> Result<Vec<GenreSection>, sqlx::Error> {
    sqlx::query_as::<_, GenreSection>("SELECT * FROM genre_sections ORDER BY code")
        .fetch_all(pool)
        .await
}

pub async fn create_section(pool: &DbPool, code: &str) -> Result<i64, sqlx::Error> {
    let result = sqlx::query("INSERT INTO genre_sections (code) VALUES (?)")
        .bind(code)
        .execute(pool)
        .await?;
    if let Some(id) = result.last_insert_id() {
        return Ok(id);
    }
    let row: (i64,) = sqlx::query_as("SELECT id FROM genre_sections WHERE code = ?")
        .bind(code)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn delete_section(pool: &DbPool, section_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM genre_sections WHERE id = ?")
        .bind(section_id)
        .execute(pool)
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
    sqlx::query_as::<_, GenreSectionTranslation>(
        "SELECT * FROM genre_section_translations WHERE section_id = ? ORDER BY lang",
    )
    .bind(section_id)
    .fetch_all(pool)
    .await
}

pub async fn upsert_section_translation(
    pool: &DbPool,
    section_id: i64,
    lang: &str,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO genre_section_translations (section_id, lang, name) VALUES (?, ?, ?) \
         ON CONFLICT (section_id, lang) DO UPDATE SET name = excluded.name",
    )
    .bind(section_id)
    .bind(lang)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_section_translation(
    pool: &DbPool,
    section_id: i64,
    lang: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM genre_section_translations WHERE section_id = ? AND lang = ?")
        .bind(section_id)
        .bind(lang)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Admin CRUD: genres
// ---------------------------------------------------------------------------

pub async fn create_genre(pool: &DbPool, code: &str, section_id: i64) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO genres (code, section, subsection, section_id) VALUES (?, '', '', ?)",
    )
    .bind(code)
    .bind(section_id)
    .execute(pool)
    .await?;
    if let Some(id) = result.last_insert_id() {
        return Ok(id);
    }
    let row: (i64,) = sqlx::query_as("SELECT id FROM genres WHERE code = ?")
        .bind(code)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn delete_genre(pool: &DbPool, genre_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM genres WHERE id = ?")
        .bind(genre_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_genre_section(
    pool: &DbPool,
    genre_id: i64,
    section_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE genres SET section_id = ? WHERE id = ?")
        .bind(section_id)
        .bind(genre_id)
        .execute(pool)
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
    sqlx::query_as::<_, GenreTranslation>(
        "SELECT * FROM genre_translations WHERE genre_id = ? ORDER BY lang",
    )
    .bind(genre_id)
    .fetch_all(pool)
    .await
}

pub async fn upsert_genre_translation(
    pool: &DbPool,
    genre_id: i64,
    lang: &str,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO genre_translations (genre_id, lang, name) VALUES (?, ?, ?) \
         ON CONFLICT (genre_id, lang) DO UPDATE SET name = excluded.name",
    )
    .bind(genre_id)
    .bind(lang)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_genre_translation(
    pool: &DbPool,
    genre_id: i64,
    lang: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM genre_translations WHERE genre_id = ? AND lang = ?")
        .bind(genre_id)
        .bind(lang)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Admin helpers
// ---------------------------------------------------------------------------

/// Languages that have at least one genre or section translation.
pub async fn get_available_languages(pool: &DbPool) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT lang FROM ( \
             SELECT lang FROM genre_section_translations \
             UNION \
             SELECT lang FROM genre_translations \
         ) ORDER BY lang",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Full admin dump: all genres with all translations (for admin genres page).
/// Returns JSON-friendly tuples: `(genre_id, code, section_code, translations)`.
pub async fn get_all_admin(
    pool: &DbPool,
) -> Result<Vec<(i64, String, String, Vec<GenreTranslation>)>, sqlx::Error> {
    // Fetch genres with section code
    let genres: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT g.id, g.code, COALESCE(gs.code, '') AS section_code \
         FROM genres g \
         LEFT JOIN genre_sections gs ON gs.id = g.section_id \
         ORDER BY section_code, g.code",
    )
    .fetch_all(pool)
    .await?;

    let mut result = Vec::with_capacity(genres.len());
    for (id, code, section_code) in genres {
        let translations = get_genre_translations(pool, id).await?;
        result.push((id, code, section_code, translations));
    }
    Ok(result)
}
