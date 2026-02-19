use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::{Deserialize, Serialize};

use crate::db::models::{Author, Genre};
use crate::db::queries::{authors, books, bookshelf, catalogs, genres, series};
use crate::state::AppState;
use crate::web::context::build_context;
use crate::web::i18n;
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
    pub doubles: i64,
    pub authors: Vec<Author>,
    pub genres: Vec<Genre>,
    pub series_list: Vec<SeriesEntry>,
    pub on_bookshelf: bool,
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
    shelf_ids: Option<&std::collections::HashSet<i64>>,
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
        read_time: String::new(),
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
        if let Some(last) = crumbs.last() {
            ctx.insert("current_cat_name", &last.name);
        }
        if crumbs.len() > 1 {
            let parent = &crumbs[crumbs.len() - 2];
            ctx.insert(
                "parent_url",
                &format!("/web/catalogs?cat_id={}", parent.cat_id.unwrap_or(0)),
            );
            ctx.insert("parent_name", &parent.name);
        } else {
            ctx.insert("parent_url", "/web/catalogs");
        }
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
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());
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
            let t = i18n::get_locale(&state.translations, &locale);
            let label = t["nav"]["authors"].as_str().unwrap_or("Authors");
            ctx.insert("back_label", label);
            if let Some(src_q) = params.src_q.as_deref().filter(|s| !s.trim().is_empty()) {
                ctx.insert(
                    "back_url",
                    &format!(
                        "/web/search/authors?type=b&q={}",
                        urlencoding::encode(src_q)
                    ),
                );
            } else {
                ctx.insert("back_url", "/web/authors");
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
            let t = i18n::get_locale(&state.translations, &locale);
            let label = t["nav"]["series"].as_str().unwrap_or("Series");
            ctx.insert("back_label", label);
            if let Some(src_q) = params.src_q.as_deref().filter(|s| !s.trim().is_empty()) {
                ctx.insert(
                    "back_url",
                    &format!("/web/search/series?type=b&q={}", urlencoding::encode(src_q)),
                );
            } else {
                ctx.insert("back_url", "/web/series");
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
            if let Ok(Some(genre)) = genres::get_by_id(&state.db, id, &locale).await {
                ctx.insert("search_label", &genre.subsection);
                // Back navigation to the genre's section
                if let Some(section_id) = genre.section_id
                    && let Ok(Some(code)) = genres::get_section_code(&state.db, section_id).await
                {
                    ctx.insert(
                        "back_url",
                        &format!("/web/genres?section={}", urlencoding::encode(&code)),
                    );
                    ctx.insert("back_label", &genre.section);
                }
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
            ctx.insert("search_label", &params.q);
            let t = i18n::get_locale(&state.translations, &locale);
            let label = t["nav"]["books"].as_str().unwrap_or("Books");
            ctx.insert("back_label", label);
            ctx.insert("back_url", "/web/books");
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
            ctx.insert("search_label", &params.q);
            (bks, cnt)
        }
    };

    let secret = state.config.server.session_secret.as_bytes();
    let shelf_ids = if let Some(user_id) = jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        crate::db::queries::bookshelf::get_book_ids_for_user(&state.db, user_id)
            .await
            .ok()
    } else {
        None
    };

    let mut book_views = Vec::with_capacity(raw_books.len());
    for book in raw_books {
        book_views.push(enrich_book(&state, book, hide_doubles, shelf_ids.as_ref(), &locale).await);
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
        // ID-based lookups (genre, direct book jump) should not prefill the search box.
        "g" | "i" => String::new(),
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

    let current_url = format!("/web/search/books?{}", pagination_qs);
    ctx.insert("current_path", &current_url);
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
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());

    match params.section {
        None => {
            let sections = genres::get_sections_with_counts(&state.db, &locale)
                .await
                .unwrap_or_default();
            ctx.insert("sections", &sections);
            ctx.insert("is_top_level", &true);
        }
        Some(ref section_code) => {
            let subsections = genres::get_by_section_with_counts(&state.db, section_code, &locale)
                .await
                .unwrap_or_default();
            // Extract translated section name from the first genre
            let section_name = subsections
                .first()
                .map(|(g, _)| g.section.clone())
                .unwrap_or_else(|| section_code.clone());
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
            ctx.insert("section_code", section_code);
            ctx.insert("section_name", &section_name);
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
    ctx.insert("back_url", "/web/authors");
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
    ctx.insert("back_url", "/web/series");
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

    let data = match crate::opds::download::read_book_file(
        root,
        &book.path,
        &book.filename,
        book.cat_type,
    ) {
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

    let download_name =
        crate::opds::download::title_to_filename(&book.title, &book.format, &book.filename);
    let mime = crate::opds::xml::mime_for_format(&book.format);

    if zip_flag == 1 && !crate::opds::xml::is_nozip_format(&book.format) {
        match crate::opds::download::wrap_in_zip(&book.filename, &data) {
            Ok(zipped) => {
                let zip_name = format!("{download_name}.zip");
                let zip_mime = crate::opds::xml::mime_for_zip(&book.format);
                crate::opds::download::file_response(&zipped, &zip_name, &zip_mime)
            }
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "ZIP error").into_response(),
        }
    } else {
        crate::opds::download::file_response(&data, &download_name, mime)
    }
}

// ── Genres JSON API ────────────────────────────────────────────────

pub async fn genres_json(State(state): State<AppState>, jar: CookieJar) -> Response {
    let secret = state.config.server.session_secret.as_bytes();
    if jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
        .is_none()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());
    let all_genres = genres::get_all(&state.db, &locale)
        .await
        .unwrap_or_default();

    let mut sections: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        std::collections::BTreeMap::new();
    for g in &all_genres {
        sections
            .entry(g.section.clone())
            .or_default()
            .push(serde_json::json!({
                "id": g.id,
                "code": g.code,
                "subsection": g.subsection,
            }));
    }

    axum::Json(serde_json::json!({ "sections": sections })).into_response()
}

