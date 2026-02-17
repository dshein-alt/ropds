use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::{Deserialize, Serialize};

use crate::db::models::{Author, Genre};
use crate::db::queries::{authors, books, bookshelf, catalogs, genres, series};
use crate::state::AppState;
use crate::web::context::build_context;
use crate::web::pagination::Pagination;

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
    pub show_epub_convert: bool,
    pub show_mobi_convert: bool,
    pub doubles: i64,
    pub authors: Vec<Author>,
    pub genres: Vec<Genre>,
    pub series_list: Vec<SeriesEntry>,
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

fn default_m() -> String {
    "m".to_string()
}

// ── Helper: enrich a Book into a BookView ───────────────────────────

async fn enrich_book(
    state: &AppState,
    book: crate::db::models::Book,
    hide_doubles: bool,
) -> BookView {
    let book_authors = authors::get_for_book(&state.db, book.id)
        .await
        .unwrap_or_default();
    let book_genres = genres::get_for_book(&state.db, book.id)
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
    let is_fb2 = book.format == "fb2";

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
        show_epub_convert: is_fb2 && !state.config.converter.fb2_to_epub.is_empty(),
        show_mobi_convert: is_fb2 && !state.config.converter.fb2_to_mobi.is_empty(),
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
    }
}

// ── Helper: render template or return error ─────────────────────────

