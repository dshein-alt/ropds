use crate::db::DbPool;

use crate::db::models::Series;

pub async fn get_by_id(pool: &DbPool, id: i64) -> Result<Option<Series>, sqlx::Error> {
    sqlx::query_as::<_, Series>("SELECT * FROM series WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn search_by_name(
    pool: &DbPool,
    term: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Series>, sqlx::Error> {
    let pattern = format!("%{term}%");
    sqlx::query_as::<_, Series>(
        "SELECT * FROM series WHERE search_ser LIKE ? \
         ORDER BY search_ser LIMIT ? OFFSET ?",
    )
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn get_by_lang_code_prefix(
    pool: &DbPool,
    lang_code: i32,
    prefix: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Series>, sqlx::Error> {
    let pattern = format!("{prefix}%");
    sqlx::query_as::<_, Series>(
        "SELECT * FROM series WHERE lang_code = ? AND search_ser LIKE ? \
         ORDER BY search_ser LIMIT ? OFFSET ?",
    )
    .bind(lang_code)
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

pub async fn find_by_name(pool: &DbPool, ser_name: &str) -> Result<Option<Series>, sqlx::Error> {
    sqlx::query_as::<_, Series>("SELECT * FROM series WHERE ser_name = ?")
        .bind(ser_name)
        .fetch_optional(pool)
        .await
}

pub async fn insert(
    pool: &DbPool,
    ser_name: &str,
    search_ser: &str,
    lang_code: i32,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO series (ser_name, search_ser, lang_code) VALUES (?, ?, ?)",
    )
    .bind(ser_name)
    .bind(search_ser)
    .bind(lang_code)
    .execute(pool)
    .await?;
    if let Some(id) = result.last_insert_id() {
        if id > 0 {
            return Ok(id);
        }
    }
    // Fallback: query back by name (INSERT OR IGNORE returns 0 on conflict)
    let row: (i64,) = sqlx::query_as("SELECT id FROM series WHERE ser_name = ?")
        .bind(ser_name)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn link_book(
    pool: &DbPool,
    book_id: i64,
    series_id: i64,
    ser_no: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO book_series (book_id, series_id, ser_no) VALUES (?, ?, ?)")
        .bind(book_id)
        .bind(series_id)
        .bind(ser_no)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_for_book(pool: &DbPool, book_id: i64) -> Result<Vec<(Series, i32)>, sqlx::Error> {
    let rows: Vec<(i64, String, String, i32, i32)> = sqlx::query_as(
        "SELECT s.id, s.ser_name, s.search_ser, s.lang_code, bs.ser_no \
         FROM series s JOIN book_series bs ON bs.series_id = s.id \
         WHERE bs.book_id = ? ORDER BY s.ser_name",
    )
    .bind(book_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, ser_name, search_ser, lang_code, ser_no)| {
            (
                Series {
                    id,
                    ser_name,
                    search_ser,
                    lang_code,
                },
                ser_no,
            )
        })
        .collect())
}

/// Count series matching a name search (contains).
pub async fn count_by_name_search(pool: &DbPool, term: &str) -> Result<i64, sqlx::Error> {
    let pattern = format!("%{term}%");
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM series WHERE search_ser LIKE ?")
        .bind(&pattern)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Alphabet drill-down: get prefix groups for series names.
pub async fn get_name_prefix_groups(
    pool: &DbPool,
    lang_code: i32,
    current_prefix: &str,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let prefix_len = (current_prefix.chars().count() + 1) as i32;
    let like_pattern = format!("{}%", current_prefix);
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT SUBSTR(search_ser, 1, ?) as prefix, COUNT(*) as cnt \
         FROM series \
         WHERE (? = 0 OR lang_code = ?) AND search_ser LIKE ? \
         GROUP BY SUBSTR(search_ser, 1, ?) \
         ORDER BY prefix",
    )
    .bind(prefix_len)
    .bind(lang_code)
    .bind(lang_code)
    .bind(&like_pattern)
    .bind(prefix_len)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
