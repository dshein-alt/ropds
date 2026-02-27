use super::*;

// ── View models for templates ───────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BookView {
    pub id: i64,
    pub title: String,
    pub filename: String,
    pub format: String,
    pub size: i64,
    pub lang: String,
    pub annotation: String,
    pub docdate: String,
    pub cover: i32,
    pub cat_type: i32,
    pub show_zip: bool,
    pub doubles: i64,
    pub authors: Vec<Author>,
    pub genres: Vec<Genre>,
    pub series_list: Vec<SeriesEntry>,
    pub on_bookshelf: bool,
    pub has_read_progress: bool,
    pub read_progress_pct: i32,
    pub read_time: String,
}

#[derive(Debug, Serialize)]
pub struct SeriesEntry {
    pub id: i64,
    pub ser_name: String,
    pub ser_no: i32,
}

#[derive(Debug, Serialize)]
pub struct CatalogEntry {
    pub id: i64,
    pub cat_name: String,
    pub cat_type: i32,
    pub is_catalog: bool,
    pub title: Option<String>,
    pub format: Option<String>,
    pub authors_str: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PrefixGroup {
    pub prefix: String,
    pub count: i64,
    pub drill_deeper: bool,
}

#[derive(Debug, Serialize)]
pub struct Breadcrumb {
    pub name: String,
    pub cat_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ContinueReadingItem {
    pub book_id: i64,
    pub title: String,
    pub format: String,
    pub progress_pct: i32,
    pub updated_at: String,
}

// ── Query parameter structs ─────────────────────────────────────────

#[derive(Deserialize)]
pub struct CatalogsParams {
    pub cat_id: Option<i64>,
    #[serde(default)]
    pub page: i32,
}

#[derive(Deserialize)]
pub struct BrowseParams {
    #[serde(default)]
    pub lang: i32,
    #[serde(default)]
    pub chars: String,
}

#[derive(Deserialize)]
pub struct GenresParams {
    pub section: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchBooksParams {
    #[serde(rename = "type", default = "default_m")]
    pub search_type: String,
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub src_q: Option<String>,
    #[serde(default)]
    pub page: i32,
}

#[derive(Deserialize)]
pub struct SearchListParams {
    #[serde(rename = "type", default = "default_m")]
    pub search_type: String,
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub page: i32,
}

#[derive(Deserialize)]
pub struct SetLanguageParams {
    pub lang: String,
    pub redirect: Option<String>,
}

#[derive(Deserialize)]
pub struct RecentBooksParams {
    #[serde(default)]
    pub page: i32,
}

#[derive(Deserialize)]
pub struct ReaderOpenParams {
    #[serde(rename = "return")]
    pub return_to: Option<String>,
}

pub(super) fn default_m() -> String {
    "m".to_string()
}

pub(super) fn sanitize_internal_redirect(path: Option<&str>) -> &str {
    path.filter(|value| value.starts_with('/') && !value.starts_with("//") && !value.contains('\\'))
        .unwrap_or("/web")
}

pub(super) fn session_user_id(state: &AppState, jar: &CookieJar) -> Option<i64> {
    let secret = state.config.server.session_secret.as_bytes();
    jar.get("session")
        .and_then(|cookie| crate::web::auth::verify_session(cookie.value(), secret))
}

// ── Helper: enrich a Book into a BookView ───────────────────────────

pub(super) async fn enrich_book(
    state: &AppState,
    book: crate::db::models::Book,
    hide_doubles: bool,
    shelf_ids: Option<&std::collections::HashSet<i64>>,
    read_progress: Option<f64>,
    lang: &str,
) -> BookView {
    let book_authors = authors::get_for_book(&state.db, book.id)
        .await
        .unwrap_or_default();
    let book_genres = genres::get_for_book(&state.db, book.id, lang)
        .await
        .unwrap_or_default();
    let book_series = series::get_for_book(&state.db, book.id)
        .await
        .unwrap_or_default();

    let doubles = if hide_doubles {
        books::count_doubles(&state.db, book.id).await.unwrap_or(1)
    } else {
        1
    };

    let is_nozip = book.format == "epub" || book.format == "mobi";

    let read_progress_pct = read_progress
        .map(|value| (value * 100.0).round() as i32)
        .unwrap_or(0);

    BookView {
        id: book.id,
        title: book.title,
        filename: book.filename,
        format: book.format.clone(),
        size: book.size,
        lang: book.lang,
        annotation: book.annotation,
        docdate: book.docdate,
        cover: book.cover,
        cat_type: book.cat_type,
        show_zip: !is_nozip,
        doubles,
        authors: book_authors,
        genres: book_genres,
        series_list: book_series
            .into_iter()
            .map(|(s, ser_no)| SeriesEntry {
                id: s.id,
                ser_name: s.ser_name,
                ser_no,
            })
            .collect(),
        on_bookshelf: shelf_ids.is_some_and(|s| s.contains(&book.id)),
        has_read_progress: read_progress.is_some(),
        read_progress_pct,
        read_time: String::new(),
    }
}

// ── Helper: render template or return error ─────────────────────────

pub(super) fn render(
    tera: &tera::Tera,
    template: &str,
    ctx: &tera::Context,
) -> Result<Html<String>, StatusCode> {
    tera.render(template, ctx).map(Html).map_err(|e| {
        tracing::error!("Template render error ({}): {}", template, e);
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

// ── Helper: build breadcrumbs for catalog hierarchy ─────────────────

pub(super) async fn build_breadcrumbs(state: &AppState, cat_id: i64) -> Vec<Breadcrumb> {
    let mut crumbs = Vec::new();
    let mut current = Some(cat_id);
    while let Some(id) = current {
        match catalogs::get_by_id(&state.db, id).await {
            Ok(Some(cat)) => {
                crumbs.push(Breadcrumb {
                    name: cat.cat_name.clone(),
                    cat_id: Some(cat.id),
                });
                current = cat.parent_id;
            }
            _ => break,
        }
    }
    crumbs.reverse();
    crumbs
}

// ═══════════════════════════════════════════════════════════════════
// HANDLERS
// ═══════════════════════════════════════════════════════════════════