fn render(
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

async fn build_breadcrumbs(state: &AppState, cat_id: i64) -> Vec<Breadcrumb> {
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

pub async fn home(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Html<String>, StatusCode> {
    let ctx = build_context(&state, &jar, "home").await;
    render(&state.tera, "web/home.html", &ctx)
}

pub async fn catalogs(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<CatalogsParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "catalogs").await;
    let max_items = state.config.opds.max_items as i32;
    let cat_id = params.cat_id.unwrap_or(0);
    let offset = params.page * max_items;

    let subcatalogs = if cat_id == 0 {
        catalogs::get_root_catalogs(&state.db)
            .await
            .unwrap_or_default()
    } else {
        catalogs::get_children(&state.db, cat_id)
            .await
            .unwrap_or_default()
    };

    let hide_doubles = state.config.opds.hide_doubles;
    let (catalog_books, book_total) = if cat_id > 0 {
        let bks = books::get_by_catalog(&state.db, cat_id, max_items, offset, hide_doubles)
            .await
            .unwrap_or_default();
        let cnt = books::count_by_catalog(&state.db, cat_id, hide_doubles)
            .await
            .unwrap_or(0);
        (bks, cnt)
    } else {
        (vec![], 0)
    };

    let mut entries: Vec<CatalogEntry> = subcatalogs
        .iter()
        .map(|c| CatalogEntry {
            id: c.id,
            cat_name: c.cat_name.clone(),
            cat_type: c.cat_type,
            is_catalog: true,
            title: None,
            format: None,
            authors_str: None,
        })
        .collect();

    for book in &catalog_books {
        let book_authors = authors::get_for_book(&state.db, book.id)
            .await
            .unwrap_or_default();
        let authors_str = book_authors
            .iter()
            .map(|a| a.full_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        entries.push(CatalogEntry {
            id: book.id,
            cat_name: String::new(),
            cat_type: 0,
            is_catalog: false,
            title: Some(book.title.clone()),
            format: Some(book.format.clone()),
            authors_str: Some(authors_str),
        });
    }

    ctx.insert("entries", &entries);
    ctx.insert("cat_id", &cat_id);
    ctx.insert("pagination_qs", &format!("cat_id={}&", cat_id));

    if cat_id > 0 {
        let crumbs = build_breadcrumbs(&state, cat_id).await;
        ctx.insert("breadcrumbs", &crumbs);
    }

    let pagination = Pagination::new(params.page, max_items, book_total);
    ctx.insert("pagination", &pagination);

    render(&state.tera, "web/catalogs.html", &ctx)
}

pub async fn search_books(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<SearchBooksParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "books").await;
    let search_target = match params.search_type.as_str() {
        "a" => "author",
        "s" => "series",
        _ => "title",
    };
    ctx.insert("search_target", search_target);
    let max_items = state.config.opds.max_items as i32;
    let offset = params.page * max_items;

    let hide_doubles = state.config.opds.hide_doubles;
    let (raw_books, total) = match params.search_type.as_str() {
        "a" => {
            let id: i64 = params.q.parse().unwrap_or(0);
            let bks = books::get_by_author(&state.db, id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default();
            let cnt = books::count_by_author(&state.db, id, hide_doubles)
                .await
                .unwrap_or(0);
            if let Ok(Some(author)) = authors::get_by_id(&state.db, id).await {
                ctx.insert("search_label", &author.full_name);
            }
            (bks, cnt)
        }
        "s" => {
            let id: i64 = params.q.parse().unwrap_or(0);
            let bks = books::get_by_series(&state.db, id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default();
            let cnt = books::count_by_series(&state.db, id, hide_doubles)
                .await
                .unwrap_or(0);
            if let Ok(Some(ser)) = series::get_by_id(&state.db, id).await {
                ctx.insert("search_label", &ser.ser_name);
            }
            (bks, cnt)
        }
        "g" => {
            let id: i64 = params.q.parse().unwrap_or(0);
            let bks = books::get_by_genre(&state.db, id, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default();
            let cnt = books::count_by_genre(&state.db, id, hide_doubles)
                .await
                .unwrap_or(0);
            if let Ok(Some(genre)) = genres::get_by_id(&state.db, id).await {
                ctx.insert("search_label", &genre.subsection);
            }
            (bks, cnt)
        }
        "b" => {
            let term = params.q.to_uppercase();
            let bks =
                books::search_by_title_prefix(&state.db, &term, max_items, offset, hide_doubles)
                    .await
                    .unwrap_or_default();
            let cnt = books::count_by_title_prefix(&state.db, &term, hide_doubles)
                .await
                .unwrap_or(0);
            (bks, cnt)
        }
        "i" => {
            let id: i64 = params.q.parse().unwrap_or(0);
            let bks = books::get_by_id(&state.db, id)
                .await
                .ok()
                .flatten()
                .map(|b| vec![b])
                .unwrap_or_default();
            let cnt = bks.len() as i64;
            (bks, cnt)
        }
        _ => {
            let term = params.q.to_uppercase();
            let bks = books::search_by_title(&state.db, &term, max_items, offset, hide_doubles)
                .await
                .unwrap_or_default();
            let cnt = books::count_by_title_search(&state.db, &term, hide_doubles)
                .await
                .unwrap_or(0);
            (bks, cnt)
        }
    };

    let mut book_views = Vec::with_capacity(raw_books.len());
    for book in raw_books {
        book_views.push(enrich_book(&state, book, hide_doubles).await);
    }

    let pagination = Pagination::new(params.page, max_items, total);

    let display_query = match params.search_type.as_str() {
        // Preserve original typed query for grouped author/series flows.
        "a" | "s" => params
            .src_q
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&params.q)
            .to_string(),
        // ID-based direct jumps (e.g. random book) should not prefill the search box.
        "i" => String::new(),
        _ => params.q.clone(),
    };

    let mut pagination_qs = format!(
        "type={}&q={}&",
        params.search_type,
        urlencoding::encode(&params.q)
    );
    if let Some(src_q) = params.src_q.as_deref().filter(|s| !s.trim().is_empty()) {
        pagination_qs.push_str(&format!("src_q={}&", urlencoding::encode(src_q)));
    }

    ctx.insert("books", &book_views);
    ctx.insert("pagination", &pagination);
    ctx.insert("search_type", &params.search_type);
    ctx.insert("search_terms", &display_query);
    ctx.insert("pagination_qs", &pagination_qs);

    render(&state.tera, "web/books.html", &ctx)
}

pub async fn books_browse(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BrowseParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "books").await;
    let split_items = state.config.opds.split_items as i64;

    let prefix = params.chars.to_uppercase();
    let groups = books::get_title_prefix_groups(&state.db, params.lang, &prefix)
        .await
        .unwrap_or_default();

    let prefix_groups: Vec<PrefixGroup> = groups
        .into_iter()
        .map(|(p, cnt)| PrefixGroup {
            prefix: p,
            count: cnt,
            drill_deeper: cnt >= split_items,
        })
        .collect();

    ctx.insert("groups", &prefix_groups);
    ctx.insert("lang", &params.lang);
    ctx.insert("chars", &prefix);
    ctx.insert("browse_type", "books");
    ctx.insert("search_url", "/web/search/books");
    ctx.insert("browse_url", "/web/books");
    ctx.insert("search_type_param", "b");

    render(&state.tera, "web/browse.html", &ctx)
}

pub async fn authors_browse(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BrowseParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "authors").await;
    let split_items = state.config.opds.split_items as i64;

    let prefix = params.chars.to_uppercase();
    let groups = authors::get_name_prefix_groups(&state.db, params.lang, &prefix)
        .await
        .unwrap_or_default();

    let prefix_groups: Vec<PrefixGroup> = groups
        .into_iter()
        .map(|(p, cnt)| PrefixGroup {
            prefix: p,
            count: cnt,
            drill_deeper: cnt >= split_items,
        })
        .collect();

    ctx.insert("groups", &prefix_groups);
    ctx.insert("lang", &params.lang);
    ctx.insert("chars", &prefix);
    ctx.insert("browse_type", "authors");
    ctx.insert("search_url", "/web/search/authors");
    ctx.insert("browse_url", "/web/authors");
    ctx.insert("search_type_param", "b");

    render(&state.tera, "web/browse.html", &ctx)
}

pub async fn series_browse(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BrowseParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "series").await;
    let split_items = state.config.opds.split_items as i64;

    let prefix = params.chars.to_uppercase();
    let groups = series::get_name_prefix_groups(&state.db, params.lang, &prefix)
        .await
        .unwrap_or_default();

    let prefix_groups: Vec<PrefixGroup> = groups
        .into_iter()
        .map(|(p, cnt)| PrefixGroup {
            prefix: p,
            count: cnt,
            drill_deeper: cnt >= split_items,
        })
        .collect();

    ctx.insert("groups", &prefix_groups);
    ctx.insert("lang", &params.lang);
    ctx.insert("chars", &prefix);
    ctx.insert("browse_type", "series");
    ctx.insert("search_url", "/web/search/series");
    ctx.insert("browse_url", "/web/series");
    ctx.insert("search_type_param", "b");

    render(&state.tera, "web/browse.html", &ctx)
}

pub async fn genres(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<GenresParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "genres").await;

    match params.section {
        None => {
            let sections = genres::get_sections_with_counts(&state.db)
                .await
                .unwrap_or_default();
            ctx.insert("sections", &sections);
            ctx.insert("is_top_level", &true);
        }
        Some(ref section) => {
            let subsections = genres::get_by_section_with_counts(&state.db, section)
                .await
                .unwrap_or_default();
            let items: Vec<serde_json::Value> = subsections
                .into_iter()
                .map(|(g, cnt)| {
                    serde_json::json!({
                        "id": g.id,
                        "subsection": g.subsection,
                        "code": g.code,
                        "count": cnt,
                    })
                })
                .collect();
            ctx.insert("subsections", &items);
            ctx.insert("is_top_level", &false);
            ctx.insert("section_name", section);
        }
    }

    render(&state.tera, "web/genres.html", &ctx)
}