// ── Bookshelf toggle handler ────────────────────────────────────────

#[derive(Deserialize)]
pub struct BookshelfToggleForm {
    pub book_id: i64,
    pub csrf_token: String,
    pub redirect: Option<String>,
}

pub async fn bookshelf_toggle(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    axum::Form(form): axum::Form<BookshelfToggleForm>,
) -> Response {
    use crate::web::context::validate_csrf;

    let is_ajax = headers
        .get("X-Requested-With")
        .and_then(|v| v.to_str().ok())
        == Some("XMLHttpRequest");

    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        if is_ajax {
            return axum::Json(serde_json::json!({"ok": false})).into_response();
        }
        return (StatusCode::FORBIDDEN, "Invalid CSRF token").into_response();
    }

    let user_id = match jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        Some(uid) => uid,
        None => {
            if is_ajax {
                return axum::Json(serde_json::json!({"ok": false})).into_response();
            }
            return Redirect::to("/web/login").into_response();
        }
    };

    let on_shelf = bookshelf::is_on_shelf(&state.db, user_id, form.book_id)
        .await
        .unwrap_or(false);
    if on_shelf {
        let _ = bookshelf::delete_one(&state.db, user_id, form.book_id).await;
    } else {
        let _ = bookshelf::upsert(&state.db, user_id, form.book_id).await;
    }

    if is_ajax {
        return axum::Json(serde_json::json!({"ok": true, "on_shelf": !on_shelf})).into_response();
    }

    let redirect = form
        .redirect
        .filter(|r| !r.is_empty() && r.starts_with('/'))
        .unwrap_or_else(|| "/web".to_string());
    Redirect::to(&redirect).into_response()
}

// ── Bookshelf helpers ───────────────────────────────────────────────

fn parse_bookshelf_sort(sort: &str, dir: &str) -> (bookshelf::SortColumn, bool) {
    let col = match sort {
        "title" => bookshelf::SortColumn::Title,
        "author" => bookshelf::SortColumn::Author,
        _ => bookshelf::SortColumn::Date,
    };
    let ascending = dir == "asc";
    (col, ascending)
}

const BOOKSHELF_BATCH: i32 = 30;

async fn fetch_bookshelf_views(
    state: &AppState,
    user_id: i64,
    sort: &bookshelf::SortColumn,
    ascending: bool,
    limit: i32,
    offset: i32,
    lang: &str,
) -> Vec<BookView> {
    let raw_books = bookshelf::get_by_user(&state.db, user_id, sort, ascending, limit, offset)
        .await
        .unwrap_or_default();
    let read_times = bookshelf::get_read_times(&state.db, user_id)
        .await
        .unwrap_or_default();

    let shelf_ids: std::collections::HashSet<i64> = raw_books.iter().map(|b| b.id).collect();
    let hide_doubles = state.config.opds.hide_doubles;
    let mut views = Vec::with_capacity(raw_books.len());
    for book in raw_books {
        let bid = book.id;
        let mut v = enrich_book(state, book, hide_doubles, Some(&shelf_ids), lang).await;
        if let Some(rt) = read_times.get(&bid) {
            v.read_time = rt.clone();
        }
        views.push(v);
    }
    views
}

// ── Bookshelf page handler ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct BookshelfPageParams {
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub dir: String,
}