pub async fn search_authors(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<SearchListParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "authors").await;
    ctx.insert("search_target", "author");
    let max_items = state.config.opds.max_items as i32;
    let offset = params.page * max_items;

    let term = params.q.to_uppercase();
    let items = authors::search_by_name(&state.db, &term, max_items, offset)
        .await
        .unwrap_or_default();
    let total = authors::count_by_name_search(&state.db, &term)
        .await
        .unwrap_or(0);

    let hide_doubles = state.config.opds.hide_doubles;
    let mut enriched: Vec<serde_json::Value> = Vec::new();
    for author in &items {
        let book_count = books::count_by_author(&state.db, author.id, hide_doubles)
            .await
            .unwrap_or(0);
        enriched.push(serde_json::json!({
            "id": author.id,
            "full_name": author.full_name,
            "book_count": book_count,
        }));
    }

    let pagination = Pagination::new(params.page, max_items, total);
    let search_terms_encoded = urlencoding::encode(&params.q).to_string();

    ctx.insert("authors", &enriched);
    ctx.insert("pagination", &pagination);
    ctx.insert("search_terms", &params.q);
    ctx.insert("search_terms_encoded", &search_terms_encoded);
    ctx.insert(
        "pagination_qs",
        &format!(
            "type={}&q={}&",
            params.search_type,
            urlencoding::encode(&params.q)
        ),
    );

    render(&state.tera, "web/authors.html", &ctx)
}