pub async fn bookshelf_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BookshelfPageParams>,
) -> Result<Html<String>, StatusCode> {
    let mut ctx = build_context(&state, &jar, "bookshelf").await;
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());

    let secret = state.config.server.session_secret.as_bytes();
    let user_id = match jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        Some(uid) => uid,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let sort_key = if params.sort.is_empty() {
        "date"
    } else {
        &params.sort
    };
    let dir_key = if params.dir.is_empty() {
        "desc"
    } else {
        &params.dir
    };
    let (sort_col, ascending) = parse_bookshelf_sort(sort_key, dir_key);

    let total = bookshelf::count_by_user(&state.db, user_id)
        .await
        .unwrap_or(0);

    let book_views = fetch_bookshelf_views(
        &state,
        user_id,
        &sort_col,
        ascending,
        BOOKSHELF_BATCH,
        0,
        &locale,
    )
    .await;

    let has_more = (book_views.len() as i64) < total;

    ctx.insert("books", &book_views);
    ctx.insert("current_path", "/web/bookshelf");
    ctx.insert("sort", sort_key);
    ctx.insert("dir", dir_key);
    ctx.insert("has_more", &has_more);
    ctx.insert("batch_size", &BOOKSHELF_BATCH);

    render(&state.tera, "web/bookshelf.html", &ctx)
}

// ── Bookshelf cards API (for infinite scroll) ───────────────────────

#[derive(Deserialize)]
pub struct BookshelfCardsParams {
    #[serde(default)]
    pub offset: i32,
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub dir: String,
}

pub async fn bookshelf_cards(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<BookshelfCardsParams>,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    let locale = jar
        .get("lang")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| state.config.web.language.clone());
    let secret = state.config.server.session_secret.as_bytes();
    let user_id = match jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        Some(uid) => uid,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let sort_key = if params.sort.is_empty() {
        "date"
    } else {
        &params.sort
    };
    let dir_key = if params.dir.is_empty() {
        "desc"
    } else {
        &params.dir
    };
    let (sort_col, ascending) = parse_bookshelf_sort(sort_key, dir_key);

    let total = bookshelf::count_by_user(&state.db, user_id)
        .await
        .unwrap_or(0);

    let book_views = fetch_bookshelf_views(
        &state,
        user_id,
        &sort_col,
        ascending,
        BOOKSHELF_BATCH,
        params.offset,
        &locale,
    )
    .await;

    let loaded = params.offset as i64 + book_views.len() as i64;
    let has_more = loaded < total;

    // Render card fragments
    let mut ctx = build_context(&state, &jar, "bookshelf").await;
    ctx.insert("books", &book_views);
    ctx.insert("current_path", "/web/bookshelf");

    let html = state
        .tera
        .render("web/_bookshelf_cards.html", &ctx)
        .unwrap_or_default();

    Ok(axum::Json(serde_json::json!({
        "html": html,
        "has_more": has_more
    })))
}

// ── Bookshelf clear handler ─────────────────────────────────────────

#[derive(Deserialize)]
pub struct BookshelfClearForm {
    pub csrf_token: String,
}

pub async fn bookshelf_clear(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<BookshelfClearForm>,
) -> Response {
    use crate::web::context::validate_csrf;

    let secret = state.config.server.session_secret.as_bytes();
    if !validate_csrf(&jar, secret, &form.csrf_token) {
        return (StatusCode::FORBIDDEN, "Invalid CSRF token").into_response();
    }

    let user_id = match jar
        .get("session")
        .and_then(|c| crate::web::auth::verify_session(c.value(), secret))
    {
        Some(uid) => uid,
        None => return Redirect::to("/web/login").into_response(),
    };

    let _ = crate::db::queries::bookshelf::clear_all(&state.db, user_id).await;
    Redirect::to("/web/bookshelf").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        Config, DatabaseConfig, LibraryConfig, OpdsConfig, ScannerConfig, ServerConfig,
        UploadConfig, WebConfig,
    };
    use crate::db::DbBackend;
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
                covers_path: PathBuf::from("/tmp/covers"),
                book_extensions: vec!["fb2".to_string(), "epub".to_string(), "zip".to_string()],
                scan_zip: true,
                zip_codepage: "cp866".to_string(),
                inpx_enable: false,
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
                show_covers: true,
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
        };

        let (db, _) = create_test_pool().await;
        let tera = tera::Tera::default();
        let mut translations = Translations::new();
        translations.insert("en".to_string(), serde_json::json!({"web": {}}));

        AppState::new(
            config,
            db,
            DbBackend::Sqlite,
            tera,
            translations,
            false,
            false,
        )
    }

    async fn ensure_catalog(pool: &crate::db::DbPool) -> i64 {
        sqlx::query("INSERT INTO catalogs (path, cat_name) VALUES ('/web-tests', 'web-tests')")
            .execute(pool)
            .await
            .unwrap();
        let row: (i64,) = sqlx::query_as("SELECT id FROM catalogs WHERE path = '/web-tests'")
            .fetch_one(pool)
            .await
            .unwrap();
        row.0
    }

    #[test]
    fn test_default_m() {
        assert_eq!(default_m(), "m".to_string());
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