pub async fn search_series(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<SearchListParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "series").await;
    ctx.insert("search_target", "series");
    let max_items = state.config.opds.max_items as i32;
    let offset = params.page * max_items;

    let term = params.q.to_uppercase();
    let items = series::search_by_name(&state.db, &term, max_items, offset)
        .await
        .unwrap_or_default();
    let total = series::count_by_name_search(&state.db, &term)
        .await
        .unwrap_or(0);

    let hide_doubles = state.config.opds.hide_doubles;
    let mut enriched: Vec<serde_json::Value> = Vec::new();
    for ser in &items {
        let book_count = books::count_by_series(&state.db, ser.id, hide_doubles)
            .await
            .unwrap_or(0);
        enriched.push(serde_json::json!({
            "id": ser.id,
            "ser_name": ser.ser_name,
            "book_count": book_count,
        }));
    }

    let pagination = Pagination::new(params.page, max_items, total);
    let search_terms_encoded = urlencoding::encode(&params.q).to_string();

    ctx.insert("series_list", &enriched);
    ctx.insert("pagination", &pagination);
    ctx.insert("search_terms", &params.q);
    ctx.insert("search_terms_encoded", &search_terms_encoded);
    ctx.insert(
        "pagination_qs",
        &format!(
            "type={}&q={}&",
            params.search_type,
            urlencoding::encode(&params.q)
        ),
    );

    render(&state.tera, "web/series.html", &ctx)
}

pub async fn set_language(
    jar: CookieJar,
    Query(params): Query<SetLanguageParams>,
) -> (CookieJar, Redirect) {
    let cookie = Cookie::build(("lang", params.lang))
        .path("/")
        .max_age(time::Duration::days(365))
        .build();
    let jar = jar.add(cookie);
    let redirect = params
        .redirect
        .as_deref()
        .filter(|r| r.starts_with('/') && !r.starts_with("//"))
        .unwrap_or("/web");
    (jar, Redirect::to(redirect))
}

/// GET /web/download/:book_id/:zip_flag — download a book via the web UI.
///
/// Reuses the OPDS download helpers for file reading and response building.
/// Tracks the download on the user's bookshelf via session cookie.
pub async fn web_download(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((book_id, zip_flag)): Path<(i64, i32)>,
) -> Response {
    let book = match books::get_by_id(&state.db, book_id).await {
        Ok(Some(b)) => b,
        Ok(None) => return (StatusCode::NOT_FOUND, "Book not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB error").into_response(),
    };

    let root = &state.config.library.root_path;

    let data = match crate::opds::download::read_book_file(root, &book.path, &book.filename, book.cat_type) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to read book {}: {e}", book_id);
            return (StatusCode::NOT_FOUND, "File not found").into_response();
        }
    };

    // Fire-and-forget bookshelf tracking via session cookie
    let secret = state.config.server.session_secret.as_bytes();
    if let Some(user_id) = jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        let _ = bookshelf::upsert(&state.db, user_id, book_id).await;
    }

    let filename = &book.filename;
    let mime = crate::opds::xml::mime_for_format(&book.format);

    if zip_flag == 1 && !crate::opds::xml::is_nozip_format(&book.format) {
        match crate::opds::download::wrap_in_zip(filename, &data) {
            Ok(zipped) => {
                let zip_name = format!("{filename}.zip");
                let zip_mime = crate::opds::xml::mime_for_zip(&book.format);
                crate::opds::download::file_response(&zipped, &zip_name, &zip_mime)
            }
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "ZIP error").into_response(),
        }
    } else {
        crate::opds::download::file_response(&data, filename, mime)
    }
}
